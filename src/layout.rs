//! tmux window-layout strings: the typed tree and the checksum.
//!
//! Wire form: `CHECKSUM,WxH,x,y<tree>` where a leaf is `WxH,x,y,<pane-id>`, a
//! left-right (horizontal) split wraps children in `{…}`, and a top-bottom
//! (vertical) split wraps them in `[…]`. Parsing and rendering land as the next
//! slice; [`checksum`] is fully specified and implemented now.

use crate::ids::PaneId;

/// A node in tmux's window layout tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Layout {
    /// A single pane: `WxH,x,y,<pane-id>`.
    Leaf {
        w: u16,
        h: u16,
        x: u16,
        y: u16,
        pane: PaneId,
    },
    /// A left-right (horizontal) split, rendered with `{…}`.
    SplitH {
        w: u16,
        h: u16,
        x: u16,
        y: u16,
        children: Vec<Layout>,
    },
    /// A top-bottom (vertical) split, rendered with `[…]`.
    SplitV {
        w: u16,
        h: u16,
        x: u16,
        y: u16,
        children: Vec<Layout>,
    },
}

/// Compute tmux's layout checksum over `layout` — the part of the wire string
/// *after* the leading `CHECKSUM,`. Rendered as four lowercase hex digits.
///
/// Direct port of `layout_checksum` in tmux `layout-custom.c`:
///
/// ```c
/// csum = (csum >> 1) + ((csum & 1) << 15);
/// csum += *layout;
/// ```
pub fn checksum(layout: &str) -> u16 {
    let mut csum: u16 = 0;
    for &c in layout.as_bytes() {
        csum = (csum >> 1) + ((csum & 1) << 15);
        csum = csum.wrapping_add(u16::from(c));
    }
    csum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_matches_tmux_reference() {
        // From the spec: `bb62,159x48,0,0{79x48,0,0,79x48,80,0}` — the checksum
        // is computed over everything after the leading `bb62,`.
        let body = "159x48,0,0{79x48,0,0,79x48,80,0}";
        assert_eq!(format!("{:04x}", checksum(body)), "bb62");
    }
}
