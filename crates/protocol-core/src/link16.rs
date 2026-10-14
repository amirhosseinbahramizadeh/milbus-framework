//! Link 16 J-series messaging and TDMA network simulation.
//!
//! [`Link16Message`] models a J-series exchange transfer with a label,
//! sublabel, source track number (STN), and payload. [`TimeSlot`] models a
//! single TDMA slot (start time, duration, assigned terminal), and
//! [`Link16Network`] wires terminals into a round-robin TDMA frame in which a
//! terminal may only transmit during its own assigned slot.
//!
//! Time handling uses [`chrono`]'s [`DateTime<Utc>`] so the network can be
//! driven deterministically in tests.

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};

/// Smallest valid Link 16 track number.
pub const MIN_TRACK_NUMBER: u8 = 1;
/// Largest valid Link 16 track number.
pub const MAX_TRACK_NUMBER: u8 = 127;

/// Errors produced by Link 16 operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Link16Error {
    /// Track number outside the legal 1..=127 range.
    #[error("track number {0} out of range (must be 1..=127)")]
    InvalidStn(u8),
    /// The network was created with an empty terminal list.
    #[error("no terminals configured")]
    NoTerminals,
    /// The message's source terminal is not part of the network.
    #[error("terminal {0} is not part of this network")]
    UnknownTerminal(u8),
    /// The requested instant does not fall inside any scheduled slot.
    #[error("no assigned slot at time {0}")]
    NoSlotAtTime(DateTime<Utc>),
    /// A terminal attempted to transmit outside its assigned slot.
    #[error("terminal {stn} may not transmit at {at}: outside its assigned slot")]
    OutsideAssignedSlot {
        /// Track number of the offending terminal.
        stn: u8,
        /// Instant at which the illegal transmission was attempted.
        at: DateTime<Utc>,
    },
    /// The message's source STN does not match the claimed transmitting STN.
    #[error("message source {message_stn} does not match transmitter {stn}")]
    StnMismatch {
        /// Track number the transmission was claimed for.
        stn: u8,
        /// Track number carried inside the message itself.
        message_stn: u8,
    },
}

/// A Link 16 J-series message (exchange transfer).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link16Message {
    /// J-series label (e.g. the network/function nibble pair).
    pub j_series_label: u8,
    /// Sublabel refining the J-series label.
    pub sublabel: u8,
    /// Track number of the originating terminal.
    pub source_track_number: u8,
    /// Raw message payload bytes.
    pub payload: Vec<u8>,
}

impl Link16Message {
    /// Build a new J-series message.
    pub fn new(
        j_series_label: u8,
        sublabel: u8,
        source_track_number: u8,
        payload: Vec<u8>,
    ) -> Self {
        Self {
            j_series_label,
            sublabel,
            source_track_number,
            payload,
        }
    }
}

/// A single TDMA time slot: when it starts, how long it lasts, and which
/// terminal (track number) owns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeSlot {
    /// Slot start instant (UTC).
    pub start_time: DateTime<Utc>,
    /// Slot length.
    pub duration: TimeDelta,
    /// Track number of the terminal that owns this slot (`None` = unassigned).
    pub assigned_terminal: Option<u8>,
}

impl TimeSlot {
    /// Instant at which this slot ends (exclusive).
    pub fn end_time(&self) -> DateTime<Utc> {
        self.start_time + self.duration
    }

    /// Whether `at` falls inside this half-open slot interval
    /// `[start_time, end_time)`.
    pub fn contains(&self, at: DateTime<Utc>) -> bool {
        at >= self.start_time && at < self.end_time()
    }

    /// Whether the slot is currently unowned.
    pub fn is_unassigned(&self) -> bool {
        self.assigned_terminal.is_none()
    }
}

/// A simulated Link 16 TDMA network.
///
/// The network owns a round-robin frame: each terminal is assigned one slot per
/// frame, in the order given by `terminals`. A terminal may only transmit during
/// its own slot. [`schedule_message`](Self::schedule_message) assigns a message
/// to the next free slot owned by the message's source terminal, growing the
/// schedule by a frame when every existing slot for that terminal is occupied.
pub struct Link16Network {
    /// Track numbers of the terminals on the network, in frame order.
    pub terminals: Vec<u8>,
    /// The scheduled TDMA slots, in chronological order.
    pub slots: Vec<TimeSlot>,
    /// Message scheduled in each slot, parallel to `slots`.
    scheduled: Vec<Option<Link16Message>>,
    /// Instant of the very first slot.
    frame_start: DateTime<Utc>,
    /// Length of each slot.
    slot_duration: TimeDelta,
}

