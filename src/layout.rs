//! tmux window-layout strings: the typed tree, the checksum, and parse/render.
//!
//! Wire form: `CHECKSUM,WxH,x,y<tree>` where a leaf is `WxH,x,y,<pane-id>`, a
//! left-right (horizontal) split wraps children in `{…}`, and a top-bottom
//! (vertical) split wraps them in `[…]`. Pane ids in a layout string are bare
//! numbers — no `%` sigil. A border consumes one row/column between children.

use std::fmt::Write as _;

use crate::error::{Error, Result};
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

impl Layout {
    /// Parse a layout string. Accepts the full `CHECKSUM,WxH,x,y…` wire form
    /// (the checksum is verified) or a bare checksum-less tree.
    pub fn parse(s: &str) -> Result<Layout> {
        let (expected, body) = split_checksum(s);
        if let Some(expected) = expected {
            let actual = checksum(body);
            if actual != expected {
                return Err(Error::Layout(format!(
                    "checksum mismatch: computed {actual:04x}, string says {expected:04x}"
                )));
            }
        }

        let (layout, rest) = parse_node(body, 0)?;
        if !rest.is_empty() {
            return Err(Error::Layout(format!(
                "trailing input after layout: {rest:?}"
            )));
        }
        Ok(layout)
    }

    /// Render the bare tree (no checksum prefix).
    pub fn render(&self) -> String {
        let mut out = String::new();
        self.write_tree(&mut out);
        out
    }

    /// Render the full wire string, including the leading 4-hex checksum — the
    /// form `select-layout` expects.
    pub fn to_layout_string(&self) -> String {
        let tree = self.render();
        format!("{:04x},{tree}", checksum(&tree))
    }

    /// The cell geometry `(w, h, x, y)` every variant carries.
    fn cell(&self) -> (u16, u16, u16, u16) {
        match self {
            Layout::Leaf { w, h, x, y, .. }
            | Layout::SplitH { w, h, x, y, .. }
            | Layout::SplitV { w, h, x, y, .. } => (*w, *h, *x, *y),
        }
    }

    fn write_tree(&self, out: &mut String) {
        let (w, h, x, y) = self.cell();
        let _ = write!(out, "{w}x{h},{x},{y}");

        match self {
            Layout::Leaf { pane, .. } => {
                // Bare id, not `PaneId` Display: layout strings carry no `%` sigil, so the
                // `.0` is required — `{pane}` would emit `%N` and corrupt the layout.
                let _ = write!(out, ",{}", pane.0);
            }
            Layout::SplitH { children, .. } => write_children(out, children, '{', '}'),
            Layout::SplitV { children, .. } => write_children(out, children, '[', ']'),
        }
    }
}

fn write_children(out: &mut String, children: &[Layout], open: char, close: char) {
    out.push(open);
    for (i, child) in children.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        child.write_tree(out);
    }
    out.push(close);
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

/// Split a leading `CHECKSUM,` off the string. A checksum is exactly four hex
/// digits and (unlike a `WxH` cell) contains no `x`, so the two never collide.
fn split_checksum(s: &str) -> (Option<u16>, &str) {
    let Some((head, tail)) = s.split_once(',') else {
        return (None, s);
    };

    let looks_like_checksum =
        head.len() == 4 && !head.contains('x') && head.bytes().all(|b| b.is_ascii_hexdigit());
    match looks_like_checksum {
        true => (u16::from_str_radix(head, 16).ok(), tail),
        false => (None, s),
    }
}

/// Maximum split-nesting depth [`Layout::parse`] descends before rejecting input.
/// Recursive descent over an untrusted layout string would otherwise overflow the
/// native stack — an uncatchable process abort — on pathological nesting (`{{{…`).
/// Real tmux layouts nest only a handful deep, so this bound is never hit in practice.
const MAX_LAYOUT_DEPTH: usize = 128;

fn parse_node(s: &str, depth: usize) -> Result<(Layout, &str)> {
    if depth > MAX_LAYOUT_DEPTH {
        return Err(Error::Layout(format!(
            "layout nesting exceeds maximum depth ({MAX_LAYOUT_DEPTH})"
        )));
    }

    let (w, s) = take_number(s, 'x')?;
    let (h, s) = take_number(s, ',')?;
    let (x, s) = take_number(s, ',')?;
    let (y, s, sep) = take_dimension(s)?;

    match sep {
        Some('{') => {
            let (children, s) = parse_children(s, '}', depth + 1)?;
            Ok((
                Layout::SplitH {
                    w,
                    h,
                    x,
                    y,
                    children,
                },
                s,
            ))
        }
        Some('[') => {
            let (children, s) = parse_children(s, ']', depth + 1)?;
            Ok((
                Layout::SplitV {
                    w,
                    h,
                    x,
                    y,
                    children,
                },
                s,
            ))
        }
        Some(',') => {
            let (pane, s) = take_pane(s)?;
            Ok((Layout::Leaf { w, h, x, y, pane }, s))
        }
        _ => Err(Error::Layout(format!(
            "expected pane or split after cell, at {s:?}"
        ))),
    }
}

