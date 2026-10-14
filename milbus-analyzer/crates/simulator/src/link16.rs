//! Link 16 TDMA device role for the multi-channel virtual bus.
//!
//! [`SimulatedLink16Device`] is a single node that both transmits and receives
//! Link 16 J-series traffic on the multi-channel hub:
//!
//! * **send** — it emits one [`Link16Frame`] per TDMA slot, stamped with the
//!   transmission instant (`transmitted_at`) and assigned a monotonically
//!   increasing slot index. Slots repeat on `slot_duration`, so a parked bus
//!   ticks the schedule deterministically even under Tokio's paused clock.
//! * **receive** — it subscribes to the Link 16 channel and, when a capture log
//!   is attached, stores incoming [`Link16Frame`]s exactly as the 1553B
//!   monitor stores `Frame`s.
//!
//! The role uses [`protocol_core::Link16Message`] and
//! [`protocol_core::Link16Frame`] for validated capture, keeping Link 16
//! traffic typed and protocol-correct instead of leaking it through 1553B
//! [`Frame`]s.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use protocol_core::Link16Message;
use tokio::sync::broadcast;
use tokio::time::{interval, MissedTickBehavior};

use std::future::Future;

use crate::bus::VirtualBus;
use crate::device::SimulatedDevice;
use crate::wire::{Link16Frame, ProtocolId, WireFrame};

/// A simulated Link 16 node: a TDMA terminal that transmits on its own slot
/// and listens for frames on every other slot.
///
/// `stn` is the source track number this node claims (1..=127). `j_series_label`,
/// `sublabel`, and `payload` define the J-series message it emits every slot.
/// `capture_log` (if set) records every received frame the way the 1553B monitor
/// records `Frame`s.
#[derive(Debug, Clone)]
pub struct SimulatedLink16Device {
    /// Source track number (STN) this node claims, 1..=127.
    pub stn: u8,
    /// J-series label emitted every slot.
    pub j_series_label: u8,
    /// Sublabel refining the J-series label.
    pub sublabel: u8,
    /// J-series message payload bytes.
    pub payload: Vec<u8>,
    /// Duration of one TDMA slot (how often the node transmits).
    pub slot_duration: Duration,
    /// Instant of the first slot; subsequent slots advance by `slot_duration`.
    pub start_at: DateTime<Utc>,
    /// Optional sink for received frames (mirrors the 1553B monitor log).
    pub capture_log: Option<Arc<std::sync::Mutex<Vec<Link16Frame>>>>,
}

impl SimulatedLink16Device {
    /// Build a new Link 16 TDMA node.
    ///
    /// `capture_log` may be `None`; received frames are silently dropped.
    pub fn new(
        stn: u8,
        j_series_label: u8,
        sublabel: u8,
        payload: Vec<u8>,
        slot_duration: Duration,
        start_at: DateTime<Utc>,
        capture_log: Option<Arc<std::sync::Mutex<Vec<Link16Frame>>>>,
    ) -> Self {
        Self {
            stn,
            j_series_label,
            sublabel,
            payload,
            slot_duration,
            start_at,
            capture_log,
        }
    }

    /// Advance the schedule by one slot duration (reused in tests to compute a
    /// slot instant without awaiting).
    pub fn offset_for_slot(slot_duration: Duration) -> ChronoDuration {
        ChronoDuration::from_std(slot_duration).expect("Link16 slot duration must be non-negative")
    }

    /// Convenience constructor that stamps the first slot at the current instant.
    ///
    /// Useful for callers (like the CLI) that don't want to pull in `chrono`
    /// just to seed the schedule.
    pub fn new_now(
        stn: u8,
        j_series_label: u8,
        sublabel: u8,
        payload: Vec<u8>,
        slot_duration: Duration,
        capture_log: Option<Arc<std::sync::Mutex<Vec<Link16Frame>>>>,
    ) -> Self {
        Self::new(
            stn,
            j_series_label,
            sublabel,
            payload,
            slot_duration,
            Utc::now(),
            capture_log,
        )
    }
}

