use thiserror::Error;

/// Crate-wide result alias.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors from the pure core. Two failure kinds live in their own types rather than
/// here: a command's `%error` reply is [`crate::CommandError`], and session teardown
/// is [`crate::Notification::Exit`] — drivers own their own `std::io` errors. So the
/// core's only fallible operation is layout parsing. `#[non_exhaustive]` keeps adding
/// a future variant non-breaking.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// A layout string did not match `CHECKSUM,WxH,x,y<tree>`.
    #[error("malformed layout string: {0}")]
    Layout(String),
}
