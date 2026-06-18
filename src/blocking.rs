//! Blocking driver: a thread-backed [`Client`] over the sans-IO [`Engine`].
//!
//! `tmux -C` is one process and two pipes, so a blocking model is the natural fit
//! (and what the `hangar` consumer uses). A dedicated reader thread pumps the
//! child's stdout through the [`Engine`]: notifications go to a `Receiver` the
//! caller drains, and [`Client::command`] blocks on a per-command channel until its
//! reply is correlated. The transport is injected as boxed `Read`/`Write`, so the
//! driver is testable over an in-memory pipe without spawning tmux.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crate::engine::{CommandError, CommandId, CommandOutput, Engine, Incoming};
use crate::notification::Notification;

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
}

impl Client {
    /// Build a client over an injected transport: the reader half is moved to the
    /// reader thread, the writer half kept for sending commands. `spawn` wraps this
    /// around a real `tmux -C` child; tests wrap it around an in-memory pipe.
    pub fn with_transport(reader: Box<dyn Read + Send>, writer: Box<dyn Write + Send>) -> Client {
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
            Ok(0) | Err(_) => break,
            Ok(n) => dispatch(shared, events, &buf[..n]),
        }
    }
    disconnect(shared);
}

fn dispatch(shared: &Arc<Mutex<Shared>>, events: &Sender<Notification>, bytes: &[u8]) {
    let mut shared = shared.lock().expect("driver mutex poisoned");
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
    let mut shared = shared.lock().expect("driver mutex poisoned");
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
    use std::os::unix::net::UnixStream;

    /// Split one end of a socket pair into the boxed reader/writer a `Client` wants.
    fn client_over(end: UnixStream) -> (Box<dyn Read + Send>, Box<dyn Write + Send>) {
        let reader = Box::new(end.try_clone().expect("clone socket")) as Box<dyn Read + Send>;
        let writer = Box::new(end) as Box<dyn Write + Send>;
        (reader, writer)
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
    fn events_receiver_closes_on_disconnect() {
        let (tmux, client_io) = UnixStream::pair().expect("socket pair");
        let (reader, writer) = client_over(client_io);
        let mut client = Client::with_transport(reader, writer);
        let events = client.events().expect("events receiver");

        drop(tmux); // peer gone → reader EOF → thread ends → events sender dropped

        assert!(events.recv().is_err());
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
