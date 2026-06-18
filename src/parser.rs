//! Incremental line parser for the tmux control-mode stream.
//!
//! Feed [`Parser::push`] one line at a time (newline already stripped). It frames
//! `%begin`…`%end`/`%error` reply blocks — buffering their content lines and
//! surfacing them as a [`Reply`] tagged with the command-number — and decodes
//! every other `%`-line into a [`Notification`]. Reply correlation (matching the
//! number back to the issuing command) is the caller's job; this layer is pure
//! and synchronous so it can be unit-tested against recorded transcripts.

use crate::ids::{PaneId, SessionId, WindowId};
use crate::layout::Layout;
use crate::notification::Notification;
use crate::output::decode_output;

/// One thing the parser surfaces from the line stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// An asynchronous `%`-notification.
    Notification(Notification),
    /// A completed command reply block, to be correlated by `number` upstream.
    Reply(Reply),
}

/// A finished `%begin`…`%end`/`%error` block: the command's output lines, plus
/// whether tmux closed it with `%error`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub number: u64,
    pub output: Vec<String>,
    pub error: bool,
}

/// Stateful line consumer. Holds the in-progress reply block, if any.
#[derive(Debug, Default)]
pub struct Parser {
    pending: Option<PendingReply>,
}

#[derive(Debug)]
struct PendingReply {
    number: u64,
    output: Vec<String>,
}

impl Parser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one line. Returns an [`Event`] when a notification is parsed or a
    /// reply block closes; returns `None` while buffering a block's content or
    /// opening a new block.
    pub fn push(&mut self, line: &str) -> Option<Event> {
        match self.pending.is_some() {
            true => self.push_within_block(line),
            false => self.push_at_top_level(line),
        }
    }

    fn push_at_top_level(&mut self, line: &str) -> Option<Event> {
        if let Some((Guard::Begin, number)) = parse_guard(line) {
            self.pending = Some(PendingReply {
                number,
                output: Vec::new(),
            });
            return None;
        }
        Some(Event::Notification(parse_notification(line)))
    }

    fn push_within_block(&mut self, line: &str) -> Option<Event> {
        let error = match parse_guard(line) {
            Some((Guard::End, _)) => false,
            Some((Guard::Error, _)) => true,
            _ => {
                // Content line — buffer verbatim, even if it looks like a `%`-line.
                if let Some(pending) = self.pending.as_mut() {
                    pending.output.push(line.to_string());
                }
                return None;
            }
        };

        let pending = self.pending.take()?;
        Some(Event::Reply(Reply {
            number: pending.number,
            output: pending.output,
            error,
        }))
    }
}

enum Guard {
    Begin,
    End,
    Error,
}

/// Recognize a `%begin`/`%end`/`%error <ts> <number> <flags>` guard line and
/// pull its command-number.
fn parse_guard(line: &str) -> Option<(Guard, u64)> {
    let rest = line.strip_prefix('%')?;
    let mut parts = rest.split(' ');

    let guard = match parts.next()? {
        "begin" => Guard::Begin,
        "end" => Guard::End,
        "error" => Guard::Error,
        _ => return None,
    };
    let _timestamp = parts.next()?;
    let number: u64 = parts.next()?.parse().ok()?;
    Some((guard, number))
}

fn parse_notification(line: &str) -> Notification {
    let unknown = || Notification::Unknown(line.to_string());

    let Some(rest) = line.strip_prefix('%') else {
        return unknown();
    };
    let (kind, args) = rest.split_once(' ').unwrap_or((rest, ""));

    let parsed = match kind {
        "output" => parse_output(args),
        "extended-output" => parse_extended_output(args),
        "layout-change" => parse_layout_change(args),
        "window-add" => first_window(args).map(Notification::WindowAdd),
        "window-close" => first_window(args).map(Notification::WindowClose),
        "window-renamed" => parse_renamed(args, Renamed::Window),
        "session-changed" => parse_renamed(args, Renamed::Session),
        "sessions-changed" => Some(Notification::SessionsChanged),
        "pane-mode-changed" => first_pane(args).map(Notification::PaneModeChanged),
        "pause" => first_pane(args).map(Notification::Pause),
        "continue" => first_pane(args).map(Notification::Continue),
        "subscription-changed" => parse_subscription(args),
        "exit" => Some(Notification::Exit(optional_reason(args))),
        _ => None,
    };
    parsed.unwrap_or_else(unknown)
}

// `%output %<pane> <data>`
fn parse_output(args: &str) -> Option<Notification> {
    let (pane, data) = args.split_once(' ').unwrap_or((args, ""));
    let pane = pane_id(pane)?;
    Some(Notification::Output {
        pane,
        bytes: decode_output(data),
    })
}

// `%extended-output %<pane> <ms-behind> : <data>`
fn parse_extended_output(args: &str) -> Option<Notification> {
    let (pane, tail) = args.split_once(' ')?;
    let pane = pane_id(pane)?;
    let (age, data) = tail.split_once(" : ")?;
    let ms_behind: u32 = age.parse().ok()?;
    Some(Notification::ExtendedOutput {
        pane,
        ms_behind,
        bytes: decode_output(data),
    })
}

