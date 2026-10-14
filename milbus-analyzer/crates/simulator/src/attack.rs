//! Frame injection and bus-attack simulation.
//!
//! This module models an adversary attached to the virtual MIL-STD-1553B bus.
//! The [`Attacker`] injects arbitrary frames into a [`VirtualBus`]; every
//! attack is recorded in a shared [`AttackLog`] guarded by a
//! [`tokio::sync::RwLock`], so the TUI can read the log concurrently while the
//! attacker writes to it.
//!
//! Attacks are driven either by calling the individual methods directly or by
//! spawning [`Attacker::run`] and controlling it over an [`AttackCommand`]
//! channel from the TUI.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;
use tokio::time::{timeout, Instant};

use crate::bus::VirtualBus;
use crate::frame::{
    status_word, CommandWord, Frame, WordType, BUS_CONTROLLER_ADDRESS, DEFAULT_DATA_WORDS,
};
use crate::wire::{Arinc429Word, Link16Frame, WireFrame};
use protocol_core::{Arinc429Frame, Link16Message, MAX_TRACK_NUMBER, MIN_TRACK_NUMBER};

/// Subaddress used by injected fake command words.
const INJECT_SUBADDRESS: u8 = 1;

/// Pause between flooded frames.
///
/// Kept non-zero so the Tokio clock can advance under `tokio::time::pause`,
/// making flood duration deterministic in tests.
const FLOOD_TICK: Duration = Duration::from_millis(1);

/// The kind of bus attack to carry out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttackType {
    /// Inject a command word that targets a spoofed RT address.
    FakeCommand,
    /// Continuously inject frames to saturate the bus.
    Flood,
    /// Inject a malformed status word to exercise error handling.
    CorruptStatus,
    /// Inject a well-formed ARINC 429 word impersonating a sensor at a label.
    Arinc429Spoof,
    /// Inject a well-formed Link 16 message impersonating a terminal (STN).
    Link16Spoof,
}

impl AttackType {
    /// Human-readable label for logs and the TUI.
    pub fn label(self) -> &'static str {
        match self {
            AttackType::FakeCommand => "FakeCommand",
            AttackType::Flood => "Flood",
            AttackType::CorruptStatus => "CorruptStatus",
            AttackType::Arinc429Spoof => "Arinc429Spoof",
            AttackType::Link16Spoof => "Link16Spoof",
        }
    }
}

/// Errors produced by the attacker.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AttackError {
    /// The requested target is not a legal remote-terminal address.
    #[error("target address {0} is out of range (must be 0..=30)")]
    InvalidAddress(u8),
    /// Nothing is listening on the bus, so injection would be a no-op.
    #[error("virtual bus has no subscribed devices to inject into")]
    BusEmpty,
    /// ARINC 429 sign/status matrix outside the legal 0..=3 range.
    #[error("ARINC 429 SSM {0} is out of range (must be 0..=3)")]
    InvalidSsm(u8),
    /// The ARINC 429 reference encoder rejected the spoofed word.
    #[error("failed to encode the spoofed ARINC 429 word")]
    EncodingFailed,
    /// Link 16 track number outside the legal 1..=127 range.
    #[error("track number {0} is out of range (must be 1..=127)")]
    InvalidStn(u8),
    /// Link 16 payload exceeds the wire bound, so it cannot be scheduled.
    #[error("Link 16 payload exceeds the {0}-byte wire bound")]
    PayloadTooLarge(usize),
}

