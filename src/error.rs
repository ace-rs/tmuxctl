use thiserror::Error;

/// Crate-wide result alias.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can go wrong talking to a tmux control session.
///
/// One enum for the whole crate — per house style, modules add variants here
/// rather than minting wrapper enums that just re-wrap `io::Error`.
#[derive(Debug, Error)]
pub enum Error {
    /// Underlying I/O on the control pipes failed.
    #[error("control-pipe i/o: {0}")]
    Io(#[from] std::io::Error),

    /// A layout string did not match `CHECKSUM,WxH,x,y<tree>`.
    #[error("malformed layout string: {0}")]
    Layout(String),

    /// tmux replied to a command with `%error`.
    #[error("tmux command failed: {0}")]
    Command(String),

    /// The control session reported `%exit`.
    #[error("control session exited: {}", .0.as_deref().unwrap_or("(no reason)"))]
    Exit(Option<String>),
}
