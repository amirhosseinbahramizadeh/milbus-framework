//! Protocol-neutral wire model for the multi-channel virtual bus hub.
//!
//! [`ProtocolId`] tags traffic so devices only observe their own protocol.
//! [`WireFrame`] is the per-protocol payload: a MIL-STD-1553B [`Frame`], an
//! ARINC 429 32-bit word ([`Arinc429Word`]), or a Link 16 transmission
//! ([`Link16Frame`]). Constructors validate eagerly so malformed wire data can
//! never be published: [`Arinc429Word::new`] parity-checks via
//! [`Arinc429Frame::from_word`], and [`Link16Frame::new`] enforces the STN and
//! payload-length bounds before a transmission is scheduled.

use std::fmt;

use chrono::{DateTime, Utc};
use protocol_core::{Arinc429Error, Arinc429Frame, Link16Error, Link16Message};
use serde::{Deserialize, Serialize};

use crate::frame::Frame;

/// Protocol discriminator tagging every frame on the multi-channel hub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProtocolId {
    /// MIL-STD-1553B command/status/data words.
    Mil1553b,
    /// ARINC 429 32-bit words.
    Arinc429,
    /// Link 16 J-series slot transmissions.
    Link16,
}

impl ProtocolId {
    /// Human-readable label for logs and the TUI.
    pub fn label(self) -> &'static str {
        match self {
            ProtocolId::Mil1553b => "MIL-1553B",
            ProtocolId::Arinc429 => "ARINC 429",
            ProtocolId::Link16 => "Link 16",
        }
    }

    /// Every protocol the hub carries, in tab order.
    pub const ALL: [ProtocolId; 3] =
        [ProtocolId::Mil1553b, ProtocolId::Arinc429, ProtocolId::Link16];
}

/// A validated ARINC 429 32-bit wire word on the multi-channel hub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Arinc429Word(
    /// The validated 32-bit word; parity was checked at construction.
    pub u32,
);

impl Arinc429Word {
    /// Wrap a raw word, parity-checking it exactly like
    /// [`Arinc429Frame::from_word`].
    ///
    /// # Errors
    ///
    /// Returns the underlying decode error (e.g. even parity) unchanged.
    pub fn new(raw: u32) -> Result<Self, Arinc429Error> {
        Arinc429Frame::from_word(raw)?;
        Ok(Self(raw))
    }

    /// Decode the validated word into its protocol fields.
    ///
    /// Infallible: construction already validated the word.
    pub fn decode(&self) -> Arinc429Frame {
        Arinc429Frame::from_word(self.0).expect("Arinc429Word validated at construction")
    }
}

/// Errors produced when stamping a Link 16 wire transmission.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Link16WireError {
    /// Source track number outside the legal 1..=127 range.
    #[error("track number {0} out of range (must be 1..=127)")]
    InvalidStn(u8),
    /// Payload exceeds the wire bound, so the message cannot be scheduled.
    #[error("payload of {len} bytes exceeds the {max}-byte wire bound (STN {stn})")]
    PayloadTooLarge {
        /// Source track number the oversized message claimed.
        stn: u8,
        /// Payload length in bytes.
        len: usize,
        /// The enforced bound.
        max: usize,
    },
}

impl From<Link16WireError> for Link16Error {
    /// Maps wire validation onto the network error: out-of-range STNs become
    /// [`Link16Error::InvalidStn`]; oversized payloads reuse
    /// [`Link16Error::StnMismatch`] as the closest network-semantics
    /// rejection (the STN cannot be scheduled with that payload).
    fn from(err: Link16WireError) -> Self {
        match err {
            Link16WireError::InvalidStn(stn) => Link16Error::InvalidStn(stn),
            Link16WireError::PayloadTooLarge { stn, .. } => Link16Error::StnMismatch {
                stn,
                message_stn: stn,
            },
        }
    }
}

/// A frame tagged with the protocol channel it travels on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireFrame {
    /// MIL-STD-1553B command/status/data word.
    Mil1553b(Frame),
    /// Validated ARINC 429 32-bit word.
    Arinc429(Arinc429Word),
    /// Link 16 J-series slot transmission.
    Link16(Link16Frame),
}

impl WireFrame {
    /// The protocol channel this frame travels on.
    pub fn protocol(&self) -> ProtocolId {
        match self {
            WireFrame::Mil1553b(_) => ProtocolId::Mil1553b,
            WireFrame::Arinc429(_) => ProtocolId::Arinc429,
            WireFrame::Link16(_) => ProtocolId::Link16,
        }
    }
}

impl fmt::Display for ProtocolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

