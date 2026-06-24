//! smol driver: an async [`SmolClient`] over the sans-IO [`Engine`].
//!
//! Mirrors the tokio driver's actor pattern on the smol ecosystem: one owner task
//! owns the `Engine`, the waiter map, and the transport, and races command requests
//! against the tmux byte stream (`smol::future::or` in place of `select!`). No lock
//! is held across an `.await`, and the task serializes register-then-write so FIFO
//! correlation holds.

use std::collections::HashMap;
use std::process::Stdio;

use smol::channel::{Receiver, Sender};
use smol::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use smol::process::{Child, Command};

use crate::commands;
use crate::engine::{CommandError, CommandId, CommandOutput, Engine, Incoming};
use crate::ids::{PaneId, WindowId};
use crate::layout::Layout;
use crate::notification::Notification;
use crate::spawn::SpawnOpts;

type CommandResult = Result<CommandOutput, CommandError>;

/// A command queued to the owner task, with the channel to deliver its reply.
struct Request {
    cmd: String,
    reply: Sender<CommandResult>,
}

/// An async tmux control-mode client backed by a smol task.
pub struct SmolClient {
    commands: Sender<Request>,
    events: Option<Receiver<Notification>>,
}

impl SmolClient {
    /// Spawn `tmux -C` (control mode — never `-CC`) over piped stdin/stdout. Must be
    /// called within a smol executor (the owner task is spawned onto it).
    pub async fn spawn(opts: SpawnOpts) -> std::io::Result<SmolClient> {
        let mut child = Command::new(&opts.program)
            .args(opts.argv())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdout = child.stdout.take().expect("piped stdout");
        let stdin = child.stdin.take().expect("piped stdin");
        Ok(Self::from_parts(stdout, stdin, Some(child)))
    }

    /// Build a client over an injected transport, no child process — the seam tests
    /// wrap around an in-memory socket pair. Must be called within a smol executor.
    pub fn with_transport<R, W>(reader: R, writer: W) -> SmolClient
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        Self::from_parts(reader, writer, None)
    }

    fn from_parts<R, W>(reader: R, writer: W, child: Option<Child>) -> SmolClient
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (cmd_tx, cmd_rx) = smol::channel::unbounded();
        let (ev_tx, ev_rx) = smol::channel::unbounded();
        smol::spawn(owner_task(reader, writer, child, cmd_rx, ev_tx)).detach();

        SmolClient {
            commands: cmd_tx,
            events: Some(ev_rx),
        }
    }

    /// Take the notification stream. Returns the receiver once; later calls return
    /// `None`. Drain it concurrently with `command`.
    pub fn events(&mut self) -> Option<Receiver<Notification>> {
        self.events.take()
    }

    /// Send a raw control-mode command and await tmux's correlated reply. Returns
    /// `Err(CommandError::Disconnected)` once the session has ended.
    pub async fn command(&self, cmd: &str) -> CommandResult {
        let (reply, rx) = smol::channel::bounded(1);
        let request = Request {
            cmd: cmd.to_string(),
            reply,
        };
        if self.commands.try_send(request).is_err() {
            return Err(CommandError::Disconnected);
        }
        rx.recv().await.unwrap_or(Err(CommandError::Disconnected))
    }

    /// Send raw bytes to a pane as key input (hex byte values — safe for arbitrary
    /// bytes and control sequences, no key-name lookup).
    pub async fn send_keys(&self, pane: PaneId, keys: &[u8]) -> Result<(), CommandError> {
        self.command(&commands::send_keys(pane, keys))
            .await
            .map(drop)
    }

    /// Set this control client's size.
    pub async fn resize(&self, cols: u16, rows: u16) -> Result<(), CommandError> {
        self.command(&commands::resize(cols, rows)).await.map(drop)
    }

    /// Set one window's size authoritatively (tmux `resize-window`, `window-size=manual`):
    /// the size holds regardless of the global `window-size` and is not arbitrated against
    /// client sizes — distinct from [`Client::resize`]. tmux bounds-checks; an out-of-range
    /// size surfaces as [`CommandError::Failed`], not a client-side check.
    pub async fn resize_window(
        &self,
        window: WindowId,
        cols: u16,
        rows: u16,
    ) -> Result<(), CommandError> {
        self.command(&commands::resize_window(window, cols, rows))
            .await
            .map(drop)
    }

    /// Push a layout onto a window. tmux arbitrates validity; a rejected layout
    /// surfaces as [`CommandError::Failed`], not a client-side check.
    pub async fn select_layout(
        &self,
        window: WindowId,
        layout: &Layout,
    ) -> Result<(), CommandError> {
        self.command(&commands::select_layout(window, layout))
            .await
            .map(drop)
    }
}

