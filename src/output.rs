//! Decoding tmux's `%output` payload escaping.

/// Decode a tmux `%output` / `%extended-output` payload back to raw bytes.
///
/// tmux escapes every byte `< 0x20` **and** the backslash itself as a 3-digit
/// octal sequence `\ooo` (so `\` → `\134`). Every other byte is passed through
/// literally. A backslash not followed by three octal digits is treated as a
/// literal backslash — tmux never emits that, but we decode defensively.
///
/// Decode before handing bytes to a VT emulator; multi-byte UTF-8 is preserved
/// verbatim (decode at the emulator, never here). Takes raw bytes — an `%output`
/// payload is *not* guaranteed valid UTF-8 (bytes `>= 0x80` pass through raw), so
/// it must never be forced through `&str`.
pub fn decode_output(escaped: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(escaped.len());

    let mut i = 0;
    while i < escaped.len() {
        let escape = escaped[i] == b'\\' && i + 3 < escaped.len();
        let Some(byte) = escape
            .then(|| parse_octal(&escaped[i + 1..i + 4]))
            .flatten()
        else {
            out.push(escaped[i]);
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
        assert_eq!(decode_output(b"hello world"), b"hello world");
    }

    #[test]
    fn decodes_escaped_backslash() {
        assert_eq!(decode_output(br"a\134b"), b"a\\b");
    }

    #[test]
    fn decodes_control_bytes() {
        // ESC [ 0 m  — a common SGR reset, ESC is octal 033.
        assert_eq!(decode_output(br"\033[0m"), vec![0x1b, b'[', b'0', b'm']);
    }

    #[test]
    fn trailing_backslash_without_digits_stays_literal() {
        assert_eq!(decode_output(br"ab\"), b"ab\\");
    }

    #[test]
    fn non_octal_after_backslash_stays_literal() {
        assert_eq!(decode_output(br"\\89"), b"\\\\89");
    }

    #[test]
    fn trailing_escape_at_end_of_input_decodes() {
        // A complete `\ooo` as the final four bytes must decode — the `i + 3 < len`
        // bound admits it (last digit at len-1). Regression guard against an
        // off-by-one misreading of that bound.
        assert_eq!(decode_output(br"\033"), vec![0x1b]);
        assert_eq!(decode_output(br"x\000"), vec![b'x', 0x00]);
    }

    #[test]
    fn high_bytes_pass_through_verbatim() {
        // A lone 0xFF and a UTF-8 multibyte (é = 0xC3 0xA9) survive unchanged —
        // exactly the bytes a &str line type would corrupt.
        assert_eq!(
            decode_output(&[0xff, b'a', 0xc3, 0xa9]),
            vec![0xff, b'a', 0xc3, 0xa9]
        );
    }
}