impl SimulatedDevice for SimulatedLink16Device {
    fn run(&mut self, bus: &VirtualBus) -> impl Future<Output = ()> + Send {
        async move {
            let mut rx = bus.subscribe_wire(ProtocolId::Link16);
            let mut slot_timer = interval(self.slot_duration);
            slot_timer.set_missed_tick_behavior(MissedTickBehavior::Burst);

            let mut transmitted_at = self.start_at;
            let mut slot_index = 0usize;

            loop {
                tokio::select! {
                    _ = slot_timer.tick() => {
                        let message = Link16Message::new(
                            self.j_series_label,
                            self.sublabel,
                            self.stn,
                            self.payload.clone(),
                        );
                        let frame = Link16Frame::new(slot_index, transmitted_at, message);
                        match frame {
                            Ok(frame) => {
                                let _ = bus.publish_wire(WireFrame::Link16(frame));
                                transmitted_at += Self::offset_for_slot(self.slot_duration);
                                slot_index += 1;
                            }
                            Err(_) => {
                                // STN or payload bound rejected the frame; drop
                                // this slot's transmission without advancing.
                            }
                        }
                    }
                    frame = rx.recv() => {
                        match frame {
                            Ok(WireFrame::Link16(link)) => {
                                if let Some(log) = &self.capture_log {
                                    log.lock().expect("capture log mutex poisoned").push(link);
                                }
                            }
                            Ok(WireFrame::Arinc429(_)) => {
                                debug_assert!(false, "ARINC frame on Link16 channel");
                            }
                            Ok(WireFrame::Mil1553b(_)) => {
                                debug_assert!(false, "1553B frame on Link16 channel");
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

    fn make_device(capture: Arc<std::sync::Mutex<Vec<Link16Frame>>>) -> SimulatedLink16Device {
        SimulatedLink16Device::new(
            7,
            3,
            2,
            vec![0x10, 0x20, 0x30],
            Duration::from_millis(20),
            chrono::Utc::now(),
            Some(capture),
        )
    }

    #[tokio::test]
    async fn sends_configured_message_periodically() {
        let bus = make_bus();
        let capture = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut device = make_device(Arc::clone(&capture));
        tokio::spawn(async move { device.run(&bus.clone()).await });

        tokio::time::pause();
        for _ in 0..4 {
            tokio::time::advance(Duration::from_millis(20)).await;
        }

        let frames = capture.lock().unwrap().clone();
        assert!(!frames.is_empty());
        for frame in &frames {
            assert_eq!(frame.message.source_track_number, 7);
            assert_eq!(frame.message.j_series_label, 3);
            assert_eq!(frame.message.sublabel, 2);
            assert_eq!(frame.message.payload, vec![0x10, 0x20, 0x30]);
        }
    }

    #[tokio::test]
    async fn receives_and_captures_link16_frames() {
        let bus = make_bus();
        let capture = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut device = make_device(Arc::clone(&capture));
        let device_bus = Arc::clone(&bus);
        tokio::spawn(async move { device.run(&device_bus).await });
        // Let the spawned task reach `subscribe_wire` before we publish.
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;

        let now = chrono::Utc::now();
        let link = crate::wire::Link16Frame::new(0, now, protocol_core::Link16Message::new(3, 2, 10, vec![0xaa, 0xbb])).unwrap();
        bus.publish_wire(crate::wire::WireFrame::Link16(link)).unwrap();

        // The receive path ignores the clock; polls suffice to drain it.
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;

        let frames = capture.lock().unwrap().clone();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].message.source_track_number, 10);
        assert_eq!(frames[0].message.payload, vec![0xaa, 0xbb]);
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
        // ARINC 429 and 1553B subscribers keep those publishes happy while the
        // Link 16 device listens only on its own channel.
        let _arinc_rx = bus.subscribe_wire(crate::wire::ProtocolId::Arinc429);
        let _legacy = bus.subscribe();

        bus.publish_wire(crate::wire::WireFrame::Arinc429(
            crate::wire::Arinc429Word::new(arinc429::encode(10, 0, 2000, 3).unwrap()).unwrap(),
        ))
        .unwrap();
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

        // Let the device process; it must capture none of these.
        tokio::task::yield_now().await;
        let frames = capture.lock().unwrap().clone();
        assert!(frames.is_empty());
    }
}