/// Configuration describing a single attack run.
///
/// `target_address` is the per-protocol target: a remote-terminal address for
/// MIL-STD-1553B attacks, the ARINC 429 label for [`AttackType::Arinc429Spoof`],
/// or the Link 16 source track number for [`AttackType::Link16Spoof`]. The
/// `arinc_*` and `link16_payload` fields only carry meaning for their matching
/// attack type and are ignored by the others.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttackConfig {
    /// Which attack to carry out.
    pub attack_type: AttackType,
    /// How long the attack runs (flood only; ignored by one-shot attacks).
    pub duration: Duration,
    /// Remote-terminal address / ARINC label / Link 16 STN the attack targets.
    pub target_address: u8,
    /// ARINC 429 19-bit data field (`Arinc429Spoof` only).
    pub arinc_data: u32,
    /// ARINC 429 sign/status matrix, 0..=3 (`Arinc429Spoof` only).
    pub arinc_ssm: u8,
    /// Link 16 J-series payload bytes (`Link16Spoof` only).
    pub link16_payload: Vec<u8>,
}

impl AttackConfig {
    /// A one-shot fake-command injection (`duration` is unused).
    pub fn fake_command(target_address: u8) -> Self {
        Self {
            attack_type: AttackType::FakeCommand,
            duration: Duration::ZERO,
            target_address,
            arinc_data: 0,
            arinc_ssm: 3,
            link16_payload: Vec::new(),
        }
    }

    /// A one-shot malformed-status injection (`duration` is unused).
    pub fn corrupt_status(target_address: u8) -> Self {
        Self {
            attack_type: AttackType::CorruptStatus,
            duration: Duration::ZERO,
            target_address,
            arinc_data: 0,
            arinc_ssm: 3,
            link16_payload: Vec::new(),
        }
    }

    /// A bus flood lasting `duration`.
    pub fn flood(target_address: u8, duration: Duration) -> Self {
        Self {
            attack_type: AttackType::Flood,
            duration,
            target_address,
            arinc_data: 0,
            arinc_ssm: 3,
            link16_payload: Vec::new(),
        }
    }

    /// A one-shot spoofed ARINC 429 word impersonating a sensor at `label`.
    ///
    /// `target_address` carries `label`. `ssm` must be within 0..=3.
    pub fn arinc429_spoof(label: u8, data: u32, ssm: u8) -> Self {
        Self {
            attack_type: AttackType::Arinc429Spoof,
            duration: Duration::ZERO,
            target_address: label,
            arinc_data: data,
            arinc_ssm: ssm,
            link16_payload: Vec::new(),
        }
    }

    /// A one-shot spoofed Link 16 message impersonating a terminal at `stn`.
    ///
    /// `target_address` carries `stn` (1..=127); `payload` is the J-series body
    /// and must fit within the wire bound.
    pub fn link16_spoof(stn: u8, payload: Vec<u8>) -> Self {
        Self {
            attack_type: AttackType::Link16Spoof,
            duration: Duration::ZERO,
            target_address: stn,
            arinc_data: 0,
            arinc_ssm: 3,
            link16_payload: payload,
        }
    }
}

/// One recorded attack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttackRecord {
    /// When the attack was carried out (respects the paused Tokio clock).
    pub timestamp: Instant,
    /// Which attack was carried out.
    pub attack_type: AttackType,
    /// Remote-terminal address the attack targeted.
    pub target_address: u8,
}

/// A log of every attack, safe for concurrent readers (the TUI) and a writer
/// (the attacker) via an [`RwLock`].
#[derive(Debug, Default)]
pub struct AttackLog {
    records: RwLock<Vec<AttackRecord>>,
}

impl AttackLog {
    /// Create an empty log.
    pub fn new() -> Self {
        Self {
            records: RwLock::new(Vec::new()),
        }
    }

    /// Append an attack to the log, returning the stored record.
    pub async fn record(&self, attack_type: AttackType, target_address: u8) -> AttackRecord {
        let record = AttackRecord {
            timestamp: Instant::now(),
            attack_type,
            target_address,
        };
        self.records.write().await.push(record.clone());
        record
    }

    /// Snapshot of all recorded attacks, oldest first.
    pub async fn records(&self) -> Vec<AttackRecord> {
        self.records.read().await.clone()
    }

