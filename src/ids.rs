//! The three wire identifiers tmux uses, each with its sigil baked into `Display`.

/// Declare a `u32` newtype whose `Display` prepends tmux's wire sigil.
macro_rules! wire_id {
    ($(#[$meta:meta])* $name:ident, $sigil:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub u32);

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, concat!($sigil, "{}"), self.0)
            }
        }
    };
}

wire_id!(
    /// A tmux pane id — `%<n>` on the wire.
    PaneId, "%"
);
wire_id!(
    /// A tmux window id — `@<n>` on the wire.
    WindowId, "@"
);
wire_id!(
    /// A tmux session id — `$<n>` on the wire.
    SessionId, "$"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_prepends_sigil() {
        assert_eq!(PaneId(3).to_string(), "%3");
        assert_eq!(WindowId(0).to_string(), "@0");
        assert_eq!(SessionId(12).to_string(), "$12");
    }
}