fn parse_children(mut s: &str, close: char, depth: usize) -> Result<(Vec<Layout>, &str)> {
    let mut children = Vec::new();
    loop {
        let (child, rest) = parse_node(s, depth)?;
        children.push(child);
        s = rest;

        match s.chars().next() {
            Some(c) if c == close => return Ok((children, &s[c.len_utf8()..])),
            Some(',') => s = &s[1..],
            Some(other) => {
                return Err(Error::Layout(format!(
                    "expected ',' or {close:?}, got {other:?}"
                )));
            }
            None => return Err(Error::Layout(format!("unterminated split, want {close:?}"))),
        }
    }
}

/// Parse leading digits as `u16`, require and consume the exact delimiter `delim`.
fn take_number(s: &str, delim: char) -> Result<(u16, &str)> {
    let (value, rest, sep) = take_dimension(s)?;
    match sep {
        Some(c) if c == delim => Ok((value, rest)),
        _ => Err(Error::Layout(format!(
            "expected {delim:?} after number in {s:?}"
        ))),
    }
}

/// Parse leading digits as `u16`; report which delimiter follows (consumed if it
/// is one of `, { [`, else left in place as `None`).
fn take_dimension(s: &str) -> Result<(u16, &str, Option<char>)> {
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    if end == 0 {
        return Err(Error::Layout(format!("expected number at {s:?}")));
    }

    let value: u16 = s[..end]
        .parse()
        .map_err(|_| Error::Layout(format!("number out of range: {:?}", &s[..end])))?;
    let rest = &s[end..];

    let Some(sep) = rest.chars().next() else {
        return Ok((value, rest, None));
    };
    match sep {
        'x' | ',' | '{' | '[' => Ok((value, &rest[sep.len_utf8()..], Some(sep))),
        _ => Ok((value, rest, None)),
    }
}

fn take_pane(s: &str) -> Result<(PaneId, &str)> {
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    if end == 0 {
        return Err(Error::Layout(format!("expected pane id at {s:?}")));
    }

    let id: u32 = s[..end]
        .parse()
        .map_err(|_| Error::Layout(format!("pane id out of range: {:?}", &s[..end])))?;
    Ok((PaneId(id), &s[end..]))
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

    #[test]
    fn parses_single_leaf() {
        let layout = Layout::parse("80x24,0,0,3").expect("parse leaf");
        assert_eq!(
            layout,
            Layout::Leaf {
                w: 80,
                h: 24,
                x: 0,
                y: 0,
                pane: PaneId(3)
            }
        );
    }

    #[test]
    fn round_trips_horizontal_split() {
        let s = "159x48,0,0{79x48,0,0,0,79x48,80,0,1}";
        let layout = Layout::parse(s).expect("parse split");
        assert_eq!(layout.render(), s);

        // The full checksummed form re-parses to the same tree.
        let full = layout.to_layout_string();
        assert_eq!(Layout::parse(&full).expect("reparse full"), layout);
    }

    #[test]
    fn round_trips_nested_splits() {
        // A left-right split whose second child is a top-bottom split.
        let s = "100x50,0,0{50x50,0,0,0,49x50,51,0[49x25,51,0,1,49x24,51,26,2]}";
        let layout = Layout::parse(s).expect("parse nested");
        assert_eq!(layout.render(), s);
    }

    #[test]
    fn rejects_checksum_mismatch() {
        assert!(Layout::parse("0000,80x24,0,0,1").is_err());
    }

    #[test]
    fn rejects_trailing_garbage() {
        assert!(Layout::parse("80x24,0,0,1xyz").is_err());
    }

    #[test]
    fn rejects_pathologically_nested_layout() {
        // Past MAX_LAYOUT_DEPTH the parser returns an error rather than recursing
        // until the native stack overflows (an uncatchable process abort).
        let mut nested = "2x2,0,0,0".to_string();
        for _ in 0..(MAX_LAYOUT_DEPTH + 5) {
            nested = format!("2x2,0,0{{{nested}}}");
        }
        assert!(Layout::parse(&nested).is_err());
    }
}