    /// Number of recorded attacks.
    pub async fn len(&self) -> usize {
        self.records.read().await.len()
    }

    /// Whether no attacks have been recorded.
    pub async fn is_empty(&self) -> bool {
        self.records.read().await.is_empty()
    }

    /// Drop all recorded attacks.
    pub async fn clear(&self) {
        self.records.write().await.clear();
    }
}

/// Commands a TUI can send to start or stop attacks over a channel.
#[derive(Debug, Clone)]
pub enum AttackCommand {
    /// Start (or restart) the configured attack.
    Run(AttackConfig),
    /// Stop the currently running attack, if any.
    Stop,
}

/// An adversary attached to the virtual bus that injects arbitrary frames.
#[derive(Clone)]
pub struct Attacker {
    bus: Arc<VirtualBus>,
    log: Arc<AttackLog>,
}

impl Attacker {
    /// Create an attacker injecting into `bus` and recording into `log`.
    ///
    /// Give the TUI a clone of [`Attacker::log`] so it can read attacks
    /// concurrently while this attacker writes to the shared log.
    pub fn new(bus: Arc<VirtualBus>, log: Arc<AttackLog>) -> Self {
        Self { bus, log }
    }

    /// Shared handle to the attack log (hand this to the TUI).
    pub fn log(&self) -> Arc<AttackLog> {
        Arc::clone(&self.log)
    }

    /// Shared handle to the virtual bus.
    pub fn bus(&self) -> Arc<VirtualBus> {
        Arc::clone(&self.bus)
    }

    fn validate_target(&self, target_address: u8) -> Result<(), AttackError> {
        if target_address >= BUS_CONTROLLER_ADDRESS {
            Err(AttackError::InvalidAddress(target_address))
        } else {
            Ok(())
        }
    }

    fn ensure_bus_has_listeners(&self) -> Result<(), AttackError> {
        if self.bus.subscriber_count() == 0 {
            Err(AttackError::BusEmpty)
        } else {
            Ok(())
        }
    }

    /// Inject a fake command word that targets a spoofed RT address.
    ///
    /// The frame is emitted as if from the bus controller (source
    /// [`BUS_CONTROLLER_ADDRESS`]) with the command word's RT-address field set
    /// to `target_address`, impersonating a legitimate poll of that terminal.
    pub async fn inject_fake_command(&self, target_address: u8) -> Result<(), AttackError> {
        self.validate_target(target_address)?;
        self.ensure_bus_has_listeners()?;

        let command = CommandWord::new(
            target_address,
            true, // request the RT to transmit
            INJECT_SUBADDRESS,
            DEFAULT_DATA_WORDS,
        );
        let frame = Frame {
            sequence: self.bus.next_sequence(),
            word_type: WordType::Command,
            word: command.to_word(),
            source: BUS_CONTROLLER_ADDRESS,
            corrupted: false,
        };
        self.bus.publish(frame).map_err(|_| AttackError::BusEmpty)?;
        self.log
            .record(AttackType::FakeCommand, target_address)
            .await;
        Ok(())
    }

    /// Continuously inject data frames for `duration` to saturate the bus.
    ///
    /// Frames carry a distinct sequence and a rolling payload; each is marked
    /// `corrupted` to distinguish flood traffic from legitimate messages. The
    /// loop yields to a short timer so the paused Tokio clock can advance,
    /// making the flood duration deterministic under test.
    pub async fn flood_bus(
        &self,
        duration: Duration,
        target_address: u8,
    ) -> Result<(), AttackError> {
        self.validate_target(target_address)?;
        self.ensure_bus_has_listeners()?;

        let mut sent: u64 = 0;
        let _ = timeout(duration, async {
            loop {
                let frame = Frame {
                    sequence: self.bus.next_sequence(),
                    word_type: WordType::Data,
                    word: (sent & 0xFFFF) as u16,
                    source: target_address,
                    corrupted: true,
                };
                if self.bus.publish(frame).is_err() {
                    break; // bus dropped its last receiver
                }
                sent += 1;
                tokio::time::sleep(FLOOD_TICK).await;
            }
        })
        .await;

        self.log.record(AttackType::Flood, target_address).await;
        Ok(())
    }

