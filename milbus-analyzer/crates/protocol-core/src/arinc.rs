//! ARINC 429 word support built on the [`arinc429`] crate.
//!
//! [`Arinc429Frame`] wraps a decoded 32-bit ARINC 429 word and exposes its
//! constituent fields (label, SDI, data, SSM, parity) plus conversions from the
//! raw data field to engineering units via BNR and BCD interpretation.

use serde::{Deserialize, Serialize};

/// Errors produced when parsing or validating an ARINC 429 word.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Arinc429Error {
    /// The underlying `arinc429` crate rejected the word (parity, range, etc.).
    #[error("arinc429 error: {0}")]
    Crate(#[from] arinc429::ArincError),
}

/// A decoded ARINC 429 word broken into its protocol fields.
///
/// Field layout follows the ARINC 429 32-bit word:
/// * bits 8-1  label (bit-reversed on the wire),
/// * bits 9-8  SDI (source/destination identifier, 0-3),
/// * bits 28-10 data field (19 bits),
/// * bits 30-29 SSM (sign/status matrix, 0-3),
/// * bit 32 parity (odd parity).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Arinc429Frame {
    /// Decimal label code (e.g. 10 = Ground Speed / label 012).
    pub label: u8,
    /// Source/Destination Identifier (0-3).
    pub sdi: u8,
    /// Raw 19-bit data field.
    pub data: u32,
    /// Sign/Status Matrix bits (0-3).
    pub ssm: u8,
    /// The parity bit carried in bit 32 (`true` = 1).
    pub parity: bool,
}

impl Arinc429Frame {
    /// Parse a raw 32-bit ARINC 429 word into its fields.
    ///
    /// Delegates to [`arinc429::decode`] for odd-parity validation and label
    /// resolution, then extracts each field directly from the wire word.
    pub fn from_word(word: u32) -> Result<Self, Arinc429Error> {
        // Validate parity and resolve the label via the reference crate.
        let decoded = arinc429::decode(word)?;

        Ok(Self {
            label: decoded.label.raw(),
            sdi: ((word >> 8) & 0x3) as u8,
            data: (word >> 10) & 0x7F_FFF,
            ssm: ((word >> 29) & 0x3) as u8,
            parity: (word >> 31) & 1 == 1,
        })
    }

    /// Encode the fields back into a 32-bit ARINC 429 word.
    ///
    /// The odd-parity bit is recomputed by [`arinc429::encode`], so any stale
    /// `parity` value here is overwritten to keep the word well-formed.
    pub fn to_word(&self) -> Result<u32, Arinc429Error> {
        Ok(arinc429::encode(self.label, self.sdi, self.data, self.ssm)?)
    }

    /// Human-readable (octal) label, e.g. `"012"` for Ground Speed.
    pub fn octal_label(&self) -> String {
        format!("{:03o}", self.label)
    }

    /// The SSM interpreted as the reference crate's [`arinc429::Ssm`] enum.
    pub fn ssm(&self) -> arinc429::Ssm {
        arinc429::Ssm::from_u8(self.ssm)
    }

    /// Interpret the raw data field as a BNR (binary) value.
    ///
    /// ARINC 429 BNR uses the most-significant bit of the 19-bit data field as
    /// a sign-magnitude flag and the remaining 18 bits as a magnitude. The
    /// magnitude is scaled by `resolution` (the weight of the least-significant
    /// data bit) to produce engineering units.
    ///
    /// # Example
    /// ```
    /// # use protocol_core::arinc::Arinc429Frame;
    /// let frame = Arinc429Frame { label: 10, sdi: 0, data: 2000, ssm: 3, parity: true };
    /// assert_eq!(frame.to_bnr(0.125, false), 250.0);
    /// ```
    pub fn to_bnr(&self, resolution: f64, signed: bool) -> f64 {
        const DATA_BITS: u32 = 19;
        const SIGN_BIT: u32 = 1 << (DATA_BITS - 1); // 0x40000, bit 18 of the data field
        let magnitude = (self.data & !SIGN_BIT) as f64;
        let value = magnitude * resolution;
        if signed && (self.data & SIGN_BIT != 0) {
            -value
        } else {
            value
        }
    }

