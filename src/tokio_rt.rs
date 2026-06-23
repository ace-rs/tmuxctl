//! tokio driver: an async [`TokioClient`] over the sans-IO [`Engine`].
//!
//! Unlike the blocking driver, this cannot hold a lock across the `.await` of a
//! write, so it uses the actor pattern: a single owner task owns the `Engine`, the
//! waiter map, and the transport, and `select!`s between command requests (from a
//! channel) and the tmux byte stream. No shared lock — the task serializes both,
//! which also preserves the FIFO register-then-write ordering correlation needs.

use std::collections::HashMap;
use std::process::Stdio;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot};

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
    reply: oneshot::Sender<CommandResult>,
}

/// An async tmux control-mode client backed by a tokio task.
pub struct TokioClient {
    commands: mpsc::UnboundedSender<Request>,
    events: Option<mpsc::UnboundedReceiver<Notification>>,
}

impl TokioClient {
    /// Spawn `tmux -C` (control mode — never `-CC`) over piped stdin/stdout. The
    /// child is killed on drop as a backstop; normally the empty-line detach on
    /// teardown exits it cleanly. Must be called within a tokio runtime.
    pub async fn spawn(opts: SpawnOpts) -> std::io::Result<TokioClient> {
        let mut child = Command::new(&opts.program)
            .args(opts.argv())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let stdout = child.stdout.take().expect("piped stdout");
        let stdin = child.stdin.take().expect("piped stdin");
        Ok(Self::from_parts(stdout, stdin, Some(child)))
    }

    /// Build a client over an injected transport, no child process — the seam tests
    /// wrap around an in-memory `duplex`. Must be called within a tokio runtime.
    pub fn with_transport<R, W>(reader: R, writer: W) -> TokioClient
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        Self::from_parts(reader, writer, None)
    }

    fn from_parts<R, W>(reader: R, writer: W, child: Option<Child>) -> TokioClient
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (ev_tx, ev_rx) = mpsc::unbounded_channel();
        tokio::spawn(owner_task(reader, writer, child, cmd_rx, ev_tx));

        TokioClient {
            commands: cmd_tx,
            events: Some(ev_rx),
        }
    }

    /// Take the notification stream. Returns the receiver once; later calls return
    /// `None`. Drain it concurrently with `command`.
    pub fn events(&mut self) -> Option<mpsc::UnboundedReceiver<Notification>> {
        self.events.take()
    }

    /// Send a raw control-mode command and await tmux's correlated reply. Returns
    /// `Err(CommandError::Disconnected)` once the session has ended.
    pub async fn command(&self, cmd: &str) -> CommandResult {
        let (reply, rx) = oneshot::channel();
        let request = Request {
            cmd: cmd.to_string(),
            reply,
        };
        if self.commands.send(request).is_err() {
            return Err(CommandError::Disconnected);
        }
        rx.await.unwrap_or(Err(CommandError::Disconnected))
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

    /// Override one window's size for this control client, layered over the global
    /// [`Client::resize`]. tmux arbitrates bounds; an out-of-range size surfaces as
    /// [`CommandError::Failed`], not a client-side check.
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

async fn owner_task<R, W>(
    mut reader: R,
    mut writer: W,
    child: Option<Child>,
    mut commands: mpsc::UnboundedReceiver<Request>,
    events: mpsc::UnboundedSender<Notification>,
) where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut engine = Engine::new();
    let mut waiters: HashMap<CommandId, oneshot::Sender<CommandResult>> = HashMap::new();
    let mut buf = [0u8; 8192];

    loop {
        tokio::select! {
            request = commands.recv() => {
                let Some(Request { cmd, reply }) = request else {
                    break; // all client handles dropped
                };
                let id = engine.register_command();
                if write_command(&mut writer, &cmd).await.is_err() {
                    let _ = reply.send(Err(CommandError::Disconnected));
                    break;
                }
                waiters.insert(id, reply);
            }
            read = reader.read(&mut buf) => {
                let n = match read {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                for incoming in engine.feed(&buf[..n]) {
                    match incoming {
                        Incoming::Notification(notification) => {
                            let _ = events.send(notification);
                        }
                        Incoming::Reply { id, result } => resolve(&mut waiters, id, result),
                    }
                }
            }
        }
    }

    // Teardown: detach (best-effort empty line), then fail every outstanding command
    // so awaiting callers unblock, then reap the child.
    let _ = write_command(&mut writer, "").await;
    for incoming in engine.on_eof() {
        if let Incoming::Reply { id, result } = incoming {
            resolve(&mut waiters, id, result);
        }
    }
    if let Some(mut child) = child {
        let _ = child.wait().await;
    }
}

fn resolve(
    waiters: &mut HashMap<CommandId, oneshot::Sender<CommandResult>>,
    id: CommandId,
    result: CommandResult,
) {
    if let Some(waiter) = waiters.remove(&id) {
        let _ = waiter.send(result);
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
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn delivers_notifications_to_events_receiver() {
        let (mut tmux, client_io) = tokio::io::duplex(256);
        let (reader, writer) = tokio::io::split(client_io);
        let mut client = TokioClient::with_transport(reader, writer);
        let mut events = client.events().expect("events receiver");

        tmux.write_all(b"%window-add @5\n").await.expect("write");

        assert_eq!(
            events.recv().await,
            Some(Notification::WindowAdd(WindowId(5)))
        );
    }

    #[tokio::test]
    async fn command_awaits_its_reply() {
        let (mut tmux, client_io) = tokio::io::duplex(256);
        let (reader, writer) = tokio::io::split(client_io);
        let client = TokioClient::with_transport(reader, writer);

        let fake = tokio::spawn(async move {
            let mut buf = [0u8; 256];
            let n = tmux.read(&mut buf).await.expect("read command");
            assert!(buf[..n].starts_with(b"list-panes"));
            tmux.write_all(b"%begin 1 1 1\npane-info\n%end 1 1 1\n")
                .await
                .expect("write reply");
            tmux // keep the transport alive until the reply is read
        });

        let result = client.command("list-panes").await;
        assert_eq!(
            result,
            Ok(CommandOutput {
                lines: vec!["pane-info".to_string()],
            })
        );
        let _tmux = fake.await.expect("fake tmux");
    }

    #[tokio::test]
    async fn send_keys_emits_hex_bytes() {
        let (mut tmux, client_io) = tokio::io::duplex(256);
        let (reader, writer) = tokio::io::split(client_io);
        let client = TokioClient::with_transport(reader, writer);

        let fake = tokio::spawn(async move {
            let mut buf = [0u8; 256];
            let n = tmux.read(&mut buf).await.expect("read command");
            let got = std::str::from_utf8(&buf[..n]).expect("utf8");
            assert_eq!(got.trim_end(), "send-keys -t %1 -H 1b 5b 41");
            tmux.write_all(b"%begin 1 1 1\n%end 1 1 1\n")
                .await
                .expect("write reply");
            tmux
        });

        client
            .send_keys(PaneId(1), &[0x1b, 0x5b, 0x41])
            .await
            .expect("send_keys");
        let _tmux = fake.await.expect("fake tmux");
    }

    #[tokio::test]
    async fn command_after_disconnect_is_disconnected() {
        let (tmux, client_io) = tokio::io::duplex(256);
        let (reader, writer) = tokio::io::split(client_io);
        let client = TokioClient::with_transport(reader, writer);

        drop(tmux); // peer gone — the owner task hits EOF and ends

        assert_eq!(
            client.command("list-panes").await,
            Err(CommandError::Disconnected)
        );
    }
}
