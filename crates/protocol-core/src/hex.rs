//! Hex-string helpers shared by the TUI injection box and the CLI.
//!
//! These helpers turn the raw text a user types (e.g. `80 00 02 3C` or
//! `0xDEAD BEEF`) into the byte slices that [`crate::BusProtocol::parse`]
//! consumes, and format bytes back into readable hex for display.

/// Error produced when parsing a hex string fails.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HexParseError {
    /// The cleaned input had an odd number of hex digits.
    #[error("odd number of hex digits ({0} after cleanup)")]
    OddLength(usize),
    /// A character was not a valid hexadecimal digit.
    #[error("invalid hex digit {0:?} at cleaned position {1}")]
    InvalidDigit(char, usize),
}

fn digit_value(c: char) -> Result<u8, HexParseError> {
    c.to_digit(16)
        .map(|d| d as u8)
        .ok_or(HexParseError::InvalidDigit(c, 0))
}

/// Parse a hex string into bytes.
///
/// Whitespace is ignored and `0x`/`0X` prefixes are stripped, so
/// `"80 00 02 3C"`, `"0x8000023C"`, and `"8000023c"` are equivalent.
///
/// # Errors
///
/// Returns [`HexParseError::OddLength`] if an odd number of digits remain,
/// or [`HexParseError::InvalidDigit`] for any non-hex character.
pub fn parse_hex(input: &str) -> Result<Vec<u8>, HexParseError> {
    // Strip whitespace and any 0x/0X prefixes.
    let mut digits = String::new();
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_whitespace() {
            continue;
        }
        if c == '0' && matches!(chars.peek(), Some('x') | Some('X')) {
            chars.next(); // consume the 'x'
            continue;
        }
        digits.push(c);
    }

    if !digits.len().is_multiple_of(2) {
        return Err(HexParseError::OddLength(digits.len()));
    }

    let raw: Vec<char> = digits.chars().collect();
    let mut bytes = Vec::with_capacity(raw.len() / 2);
    for (pair_index, pair) in raw.chunks(2).enumerate() {
        let hi = digit_value(pair[0]).map_err(|e| match e {
            HexParseError::InvalidDigit(c, _) => HexParseError::InvalidDigit(c, pair_index * 2),
            other => other,
        })?;
        let lo = digit_value(pair[1]).map_err(|e| match e {
            HexParseError::InvalidDigit(c, _) => HexParseError::InvalidDigit(c, pair_index * 2 + 1),
            other => other,
        })?;
        bytes.push((hi << 4) | lo);
    }
    Ok(bytes)
}

/// Format bytes as uppercase hex with no separators.
pub fn format_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_and_prefixed_hex() {
        assert_eq!(parse_hex("DEADBEEF").unwrap(), vec![0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(
            parse_hex("0xDEAD 0xBEEF").unwrap(),
            vec![0xDE, 0xAD, 0xBE, 0xEF]
        );
        assert_eq!(
            parse_hex(" de ad be ef ").unwrap(),
            vec![0xDE, 0xAD, 0xBE, 0xEF]
        );
    }

    #[test]
    fn rejects_odd_length_and_bad_digits() {
        assert_eq!(parse_hex("ABC"), Err(HexParseError::OddLength(3)));
        assert!(matches!(
            parse_hex("ZZ"),
            Err(HexParseError::InvalidDigit('Z', 0))
        ));
    }

    #[test]
    fn formats_round_trip() {
        let bytes = vec![0x80, 0x00, 0x02, 0x3C];
        assert_eq!(format_hex(&bytes), "8000023C");
        assert_eq!(parse_hex(&format_hex(&bytes)).unwrap(), bytes);
    }
}
