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
//! Published. The protocol core — value types ([`PaneId`], [`Notification`],
//! [`Layout`]), pure helpers ([`decode_output`], [`layout::checksum`],
//! [`Layout::parse`]), the line [`Parser`], and the reply-correlation [`Engine`] —
//! is complete and tested. Three feature-gated drivers wrap one core: `Client`
//! (`blocking`), `SmolClient` (`smol`), and `TokioClient` (`tokio`). The protocol is
//! ported against tmux 3.6b.

mod engine;
mod error;
mod ids;
mod notification;
mod output;

#[cfg(any(feature = "blocking", feature = "tokio", feature = "smol"))]
mod commands;
#[cfg(any(feature = "blocking", feature = "tokio", feature = "smol"))]
mod spawn;

#[cfg(feature = "blocking")]
mod blocking;
#[cfg(feature = "smol")]
mod smol_rt;
#[cfg(feature = "tokio")]
mod tokio_rt;

pub mod layout;
pub mod parser;

#[cfg(any(feature = "blocking", feature = "tokio", feature = "smol"))]
pub use spawn::SpawnOpts;

#[cfg(feature = "blocking")]
pub use blocking::Client;
pub use engine::{CommandError, CommandId, CommandOutput, Engine, Incoming};
pub use error::{Error, Result};
pub use ids::{PaneId, SessionId, WindowId};
pub use layout::Layout;
pub use notification::{Notification, WindowFlags};
pub use output::decode_output;
pub use parser::{Event, Parser, Reply};
#[cfg(feature = "smol")]
pub use smol_rt::SmolClient;
#[cfg(feature = "tokio")]
pub use tokio_rt::TokioClient;
