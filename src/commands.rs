//! Control-mode command strings shared by the runtime drivers.
//!
//! The typed helpers (`send_keys`, `resize`, `select_layout`) format identical commands
//! regardless of runtime, so the strings live here rather than being duplicated per driver.

use crate::ids::{PaneId, WindowId};
use crate::layout::Layout;

/// `send-keys -t %<pane> -H <hex> …` — inject raw bytes as hex byte values (`-H`),
/// the safe path for arbitrary bytes and control sequences (no key-name lookup).
pub(crate) fn send_keys(pane: PaneId, keys: &[u8]) -> String {
    let mut cmd = format!("send-keys -t {pane} -H");
    for byte in keys {
        cmd.push_str(&format!(" {byte:02x}"));
    }
    cmd
}

/// `refresh-client -C <cols>x<rows>` — set this control client's size.
pub(crate) fn resize(cols: u16, rows: u16) -> String {
    format!("refresh-client -C {cols}x{rows}")
}

/// `resize-window -t @<window> -x <cols> -y <rows>` — set one window's size authoritatively.
///
/// This latches the window to `window-size=manual` (a per-window option on `w->options`,
/// `cmd-resize-window.c:109`), so the size holds regardless of the global `window-size` and
/// survives every later recalc — it is not arbitrated against client tty sizes. Distinct
/// from [`resize`]'s `refresh-client -C`, which only sets *this client's* desired size and
/// is clamped/arbitrated. tmux bounds-checks against `WINDOW_MINIMUM..=WINDOW_MAXIMUM`; an
/// out-of-range size comes back as `%error`, so validity is tmux's call, not a client-side
/// check (parity with `resize`).
///
/// Caveat: the manual size is still clamped *down* by any per-client per-window size a
/// client has set via `refresh-client -C @<window>:WxH` (`resize.c:222-244`) — don't drive
/// the same window through both layers.
pub(crate) fn resize_window(window: WindowId, cols: u16, rows: u16) -> String {
    format!("resize-window -t {window} -x {cols} -y {rows}")
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
    fn resize_window_targets_window_with_size_override() {
        // tmux's own `resize-window`: window id via the `@` sigil on `-t`, size on `-x`/`-y`.
        // Authoritative (window-size=manual), not the per-client `refresh-client -C` clamp.
        assert_eq!(
            resize_window(WindowId(2), 80, 24),
            "resize-window -t @2 -x 80 -y 24"
        );
    }

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
