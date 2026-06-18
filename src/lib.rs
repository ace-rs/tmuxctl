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
//! Early. The value types ([`PaneId`], [`Notification`], [`Layout`]), the pure
//! helpers ([`decode_output`], [`layout::checksum`], [`Layout::parse`]), and the
//! synchronous line [`Parser`] (framing + `%begin`/`%end` reply blocks) are in
//! place and tested. The async `Client` (spawn `tmux -C`, drive the parser over
//! tokio pipes, correlate replies to futures) is the next slice.

mod engine;
mod error;
mod ids;
mod notification;
mod output;

pub mod layout;
pub mod parser;

pub use engine::{CommandError, CommandId, CommandOutput, Engine, Incoming};
pub use error::{Error, Result};
pub use ids::{PaneId, SessionId, WindowId};
pub use layout::Layout;
pub use notification::Notification;
pub use output::decode_output;
pub use parser::{Event, Parser, Reply};