    /// Inject a malformed status word to exercise downstream error handling.
    ///
    /// A well-formed status word keeps the reserved instrumentation bit (9)
    /// clear and reports only defined condition flags. This sets bit 9 and all
    /// low condition bits so validation must reject the word, and marks the
    /// frame `corrupted` at the transport level.
    pub async fn corrupt_status(&self, target_address: u8) -> Result<(), AttackError> {
        self.validate_target(target_address)?;
        self.ensure_bus_has_listeners()?;

        let word = status_word(target_address) | (1 << 9) | 0x00FF;
        let frame = Frame {
            sequence: self.bus.next_sequence(),
            word_type: WordType::Status,
            word,
            source: target_address,
            corrupted: true,
        };
        self.bus.publish(frame).map_err(|_| AttackError::BusEmpty)?;
        self.log
            .record(AttackType::CorruptStatus, target_address)
            .await;
        Ok(())
    }

    /// Inject a well-formed ARINC 429 word impersonating a sensor at `label`.
    ///
    /// The word is encoded through the reference crate so parity is correct and
    /// it passes every receiver's validation — the deception is the *identity*
    /// (label / data / SSM), not a framing error. `ssm` must be within 0..=3.
    /// The frame is published on the ARINC channel, so only ARINC listeners see
    /// it.
    pub async fn inject_arinc429_spoof(
        &self,
        label: u8,
        sdi: u8,
        data: u32,
        ssm: u8,
    ) -> Result<(), AttackError> {
        if ssm > 3 {
            return Err(AttackError::InvalidSsm(ssm));
        }
        let frame = Arinc429Frame {
            label,
            sdi,
            data,
            ssm,
            parity: true,
        };
        let raw = frame.to_word().map_err(|_| AttackError::EncodingFailed)?;
        let word = Arinc429Word::new(raw).map_err(|_| AttackError::EncodingFailed)?;
        self.bus
            .publish_wire(WireFrame::Arinc429(word))
            .map_err(|_| AttackError::BusEmpty)?;
        self.log.record(AttackType::Arinc429Spoof, label).await;
        Ok(())
    }

    /// Inject a well-formed Link 16 message impersonating a terminal at `stn`.
    ///
    /// `stn` must be a legal track number (1..=127) and `payload` must fit the
    /// wire bound; the message is stamped with the current instant and published
    /// on the Link 16 channel, so only Link 16 listeners see it.
    pub async fn inject_link16_spoof(
        &self,
        stn: u8,
        payload: Vec<u8>,
    ) -> Result<(), AttackError> {
        if !(MIN_TRACK_NUMBER..=MAX_TRACK_NUMBER).contains(&stn) {
            return Err(AttackError::InvalidStn(stn));
        }
        if payload.len() > Link16Frame::MAX_PAYLOAD_LEN {
            return Err(AttackError::PayloadTooLarge(Link16Frame::MAX_PAYLOAD_LEN));
        }
        let message = Link16Message::new(0, 0, stn, payload);
        let frame = Link16Frame::new(0, chrono::Utc::now(), message)
            .expect("STN and payload validated above");
        self.bus
            .publish_wire(WireFrame::Link16(frame))
            .map_err(|_| AttackError::BusEmpty)?;
        self.log.record(AttackType::Link16Spoof, stn).await;
        Ok(())
    }

