#![deny(warnings)]
//! `tmuxctl` — a bidirectional tmux **control-mode** (`tmux -C`) client.
//!
//! This crate is the wire between a Rust front-end and a tmux server: it spawns
//! `tmux -C`, parses the `%`-prefixed notification stream into typed
//! [`Notification`]s, correlates command replies by command-number, octal-decodes
//! pane output to raw bytes, and models tmux's layout tree ([`Layout`]).
//!
//! It is a **protocol layer only** — no terminal emulation, no rendering, no UI.
//! Those are the consumer's job (see [`hangar`]).
//!
//! The protocol contract lives in `docs/spec/overview.md`; a map of the tmux C
//! source that backs each wire detail lives in `docs/reference/tmux-source-map.md`.
//!
//! [`hangar`]: https://github.com/ace-rs/hangar
//!
//! # Status
//!
//! Pre-implementation. The value types ([`PaneId`], [`Notification`], [`Layout`])
//! and the fully-specified pure helpers ([`decode_output`], [`layout::checksum`])
//! are in place; the async [`Client`](client) and the line parser are the next
//! slices.

mod error;
mod ids;
mod notification;
mod output;

pub mod layout;

pub use error::{Error, Result};
pub use ids::{PaneId, SessionId, WindowId};
pub use layout::Layout;
pub use notification::Notification;
pub use output::decode_output;
