//! Core protocol types and trait definitions for milbus-analyzer.
//!
//! This module defines the BusProtocol trait, which unifies frame
//! parsing, encoding, and validation across the supported bus protocols:
//! MIL-STD-1553B, ARINC 429, and Link 16.

/// A bus protocol with a distinct frame, command, and error representation.
///
/// Implementors provide protocol-specific parsing of raw wire bytes,
/// encoding of frames back to bytes, and structural validation of frames.
pub trait BusProtocol {
    /// Protocol-specific frame representation.
    type Frame;

    /// Protocol-specific command representation.
    type Command;

    /// Error type produced during parsing and validation.
    type Error: std::error::Error;

    /// Parse a raw byte slice into a protocol frame.
    fn parse(data: &[u8]) -> Result<Self::Frame, Self::Error>;

    /// Encode a protocol frame into raw bytes.
    fn encode(frame: &Self::Frame) -> Vec<u8>;

    /// Returns `true` if the frame is structurally valid for this protocol.
    fn validate(frame: &Self::Frame) -> bool;
}
pub mod arinc;
pub mod hex;
pub mod link16;
pub use arinc::{Arinc429Error, Arinc429Frame};
pub use link16::{
    Link16Error, Link16Message, Link16Network, TimeSlot, MAX_TRACK_NUMBER, MIN_TRACK_NUMBER,
};
