//! Control-mode command strings shared by the runtime drivers.
//!
//! The typed helpers (`send_keys`, `resize`) format identical commands regardless of
//! runtime, so the strings live here rather than being duplicated per driver.

use crate::ids::PaneId;

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