    /// Interpret the raw data field as BCD (binary-coded decimal) engineering
    /// units.
    ///
    /// Each nibble of the low 16 bits of the data field is treated as one
    /// decimal digit (most-significant nibble first) and reconstructed into a
    /// decimal integer, which is then scaled by `resolution`.
    ///
    /// # Example
    /// ```
    /// # use protocol_core::arinc::Arinc429Frame;
    /// let frame = Arinc429Frame { label: 150, sdi: 0, data: 0x1234, ssm: 3, parity: true };
    /// assert_eq!(frame.to_bcd(1.0), 1234.0);
    /// ```
    pub fn to_bcd(&self, resolution: f64) -> f64 {
        let mut decimal: u32 = 0;
        // Reconstruct up to four BCD digits from the low 16 bits, most
        // significant nibble first.
        for nibble_index in (0..4).rev() {
            let nibble = (self.data >> (nibble_index * 4)) & 0xF;
            decimal = decimal * 10 + nibble.min(9);
        }
        decimal as f64 * resolution
    }

    /// Best-effort engineering units for known labels, delegating to the
    /// reference crate's physical-value tables.
    ///
    /// Returns `None` when the SSM is not Normal Operation or the label has no
    /// built-in interpretation.
    pub fn engineering_units(&self) -> Option<f64> {
        let word = arinc429::ArincWord {
            label: arinc429::Label::from_u8(self.label),
            sdi: self.sdi,
            data: self.data,
            ssm: arinc429::Ssm::from_u8(self.ssm),
        };
        word.to_physical()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_word_through_fields() {
        // Ground Speed = 250 knots encoded by the reference crate.
        let word = arinc429::encode(10, 0, 2000, 3).unwrap();
        let frame = Arinc429Frame::from_word(word).unwrap();
        assert_eq!(frame.label, 10);
        assert_eq!(frame.sdi, 0);
        assert_eq!(frame.data, 2000);
        assert_eq!(frame.ssm, 3);
        assert!(frame.parity);
        assert_eq!(frame.to_word().unwrap(), word);
    }

    #[test]
    fn bnr_converts_with_resolution_and_sign() {
        let unsigned = Arinc429Frame {
            label: 10,
            sdi: 0,
            data: 2000,
            ssm: 3,
            parity: true,
        };
        assert_eq!(unsigned.to_bnr(0.125, false), 250.0);

        // Set the sign bit (bit 18) with magnitude 100 -> -12.5 at 0.125/LSB.
        let signed = Arinc429Frame {
            label: 131,
            sdi: 0,
            data: 0x40000 | 100,
            ssm: 3,
            parity: true,
        };
        assert_eq!(signed.to_bnr(0.125, true), -12.5);
    }

    #[test]
    fn bcd_reconstructs_decimal_digits() {
        let frame = Arinc429Frame {
            label: 150,
            sdi: 0,
            data: 0x1234,
            ssm: 3,
            parity: true,
        };
        assert_eq!(frame.to_bcd(1.0), 1234.0);
        assert_eq!(frame.to_bcd(0.1), 123.4);
    }

    #[test]
    fn rejects_even_parity() {
        // Clear the parity bit of an otherwise valid word to force even parity.
        let word = arinc429::encode(10, 0, 2000, 3).unwrap() & !(1 << 31);
        assert!(Arinc429Frame::from_word(word).is_err());
    }

    #[test]
    fn engineering_units_uses_reference_tables() {
        let word = arinc429::encode(10, 0, 2000, 3).unwrap(); // Ground Speed
        let frame = Arinc429Frame::from_word(word).unwrap();
        assert_eq!(frame.engineering_units(), Some(250.0));
    }
}
