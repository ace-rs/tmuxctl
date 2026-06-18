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
use crate::notification::{Notification, WindowFlags};
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
    /// `true` when the `%begin` flags field is set — a reply to a command *we* sent
    /// over the control channel, not a server-internal command whose output tmux
    /// echoed to us. Reply correlation must consume only control replies, else a
    /// server-internal block would desync the command FIFO.
    pub control: bool,
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
    control: bool,
    output: Vec<String>,
}

impl Parser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one line (the framing `\n` already stripped). Returns an [`Event`] when
    /// a notification is parsed or a reply block closes; `None` while buffering a
    /// block's content or opening a new block.
    ///
    /// `%output` / `%extended-output` carry raw bytes (`>= 0x80` verbatim) and only
    /// ever appear at top level, so they are decoded on the byte path before any
    /// UTF-8 conversion. Every other line — guards, the text notifications, reply
    /// content — is treated as UTF-8 (lossily, so a stray non-UTF-8 line degrades to
    /// `Notification::Unknown` rather than panicking).
    pub fn push(&mut self, line: &[u8]) -> Option<Event> {
        if self.pending.is_none()
            && let Some(notification) = parse_output_line(line)
        {
            return Some(Event::Notification(notification));
        }

        let text = String::from_utf8_lossy(line);
        match self.pending.is_some() {
            true => self.push_within_block(&text),
            false => self.push_at_top_level(&text),
        }
    }

    fn push_at_top_level(&mut self, line: &str) -> Option<Event> {
        if line.is_empty() {
            return None; // stray blank line between notifications — not an `Unknown`
        }
        if let Some(Guard {
            kind: GuardKind::Begin,
            number,
            control,
        }) = parse_guard(line)
        {
            self.pending = Some(PendingReply {
                number,
                control,
                output: Vec::new(),
            });
            return None;
        }
        Some(Event::Notification(parse_notification(line)))
    }

    fn push_within_block(&mut self, line: &str) -> Option<Event> {
        match parse_guard(line) {
            Some(Guard {
                kind: GuardKind::End,
                ..
            }) => self.close_block(false),
            Some(Guard {
                kind: GuardKind::Error,
                ..
            }) => self.close_block(true),
            Some(Guard {
                kind: GuardKind::Begin,
                number,
                control,
            }) => {
                // A `%begin` while a block is open means the prior `%end` was lost
                // (tmux never nests blocks). Flush the truncated block as an error
                // reply — so its command fails fast and the FIFO stays aligned —
                // rather than buffering the rest of the stream forever, then open
                // the new block.
                let truncated = self.close_block(true);
                self.pending = Some(PendingReply {
                    number,
                    control,
                    output: Vec::new(),
                });
                truncated
            }
            None => {
                // Content line — buffer verbatim, even if it looks like a `%`-line.
                if let Some(pending) = self.pending.as_mut() {
                    pending.output.push(line.to_string());
                }
                None
            }
        }
    }

    fn close_block(&mut self, error: bool) -> Option<Event> {
        let pending = self.pending.take()?;
        Some(Event::Reply(Reply {
            number: pending.number,
            control: pending.control,
            output: pending.output,
            error,
        }))
    }
}

struct Guard {
    kind: GuardKind,
    number: u64,
    control: bool,
}

enum GuardKind {
    Begin,
    End,
    Error,
}

/// Recognize a `%begin`/`%end`/`%error <ts> <number> <flags>` guard line and pull
/// its command-number and the control flag (`flags != 0`).
fn parse_guard(line: &str) -> Option<Guard> {
    let rest = line.strip_prefix('%')?;
    let mut parts = rest.split(' ');

    let kind = match parts.next()? {
        "begin" => GuardKind::Begin,
        "end" => GuardKind::End,
        "error" => GuardKind::Error,
        _ => return None,
    };
    let _timestamp = parts.next()?;
    let number: u64 = parts.next()?.parse().ok()?;
    let control = parts.next()?.parse::<u32>().ok()? != 0;
    Some(Guard {
        kind,
        number,
        control,
    })
}