/// What the owner task's race resolved to this iteration.
enum Step {
    Command(Option<Request>),
    Read(std::io::Result<usize>),
}

async fn owner_task<R, W>(
    mut reader: R,
    mut writer: W,
    child: Option<Child>,
    commands: Receiver<Request>,
    events: Sender<Notification>,
) where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut engine = Engine::new();
    let mut waiters: HashMap<CommandId, Sender<CommandResult>> = HashMap::new();
    let mut buf = [0u8; 8192];

    loop {
        let step = smol::future::or(async { Step::Command(commands.recv().await.ok()) }, async {
            Step::Read(reader.read(&mut buf).await)
        })
        .await;

        match step {
            Step::Command(None) => break, // all client handles dropped
            Step::Command(Some(Request { cmd, reply })) => {
                let id = engine.register_command();
                if write_command(&mut writer, &cmd).await.is_err() {
                    let _ = reply.try_send(Err(CommandError::Disconnected));
                    break;
                }
                waiters.insert(id, reply);
            }
            Step::Read(Ok(0)) | Step::Read(Err(_)) => break,
            Step::Read(Ok(n)) => {
                for incoming in engine.feed(&buf[..n]) {
                    match incoming {
                        Incoming::Notification(notification) => {
                            let _ = events.try_send(notification);
                        }
                        Incoming::Reply { id, result } => resolve(&mut waiters, id, result),
                    }
                }
            }
        }
    }

    // Teardown: detach (best-effort empty line), fail outstanding commands, reap child.
    let _ = write_command(&mut writer, "").await;
    for incoming in engine.on_eof() {
        if let Incoming::Reply { id, result } = incoming {
            resolve(&mut waiters, id, result);
        }
    }
    if let Some(mut child) = child {
        let _ = child.status().await;
    }
}

fn resolve(
    waiters: &mut HashMap<CommandId, Sender<CommandResult>>,
    id: CommandId,
    result: CommandResult,
) {
    if let Some(waiter) = waiters.remove(&id) {
        let _ = waiter.try_send(result);
    }
}

async fn write_command<W: AsyncWrite + Unpin>(writer: &mut W, cmd: &str) -> std::io::Result<()> {
    writer.write_all(cmd.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::WindowId;
    use std::os::unix::net::UnixStream;

    /// (tmux side, client reader, client writer) — async-wrapped socket-pair ends.
    fn transport() -> (
        smol::Async<UnixStream>,
        smol::Async<UnixStream>,
        smol::Async<UnixStream>,
    ) {
        let (tmux, client) = UnixStream::pair().expect("socket pair");
        let reader = smol::Async::new(client.try_clone().expect("clone")).expect("async reader");
        let writer = smol::Async::new(client).expect("async writer");
        let tmux = smol::Async::new(tmux).expect("async tmux");
        (tmux, reader, writer)
    }

    #[test]
    fn delivers_notifications_to_events_receiver() {
        smol::block_on(async {
            let (mut tmux, reader, writer) = transport();
            let mut client = SmolClient::with_transport(reader, writer);
            let events = client.events().expect("events receiver");

            tmux.write_all(b"%window-add @5\n").await.expect("write");

            assert_eq!(
                events.recv().await.ok(),
                Some(Notification::WindowAdd(WindowId(5)))
            );
        });
    }

    #[test]
    fn command_awaits_its_reply() {
        smol::block_on(async {
            let (tmux, reader, writer) = transport();
            let client = SmolClient::with_transport(reader, writer);

            let fake = smol::spawn(async move {
                let mut tmux = tmux;
                let mut buf = [0u8; 256];
                let n = tmux.read(&mut buf).await.expect("read command");
                assert!(buf[..n].starts_with(b"list-panes"));
                tmux.write_all(b"%begin 1 1 1\npane-info\n%end 1 1 1\n")
                    .await
                    .expect("write reply");
                tmux
            });

            let result = client.command("list-panes").await;
            assert_eq!(
                result,
                Ok(CommandOutput {
                    lines: vec!["pane-info".to_string()],
                })
            );
            let _tmux = fake.await;
        });
    }

    #[test]
    fn command_after_disconnect_is_disconnected() {
        smol::block_on(async {
            let (tmux, reader, writer) = transport();
            let client = SmolClient::with_transport(reader, writer);

            drop(tmux); // peer gone — the owner task hits EOF and ends

            assert_eq!(
                client.command("list-panes").await,
                Err(CommandError::Disconnected)
            );
        });
    }
}
