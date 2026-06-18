//! The sans-IO protocol engine: line stream in, correlated outcomes out.
//!
//! [`Engine`] owns the line [`Parser`] and the outstanding-command queue. It does
//! no I/O and holds no runtime — a driver feeds it lines and reads back
//! [`Incoming`] values, layering the spawn/read/write and the per-command
//! handoff on top. This keeps the protocol logic pure and unit-testable by
//! feeding bytes directly, with no process and no executor.
//!
//! Correlation is **FIFO**: tmux runs the command queue serially, so reply blocks
//! arrive in the order commands were sent. The driver must therefore call
//! [`Engine::register_command`] in that same send order. Only *control* replies
//! (the `%begin` flags field set — see [`Reply::control`]) consume the queue; a
//! server-internal block is passed over so it cannot desync correlation.

use std::collections::VecDeque;

use thiserror::Error;

use crate::notification::Notification;
use crate::parser::{Event, Parser, Reply};

/// An opaque handle correlating a sent command to its eventual reply. A driver
/// tags each outstanding command with the id from [`Engine::register_command`]
/// and matches it against [`Incoming::Reply`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CommandId(u64);

/// A command's `%end` reply: its (possibly empty) output lines.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct CommandOutput {
    pub lines: Vec<String>,
}

/// Why a command did not return successful output.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum CommandError {
    /// tmux replied `%error` — its failure message lines.
    #[error("tmux command failed: {}", .lines.join("; "))]
    Failed { lines: Vec<String> },
    /// The control session ended (pipe EOF / `%exit`) before the reply arrived.
    #[error("control session disconnected before reply")]
    Disconnected,
}

/// A correlated outcome the engine surfaces from one input line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incoming {
    /// An asynchronous notification, unrelated to any command.
    Notification(Notification),
    /// A reply block correlated to the command that produced it.
    Reply {
        id: CommandId,
        result: Result<CommandOutput, CommandError>,
    },
}

/// The sans-IO protocol core: parser + outstanding-command FIFO + framing buffer.
#[derive(Debug, Default)]
pub struct Engine {
    parser: Parser,
    pending: VecDeque<CommandId>,
    next_id: u64,
    buf: Vec<u8>,
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a command as sent and return its correlation id. Call this in the
    /// exact order commands are written to tmux — correlation is FIFO.
    pub fn register_command(&mut self) -> CommandId {
        let id = CommandId(self.next_id);
        self.next_id += 1;
        self.pending.push_back(id);
        id
    }

    /// Feed a raw chunk of the tmux byte stream. Frames complete lines on `\n`,
    /// buffering any partial trailing line until the next call, and returns the
    /// correlated outcomes in order. This is the driver's entry point — it owns the
    /// `read()` loop and hands chunks here; empty lines are skipped.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Incoming> {
        self.buf.extend_from_slice(bytes);

