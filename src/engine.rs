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

/// A command's `%error` reply: tmux's failure message lines.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("tmux command failed: {}", .lines.join("; "))]
#[non_exhaustive]
pub struct CommandError {
    pub lines: Vec<String>,
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

/// The sans-IO protocol core: parser + outstanding-command FIFO.
#[derive(Debug, Default)]
pub struct Engine {
    parser: Parser,
    pending: VecDeque<CommandId>,
    next_id: u64,
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

    /// Feed one control-mode line (newline stripped). Returns an [`Incoming`] when
    /// a notification parses or a reply block closes; `None` while buffering a
    /// block's content, for a server-internal reply, or for a control reply with no
    /// outstanding command to correlate it to.
    pub fn on_line(&mut self, line: &str) -> Option<Incoming> {
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
            true => Err(CommandError {
                lines: reply.output,
            }),
        };
        Some(Incoming::Reply { id, result })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::WindowId;

    #[test]
    fn notification_passes_through() {
        let mut engine = Engine::new();
        let incoming = engine.on_line("%sessions-changed");
        assert_eq!(
            incoming,
            Some(Incoming::Notification(Notification::SessionsChanged))
        );
    }

    #[test]
    fn correlates_successful_reply() {
        let mut engine = Engine::new();
        let id = engine.register_command();

        assert_eq!(engine.on_line("%begin 1 10 1"), None);
        assert_eq!(engine.on_line("out"), None);
        let done = engine.on_line("%end 1 10 1");

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

        engine.on_line("%begin 1 11 1");
        engine.on_line("no such window");
        let done = engine.on_line("%error 1 11 1");

        assert_eq!(
            done,
            Some(Incoming::Reply {
                id,
                result: Err(CommandError {
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

        engine.on_line("%begin 1 20 1");
        let a = engine.on_line("%end 1 20 1");
        engine.on_line("%begin 1 21 1");
        let b = engine.on_line("%end 1 21 1");

        assert!(matches!(a, Some(Incoming::Reply { id, .. }) if id == first));
        assert!(matches!(b, Some(Incoming::Reply { id, .. }) if id == second));
    }

    #[test]
    fn server_internal_reply_does_not_consume_pending() {
        // A flags=0 (server-internal) block arrives before our reply. It must not
        // pop the FIFO, so our command still correlates to its own id.
        let mut engine = Engine::new();
        let id = engine.register_command();

        engine.on_line("%begin 1 30 0");
        let internal = engine.on_line("%end 1 30 0");
        assert_eq!(internal, None);

        engine.on_line("%begin 1 31 1");
        let ours = engine.on_line("%end 1 31 1");
        assert!(matches!(ours, Some(Incoming::Reply { id: got, .. }) if got == id));
    }

    #[test]
    fn notification_interleaves_before_a_reply() {
        let mut engine = Engine::new();
        let id = engine.register_command();

        let note = engine.on_line("%window-add @3");
        assert_eq!(
            note,
            Some(Incoming::Notification(Notification::WindowAdd(WindowId(3))))
        );

        engine.on_line("%begin 1 40 1");
        let done = engine.on_line("%end 1 40 1");
        assert!(matches!(done, Some(Incoming::Reply { id: got, .. }) if got == id));
    }

    #[test]
    fn control_reply_without_pending_is_dropped() {
        let mut engine = Engine::new();

        engine.on_line("%begin 1 50 1");
        let orphan = engine.on_line("%end 1 50 1");

        assert_eq!(orphan, None);
    }
}