    /// Run the attack described by `config`.
    pub async fn run_attack(&self, config: &AttackConfig) -> Result<(), AttackError> {
        match config.attack_type {
            AttackType::FakeCommand => self.inject_fake_command(config.target_address).await,
            AttackType::CorruptStatus => self.corrupt_status(config.target_address).await,
            AttackType::Flood => self.flood_bus(config.duration, config.target_address).await,
            AttackType::Arinc429Spoof => {
                self.inject_arinc429_spoof(
                    config.target_address,
                    0,
                    config.arinc_data,
                    config.arinc_ssm,
                )
                .await
            }
            AttackType::Link16Spoof => {
                self.inject_link16_spoof(config.target_address, config.link16_payload.clone())
                    .await
            }
        }
    }

    /// Drive the attacker from a command channel until the channel closes.
    ///
    /// [`AttackCommand::Run`] starts (or restarts) an attack on a background
    /// task; [`AttackCommand::Stop`] aborts the in-flight attack. This is how
    /// the TUI starts and stops attacks.
    pub async fn run(self, mut commands: mpsc::UnboundedReceiver<AttackCommand>) {
        let mut in_flight: Option<JoinHandle<()>> = None;
        while let Some(command) = commands.recv().await {
            match command {
                AttackCommand::Run(config) => {
                    if let Some(handle) = in_flight.take() {
                        handle.abort();
                    }
                    let attacker = self.clone();
                    in_flight = Some(tokio::spawn(async move {
                        let _ = attacker.run_attack(&config).await;
                    }));
                }
                AttackCommand::Stop => {
                    if let Some(handle) = in_flight.take() {
                        handle.abort();
                    }
                }
            }
        }
        if let Some(handle) = in_flight {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn inject_fake_command_targets_spoofed_rt_and_logs() {
        let bus = Arc::new(VirtualBus::new(16));
        let log = Arc::new(AttackLog::new());
        let attacker = Attacker::new(Arc::clone(&bus), Arc::clone(&log));
        let mut rx = bus.subscribe();

        attacker.inject_fake_command(5).await.unwrap();

        let frame = rx.try_recv().expect("fake command injected");
        assert_eq!(frame.word_type, WordType::Command);
        assert_eq!(frame.source, BUS_CONTROLLER_ADDRESS);
        let command = CommandWord::from_word(frame.word);
        assert_eq!(command.rt_address, 5);
        assert!(command.transmit);

        let records = log.records().await;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].attack_type, AttackType::FakeCommand);
        assert_eq!(records[0].target_address, 5);
    }

    #[tokio::test]
    async fn corrupt_status_sends_malformed_word() {
        let bus = Arc::new(VirtualBus::new(16));
        let log = Arc::new(AttackLog::new());
        let attacker = Attacker::new(Arc::clone(&bus), Arc::clone(&log));
        let mut rx = bus.subscribe();

        attacker.corrupt_status(3).await.unwrap();

        let frame = rx.try_recv().expect("corrupt status injected");
        assert_eq!(frame.word_type, WordType::Status);
        assert_eq!(frame.source, 3);
        assert!(frame.corrupted);
        // Reserved instrumentation bit (9) set => malformed.
        assert_eq!((frame.word >> 9) & 1, 1);
        // RT address field still encodes the target.
        assert_eq!((frame.word >> 11) & 0x1F, 3);

        assert_eq!(log.len().await, 1);
        assert_eq!(
            log.records().await[0].attack_type,
            AttackType::CorruptStatus
        );
    }

    #[tokio::test]
    async fn flood_bus_emits_frames_for_duration() {
        tokio::time::pause();
        let bus = Arc::new(VirtualBus::new(256));
        let log = Arc::new(AttackLog::new());
        let attacker = Attacker::new(Arc::clone(&bus), Arc::clone(&log));
        let mut rx = bus.subscribe();

        let handle = tokio::spawn({
            let attacker = attacker.clone();
            async move { attacker.flood_bus(Duration::from_millis(10), 2).await }
        });

        // Drive the paused clock so per-frame sleeps and the timeout elapse.
        for _ in 0..20 {
            tokio::time::advance(Duration::from_millis(1)).await;
            tokio::task::yield_now().await;
        }
        handle.await.unwrap().unwrap();

        let mut frames = 0usize;
        while rx.try_recv().is_ok() {
            frames += 1;
        }
        assert!(frames > 1, "expected multiple flooded frames, got {frames}");
        assert_eq!(log.len().await, 1);
        assert_eq!(log.records().await[0].attack_type, AttackType::Flood);
    }