        let mut out = Vec::new();
        while let Some(newline) = self.buf.iter().position(|&b| b == b'\n') {
            let mut line: Vec<u8> = self.buf.drain(..=newline).collect();
            line.pop(); // drop the trailing '\n'
            // Empty lines pass through: a blank line *inside* a reply block is real
            // command output. Only a stray top-level blank is dropped — by the parser,
            // which alone knows the block-vs-top-level context.
            if let Some(incoming) = self.on_line(&line) {
                out.push(incoming);
            }
        }
        out
    }

    /// Feed one already-framed line (newline stripped). Returns an [`Incoming`] when
    /// a notification parses or a reply block closes; `None` while buffering a
    /// block's content, for a server-internal reply, or for a control reply with no
    /// outstanding command to correlate it to.
    pub fn on_line(&mut self, line: &[u8]) -> Option<Incoming> {
        match self.parser.push(line)? {
            Event::Notification(notification) => Some(Incoming::Notification(notification)),
            Event::Reply(reply) => self.correlate(reply),
        }
    }

    fn correlate(&mut self, reply: Reply) -> Option<Incoming> {
        if !reply.control {
            return None;
        }

        let id = self.pending.pop_front()?;
        let result = match reply.error {
            false => Ok(CommandOutput {
                lines: reply.output,
            }),
            true => Err(CommandError::Failed {
                lines: reply.output,
            }),
        };
        Some(Incoming::Reply { id, result })
    }

    /// Signal that the control stream ended (pipe EOF). Drains every outstanding
    /// command, resolving each as `Err(CommandError::Disconnected)` so a blocking
    /// `command()` caller unblocks instead of hanging when tmux exits. Leaves the
    /// engine empty; the driver stops feeding after this.
    pub fn on_eof(&mut self) -> Vec<Incoming> {
        self.pending
            .drain(..)
            .map(|id| Incoming::Reply {
                id,
                result: Err(CommandError::Disconnected),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::WindowId;

    #[test]
    fn notification_passes_through() {
        let mut engine = Engine::new();
        let incoming = engine.on_line(b"%sessions-changed");
        assert_eq!(
            incoming,
            Some(Incoming::Notification(Notification::SessionsChanged))
        );
    }

    #[test]
    fn correlates_successful_reply() {
        let mut engine = Engine::new();
        let id = engine.register_command();

        assert_eq!(engine.on_line(b"%begin 1 10 1"), None);
        assert_eq!(engine.on_line(b"out"), None);
        let done = engine.on_line(b"%end 1 10 1");

        assert_eq!(
            done,
            Some(Incoming::Reply {
                id,
                result: Ok(CommandOutput {
                    lines: vec!["out".to_string()],
                }),
            })
        );
    }

    #[test]
    fn error_reply_resolves_to_err() {
        let mut engine = Engine::new();
        let id = engine.register_command();

        engine.on_line(b"%begin 1 11 1");
        engine.on_line(b"no such window");
        let done = engine.on_line(b"%error 1 11 1");

        assert_eq!(
            done,
            Some(Incoming::Reply {
                id,
                result: Err(CommandError::Failed {
                    lines: vec!["no such window".to_string()],
                }),
            })
        );
    }

    #[test]
    fn correlates_two_commands_in_fifo_order() {
        let mut engine = Engine::new();
        let first = engine.register_command();
        let second = engine.register_command();

        engine.on_line(b"%begin 1 20 1");
        let a = engine.on_line(b"%end 1 20 1");
        engine.on_line(b"%begin 1 21 1");
        let b = engine.on_line(b"%end 1 21 1");

        assert!(matches!(a, Some(Incoming::Reply { id, .. }) if id == first));
        assert!(matches!(b, Some(Incoming::Reply { id, .. }) if id == second));
    }

    #[test]
    fn server_internal_reply_does_not_consume_pending() {
        // A flags=0 (server-internal) block arrives before our reply. It must not
        // pop the FIFO, so our command still correlates to its own id.
        let mut engine = Engine::new();
        let id = engine.register_command();

        engine.on_line(b"%begin 1 30 0");
        let internal = engine.on_line(b"%end 1 30 0");
        assert_eq!(internal, None);

        engine.on_line(b"%begin 1 31 1");
        let ours = engine.on_line(b"%end 1 31 1");
        assert!(matches!(ours, Some(Incoming::Reply { id: got, .. }) if got == id));
    }

    #[test]
    fn notification_interleaves_before_a_reply() {
        let mut engine = Engine::new();
        let id = engine.register_command();

        let note = engine.on_line(b"%window-add @3");
        assert_eq!(
            note,
            Some(Incoming::Notification(Notification::WindowAdd(WindowId(3))))
        );

        engine.on_line(b"%begin 1 40 1");
        let done = engine.on_line(b"%end 1 40 1");
        assert!(matches!(done, Some(Incoming::Reply { id: got, .. }) if got == id));
    }

    #[test]
    fn control_reply_without_pending_is_dropped() {
        let mut engine = Engine::new();

        engine.on_line(b"%begin 1 50 1");
        let orphan = engine.on_line(b"%end 1 50 1");

        assert_eq!(orphan, None);
    }

    #[test]
    fn on_eof_drains_pending_as_disconnected() {
        let mut engine = Engine::new();
        let first = engine.register_command();
        let second = engine.register_command();

        let drained = engine.on_eof();
        assert_eq!(
            drained,
            vec![
                Incoming::Reply {
                    id: first,
                    result: Err(CommandError::Disconnected),
                },
                Incoming::Reply {
                    id: second,
                    result: Err(CommandError::Disconnected),
                },
            ]
        );
        // Idempotent once drained — nothing left to resolve.
        assert!(engine.on_eof().is_empty());
    }

    #[test]
    fn reply_preserves_interior_blank_lines() {
        // A blank line inside a reply block is real command output — it must survive
        // the framer, which sits beneath block buffering.
        let mut engine = Engine::new();
        let id = engine.register_command();

        let out = engine.feed(b"%begin 1 1 1\na\n\nb\n%end 1 1 1\n");
        assert_eq!(
            out,
            vec![Incoming::Reply {
                id,
                result: Ok(CommandOutput {
                    lines: vec!["a".to_string(), String::new(), "b".to_string()],
                }),
            }]
        );
    }

    #[test]
    fn feed_frames_lines_across_chunk_boundaries() {
        // A notification split mid-line across two feeds, with a raw 0xFF byte in a
        // following %output payload that straddles a third chunk — the framer must
        // reassemble both and preserve the non-UTF-8 byte.
        let mut engine = Engine::new();

        assert!(engine.feed(b"%sessions-chan").is_empty()); // partial line, buffered
        let first = engine.feed(b"ged\n%output %1 ab");
        assert_eq!(
            first,
            vec![Incoming::Notification(Notification::SessionsChanged)]
        );

        let second = engine.feed(&[0xff, b'\n']); // completes the %output line
        assert_eq!(
            second,
            vec![Incoming::Notification(Notification::Output {
                pane: crate::ids::PaneId(1),
                bytes: vec![b'a', b'b', 0xff],
            })]
        );
    }
}
