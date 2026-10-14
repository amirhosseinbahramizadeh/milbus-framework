//! End-to-end integration tests wiring the simulator and protocol crates.
//!
//! These exercise the same public APIs the CLI drives: a virtual bus with one
//! bus controller and three remote terminals, a headless injection observed by
//! the bus monitor, and ARINC 429 parsing against a known-good test vector.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use protocol_core::Arinc429Frame;
use simulator::{
    spawn_device, AttackLog, Attacker, CommandWord, Frame, SimulatedBusController,
    SimulatedBusMonitor, SimulatedRemoteTerminal, SimulatorConfig, VirtualBus, WordType,
};

#[tokio::test]
async fn bus_controller_receives_responses_from_three_remote_terminals() {
    tokio::time::pause();

    let bus = Arc::new(VirtualBus::new(256));
    // One BC polling three RTs (addresses 1..=3) every 10 ms, no error injection.
    let config = SimulatorConfig::new(3, Duration::from_millis(10), 0.0);

    let mut rx = bus.subscribe();
    let _bc = spawn_device(SimulatedBusController::new(config), Arc::clone(&bus));
    for address in 1..=3u8 {
        let _rt = spawn_device(SimulatedRemoteTerminal::new(address), Arc::clone(&bus));
    }

    // Drive the paused clock so the BC polls every RT at least once.
    for _ in 0..12 {
        tokio::time::advance(Duration::from_millis(10)).await;
        tokio::task::yield_now().await;
    }

    let mut status_sources = HashSet::new();
    let mut data_sources = HashSet::new();
    let mut poll_targets = HashSet::new();
    while let Ok(frame) = rx.try_recv() {
        match frame.word_type {
            WordType::Command => {
                poll_targets.insert(CommandWord::from_word(frame.word).rt_address);
            }
            WordType::Status => {
                status_sources.insert(frame.source);
            }
            WordType::Data => {
                data_sources.insert(frame.source);
            }
        }
    }

    let expected: HashSet<u8> = HashSet::from([1, 2, 3]);
    // The BC polled all three RTs.
    assert!(
        poll_targets.is_superset(&expected),
        "polled {poll_targets:?}"
    );
    // Each RT returned a status word.
    assert!(
        status_sources.is_superset(&expected),
        "statuses {status_sources:?}"
    );
    // Each RT returned data words.
    assert!(data_sources.is_superset(&expected), "data {data_sources:?}");
}

#[tokio::test]
async fn injected_fake_command_is_logged_by_the_bus_monitor() {
    let bus = Arc::new(VirtualBus::new(64));
    let log: Arc<Mutex<Vec<Frame>>> = Arc::new(Mutex::new(Vec::new()));
    let _monitor = spawn_device(
        SimulatedBusMonitor::with_log(Arc::clone(&log)),
        Arc::clone(&bus),
    );

    let attack_log = Arc::new(AttackLog::new());
    let attacker = Attacker::new(Arc::clone(&bus), Arc::clone(&attack_log));

    // Let the monitor subscribe before injecting.
    tokio::task::yield_now().await;

    attacker.inject_fake_command(5).await.expect("injection");
    // Let the monitor drain the broadcast channel.
    tokio::task::yield_now().await;

    let frames = log.lock().expect("log lock").clone();
    assert!(!frames.is_empty(), "monitor saw no frames");
    let command = frames
        .iter()
        .find(|f| f.word_type == WordType::Command)
        .expect("a command word was logged");
    // The spoofed command targets RT 5.
    assert_eq!(CommandWord::from_word(command.word).rt_address, 5);

    // The attack log recorded the fake-command attack against RT 5.
    let records = attack_log.records().await;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].target_address, 5);
}

#[test]
fn arinc429_parser_decodes_known_test_vector() {
    // Ground Speed (label 012 / decimal 10), data 2000, SSM Normal (3).
    // Encoded by the arinc429 reference crate -> 0xE01F4050.
    let frame = Arinc429Frame::from_word(0xE01F_4050).expect("valid ARINC word");
    assert_eq!(frame.label, 10);
    assert_eq!(frame.sdi, 0);
    assert_eq!(frame.data, 2000);
    assert_eq!(frame.ssm, 3);
    assert!(frame.parity);
    // BNR at 0.125 knots/LSB -> 250 knots.
    assert_eq!(frame.to_bnr(0.125, false), 250.0);
    // Round-trips back to the same wire word.
    assert_eq!(frame.to_word().expect("re-encode"), 0xE01F_4050);
}