impl fmt::Display for WireFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WireFrame::Mil1553b(frame) => write!(
                f,
                "[{}] seq={} {:?} word={:#06X} src={}{}",
                ProtocolId::Mil1553b.label(),
                frame.sequence,
                frame.word_type,
                frame.word,
                frame.source,
                if frame.corrupted { " CORRUPT" } else { "" },
            ),
            WireFrame::Arinc429(word) => {
                let decoded = word.decode();
                write!(
                    f,
                    "[{}] {:#010X} label={} sdi={} ssm={:?}",
                    ProtocolId::Arinc429.label(),
                    word.0,
                    decoded.octal_label(),
                    decoded.sdi,
                    decoded.ssm(),
                )
            }
            WireFrame::Link16(link) => write!(
                f,
                "[{}] slot={} stn={} J{:02}/{:02} payload={}B",
                ProtocolId::Link16.label(),
                link.slot_index,
                link.message.source_track_number,
                link.message.j_series_label,
                link.message.sublabel,
                link.message.payload.len(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link_message(stn: u8) -> Link16Message {
        Link16Message::new(3, 2, stn, vec![0xDE, 0xAD])
    }

    #[test]
    fn arinc_word_rejects_even_parity() {
        let word = arinc429::encode(10, 0, 2000, 3).unwrap() & !(1 << 31);
        assert!(Arinc429Word::new(word).is_err());
    }

    #[test]
    fn arinc_word_round_trips_through_frame() {
        let raw = arinc429::encode(10, 0, 2000, 3).unwrap();
        let wire = Arinc429Word::new(raw).unwrap();
        assert_eq!(wire.decode().to_bnr(0.125, false), 250.0);
    }

    #[test]
    fn link16_frame_bounds_payload_and_stn() {
        let at = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        assert!(Link16Frame::new(0, at, link_message(0)).is_err());
        let oversized =
            Link16Message::new(3, 2, 10, vec![0u8; Link16Frame::MAX_PAYLOAD_LEN + 1]);
        assert!(Link16Frame::new(0, at, oversized).is_err());
        assert!(Link16Frame::new(2, at, link_message(10)).is_ok());
    }

    #[test]
    fn wire_frame_protocol_tags_match() {
        let frame = Frame {
            sequence: 0,
            word_type: crate::frame::WordType::Data,
            word: 0,
            source: 1,
            corrupted: false,
        };
        assert_eq!(WireFrame::Mil1553b(frame).protocol(), ProtocolId::Mil1553b);
        let raw = arinc429::encode(10, 0, 2000, 3).unwrap();
        assert_eq!(
            WireFrame::Arinc429(Arinc429Word::new(raw).unwrap()).protocol(),
            ProtocolId::Arinc429
        );
        let at = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let link = Link16Frame::new(0, at, link_message(4)).unwrap();
        assert_eq!(WireFrame::Link16(link).protocol(), ProtocolId::Link16);
    }

    #[test]
    fn wire_frame_display_mentions_protocol_and_fields() {
        let at = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let link = Link16Frame::new(3, at, link_message(7)).unwrap();
        let text = WireFrame::Link16(link).to_string();
        assert!(text.contains("Link 16") && text.contains("stn=7"));
    }
}

/// A Link 16 J-series transmission stamped with its TDMA slot and instant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link16Frame {
    /// Index of the TDMA slot in the network schedule.
    pub slot_index: usize,
    /// Instant the transmission occurred (UTC).
    pub transmitted_at: DateTime<Utc>,
    /// The J-series message payload.
    pub message: Link16Message,
}

impl Link16Frame {
    /// Maximum payload size accepted on the wire (Link 16 J-words are small;
    /// this keeps channel buffers bounded).
    pub const MAX_PAYLOAD_LEN: usize = 128;

    /// Build a stamped transmission, enforcing STN range and payload bound.
    ///
    /// # Errors
    ///
    /// Returns [`Link16WireError::InvalidStn`] or
    /// [`Link16WireError::PayloadTooLarge`].
    pub fn new(
        slot_index: usize,
        transmitted_at: DateTime<Utc>,
        message: Link16Message,
    ) -> Result<Self, Link16WireError> {
        if !(protocol_core::MIN_TRACK_NUMBER..=protocol_core::MAX_TRACK_NUMBER)
            .contains(&message.source_track_number)
        {
            return Err(Link16WireError::InvalidStn(message.source_track_number));
        }
        if message.payload.len() > Self::MAX_PAYLOAD_LEN {
            return Err(Link16WireError::PayloadTooLarge {
                stn: message.source_track_number,
                len: message.payload.len(),
                max: Self::MAX_PAYLOAD_LEN,
            });
        }
        Ok(Self {
            slot_index,
            transmitted_at,
            message,
        })
    }
}

impl From<Arinc429Frame> for Arinc429Word {
    /// Re-encode a decoded frame. Panics only if the fields were mutated into
    /// an out-of-range state after decoding (constructors reject this first).
    fn from(frame: Arinc429Frame) -> Self {
        let raw = frame
            .to_word()
            .expect("Arinc429Frame fields in range after successful decode");
        Self(raw)
    }
}


