//! The typed notification stream — one variant per `%`-prefixed control line.

use crate::ids::{PaneId, SessionId, WindowId};
use crate::layout::Layout;

/// A parsed tmux control-mode notification.
///
/// Output payloads are already octal-decoded ([`crate::decode_output`]); unknown
/// `%…` lines are preserved verbatim in [`Notification::Unknown`] for
/// forward-compatibility rather than dropped.
///
/// `#[non_exhaustive]`: tmux's notification surface grows across versions, so
/// consumers must carry a catch-all arm (they already do, for [`Notification::Unknown`])
/// and a new variant in a minor release is not a breaking change.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Notification {
    /// `%output %<pane> <data>` — pane output, decoded to raw bytes.
    Output { pane: PaneId, bytes: Vec<u8> },

    /// `%extended-output %<pane> <ms-behind> : <data>` — output under flow control.
    /// `ms_behind` is tmux's `%llu` age, so `u64` (a `u32` overflows after ~49 days).
    ExtendedOutput {
        pane: PaneId,
        ms_behind: u64,
        bytes: Vec<u8>,
    },

    /// `%layout-change @<win> <layout> [<visible-layout> <flags>]` — a window's
    /// layout changed. `visible_layout` diverges from `layout` under zoom; both it
    /// and `flags` (raw window flags) are absent on tmux too old to report them.
    LayoutChange {
        window: WindowId,
        layout: Layout,
        visible_layout: Option<Layout>,
        flags: Option<WindowFlags>,
    },

    /// `%window-add @<win>` — a window was created in the attached session.
    WindowAdd(WindowId),
    /// `%window-close @<win>` — a window was closed.
    WindowClose(WindowId),
    /// `%window-renamed @<win> <name>` — a window was renamed.
    WindowRenamed(WindowId, String),
    /// `%window-pane-changed @<win> %<pane>` — a window's active pane changed.
    WindowPaneChanged { window: WindowId, pane: PaneId },

    /// `%unlinked-window-add @<win>` — a window was created in *another* session.
    UnlinkedWindowAdd(WindowId),
    /// `%unlinked-window-close @<win>` — an unlinked window was closed.
    UnlinkedWindowClose(WindowId),
    /// `%unlinked-window-renamed @<win> <name>` — an unlinked window was renamed.
    UnlinkedWindowRenamed(WindowId, String),

    /// `%session-changed $<sess> <name>` — the attached session changed.
    SessionChanged(SessionId, String),
    /// `%session-renamed $<sess> <name>` — a session was renamed.
    SessionRenamed(SessionId, String),
    /// `%session-window-changed $<sess> @<win>` — a session's active window changed.
    SessionWindowChanged {
        session: SessionId,
        window: WindowId,
    },
    /// `%client-session-changed <client> $<sess> <name>` — another client's session
    /// changed (`<client>` is the client name, e.g. its tty path — no sigil).
    ClientSessionChanged {
        client: String,
        session: SessionId,
        name: String,
    },
    /// `%sessions-changed` — a session was created or destroyed.
    SessionsChanged,

    /// `%pane-mode-changed %<pane>` — a pane entered or left a mode (copy, etc.).
    PaneModeChanged(PaneId),

    /// `%pause %<pane>` — flow control paused this pane.
    Pause(PaneId),
    /// `%continue %<pane>` — flow control resumed.
    Continue(PaneId),

    /// `%subscription-changed <name> …` — a format subscription pushed a value.
    SubscriptionChanged { name: String, value: String },

    /// `%exit [<reason>]` — the control session is ending.
    Exit(Option<String>),

    /// An unrecognized `%…` line, kept verbatim. Log and skip.
    Unknown(String),
}

/// tmux's window flags, the raw-flags field of `%layout-change`
/// (`window_printable_flags` in tmux `window.c`). Each known flag is a named field;
/// any character tmux adds that we don't model yet is retained in `unknown`, so the
/// set is forward-compatible and nothing is dropped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct WindowFlags {
    /// `*` — the session's current window.
    pub current: bool,
    /// `-` — the session's last (previously current) window.
    pub last: bool,
    /// `#` — activity in the window.
    pub activity: bool,
    /// `!` — a bell rang in the window.
    pub bell: bool,
    /// `~` — the window has been silent (silence alert).
    pub silence: bool,
    /// `M` — the window contains the marked pane.
    pub marked: bool,
    /// `Z` — the window's active pane is zoomed.
    pub zoomed: bool,
    /// Any flag characters tmux emitted that this version does not model.
    pub unknown: String,
}
