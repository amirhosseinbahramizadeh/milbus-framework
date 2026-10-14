//! MIL-STD-1553B-flavoured frame representation for the virtual bus.
//!
//! The virtual bus transports one 16-bit word per [`Frame`]. Words are tagged
//! with their role using the field layout defined by MIL-STD-1553B:
//!
//! * Command word: bits 15..11 = RT address, bit 10 = T/R bit (`1` = RT
//!   transmits to the bus controller), bits 9..5 = subaddress, bits 4..0 =
//!   word count.
//! * Status word: bits 15..11 = responding RT address, remaining bits hold
//!   status flags (simplified for the simulation).
//! * Data word: arbitrary 16-bit payload.

use serde::{Deserialize, Serialize};

/// RT address reserved for the bus controller in this simulation.
///
/// MIL-STD-1553B reserves address 31 for broadcast; since the bus controller
/// itself has no RT address, the simulator uses this value to tag
/// controller-originated frames.
pub const BUS_CONTROLLER_ADDRESS: u8 = 31;

/// Maximum number of data words in a single RT response.
pub const MAX_WORD_COUNT: u8 = 32;

/// Default number of data words the controller requests per poll.
pub const DEFAULT_DATA_WORDS: u8 = 4;

/// Role of a 16-bit word on the bus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WordType {
    /// Bus controller command word.
    Command,
    /// Remote terminal status word.
    Status,
    /// Data word.
    Data,
}

/// A single word observed on the virtual bus.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    /// Monotonically increasing sequence number assigned by the bus.
    pub sequence: u64,
    /// Role of the word on the bus.
    pub word_type: WordType,
    /// Raw 16-bit word content.
    pub word: u16,
    /// Address of the terminal that transmitted the word.
    pub source: u8,
    /// `true` when the frame was corrupted by error injection.
    pub corrupted: bool,
}

/// Decoded MIL-STD-1553B command word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandWord {
    /// Target remote terminal address (0..=31).
    pub rt_address: u8,
    /// `true` when the RT is asked to transmit to the bus controller.
    pub transmit: bool,
    /// Subaddress (or mode-code selector).
    pub subaddress: u8,
    /// Number of data words expected (1..=32).
    pub word_count: u8,
}

impl CommandWord {
    /// Build a command word, masking fields to their wire widths.
    pub fn new(rt_address: u8, transmit: bool, subaddress: u8, word_count: u8) -> Self {
        Self {
            rt_address: rt_address & 0x1F,
            transmit,
            subaddress: subaddress & 0x1F,
            word_count: word_count.clamp(1, MAX_WORD_COUNT),
        }
    }

    /// Encode the command word into its 16-bit wire representation.
    pub fn to_word(&self) -> u16 {
        ((self.rt_address as u16) << 11)
            | ((self.transmit as u16) << 10)
            | ((self.subaddress as u16) << 5)
            | (self.word_count as u16 & 0x1F)
    }

    /// Decode a raw 16-bit command word.
    ///
    /// A word-count field of `0` decodes to 32 data words, matching the
    /// MIL-STD-1553B convention.
    pub fn from_word(word: u16) -> Self {
        let count_field = (word & 0x1F) as u8;
        Self {
            rt_address: ((word >> 11) & 0x1F) as u8,
            transmit: (word >> 10) & 0x1 == 1,
            subaddress: ((word >> 5) & 0x1F) as u8,
            word_count: if count_field == 0 {
                MAX_WORD_COUNT
            } else {
                count_field.min(MAX_WORD_COUNT)
            },
        }
    }
}

/// Build a status word for `rt_address` with all status flags clear.
pub fn status_word(rt_address: u8) -> u16 {
    ((rt_address & 0x1F) as u16) << 11
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_word_round_trips() {
        let command = CommandWord::new(7, true, 3, 5);
        assert_eq!(CommandWord::from_word(command.to_word()), command);
    }

    #[test]
    fn zero_word_count_decodes_as_thirty_two() {
        let word = CommandWord::new(1, false, 0, 32).to_word();
        assert_eq!(CommandWord::from_word(word).word_count, MAX_WORD_COUNT);
    }

    #[test]
    fn word_count_is_clamped_to_valid_range() {
        assert_eq!(CommandWord::new(1, true, 1, 0).word_count, 1);
        assert_eq!(CommandWord::new(1, true, 1, 99).word_count, MAX_WORD_COUNT);
    }
}
