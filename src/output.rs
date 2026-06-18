//! Decoding tmux's `%output` payload escaping.

/// Decode a tmux `%output` / `%extended-output` payload back to raw bytes.
///
/// tmux escapes every byte `< 0x20` **and** the backslash itself as a 3-digit
/// octal sequence `\ooo` (so `\` → `\134`). Every other byte is passed through
/// literally. A backslash not followed by three octal digits is treated as a
/// literal backslash — tmux never emits that, but we decode defensively.
///
/// Decode before handing bytes to a VT emulator; multi-byte UTF-8 is preserved
/// verbatim (decode at the emulator, never here).
pub fn decode_output(escaped: &str) -> Vec<u8> {
    let bytes = escaped.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());

    let mut i = 0;
    while i < bytes.len() {
        let escape = bytes[i] == b'\\' && i + 3 < bytes.len();
        let Some(byte) = escape.then(|| parse_octal(&bytes[i + 1..i + 4])).flatten() else {
            out.push(bytes[i]);
            i += 1;
            continue;
        };

        out.push(byte);
        i += 4;
    }

    out
}

/// Parse exactly three octal digits into a byte, or `None` if any digit is
/// out of range or the value overflows a `u8`.
fn parse_octal(digits: &[u8]) -> Option<u8> {
    let mut value: u16 = 0;
    for &d in digits {
        if !d.is_ascii_digit() || d > b'7' {
            return None;
        }
        value = value * 8 + u16::from(d - b'0');
    }
    u8::try_from(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_ascii_passes_through() {
        assert_eq!(decode_output("hello world"), b"hello world");
    }

    #[test]
    fn decodes_escaped_backslash() {
        assert_eq!(decode_output(r"a\134b"), b"a\\b");
    }

    #[test]
    fn decodes_control_bytes() {
        // ESC [ 0 m  — a common SGR reset, ESC is octal 033.
        assert_eq!(decode_output(r"\033[0m"), vec![0x1b, b'[', b'0', b'm']);
    }

    #[test]
    fn trailing_backslash_without_digits_stays_literal() {
        assert_eq!(decode_output(r"ab\"), b"ab\\");
    }

    #[test]
    fn non_octal_after_backslash_stays_literal() {
        assert_eq!(decode_output(r"\\89"), b"\\\\89");
    }

    #[test]
    fn utf8_multibyte_passes_through() {
        assert_eq!(decode_output("café"), "café".as_bytes());
    }
}
