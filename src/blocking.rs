//! Blocking driver: a thread-backed [`Client`] over the sans-IO [`Engine`].
//!
//! `tmux -C` is one process and two pipes, so a blocking model is the natural fit
//! (and what the `hangar` consumer uses). A dedicated reader thread pumps the
//! child's stdout through the [`Engine`]: notifications go to a `Receiver` the
//! caller drains, and [`Client::command`] blocks on a per-command channel until its
//! reply is correlated. The transport is injected as boxed `Read`/`Write`, so the
//! driver is testable over an in-memory pipe without spawning tmux.

use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crate::commands;
use crate::engine::{CommandError, CommandId, CommandOutput, Engine, Incoming};
use crate::ids::{PaneId, WindowId};
use crate::layout::Layout;
use crate::notification::Notification;
use crate::spawn::SpawnOpts;

/// The outcome of a command: tmux's output, or why it failed.
type CommandResult = Result<CommandOutput, CommandError>;

/// State shared between the public `Client` and its reader thread, behind one lock
/// so that registering a command and writing its bytes stay atomic — correlation is
/// FIFO, so register order must equal write order.
struct Shared {
    engine: Engine,
    writer: Box<dyn Write + Send>,
    waiters: HashMap<CommandId, Sender<CommandResult>>,
    connected: bool,
}

impl Shared {
    /// Hand a correlated reply to whoever is blocked in `command` for it.
    fn resolve(&mut self, id: CommandId, result: CommandResult) {
        if let Some(waiter) = self.waiters.remove(&id) {
            let _ = waiter.send(result);
        }
    }
}

/// A blocking tmux control-mode client.
pub struct Client {
    shared: Arc<Mutex<Shared>>,
    events: Option<Receiver<Notification>>,
    reader: Option<JoinHandle<()>>,
    child: Option<Child>,
}

impl Client {
    /// Spawn `tmux -C` (control mode — never `-CC`) over piped stdin/stdout and wrap
    /// it. The empty input line that [`Drop`] writes detaches the control client, so
    /// the child exits and is reaped.
    pub fn spawn(opts: SpawnOpts) -> std::io::Result<Client> {
        let mut command = Command::new(&opts.program);
        command
            .args(opts.argv())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut child = command.spawn()?;
        let stdout = child.stdout.take().expect("piped stdout");
        let stdin = child.stdin.take().expect("piped stdin");
        Ok(Self::from_parts(
            Box::new(stdout),
            Box::new(stdin),
            Some(child),
        ))
    }

    /// Build a client over an injected transport, no child process. `spawn` is the
    /// real entry point; this is the seam tests wrap around an in-memory pipe.
    pub fn with_transport(reader: Box<dyn Read + Send>, writer: Box<dyn Write + Send>) -> Client {
        Self::from_parts(reader, writer, None)
    }

    fn from_parts(
        reader: Box<dyn Read + Send>,
        writer: Box<dyn Write + Send>,
        child: Option<Child>,
    ) -> Client {
        let shared = Arc::new(Mutex::new(Shared {
            engine: Engine::new(),
            writer,
            waiters: HashMap::new(),
            connected: true,
        }));
        let (events_tx, events_rx) = mpsc::channel();

        let thread_shared = Arc::clone(&shared);
        let reader = thread::spawn(move || read_loop(reader, &thread_shared, &events_tx));

        Client {
            shared,
            events: Some(events_rx),
            reader: Some(reader),
            child,
        }
    }

    /// Take the notification stream. Returns the `Receiver` once; later calls return
    /// `None`. The caller drains it on a thread of its choosing.
    pub fn events(&mut self) -> Option<Receiver<Notification>> {
        self.events.take()
    }

    /// Send a raw control-mode command and block until tmux's reply is correlated.
    /// Returns `Err(CommandError::Disconnected)` once the session has ended.
    pub fn command(&self, cmd: &str) -> CommandResult {
        let (tx, rx) = mpsc::channel();
        {
            // A poisoned lock means the reader thread panicked — the session is gone,
            // so report disconnect rather than propagating the panic to the caller.
            let Ok(mut shared) = self.shared.lock() else {
                return Err(CommandError::Disconnected);
            };
            if !shared.connected {
                return Err(CommandError::Disconnected);
            }

            let id = shared.engine.register_command();
            if write_command(shared.writer.as_mut(), cmd).is_err() {
                shared.connected = false;
                return Err(CommandError::Disconnected);
            }
            shared.waiters.insert(id, tx);
        }
        rx.recv().unwrap_or(Err(CommandError::Disconnected))
    }

    /// Send raw bytes to a pane as key input (hex byte values — safe for arbitrary
    /// bytes and control sequences, no key-name lookup).
    pub fn send_keys(&self, pane: PaneId, keys: &[u8]) -> Result<(), CommandError> {
        self.command(&commands::send_keys(pane, keys)).map(drop)
    }