    #[tokio::test]
    async fn invalid_target_address_is_rejected() {
        let bus = Arc::new(VirtualBus::new(8));
        let log = Arc::new(AttackLog::new());
        let attacker = Attacker::new(bus, log);
        assert!(matches!(
            attacker.inject_fake_command(BUS_CONTROLLER_ADDRESS).await,
            Err(AttackError::InvalidAddress(31))
        ));
    }

    #[tokio::test]
    async fn empty_bus_is_rejected() {
        let bus = Arc::new(VirtualBus::new(8));
        let log = Arc::new(AttackLog::new());
        let attacker = Attacker::new(bus, log);
        assert!(matches!(
            attacker.inject_fake_command(1).await,
            Err(AttackError::BusEmpty)
        ));
    }

    #[tokio::test]
    async fn attacker_runs_and_stops_via_channel() {
        let bus = Arc::new(VirtualBus::new(32));
        let log = Arc::new(AttackLog::new());
        let attacker = Attacker::new(Arc::clone(&bus), Arc::clone(&log));
        let mut rx = bus.subscribe();

        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let driver = tokio::spawn(attacker.run(cmd_rx));

        cmd_tx
            .send(AttackCommand::Run(AttackConfig::fake_command(9)))
            .unwrap();
        for _ in 0..4 {
            tokio::task::yield_now().await;
        }

        let frame = rx.try_recv().expect("fake command injected via channel");
        assert_eq!(frame.word_type, WordType::Command);
        assert_eq!(CommandWord::from_word(frame.word).rt_address, 9);
        assert_eq!(log.len().await, 1);

        // Stop and close the channel; the driver should finish cleanly.
        cmd_tx.send(AttackCommand::Stop).unwrap();
        drop(cmd_tx);
        driver.await.unwrap();
    }