fn validate_stn(stn: u8) -> Result<(), Link16Error> {
    if (MIN_TRACK_NUMBER..=MAX_TRACK_NUMBER).contains(&stn) {
        Ok(())
    } else {
        Err(Link16Error::InvalidStn(stn))
    }
}

impl Link16Network {
    /// Create a network with the given terminals and a single round-robin frame
    /// starting at `frame_start`.
    pub fn new(
        terminals: Vec<u8>,
        frame_start: DateTime<Utc>,
        slot_duration: TimeDelta,
    ) -> Result<Self, Link16Error> {
        if terminals.is_empty() {
            return Err(Link16Error::NoTerminals);
        }
        for &terminal in &terminals {
            validate_stn(terminal)?;
        }

        let mut network = Self {
            terminals,
            slots: Vec::new(),
            scheduled: Vec::new(),
            frame_start,
            slot_duration,
        };
        network.extend_frame();
        Ok(network)
    }

    /// Append one more round-robin frame of slots to the schedule.
    fn extend_frame(&mut self) {
        let terminals_count = self.terminals.len();
        let base = self.slots.len();
        for offset in 0..terminals_count {
            let index = base + offset;
            let terminal = self.terminals[offset];
            self.slots.push(TimeSlot {
                start_time: self.frame_start + self.slot_duration * (index as i32),
                duration: self.slot_duration,
                assigned_terminal: Some(terminal),
            });
            self.scheduled.push(None);
        }
    }

    /// Assign `message` to the next free slot owned by its source terminal.
    ///
    /// Returns the index of the scheduled slot. Grows the schedule by a frame
    /// if the terminal currently has no free slot.
    pub fn schedule_message(&mut self, message: Link16Message) -> Result<usize, Link16Error> {
        let stn = message.source_track_number;
        validate_stn(stn)?;
        if !self.terminals.contains(&stn) {
            return Err(Link16Error::UnknownTerminal(stn));
        }

        let slot_index = self.free_slot_for(stn);
        self.scheduled[slot_index] = Some(message);
        Ok(slot_index)
    }

    /// Index of the next free slot owned by `stn`, extending the frame if
    /// necessary. `stn` must be a member of the network.
    fn free_slot_for(&mut self, stn: u8) -> usize {
        if let Some(index) = (0..self.slots.len())
            .find(|&i| self.slots[i].assigned_terminal == Some(stn) && self.scheduled[i].is_none())
        {
            return index;
        }

        // Every existing slot for this terminal is full: add another frame.
        let base = self.slots.len();
        self.extend_frame();
        let offset = self
            .terminals
            .iter()
            .position(|&t| t == stn)
            .expect("stn membership checked by caller");
        base + offset
    }

    /// The slot in effect at `at`, if any.
    pub fn slot_at(&self, at: DateTime<Utc>) -> Option<&TimeSlot> {
        self.slots.iter().find(|slot| slot.contains(at))
    }

    /// Whether `stn` is permitted to transmit at `at` (i.e. `at` falls inside
    /// one of its assigned slots).
    pub fn can_transmit(&self, stn: u8, at: DateTime<Utc>) -> bool {
        self.slot_at(at).and_then(|slot| slot.assigned_terminal) == Some(stn)
    }

    /// Transmit `message` at instant `at`, enforcing TDMA discipline.
    ///
    /// Succeeds only when `at` lies inside a slot owned by `stn` and the
    /// message's source STN matches `stn`; returns the slot index otherwise an
    /// error describing the violation.
    pub fn transmit(
        &mut self,
        stn: u8,
        at: DateTime<Utc>,
        message: Link16Message,
    ) -> Result<usize, Link16Error> {
        if message.source_track_number != stn {
            return Err(Link16Error::StnMismatch {
                stn,
                message_stn: message.source_track_number,
            });
        }

        let index = self
            .slots
            .iter()
            .position(|slot| slot.contains(at))
            .ok_or(Link16Error::NoSlotAtTime(at))?;

        if self.slots[index].assigned_terminal != Some(stn) {
            return Err(Link16Error::OutsideAssignedSlot { stn, at });
        }

        self.scheduled[index] = Some(message);
        Ok(index)
    }

