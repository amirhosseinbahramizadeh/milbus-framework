//! ARINC 429 device role for the multi-channel virtual bus.
//!
//! [`SimulatedArinc429Device`] is a single role acting as both transmitter and
//! receiver:
//!
//! * **send** — every `send_interval` it encodes the configured 32-bit word
//!   (`label` / `sdi` / `data` / `ssm`) into an [`Arinc429Word`] and publishes
//!   it via [`VirtualBus::publish_wire`] on the ARINC channel.
//! * **receive** — it subscribes to the ARINC channel and, when a capture log
//!   is attached, stores each received decoded [`Arinc429Frame`], mirroring the
//!   [`SimulatedBusMonitor`] behaviour for 1553B.
//!
//! Transmission and capture run concurrently under [`tokio::select!`], so a
//! paused Tokio clock only delays emission; capture keeps draining.
//!
//! The role uses [`protocol_core::Arinc429Frame`] for validated capture, so
//! ARINC traffic stays typed and protocol-correct from emission through capture
//! instead of being leaked as ambiguous 1553B [`Frame`]s.
use std::sync::Arc;
use std::time::Duration;
use protocol_core::Arinc429Frame;
use tokio::sync::broadcast;
use tokio::time::{interval, MissedTickBehavior};
use std::future::Future;
use crate::bus::VirtualBus;
use crate::device::SimulatedDevice;
use crate::wire::{Arinc429Word, ProtocolId, WireFrame};
/// A simulated ARINC 429 transmitter/receiver.
///
/// The send fields are exposed so callers can tune the word without swapping
/// the device. The `capture_log` sink records received frames the way the 1553B
/// monitor records `Frame`s.
#[derive(Debug, Clone)]
pub struct SimulatedArinc429Device {
    /// ARINC 429 label (8-bit code, e.g. 10 = ground speed).
    pub label: u8,
    /// Source/destination identifier, 0..=3.
    pub sdi: u8,
    /// Raw 19-bit data field.
    pub data: u32,
    /// Sign/status matrix, 0..=3.
    pub ssm: u8,
    /// Interval between outbound transmissions.
    pub send_interval: Duration,
    /// Optional sink for received frames (mirrors the 1553B monitor log).
    pub capture_log: Option<Arc<std::sync::Mutex<Vec<Arinc429Frame>>>>,
}
impl SimulatedArinc429Device {
    /// Build a new ARINC 429 transmitter.
    ///
    /// `capture_log` may be `None`; received frames are then silently dropped.
    pub fn new(
        label: u8,
        sdi: u8,
        data: u32,
        ssm: u8,
        send_interval: Duration,
        capture_log: Option<Arc<std::sync::Mutex<Vec<Arinc429Frame>>>>,
    ) -> Self {
        Self {
            label,
            sdi,
            data,
            ssm,
            send_interval,
            capture_log,
        }
    }
    /// Encode the configured word and publish it on the ARINC wire channel.
    ///
    /// Parity is recomputed on encode; a word that fails decode validation is
    /// silently dropped rather than injected into the bus.
    fn publish_sent_word(&self, bus: &VirtualBus) {
        let frame = Arinc429Frame {
            label: self.label,
            sdi: self.sdi,
            data: self.data,
            ssm: self.ssm,
            parity: true,
        };
        let raw = match frame.to_word() {
            Ok(raw) => raw,
            Err(_) => return,
        };
        let word = match Arinc429Word::new(raw) {
            Ok(word) => word,
            Err(_) => return,
        };
        let _ = bus.publish_wire(WireFrame::Arinc429(word));
    }
    /// Store a received ARINC 429 word into the capture log, if any.
    fn capture_received(&self, word: Arinc429Word) {
        if let Some(log) = &self.capture_log {
            let frame = word.decode();
            log.lock().expect("capture log mutex poisoned").push(frame);
        }
    }
}
impl SimulatedDevice for SimulatedArinc429Device {
    fn run(&mut self, bus: &VirtualBus) -> impl Future<Output = ()> + Send {
        async move {
            let mut rx = bus.subscribe_wire(ProtocolId::Arinc429);
            let mut send_timer = interval(self.send_interval);
            send_timer.set_missed_tick_behavior(MissedTickBehavior::Burst);
            loop {
                tokio::select! {
                    _ = send_timer.tick() => {
                        self.publish_sent_word(bus);
                    }
                    frame = rx.recv() => {
                        match frame {
                            Ok(WireFrame::Arinc429(word)) => {
                                self.capture_received(word);
                            }
                            Ok(WireFrame::Mil1553b(_)) => {
                                // The hub guarantees isolation; 1553B on the
                                // ARINC channel would be a harness fault.
                                debug_assert!(false, "1553B frame on ARINC channel");
                            }
                            Ok(WireFrame::Link16(_)) => {
                                // Isolating: link traffic must never land here.
                                debug_assert!(false, "Link16 frame on ARINC channel");
                            }
                            Err(broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(broadcast::error::RecvError::Closed) => break,
                        }
                    }
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::WordType;
    fn make_bus() -> Arc<VirtualBus> {
        let bus = VirtualBus::new(8);
        Arc::new(bus)
    }
    fn make_device(capture: Arc<std::sync::Mutex<Vec<Arinc429Frame>>>) -> SimulatedArinc429Device {
        SimulatedArinc429Device::new(10, 0, 2000, 3, Duration::from_millis(10), Some(capture))
    }
    #[tokio::test]
    async fn sends_configured_word_periodically() {
        let bus = make_bus();
        let capture = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut device = make_device(Arc::clone(&capture));
        tokio::spawn(async move { device.run(&bus.clone()).await });
        tokio::time::pause();
        // `interval` ticks immediately on first await; four 10 ms ticks give
        // four emissions once the clock resumes past the burst.
        for _ in 0..4 {
            tokio::time::advance(Duration::from_millis(10)).await;
        }
        let frames = capture.lock().unwrap().clone();
        assert!(!frames.is_empty());
        for frame in &frames {
            assert_eq!(frame.label, 10);
            assert_eq!(frame.sdi, 0);
            assert_eq!(frame.data, 2000);
            assert_eq!(frame.ssm, 3);
        }
    }
    #[tokio::test]
    async fn receives_and_captures_arinc_frames() {
        let bus = make_bus();
        let capture = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut device = make_device(Arc::clone(&capture));
        let device_bus = Arc::clone(&bus);
        tokio::spawn(async move { device.run(&device_bus).await });
        // Let the spawned task reach `subscribe_wire` before we publish.
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        let raw = arinc429::encode(5, 0, 1234, 1).unwrap();
        bus.publish_wire(crate::wire::WireFrame::Arinc429(
            crate::wire::Arinc429Word::new(raw).unwrap(),
        ))
        .unwrap();
        // Give the device's `select!` loop polls so it reaches `rx.recv()`
        // and drains the queued frame (the receive path ignores the clock).
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        let frames = capture.lock().unwrap().clone();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].label, 5);
        assert_eq!(frames[0].data, 1234);
        assert_eq!(frames[0].ssm, 1);
    }
    #[tokio::test]
    async fn filters_foreign_protocol_traffic() {
        let bus = make_bus();
        let capture = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut device = make_device(Arc::clone(&capture));
        let device_bus = Arc::clone(&bus);
        tokio::spawn(async move { device.run(&device_bus).await });
        // Let the device subscribe so its channel has an audience.
        tokio::task::yield_now().await;
        // 1553B and Link 16 subscribers keep those publishes happy while the
        // ARINC device listens only on its own channel.
        let _legacy = bus.subscribe();
        let _link_rx = bus.subscribe_wire(crate::wire::ProtocolId::Link16);
        bus.publish_wire(crate::wire::WireFrame::Mil1553b(
            crate::frame::Frame {
                sequence: 0,
                word_type: WordType::Data,
                word: 0,
                source: 0,
                corrupted: false,
            },
        ))
        .unwrap();
        bus.publish_wire(crate::wire::WireFrame::Link16(
            crate::wire::Link16Frame::new(0, chrono::Utc::now(), protocol_core::Link16Message::new(3, 2, 10, vec![]))
                .unwrap(),
        ))
        .unwrap();
        // Let the device process; it must capture none of these.
        tokio::task::yield_now().await;
        let frames = capture.lock().unwrap().clone();
        assert!(frames.is_empty());
    }
}
