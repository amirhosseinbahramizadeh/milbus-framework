//! Virtual bus broadcasting raw MIL-STD-1553B words to all participants.

use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::broadcast;

use crate::frame::{Frame, WordType};
use crate::wire::{ProtocolId, WireFrame};

/// Errors from publishing onto a protocol channel.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PublishError {
    /// Nothing is listening on this protocol channel.
    #[error("no subscribers on protocol channel {0}")]
    ChannelEmpty(ProtocolId),
}

/// A virtual MIL-STD-1553B bus, extended as a protocol-tagged multi-channel hub.
///
/// The legacy `Frame` channel (`subscribe` / `publish` / `send_word` /
/// `subscriber_count`) is unchanged: devices written against it behave exactly
/// as before. In addition, the hub carries one typed broadcast channel per
/// [`ProtocolId`]: [`subscribe_wire`] and [`publish_wire`] route
/// [`WireFrame`]s so ARINC 429 and Link 16 devices never observe 1553B words
/// and vice versa.
pub struct VirtualBus {
    tx: broadcast::Sender<Frame>,
    arinc_tx: broadcast::Sender<WireFrame>,
    link_tx: broadcast::Sender<WireFrame>,
    /// Tag-aware mirror of the legacy 1553B `Frame` channel, so wire
    /// subscribers can observe `WireFrame::Mil1553b` without disturbing the
    /// pre-existing `broadcast::Sender<Frame>` behaviour.
    mil_wire_tx: broadcast::Sender<WireFrame>,
    sequence: AtomicU64,
}