    /// Set this control client's size.
    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), CommandError> {
        self.command(&commands::resize(cols, rows)).map(drop)
    }

    /// Override one window's size for this control client, layered over the global
    /// [`Client::resize`]. tmux arbitrates bounds; an out-of-range size surfaces as
    /// [`CommandError::Failed`], not a client-side check.
    pub fn resize_window(
        &self,
        window: WindowId,
        cols: u16,
        rows: u16,
    ) -> Result<(), CommandError> {
        self.command(&commands::resize_window(window, cols, rows))
            .map(drop)
    }

    /// Push a layout onto a window. tmux arbitrates validity; a rejected layout
    /// surfaces as [`CommandError::Failed`], not a client-side check.
    pub fn select_layout(&self, window: WindowId, layout: &Layout) -> Result<(), CommandError> {
        self.command(&commands::select_layout(window, layout))
            .map(drop)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        // An empty line detaches the control client: a tmux (or any transport that
        // honors empty-line detach) exits, the reader hits EOF, and the thread ends —
        // so the join below terminates rather than hanging on a live session.
        if let Ok(mut shared) = self.shared.lock() {
            let _ = shared.writer.write_all(b"\n");
            let _ = shared.writer.flush();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        // Reap the tmux child (it exited on the detach above) so it isn't a zombie.
        if let Some(mut child) = self.child.take() {
            let _ = child.wait();
        }
    }
}

