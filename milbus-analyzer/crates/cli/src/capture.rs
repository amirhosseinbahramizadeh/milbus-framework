//! Frame capture: record bus traffic to and from JSON files.
//!
//! A [`Capture`] is an ordered log of [`Frame`]s. It can be streamed to disk as
//! pretty-printed JSON via [`Capture::write_to`] and loaded back with
//! [`Capture::read_from`], which powers the `replay` subcommand. Because
//! [`Frame`] is `serde`-serializable, captures round-trip losslessly.
//!
//! A [`WireCapture`] is the multi-protocol counterpart: an ordered log of
//! [`WireFrame`]s (one tagged entry per protocol channel). It is streamed to
//! disk the same way and powers the headless `wire-capture` subcommand, which
//! records MIL-STD-1553B, ARINC 429, and Link 16 traffic into a single file
//! without opening the interactive TUI.

use std::fs;
use std::path::Path;

use simulator::wire::WireFrame;
use simulator::Frame;

/// Errors produced while reading or writing a capture file.
#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    /// Underlying filesystem I/O failed.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// JSON (de)serialization failed.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// An ordered log of captured bus frames.
#[derive(Debug, Default, Clone)]
pub struct Capture {
    frames: Vec<Frame>,
}

impl Capture {
    /// Create an empty capture.
    pub fn new() -> Self {
        Self { frames: Vec::new() }
    }

    /// Append a frame to the capture.
    pub fn record(&mut self, frame: Frame) {
        self.frames.push(frame);
    }

    /// The captured frames, oldest first.
    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }

    /// Number of captured frames.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Whether the capture holds no frames.
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Write the capture to `path` as pretty-printed JSON.
    pub fn write_to(&self, path: impl AsRef<Path>) -> Result<(), CaptureError> {
        let json = serde_json::to_string_pretty(&self.frames)?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Read a capture back from a JSON file produced by [`Capture::write_to`].
    pub fn read_from(path: impl AsRef<Path>) -> Result<Self, CaptureError> {
        let data = fs::read_to_string(path)?;
        let frames: Vec<Frame> = serde_json::from_str(&data)?;
        Ok(Self { frames })
    }
}

/// An ordered log of protocol-tagged bus frames, one entry per channel.
///
/// Unlike [`Capture`] (MIL-STD-1553B only), this records every protocol the
/// hub carries, so a single file holds ARINC 429 and Link 16 traffic alongside
/// 1553B words. [`WireFrame`] is `serde`-serializable, so captures
/// round-trip losslessly.
#[derive(Debug, Default, Clone)]
pub struct WireCapture {
    frames: Vec<WireFrame>,
}

impl WireCapture {
    /// Create an empty wire capture.
    pub fn new() -> Self {
        Self { frames: Vec::new() }
    }

    /// Append a protocol-tagged frame to the capture.
    pub fn record(&mut self, frame: WireFrame) {
        self.frames.push(frame);
    }

    /// The captured frames, oldest first.
    pub fn frames(&self) -> &[WireFrame] {
        &self.frames
    }

    /// Number of captured frames.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Whether the capture holds no frames.
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Frames observed on the given protocol channel, oldest first.
    pub fn frames_for(&self, protocol: simulator::wire::ProtocolId) -> Vec<&WireFrame> {
        self.frames
            .iter()
            .filter(|frame| frame.protocol() == protocol)
            .collect()
    }

    /// Write the capture to `path` as pretty-printed JSON.
    pub fn write_to(&self, path: impl AsRef<Path>) -> Result<(), CaptureError> {
        let json = serde_json::to_string_pretty(&self.frames)?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Read a capture back from a JSON file produced by [`WireCapture::write_to`].
    pub fn read_from(path: impl AsRef<Path>) -> Result<Self, CaptureError> {
        let data = fs::read_to_string(path)?;
        let frames: Vec<WireFrame> = serde_json::from_str(&data)?;
        Ok(Self { frames })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use simulator::WordType;

    fn sample() -> Frame {
        Frame {
            sequence: 7,
            word_type: WordType::Command,
            word: 0x0820,
            source: 31,
            corrupted: false,
        }
    }

    #[test]
    fn round_trips_through_json() {
        let dir = std::env::temp_dir().join(format!("milbus-capture-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("frames.json");

        let mut capture = Capture::new();
        capture.record(sample());
        capture.write_to(&path).unwrap();

        let loaded = Capture::read_from(&path).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded.frames(), &[sample()]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn sample_wire() -> WireFrame {
        let raw = protocol_core::Arinc429Frame {
            label: 10,
            sdi: 0,
            data: 2000,
            ssm: 3,
            parity: true,
        }
        .to_word()
        .unwrap();
        WireFrame::Arinc429(simulator::wire::Arinc429Word::new(raw).unwrap())
    }

    #[test]
    fn wire_capture_round_trips_and_filters_by_protocol() {
        let dir = std::env::temp_dir().join(format!("milbus-wire-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("wire.json");

        let mut capture = WireCapture::new();
        assert!(capture.is_empty());
        capture.record(WireFrame::Mil1553b(sample()));
        capture.record(sample_wire());
        capture.write_to(&path).unwrap();

        let loaded = WireCapture::read_from(&path).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded.frames(), capture.frames());
        assert_eq!(
            loaded
                .frames_for(simulator::wire::ProtocolId::Mil1553b)
                .len(),
            1
        );
        assert_eq!(
            loaded
                .frames_for(simulator::wire::ProtocolId::Arinc429)
                .len(),
            1
        );
        assert!(
            loaded
                .frames_for(simulator::wire::ProtocolId::Link16)
                .is_empty()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