// `%layout-change @<win> <layout> [<visible-layout> <flags>]`
fn parse_layout_change(args: &str) -> Option<Notification> {
    let mut parts = args.split(' ');
    let window = window_id(parts.next()?)?;
    let layout = Layout::parse(parts.next()?).ok()?;
    Some(Notification::LayoutChange { window, layout })
}

enum Renamed {
    Window,
    Session,
}

// `%window-renamed @<win> <name>` / `%session-changed $<sess> <name>`
fn parse_renamed(args: &str, which: Renamed) -> Option<Notification> {
    let (id, name) = args.split_once(' ')?;
    match which {
        Renamed::Window => Some(Notification::WindowRenamed(
            window_id(id)?,
            name.to_string(),
        )),
        Renamed::Session => Some(Notification::SessionChanged(
            session_id(id)?,
            name.to_string(),
        )),
    }
}

// `%subscription-changed <name> <session> <window> <pane> : <value>` — the
// trailer past the ` : ` is the format value; the head token is the name.
fn parse_subscription(args: &str) -> Option<Notification> {
    let name = args.split(' ').next()?.to_string();
    let value = args
        .split_once(" : ")
        .map(|(_, v)| v)
        .unwrap_or("")
        .to_string();
    Some(Notification::SubscriptionChanged { name, value })
}

fn optional_reason(args: &str) -> Option<String> {
    match args.trim() {
        "" => None,
        reason => Some(reason.to_string()),
    }
}

fn first_pane(args: &str) -> Option<PaneId> {
    pane_id(args.split(' ').next()?)
}

fn first_window(args: &str) -> Option<WindowId> {
    window_id(args.split(' ').next()?)
}

fn pane_id(token: &str) -> Option<PaneId> {
    token.strip_prefix('%')?.parse().ok().map(PaneId)
}

fn window_id(token: &str) -> Option<WindowId> {
    token.strip_prefix('@')?.parse().ok().map(WindowId)
}

fn session_id(token: &str) -> Option<SessionId> {
    token.strip_prefix('$')?.parse().ok().map(SessionId)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(lines: &[&str]) -> Vec<Event> {
        let mut parser = Parser::new();
        lines.iter().filter_map(|l| parser.push(l)).collect()
    }

    #[test]
    fn parses_output_with_decoding() {
        let events = drain(&[r"%output %1 hi\033there"]);
        assert_eq!(
            events,
            vec![Event::Notification(Notification::Output {
                pane: PaneId(1),
                bytes: b"hi\x1bthere".to_vec(),
            })]
        );
    }

    #[test]
    fn frames_a_reply_block() {
        let events = drain(&[
            "%begin 1700000000 7 1",
            "line one",
            "line two",
            "%end 1700000000 7 1",
        ]);
        assert_eq!(
            events,
            vec![Event::Reply(Reply {
                number: 7,
                output: vec!["line one".to_string(), "line two".to_string()],
                error: false,
            })]
        );
    }

    #[test]
    fn marks_error_replies() {
        let events = drain(&["%begin 1 9 1", "no such window", "%error 1 9 1"]);
        let Event::Reply(reply) = &events[0] else {
            panic!("expected a reply");
        };
        assert!(reply.error);
        assert_eq!(reply.number, 9);
    }

    #[test]
    fn buffers_percent_lines_inside_a_block() {
        // A content line that looks like a notification must not be parsed as one.
        let events = drain(&["%begin 1 2 1", "%output not-a-real-event", "%end 1 2 1"]);
        let Event::Reply(reply) = &events[0] else {
            panic!("expected a reply");
        };
        assert_eq!(reply.output, vec!["%output not-a-real-event".to_string()]);
    }

    #[test]
    fn notifications_interleave_around_blocks() {
        let events = drain(&[
            "%window-add @3",
            "%begin 1 4 1",
            "ok",
            "%end 1 4 1",
            "%sessions-changed",
        ]);
        assert_eq!(events.len(), 3);
        assert_eq!(
            events[0],
            Event::Notification(Notification::WindowAdd(WindowId(3)))
        );
        assert!(matches!(events[1], Event::Reply(_)));
        assert_eq!(
            events[2],
            Event::Notification(Notification::SessionsChanged)
        );
    }

    #[test]
    fn parses_layout_change() {
        let events = drain(&["%layout-change @0 159x48,0,0{79x48,0,0,0,79x48,80,0,1}"]);
        let Event::Notification(Notification::LayoutChange { window, .. }) = &events[0] else {
            panic!("expected a layout change");
        };
        assert_eq!(*window, WindowId(0));
    }

    #[test]
    fn parses_exit_with_and_without_reason() {
        assert_eq!(
            drain(&["%exit"]),
            vec![Event::Notification(Notification::Exit(None))]
        );
        assert_eq!(
            drain(&["%exit server exited"]),
            vec![Event::Notification(Notification::Exit(Some(
                "server exited".to_string()
            )))]
        );
    }

    #[test]
    fn unknown_lines_are_preserved() {
        let events = drain(&["%made-up-thing whatever"]);
        assert_eq!(
            events,
            vec![Event::Notification(Notification::Unknown(
                "%made-up-thing whatever".to_string()
            ))]
        );
    }
}