    /// The message scheduled in the slot at `index`, if any.
    pub fn scheduled_message(&self, index: usize) -> Option<&Link16Message> {
        self.scheduled.get(index).and_then(|m| m.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).unwrap()
    }

    fn one_second() -> TimeDelta {
        TimeDelta::seconds(1)
    }

    #[test]
    fn builds_round_robin_frame() {
        let net = Link16Network::new(vec![10, 20, 30], start(), one_second()).unwrap();
        assert_eq!(net.slots.len(), 3);
        assert_eq!(net.slots[0].assigned_terminal, Some(10));
        assert_eq!(net.slots[1].assigned_terminal, Some(20));
        assert_eq!(net.slots[2].assigned_terminal, Some(30));
        assert_eq!(net.slots[1].start_time, start() + one_second());
        assert_eq!(net.slots[0].end_time(), start() + one_second());
    }

    #[test]
    fn schedule_message_uses_the_terminals_own_slot() {
        let mut net = Link16Network::new(vec![10, 20, 30], start(), one_second()).unwrap();
        let msg = Link16Message::new(0x32, 0x01, 20, vec![0xDE, 0xAD]);
        let index = net.schedule_message(msg.clone()).unwrap();
        // Terminal 20 owns slot 1 in the first frame.
        assert_eq!(index, 1);
        assert_eq!(net.scheduled_message(index), Some(&msg));
    }

    #[test]
    fn schedule_message_grows_frame_when_terminals_slots_fill() {
        let mut net = Link16Network::new(vec![10, 20], start(), one_second()).unwrap();
        for _ in 0..3 {
            net.schedule_message(Link16Message::new(0x32, 0, 10, vec![]))
                .unwrap();
        }
        // Terminal 10 had slots 0 and 2 in two frames; the third message needed
        // a third frame, so the schedule grew.
        assert!(net.slots.len() >= 6);
    }

    #[test]
    fn transmit_is_rejected_outside_assigned_slot() {
        let mut net = Link16Network::new(vec![10, 20], start(), one_second()).unwrap();
        // Terminal 10 owns [0,1); transmitting at t=1s falls in terminal 20's slot.
        let at = start() + one_second();
        let msg = Link16Message::new(0x32, 0, 10, vec![1]);
        assert!(matches!(
            net.transmit(10, at, msg),
            Err(Link16Error::OutsideAssignedSlot { stn: 10, .. })
        ));
    }

    #[test]
    fn transmit_succeeds_within_assigned_slot() {
        let mut net = Link16Network::new(vec![10, 20], start(), one_second()).unwrap();
        let at = start() + TimeDelta::milliseconds(500);
        let msg = Link16Message::new(0x32, 0, 10, vec![7]);
        assert_eq!(net.transmit(10, at, msg.clone()), Ok(0));
        assert_eq!(net.scheduled_message(0), Some(&msg));
    }

    #[test]
    fn can_transmit_tracks_slot_ownership() {
        let net = Link16Network::new(vec![10, 20], start(), one_second()).unwrap();
        assert!(net.can_transmit(10, start()));
        assert!(!net.can_transmit(20, start()));
        assert!(net.can_transmit(20, start() + one_second()));
    }

    #[test]
    fn rejects_unknown_and_invalid_terminals() {
        assert!(matches!(
            Link16Network::new(vec![], start(), one_second()),
            Err(Link16Error::NoTerminals)
        ));
        assert!(matches!(
            Link16Network::new(vec![0], start(), one_second()),
            Err(Link16Error::InvalidStn(0))
        ));

        let mut net = Link16Network::new(vec![10], start(), one_second()).unwrap();
        assert!(matches!(
            net.schedule_message(Link16Message::new(0x32, 0, 99, vec![])),
            Err(Link16Error::UnknownTerminal(99))
        ));
    }
}