impl VirtualBus {
    /// Create a bus with room for `capacity` in-flight frames per receiver.
    pub fn new(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity.max(1));
        let (arinc_tx, _arinc_rx) = broadcast::channel(capacity.max(1));
        let (link_tx, _link_rx) = broadcast::channel(capacity.max(1));
        let (mil_wire_tx, _mil_wire_rx) = broadcast::channel(capacity.max(1));
        Self {
            tx,
            arinc_tx,
            link_tx,
            mil_wire_tx,
            sequence: AtomicU64::new(0),
        }
    }

    /// Subscribe a new device to the bus.
    pub fn subscribe(&self) -> broadcast::Receiver<Frame> {
        self.tx.subscribe()
    }

    /// Publish a pre-built frame to all subscribers.
    ///
    /// Returns the number of receivers the frame was delivered to.
    pub fn publish(&self, frame: Frame) -> Result<usize, broadcast::error::SendError<Frame>> {
        self.tx.send(frame)
    }

    /// Build a frame from a raw word and publish it to all subscribers.
    pub fn send_word(
        &self,
        word_type: WordType,
        word: u16,
        source: u8,
    ) -> Result<usize, broadcast::error::SendError<Frame>> {
        self.publish(Frame {
            sequence: self.next_sequence(),
            word_type,
            word,
            source,
            corrupted: false,
        })
    }

    /// Allocate the next unique frame sequence number.
    pub fn next_sequence(&self) -> u64 {
        self.sequence.fetch_add(1, Ordering::Relaxed)
    }

    /// Number of devices currently subscribed to the bus.
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }

    /// Subscribe to one protocol channel of the multi-channel hub.
    ///
    /// MIL-STD-1553B subscribers keep using [`VirtualBus::subscribe`]; this is
    /// the typed entry point for ARINC 429 and Link 16 devices. Each call
    /// returns a receiver that observes only that protocol's traffic.
    ///
    /// 1553B wire traffic is observed through [`publish_wire`] on the same
    /// channel, so this entry point is isolation-safe for every protocol.
    pub fn subscribe_wire(&self, protocol: ProtocolId) -> broadcast::Receiver<WireFrame> {
        match protocol {
            ProtocolId::Mil1553b => self.mil_wire_tx.subscribe(),
            ProtocolId::Arinc429 => self.arinc_tx.subscribe(),
            ProtocolId::Link16 => self.link_tx.subscribe(),
        }
    }

    /// Publish a protocol-tagged frame onto its channel.
    ///
    /// Returns the number of receivers the frame was delivered to.
    /// `Mil1553b` frames are also mirrored onto the legacy `Frame` channel so
    /// pre-existing subscribers keep working.
    ///
    /// # Errors
    ///
    /// Returns [`PublishError::ChannelEmpty`] when nothing listens on that
    /// protocol channel.
    pub fn publish_wire(&self, frame: WireFrame) -> Result<usize, PublishError> {
        match frame {
            WireFrame::Mil1553b(inner) => {
                let mirrored = inner.clone();
                let count = self
                    .tx
                    .send(inner)
                    .map_err(|_| PublishError::ChannelEmpty(ProtocolId::Mil1553b))?;
                // Mirror onto the tag-aware side; ignore a missing wire
                // audience since the legacy delivery already succeeded.
                let _ = self.mil_wire_tx.send(WireFrame::Mil1553b(mirrored));
                Ok(count)
            }
            WireFrame::Arinc429(word) => self
                .arinc_tx
                .send(WireFrame::Arinc429(word))
                .map_err(|_| PublishError::ChannelEmpty(ProtocolId::Arinc429)),
            WireFrame::Link16(link) => self
                .link_tx
                .send(WireFrame::Link16(link))
                .map_err(|_| PublishError::ChannelEmpty(ProtocolId::Link16)),
        }
    }

    /// Number of devices subscribed to the given protocol channel.
    pub fn wire_subscriber_count(&self, protocol: ProtocolId) -> usize {
        match protocol {
            ProtocolId::Mil1553b => self.mil_wire_tx.receiver_count(),
            ProtocolId::Arinc429 | ProtocolId::Link16 => self.link_tx.receiver_count(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_fanned_out_to_all_subscribers() {
        let bus = VirtualBus::new(8);
        let mut first = bus.subscribe();
        let mut second = bus.subscribe();
        assert_eq!(bus.subscriber_count(), 2);

        let delivered = bus.send_word(WordType::Data, 0xBEEF, 3).unwrap();
        assert_eq!(delivered, 2);

        let a = first.try_recv().unwrap();
        let b = second.try_recv().unwrap();
        assert_eq!(a, b);
        assert_eq!(a.word, 0xBEEF);
        assert_eq!(a.sequence, 0);
        assert_eq!(bus.next_sequence(), 1);
    }

    #[tokio::test]
    async fn wire_channels_isolate_protocols() {
        use crate::wire::Link16Frame;
        use crate::wire::{Arinc429Word, ProtocolId, WireFrame};
        use chrono::DateTime;
        use protocol_core::Link16Message;

        let bus = VirtualBus::new(8);
        // 1553B legacy traffic keeps flowing on the original entry points...
        let mut legacy = bus.subscribe();
        let delivered = bus
            .send_word(crate::frame::WordType::Data, 0xBEEF, 3)
            .unwrap();
        assert_eq!(delivered, 1);
        assert_eq!(legacy.try_recv().unwrap().word, 0xBEEF);

        // ...and publishing a 1553B wire frame also reaches legacy listeners.
        let _wire_rx = bus.subscribe_wire(ProtocolId::Mil1553b);
        let published = Frame {
            sequence: bus.next_sequence(),
            word_type: crate::frame::WordType::Data,
            word: 0xCAFE,
            source: 9,
            corrupted: false,
        };
        assert_eq!(
            bus.publish_wire(WireFrame::Mil1553b(published)).unwrap(),
            1
        );
        assert_eq!(legacy.try_recv().unwrap().word, 0xCAFE);

        // ARINC 429 traffic never touches the Link 16 (or 1553B) channel.
        let raw = arinc429::encode(10, 0, 2000, 3).unwrap();
        let mut arinc_rx = bus.subscribe_wire(ProtocolId::Arinc429);
        let mut link_rx = bus.subscribe_wire(ProtocolId::Link16);
        let sent = bus
            .publish_wire(WireFrame::Arinc429(Arinc429Word::new(raw).unwrap()))
            .unwrap();
        assert_eq!(sent, 1);
        assert!(matches!(
            arinc_rx.try_recv().unwrap(),
            WireFrame::Arinc429(_)
        ));
        let _ = _wire_rx;
        assert!(link_rx.try_recv().is_err());

        // Publishing with no audience reports the channel, not a panic.
        let lonely = VirtualBus::new(4);
        let at = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let link = Link16Frame::new(0, at, Link16Message::new(3, 2, 10, vec![])).unwrap();
        assert_eq!(
            lonely.publish_wire(WireFrame::Link16(link)),
            Err(PublishError::ChannelEmpty(ProtocolId::Link16))
        );
        assert_eq!(
            lonely.publish_wire(WireFrame::Arinc429(Arinc429Word::new(raw).unwrap())),
            Err(PublishError::ChannelEmpty(ProtocolId::Arinc429))
        );
    }
}