    #[tokio::test]
    async fn inject_arinc429_spoof_publishes_on_arinc_channel_and_logs() {
        let bus = Arc::new(VirtualBus::new(16));
        let log = Arc::new(AttackLog::new());
        let attacker = Attacker::new(Arc::clone(&bus), Arc::clone(&log));
        let mut rx = bus.subscribe_wire(crate::wire::ProtocolId::Arinc429);

        attacker
            .inject_arinc429_spoof(10, 0, 2000, 3)
            .await
            .unwrap();

        let frame = rx.try_recv().expect("ARINC spoof injected");
        let crate::wire::WireFrame::Arinc429(word) = frame else {
            panic!("expected an ARINC 429 frame");
        };
        let decoded = word.decode();
        assert_eq!(decoded.label, 10);
        assert_eq!(decoded.data, 2000);
        assert_eq!(decoded.ssm, 3);

        let records = log.records().await;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].attack_type, AttackType::Arinc429Spoof);
        assert_eq!(records[0].target_address, 10);
    }

    #[tokio::test]
    async fn inject_arinc429_spoof_rejects_invalid_ssm() {
        let bus = Arc::new(VirtualBus::new(16));
        let log = Arc::new(AttackLog::new());
        let attacker = Attacker::new(Arc::clone(&bus), Arc::clone(&log));

        assert_eq!(
            attacker.inject_arinc429_spoof(10, 0, 2000, 4).await,
            Err(AttackError::InvalidSsm(4))
        );
        assert!(log.is_empty().await);
    }

    #[tokio::test]
    async fn inject_link16_spoof_publishes_on_link16_channel_and_logs() {
        let bus = Arc::new(VirtualBus::new(16));
        let log = Arc::new(AttackLog::new());
        let attacker = Attacker::new(Arc::clone(&bus), Arc::clone(&log));
        let mut rx = bus.subscribe_wire(crate::wire::ProtocolId::Link16);

        attacker
            .inject_link16_spoof(7, vec![0xde, 0xad, 0xbe, 0xef])
            .await
            .unwrap();

        let frame = rx.try_recv().expect("Link 16 spoof injected");
        let crate::wire::WireFrame::Link16(link) = frame else {
            panic!("expected a Link 16 frame");
        };
        assert_eq!(link.message.source_track_number, 7);
        assert_eq!(link.message.payload, vec![0xde, 0xad, 0xbe, 0xef]);

        let records = log.records().await;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].attack_type, AttackType::Link16Spoof);
        assert_eq!(records[0].target_address, 7);
    }

    #[tokio::test]
    async fn inject_link16_spoof_rejects_invalid_stn_and_oversized_payload() {
        let bus = Arc::new(VirtualBus::new(16));
        let log = Arc::new(AttackLog::new());
        let attacker = Attacker::new(Arc::clone(&bus), Arc::clone(&log));

        assert_eq!(
            attacker.inject_link16_spoof(0, vec![]).await,
            Err(AttackError::InvalidStn(0))
        );
        assert_eq!(
            attacker.inject_link16_spoof(200, vec![]).await,
            Err(AttackError::InvalidStn(200))
        );
        let oversized = vec![0u8; crate::wire::Link16Frame::MAX_PAYLOAD_LEN + 1];
        assert_eq!(
            attacker.inject_link16_spoof(7, oversized).await,
            Err(AttackError::PayloadTooLarge(
                crate::wire::Link16Frame::MAX_PAYLOAD_LEN
            ))
        );
        assert!(log.is_empty().await);
    }

    #[tokio::test]
    async fn wire_attacks_reject_when_channel_has_no_listeners() {
        let bus = Arc::new(VirtualBus::new(16));
        let log = Arc::new(AttackLog::new());
        let attacker = Attacker::new(Arc::clone(&bus), Arc::clone(&log));

        // No ARINC subscriber -> the spoof has nowhere to go.
        assert_eq!(
            attacker.inject_arinc429_spoof(10, 0, 2000, 3).await,
            Err(AttackError::BusEmpty)
        );
        // No Link 16 subscriber -> same.
        assert_eq!(
            attacker.inject_link16_spoof(7, vec![0x01]).await,
            Err(AttackError::BusEmpty)
        );
        assert!(log.is_empty().await);
    }

    #[tokio::test]
    async fn run_attack_dispatches_wire_spoofs_via_config() {
        let bus = Arc::new(VirtualBus::new(16));
        let log = Arc::new(AttackLog::new());
        let attacker = Attacker::new(Arc::clone(&bus), Arc::clone(&log));
        let mut arinc_rx = bus.subscribe_wire(crate::wire::ProtocolId::Arinc429);
        let mut link_rx = bus.subscribe_wire(crate::wire::ProtocolId::Link16);

        attacker
            .run_attack(&AttackConfig::arinc429_spoof(11, 4096, 2))
            .await
            .unwrap();
        attacker
            .run_attack(&AttackConfig::link16_spoof(9, vec![0x55]))
            .await
            .unwrap();

        let arinc = arinc_rx.try_recv().unwrap();
        let crate::wire::WireFrame::Arinc429(word) = arinc else {
            panic!("expected an ARINC 429 frame");
        };
        assert_eq!(word.decode().label, 11);
        let link = link_rx.try_recv().unwrap();
        let crate::wire::WireFrame::Link16(link) = link else {
            panic!("expected a Link 16 frame");
        };
        assert_eq!(link.message.source_track_number, 9);

        let records = log.records().await;
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].attack_type, AttackType::Arinc429Spoof);
        assert_eq!(records[1].attack_type, AttackType::Link16Spoof);
    }
}
