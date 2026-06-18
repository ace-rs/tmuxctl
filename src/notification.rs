//! The typed notification stream — one variant per `%`-prefixed control line.

use crate::ids::{PaneId, SessionId, WindowId};
use crate::layout::Layout;

/// A parsed tmux control-mode notification.
///
/// Output payloads are already octal-decoded ([`crate::decode_output`]); unknown
/// `%…` lines are preserved verbatim in [`Notification::Unknown`] for
/// forward-compatibility rather than dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notification {
    /// `%output %<pane> <data>` — pane output, decoded to raw bytes.
    Output { pane: PaneId, bytes: Vec<u8> },

    /// `%extended-output %<pane> <ms-behind> : <data>` — output under flow control.
    ExtendedOutput {
        pane: PaneId,
        ms_behind: u32,
        bytes: Vec<u8>,
    },

    /// `%layout-change @<win> <layout> …` — a window's layout changed.
    LayoutChange { window: WindowId, layout: Layout },

    /// `%window-add @<win>` — a window was created in the attached session.
    WindowAdd(WindowId),
    /// `%window-close @<win>` — a window was closed.
    WindowClose(WindowId),
    /// `%window-renamed @<win> <name>` — a window was renamed.
    WindowRenamed(WindowId, String),

    /// `%session-changed $<sess> <name>` — the attached session changed.
    SessionChanged(SessionId, String),
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
