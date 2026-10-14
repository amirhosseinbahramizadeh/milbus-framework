//! Deterministic integration tests driving the simulation with Tokio's
//! paused clock (`tokio::time::pause` / `tokio::time::advance`).
//!
//! Under the paused clock, timers -- including the bus controller's interval
//! ticks -- fire only when `tokio::time::advance` moves the clock forward.
//! Each `advance(polling_interval)` therefore produces exactly one command
//! word, which keeps these tests fully deterministic.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use simulator::{
    spawn_device, CommandWord, Frame, SimulatedBusController, SimulatedBusMonitor,
    SimulatedRemoteTerminal, SimulatorConfig, VirtualBus, WordType, BUS_CONTROLLER_ADDRESS,
};

/// Let spawned tasks process all ready work without advancing the clock.
///
/// The test task stays runnable throughout, so the paused clock never
/// auto-advances while we settle.
async fn settle() {
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn bus_controller_polls_remote_terminals_in_round_robin() {
    tokio::time::pause();

    let bus = Arc::new(VirtualBus::new(64));
    let config = SimulatorConfig::new(2, Duration::from_millis(10), 0.0);
    let mut rx = bus.subscribe();

    let controller = spawn_device(SimulatedBusController::new(config), Arc::clone(&bus));
    settle().await;

    // One tick per advance: RT 1, then RT 2, then back to RT 1.
    tokio::time::advance(Duration::from_millis(10)).await;
    settle().await;
    let first = rx.try_recv().expect("first command word");
    assert_eq!(first.word_type, WordType::Command);
    assert_eq!(first.source, BUS_CONTROLLER_ADDRESS);
    assert!(!first.corrupted);
    let decoded = CommandWord::from_word(first.word);
    assert_eq!(decoded.rt_address, 1);
    assert!(decoded.transmit);

    tokio::time::advance(Duration::from_millis(10)).await;
    settle().await;
    let second = rx.try_recv().expect("second command word");
    assert_eq!(CommandWord::from_word(second.word).rt_address, 2);

    tokio::time::advance(Duration::from_millis(10)).await;
    settle().await;
    let third = rx.try_recv().expect("third command word");
    assert_eq!(CommandWord::from_word(third.word).rt_address, 1);

    assert!(first.sequence < second.sequence);
    assert!(second.sequence < third.sequence);

    controller.abort();
}

#[tokio::test]
async fn remote_terminal_responds_with_status_and_data_words() {
    tokio::time::pause();

    let bus = Arc::new(VirtualBus::new(64));
    let log = Arc::new(Mutex::new(Vec::new()));

    let monitor = spawn_device(
        SimulatedBusMonitor::with_log(Arc::clone(&log)),
        Arc::clone(&bus),
    );
    let rt = spawn_device(SimulatedRemoteTerminal::new(2), Arc::clone(&bus));
    settle().await;

    // Bus controller asks RT 2 to transmit 3 data words.
    let command = CommandWord::new(2, true, 1, 3);
    bus.send_word(WordType::Command, command.to_word(), BUS_CONTROLLER_ADDRESS)
        .expect("monitor keeps the bus alive");
    settle().await;

    let frames = log.lock().expect("log lock").clone();
    let responses: Vec<&Frame> = frames.iter().filter(|f| f.source == 2).collect();
    assert_eq!(responses.len(), 4, "status word + 3 data words");
    assert_eq!(responses[0].word_type, WordType::Status);
    // The status word carries the responding RT address in bits 15..11.
    assert_eq!((responses[0].word >> 11) & 0x1F, 2);
    assert!(responses[1..].iter().all(|f| f.word_type == WordType::Data));
    // The simulated data payload is an incrementing counter.
    let payloads: Vec<u16> = responses[1..].iter().map(|f| f.word).collect();
    assert_eq!(payloads, vec![0, 1, 2]);

    monitor.abort();
    rt.abort();
}

#[tokio::test]
async fn bus_monitor_records_all_bus_traffic() {
    tokio::time::pause();

    let bus = Arc::new(VirtualBus::new(64));
    let monitor = SimulatedBusMonitor::new();
    let log = monitor.log();

    let monitor_task = spawn_device(monitor, Arc::clone(&bus));
    let controller = spawn_device(
        SimulatedBusController::new(SimulatorConfig::default()),
        Arc::clone(&bus),
    );
    settle().await;

    tokio::time::advance(Duration::from_millis(10)).await;
    settle().await;
    tokio::time::advance(Duration::from_millis(10)).await;
    settle().await;

    let frames = log.lock().expect("log lock").clone();
    assert_eq!(frames.len(), 2, "one command word per advanced tick");
    assert!(frames.iter().all(|f| f.word_type == WordType::Command));
    // The default config polls RT addresses 1..=4 in order.
    let addresses: Vec<u8> = frames
        .iter()
        .map(|f| CommandWord::from_word(f.word).rt_address)
        .collect();
    assert_eq!(addresses, vec![1, 2]);

    monitor_task.abort();
    controller.abort();
}

#[tokio::test]
async fn error_injection_corrupts_polls_and_remote_terminals_stay_silent() {
    tokio::time::pause();

    let bus = Arc::new(VirtualBus::new(64));
    let log = Arc::new(Mutex::new(Vec::new()));

    let monitor = spawn_device(
        SimulatedBusMonitor::with_log(Arc::clone(&log)),
        Arc::clone(&bus),
    );
    let rt = spawn_device(SimulatedRemoteTerminal::new(1), Arc::clone(&bus));
    let controller = spawn_device(
        SimulatedBusController::new(SimulatorConfig::new(1, Duration::from_millis(10), 1.0)),
        Arc::clone(&bus),
    );
    settle().await;

    tokio::time::advance(Duration::from_millis(10)).await;
    settle().await;
    tokio::time::advance(Duration::from_millis(10)).await;
    settle().await;

    // With an injection rate of 1.0 every command word is corrupted. The
    // remote terminal ignores corrupted commands, so only (corrupted)
    // command words appear on the bus -- never status or data words.
    let frames = log.lock().expect("log lock").clone();
    assert_eq!(frames.len(), 2, "two polls observed");
    assert!(frames.iter().all(|f| f.word_type == WordType::Command));
    assert!(frames.iter().all(|f| f.corrupted));

    monitor.abort();
    rt.abort();
    controller.abort();
}