fn parse_notification(line: &str) -> Notification {
    let unknown = || Notification::Unknown(line.to_string());

    let Some(rest) = line.strip_prefix('%') else {
        return unknown();
    };
    let (kind, args) = rest.split_once(' ').unwrap_or((rest, ""));

    let parsed = match kind {
        "layout-change" => parse_layout_change(args),

        "window-add" => first_window(args).map(Notification::WindowAdd),
        "window-close" => first_window(args).map(Notification::WindowClose),
        "window-renamed" => parse_id_name(args, window_id, Notification::WindowRenamed),
        "window-pane-changed" => parse_window_pane_changed(args),
        "unlinked-window-add" => first_window(args).map(Notification::UnlinkedWindowAdd),
        "unlinked-window-close" => first_window(args).map(Notification::UnlinkedWindowClose),
        "unlinked-window-renamed" => {
            parse_id_name(args, window_id, Notification::UnlinkedWindowRenamed)
        }

        "session-changed" => parse_id_name(args, session_id, Notification::SessionChanged),
        "session-renamed" => parse_id_name(args, session_id, Notification::SessionRenamed),
        "session-window-changed" => parse_session_window_changed(args),
        "client-session-changed" => parse_client_session_changed(args),
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

// The byte path: `%output` / `%extended-output` are the only lines carrying raw
// (possibly non-UTF-8) pane bytes. Returns `Some` for an output line — including
// `Some(Unknown)` for a malformed one, so it never falls through to the text path —
// and `None` for any other keyword, which the text path then handles.
fn parse_output_line(line: &[u8]) -> Option<Notification> {
    let unknown = || Notification::Unknown(String::from_utf8_lossy(line).into_owned());

    if let Some(rest) = line.strip_prefix(b"%output ") {
        return Some(parse_output(rest).unwrap_or_else(unknown));
    }
    if let Some(rest) = line.strip_prefix(b"%extended-output ") {
        return Some(parse_extended_output(rest).unwrap_or_else(unknown));
    }
    None
}

// `%output %<pane> <data>` (prefix already stripped) — `<data>` decoded as raw bytes.
fn parse_output(rest: &[u8]) -> Option<Notification> {
    let (pane, data) = match rest.iter().position(|&b| b == b' ') {
        Some(sp) => (&rest[..sp], &rest[sp + 1..]),
        None => (rest, &b""[..]),
    };
    Some(Notification::Output {
        pane: pane_id_bytes(pane)?,
        bytes: decode_output(data),
    })
}

// `%extended-output %<pane> <ms-behind> : <data>` (prefix already stripped).
fn parse_extended_output(rest: &[u8]) -> Option<Notification> {
    let space = rest.iter().position(|&b| b == b' ')?;
    let pane = pane_id_bytes(&rest[..space])?;
    let tail = &rest[space + 1..];

    let sep = find_subslice(tail, b" : ")?;
    let ms_behind: u64 = std::str::from_utf8(&tail[..sep]).ok()?.parse().ok()?;
    let data = &tail[sep + 3..];
    Some(Notification::ExtendedOutput {
        pane,
        ms_behind,
        bytes: decode_output(data),
    })
}

// A pane id from raw bytes: `%<digits>` (ASCII).
fn pane_id_bytes(token: &[u8]) -> Option<PaneId> {
    let digits = token.strip_prefix(b"%")?;
    std::str::from_utf8(digits).ok()?.parse().ok().map(PaneId)
}

// Index of the first occurrence of `needle` in `haystack`.
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

// `%layout-change @<win> <layout> [<visible-layout> <flags>]`. Layout strings hold
// no spaces, so a plain space split cleanly separates the up-to-four fields. An
// unparseable visible-layout fails the whole line to `Unknown` so drift surfaces.
fn parse_layout_change(args: &str) -> Option<Notification> {
    let mut parts = args.split(' ');
    let window = window_id(parts.next()?)?;
    let layout = Layout::parse(parts.next()?).ok()?;
    let visible_layout = parts.next().map(Layout::parse).transpose().ok()?;
    let flags = parts.next().map(parse_window_flags);
    Some(Notification::LayoutChange {
        window,
        layout,
        visible_layout,
        flags,
    })
}

// The raw window-flags field of `%layout-change` — see `window_printable_flags`
// in tmux `window.c`. Unrecognized characters are retained, never dropped.
fn parse_window_flags(field: &str) -> WindowFlags {
    let mut flags = WindowFlags::default();
    for ch in field.chars() {
        match ch {
            '*' => flags.current = true,
            '-' => flags.last = true,
            '#' => flags.activity = true,
            '!' => flags.bell = true,
            '~' => flags.silence = true,
            'M' => flags.marked = true,
            'Z' => flags.zoomed = true,
            other => flags.unknown.push(other),
        }
    }
    flags
}

// `%window-pane-changed @<win> %<pane>`
fn parse_window_pane_changed(args: &str) -> Option<Notification> {
    let (window, pane) = args.split_once(' ')?;
    Some(Notification::WindowPaneChanged {
        window: window_id(window)?,
        pane: pane_id(pane)?,
    })
}

// `%session-window-changed $<sess> @<win>`
fn parse_session_window_changed(args: &str) -> Option<Notification> {
    let (session, window) = args.split_once(' ')?;
    Some(Notification::SessionWindowChanged {
        session: session_id(session)?,
        window: window_id(window)?,
    })
}

// `%client-session-changed <client> $<sess> <name>`
fn parse_client_session_changed(args: &str) -> Option<Notification> {
    let (client, rest) = args.split_once(' ')?;
    let (session, name) = rest.split_once(' ')?;
    Some(Notification::ClientSessionChanged {
        client: client.to_string(),
        session: session_id(session)?,
        name: name.to_string(),
    })
}

// The `<id> <name>` shape shared by the `*-renamed` and `session-changed` lines:
// parse the sigil'd id, take the remainder as the name, build the variant.
fn parse_id_name<I>(
    args: &str,
    parse_id: fn(&str) -> Option<I>,
    build: fn(I, String) -> Notification,
) -> Option<Notification> {
    let (id, name) = args.split_once(' ')?;
    Some(build(parse_id(id)?, name.to_string()))
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
        lines
            .iter()
            .filter_map(|l| parser.push(l.as_bytes()))
            .collect()
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
    fn output_preserves_non_utf8_bytes() {
        // A raw 0xFF in the payload is invalid UTF-8 the &str path would mangle;
        // the byte path passes it through verbatim.
        let mut parser = Parser::new();
        let mut line = b"%output %1 ".to_vec();
        line.extend_from_slice(&[0xff, b'e', b'n', b'd']);

        assert_eq!(
            parser.push(&line),
            Some(Event::Notification(Notification::Output {
                pane: PaneId(1),
                bytes: vec![0xff, b'e', b'n', b'd'],
            }))
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
                control: true,
                output: vec!["line one".to_string(), "line two".to_string()],
                error: false,
            })]
        );
    }

    #[test]
    fn reply_carries_control_flag() {
        // flags=1 → reply to our control-channel command; flags=0 → server-internal
        // command output echoed to us, which correlation must not consume.
        let ours = drain(&["%begin 1 5 1", "ok", "%end 1 5 1"]);
        let theirs = drain(&["%begin 1 6 0", "internal", "%end 1 6 0"]);

        let Event::Reply(ours) = &ours[0] else {
            panic!("expected a reply");
        };
        let Event::Reply(theirs) = &theirs[0] else {
            panic!("expected a reply");
        };
        assert!(ours.control);
        assert!(!theirs.control);
    }

    #[test]
    fn dropped_end_recovers_on_next_begin() {
        // The first block never sees its %end; the next %begin flushes it as an
        // error reply (bounded), and the second block closes normally.
        let events = drain(&[
            "%begin 1 1 1",
            "partial-a",
            "%begin 1 2 1",
            "full-b",
            "%end 1 2 1",
        ]);
        assert_eq!(events.len(), 2);

        let Event::Reply(truncated) = &events[0] else {
            panic!("expected the truncated reply");
        };
        assert!(truncated.error);
        assert_eq!(truncated.number, 1);
        assert_eq!(truncated.output, vec!["partial-a".to_string()]);

        let Event::Reply(complete) = &events[1] else {
            panic!("expected the complete reply");
        };
        assert!(!complete.error);
        assert_eq!(complete.number, 2);
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
    fn parses_window_pane_changed() {
        let events = drain(&["%window-pane-changed @2 %5"]);
        assert_eq!(
            events,
            vec![Event::Notification(Notification::WindowPaneChanged {
                window: WindowId(2),
                pane: PaneId(5),
            })]
        );
    }

    #[test]
    fn parses_unlinked_window_lines() {
        let events = drain(&[
            "%unlinked-window-add @7",
            "%unlinked-window-close @7",
            "%unlinked-window-renamed @7 other",
        ]);
        assert_eq!(
            events,
            vec![
                Event::Notification(Notification::UnlinkedWindowAdd(WindowId(7))),
                Event::Notification(Notification::UnlinkedWindowClose(WindowId(7))),
                Event::Notification(Notification::UnlinkedWindowRenamed(
                    WindowId(7),
                    "other".to_string()
                )),
            ]
        );
    }

    #[test]
    fn parses_session_renamed_and_window_changed() {
        let events = drain(&["%session-renamed $1 work", "%session-window-changed $1 @4"]);
        assert_eq!(
            events,
            vec![
                Event::Notification(Notification::SessionRenamed(
                    SessionId(1),
                    "work".to_string()
                )),
                Event::Notification(Notification::SessionWindowChanged {
                    session: SessionId(1),
                    window: WindowId(4),
                }),
            ]
        );
    }

    #[test]
    fn parses_client_session_changed() {
        let events = drain(&["%client-session-changed /dev/ttys003 $2 main"]);
        assert_eq!(
            events,
            vec![Event::Notification(Notification::ClientSessionChanged {
                client: "/dev/ttys003".to_string(),
                session: SessionId(2),
                name: "main".to_string(),
            })]
        );
    }

    #[test]
    fn layout_change_carries_visible_layout_and_flags() {
        // Bare (checksum-less) layouts so the test needn't precompute checksums.
        let events =
            drain(&["%layout-change @1 159x48,0,0{79x48,0,0,0,79x48,80,0,1} 159x48,0,0,0 *Z"]);
        let Event::Notification(Notification::LayoutChange {
            window,
            visible_layout,
            flags,
            ..
        }) = &events[0]
        else {
            panic!("expected a layout change");
        };
        assert_eq!(*window, WindowId(1));
        assert!(visible_layout.is_some());
        assert_eq!(
            flags,
            &Some(WindowFlags {
                current: true,
                zoomed: true,
                ..Default::default()
            })
        );
    }

    #[test]
    fn window_flags_retain_unknown_chars() {
        // A flag char this version doesn't model must be kept, not dropped.
        let events = drain(&["%layout-change @0 159x48,0,0,0 159x48,0,0,0 !Q"]);
        let Event::Notification(Notification::LayoutChange { flags, .. }) = &events[0] else {
            panic!("expected a layout change");
        };
        assert_eq!(
            flags,
            &Some(WindowFlags {
                bell: true,
                unknown: "Q".to_string(),
                ..Default::default()
            })
        );
    }

    #[test]
    fn layout_change_without_visible_layout_is_back_compatible() {
        let events = drain(&["%layout-change @0 159x48,0,0{79x48,0,0,0,79x48,80,0,1}"]);
        let Event::Notification(Notification::LayoutChange {
            visible_layout,
            flags,
            ..
        }) = &events[0]
        else {
            panic!("expected a layout change");
        };
        assert!(visible_layout.is_none());
        assert!(flags.is_none());
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
    fn stray_blank_line_is_skipped_not_unknown() {
        assert_eq!(drain(&[""]), vec![]);
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