fn write_command(writer: &mut dyn Write, cmd: &str) -> std::io::Result<()> {
    writer.write_all(cmd.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()
}

fn read_loop(
    mut reader: Box<dyn Read + Send>,
    shared: &Arc<Mutex<Shared>>,
    events: &Sender<Notification>,
) {
    let mut buf = [0u8; 8192];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => dispatch(shared, events, &buf[..n]),
            // A signal can interrupt a read mid-syscall; retry rather than mistake it
            // for EOF and tear down a session whose peer is still alive.
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    disconnect(shared);
}

fn dispatch(shared: &Arc<Mutex<Shared>>, events: &Sender<Notification>, bytes: &[u8]) {
    // Recover a poisoned lock rather than panic: the reader thread must keep draining
    // replies and still reach the EOF drain in `disconnect` even if a caller panicked
    // mid-`command`, or blocked waiters would hang forever. Matches `command`'s graceful
    // poison handling — degrade, don't amplify.
    let mut shared = shared
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for incoming in shared.engine.feed(bytes) {
        match incoming {
            Incoming::Notification(notification) => {
                let _ = events.send(notification);
            }
            Incoming::Reply { id, result } => shared.resolve(id, result),
        }
    }
}

fn disconnect(shared: &Arc<Mutex<Shared>>) {
    let mut shared = shared
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    shared.connected = false;
    for incoming in shared.engine.on_eof() {
        if let Incoming::Reply { id, result } = incoming {
            shared.resolve(id, result);
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::ids::WindowId;
    use crate::layout::Layout;
    use std::os::unix::net::UnixStream;

    /// Split one end of a socket pair into the boxed reader/writer a `Client` wants.
    fn client_over(end: UnixStream) -> (Box<dyn Read + Send>, Box<dyn Write + Send>) {
        let reader = Box::new(end.try_clone().expect("clone socket")) as Box<dyn Read + Send>;
        let writer = Box::new(end) as Box<dyn Write + Send>;
        (reader, writer)
    }

    /// A fake tmux: read one command line, assert it equals `expected`, reply `%end`.
    fn fake_tmux_expecting(
        mut sock: UnixStream,
        expected: impl Into<String>,
    ) -> thread::JoinHandle<()> {
        let expected = expected.into();
        thread::spawn(move || {
            let mut buf = [0u8; 256];
            let n = sock.read(&mut buf).expect("read command");
            let got = std::str::from_utf8(&buf[..n]).expect("utf8 command");
            // Reply before asserting: a mismatched command then fails the assertion
            // cleanly (surfaced via join()) instead of hanging the client, which would
            // otherwise block forever waiting for a reply the panicking fake never sent.
            sock.write_all(b"%begin 1 1 1\n%end 1 1 1\n")
                .expect("write reply");
            assert_eq!(got.trim_end(), expected.as_str());
        })
    }

    #[test]
    fn delivers_notifications_to_events_receiver() {
        let (mut tmux, client_io) = UnixStream::pair().expect("socket pair");
        let (reader, writer) = client_over(client_io);
        let mut client = Client::with_transport(reader, writer);
        let events = client.events().expect("events receiver");

        tmux.write_all(b"%window-add @5\n").expect("write");

        assert_eq!(
            events.recv().expect("recv"),
            Notification::WindowAdd(WindowId(5))
        );

        drop(tmux);
    }

    #[test]
    fn command_blocks_until_its_reply() {
        let (tmux, client_io) = UnixStream::pair().expect("socket pair");
        let (reader, writer) = client_over(client_io);
        let client = Client::with_transport(reader, writer);

        // Fake tmux: read the command, answer with a %begin/%end block.
        let mut fake_tmux = tmux.try_clone().expect("clone");
        let fake = thread::spawn(move || {
            let mut buf = [0u8; 256];
            let n = fake_tmux.read(&mut buf).expect("read command");
            assert!(buf[..n].starts_with(b"list-panes"));
            fake_tmux
                .write_all(b"%begin 1 1 1\npane-info\n%end 1 1 1\n")
                .expect("write reply");
        });

        let result = client.command("list-panes");
        assert_eq!(
            result,
            Ok(CommandOutput {
                lines: vec!["pane-info".to_string()],
            })
        );

        fake.join().expect("fake tmux");
        drop(tmux);
    }

    #[test]
    fn send_keys_emits_hex_bytes() {
        let (tmux, client_io) = UnixStream::pair().expect("socket pair");
        let (reader, writer) = client_over(client_io);
        let client = Client::with_transport(reader, writer);

        let fake = fake_tmux_expecting(
            tmux.try_clone().expect("clone"),
            "send-keys -t %1 -H 1b 5b 41",
        );
        client
            .send_keys(PaneId(1), &[0x1b, 0x5b, 0x41])
            .expect("send_keys");

        fake.join().expect("fake tmux");
        drop(tmux);
    }

    #[test]
    fn resize_emits_client_size() {
        let (tmux, client_io) = UnixStream::pair().expect("socket pair");
        let (reader, writer) = client_over(client_io);
        let client = Client::with_transport(reader, writer);

        let fake = fake_tmux_expecting(tmux.try_clone().expect("clone"), "refresh-client -C 80x24");
        client.resize(80, 24).expect("resize");

        fake.join().expect("fake tmux");
        drop(tmux);
    }

    #[test]
    fn resize_window_emits_window_size_override() {
        let (tmux, client_io) = UnixStream::pair().expect("socket pair");
        let (reader, writer) = client_over(client_io);
        let client = Client::with_transport(reader, writer);

        let fake = fake_tmux_expecting(
            tmux.try_clone().expect("clone"),
            "refresh-client -C @2:80x24",
        );
        client
            .resize_window(WindowId(2), 80, 24)
            .expect("resize_window");

        fake.join().expect("fake tmux");
        drop(tmux);
    }

    #[test]
    fn select_layout_emits_layout_string() {
        let (tmux, client_io) = UnixStream::pair().expect("socket pair");
        let (reader, writer) = client_over(client_io);
        let client = Client::with_transport(reader, writer);

        let layout = Layout::parse("159x48,0,0{79x48,0,0,0,79x48,80,0,1}").expect("parse");
        let expected = format!("select-layout -t @2 {}", layout.to_layout_string());
        let fake = fake_tmux_expecting(tmux.try_clone().expect("clone"), expected);
        client
            .select_layout(WindowId(2), &layout)
            .expect("select_layout");

        fake.join().expect("fake tmux");
        drop(tmux);
    }

    #[test]
    fn events_receiver_closes_on_disconnect() {
        let (tmux, client_io) = UnixStream::pair().expect("socket pair");
        let (reader, writer) = client_over(client_io);
        let mut client = Client::with_transport(reader, writer);
        let events = client.events().expect("events receiver");

        drop(tmux); // peer gone → reader EOF → thread ends → events sender dropped

        assert!(events.recv().is_err());
    }

    #[test]
    fn read_interrupted_is_retried_not_disconnected() {
        // A signal-interrupted read must not be mistaken for EOF: the session keeps
        // running and the notification on the next read still arrives.
        struct InterruptThenData {
            calls: usize,
        }
        impl Read for InterruptThenData {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                self.calls += 1;
                match self.calls {
                    1 => Err(std::io::Error::from(std::io::ErrorKind::Interrupted)),
                    2 => {
                        let data = b"%window-add @7\n";
                        buf[..data.len()].copy_from_slice(data);
                        Ok(data.len())
                    }
                    _ => Ok(0),
                }
            }
        }

        let mut client = Client::with_transport(
            Box::new(InterruptThenData { calls: 0 }),
            Box::new(std::io::sink()),
        );
        let events = client.events().expect("events receiver");
        assert_eq!(
            events.recv().expect("recv"),
            Notification::WindowAdd(WindowId(7))
        );
    }

    #[test]
    fn command_after_disconnect_is_disconnected() {
        let (tmux, client_io) = UnixStream::pair().expect("socket pair");
        let (reader, writer) = client_over(client_io);
        let client = Client::with_transport(reader, writer);

        drop(tmux); // disconnect; outcome is Disconnected whether the write fails or
        // the reader's EOF drains the waiter first.
        assert_eq!(
            client.command("list-panes"),
            Err(CommandError::Disconnected)
        );
    }
}
