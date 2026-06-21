//! Control-mode command strings shared by the runtime drivers.
//!
//! The typed helpers (`send_keys`, `resize`, `select_layout`) format identical commands
//! regardless of runtime, so the strings live here rather than being duplicated per driver.

use crate::ids::{PaneId, WindowId};
use crate::layout::Layout;

/// `send-keys -t %<pane> -H <hex> …` — inject raw bytes as hex byte values (`-H`),
/// the safe path for arbitrary bytes and control sequences (no key-name lookup).
pub(crate) fn send_keys(pane: PaneId, keys: &[u8]) -> String {
    let mut cmd = format!("send-keys -t %{} -H", pane.0);
    for byte in keys {
        cmd.push_str(&format!(" {byte:02x}"));
    }
    cmd
}

/// `refresh-client -C <cols>x<rows>` — set this control client's size.
pub(crate) fn resize(cols: u16, rows: u16) -> String {
    format!("refresh-client -C {cols}x{rows}")
}

/// `select-layout -t @<window> <layout-string>` — push a layout onto a window.
///
/// Sends the checksummed `to_layout_string()` form: tmux's `layout_parse` rejects a
/// tree without the leading 4-hex checksum. Layout validity (pane-count fit, geometry)
/// is tmux's call — a bad layout comes back as `%error`, not a client-side check.
pub(crate) fn select_layout(window: WindowId, layout: &Layout) -> String {
    format!("select-layout -t {window} {}", layout.to_layout_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::WindowId;
    use crate::layout::Layout;

    #[test]
    fn select_layout_targets_window_with_checksummed_string() {
        // Targets the window via the `@` sigil and sends the checksummed
        // `to_layout_string()` form — tmux's `layout_parse` rejects a bare tree.
        let layout = Layout::parse("159x48,0,0{79x48,0,0,0,79x48,80,0,1}").expect("parse");
        assert_eq!(
            select_layout(WindowId(2), &layout),
            format!("select-layout -t @2 {}", layout.to_layout_string())
        );
    }
}
