//! milbus-analyzer command-line entry point.
//!
//! Three subcommands are exposed:
//! * `simulate` — run the Ratatui TUI against a live virtual MIL-STD-1553B bus
//!   (one bus controller and N remote terminals), optionally capturing traffic.
//! * `inject`   — run a headless bus attack and report what the monitor saw.
//! * `replay`   — load a JSON capture and replay it through a fresh bus.
//!
//! This tool is intended for educational use and authorized testing only.

use std::error::Error;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};
use cli::{Capture, WireCapture};
use simulator::wire::{ProtocolId, WireFrame};
use simulator::{
    spawn_device, AttackConfig, AttackLog, Attacker, CommandWord, Frame, SimulatedArinc429Device,
    SimulatedBusController, SimulatedBusMonitor, SimulatedLink16Device, SimulatedRemoteTerminal,
    SimulatorConfig, VirtualBus, WordType, BUS_CONTROLLER_ADDRESS,
};
use tokio::sync::mpsc;

/// MIL-STD-1553B / ARINC 429 / Link 16 bus analyzer and simulator.
#[derive(Parser, Debug)]
#[command(name = "milbus-analyzer", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run the interactive TUI against a virtual bus (1 BC + N RTs).
    Simulate {
        /// Number of remote terminals to simulate.
        #[arg(long, default_value_t = 3)]
        rts: usize,
        /// Bus-controller polling interval, in milliseconds.
        #[arg(long, default_value_t = 10)]
        interval: u64,
        /// Number of ARINC 429 sensors to simulate.
        #[arg(long, default_value_t = 1)]
        arinc: usize,
        /// Number of Link 16 nodes to simulate.
        #[arg(long, default_value_t = 1)]
        link16: usize,
        /// Optional path to capture all bus frames to JSON on exit.
        #[arg(long)]
        capture: Option<PathBuf>,
    },
    /// Run a headless bus attack and report observed frames.
    Inject {
        /// Which attack to carry out.
        #[arg(long, value_enum, default_value_t = AttackKind::FakeCommand)]
        kind: AttackKind,
        /// Target remote-terminal address (0..=30).
        #[arg(long, default_value_t = 1)]
        target: u8,
        /// Flood duration, in milliseconds (ignored by other attack kinds).
        #[arg(long, default_value_t = 1000)]
        duration: u64,
    },
    /// Load a JSON capture and replay it through a fresh bus.
    Replay {
        /// Path to a capture file written by `simulate --capture`.
        path: PathBuf,
    },
    /// Run the full bus headlessly for `--duration` ms and record every
    /// protocol-tagged frame (MIL-STD-1553B, ARINC 429, Link 16) to a JSON
    /// file, without opening the interactive TUI.
    WireCapture {
        /// Number of remote terminals to simulate.
        #[arg(long, default_value_t = 3)]
        rts: usize,
        /// Bus-controller polling interval, in milliseconds.
        #[arg(long, default_value_t = 10)]
        interval: u64,
        /// Number of ARINC 429 sensors to simulate.
        #[arg(long, default_value_t = 1)]
        arinc: usize,
        /// Number of Link 16 nodes to simulate.
        #[arg(long, default_value_t = 1)]
        link16: usize,
        /// Capture window, in milliseconds.
        #[arg(long, default_value_t = 1000)]
        duration: u64,
        /// Path to write the JSON wire capture to.
        path: PathBuf,
    },
}

/// The supported attack kinds, selectable from the command line.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum AttackKind {
    /// Inject a fake command word with a spoofed RT address.
    FakeCommand,
    /// Continuously inject frames to saturate the bus.
    Flood,
    /// Inject a malformed status word.
    CorruptStatus,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Simulate {
            rts,
            interval,
            arinc,
            link16,
            capture,
        } => simulate(rts, interval, arinc, link16, capture).await,
        Command::Inject {
            kind,
            target,
            duration,
        } => inject(kind, target, duration).await,
        Command::Replay { path } => replay(path).await,
        Command::WireCapture {
            rts,
            interval,
            arinc,
            link16,
            duration,
            path,
        } => wire_capture(rts, interval, arinc, link16, duration, path).await,
    }
}

/// Spin up a virtual bus (one BC + `rts` remote terminals + ARINC 429 sensors +
/// Link 16 nodes), pump frames into the TUI, and optionally capture the 1553B
/// traffic to `capture` on exit.
async fn simulate(
    rts: usize,
    interval_ms: u64,
    arinc: usize,
    link16: usize,
    capture_path: Option<PathBuf>,
) -> Result<(), Box<dyn Error>> {
    let bus = Arc::new(VirtualBus::new(1024));
    let config = SimulatorConfig::new(rts, Duration::from_millis(interval_ms), 0.0);

    // Bus controller + remote terminals at addresses 1..=rts.
    let _bc = spawn_device(SimulatedBusController::new(config), Arc::clone(&bus));
    for address in 1..=rts as u8 {
        let _rt = spawn_device(SimulatedRemoteTerminal::new(address), Arc::clone(&bus));
    }

    // ARINC 429 sensors, each transmitting on its own label.
    for index in 0..arinc {
        let label = 10 + index as u8;
        let sensor = SimulatedArinc429Device::new(
            label,
            0,
            2000 + index as u32 * 100,
            3,
            Duration::from_millis(20),
            None,
        );
        let _sensor_task = spawn_device(sensor, Arc::clone(&bus));
    }

    // Link 16 nodes, each transmitting on its own STN.
    for index in 0..link16 {
        let stn = 7 + index as u8;
        let node = SimulatedLink16Device::new_now(
            stn,
            3,
            2,
            vec![index as u8, 0xAA, 0xBB],
            Duration::from_millis(40),
            None,
        );
        let _node_task = spawn_device(node, Arc::clone(&bus));
    }

    // Monitor for capture.
    let shared_log: Arc<Mutex<Vec<Frame>>> = Arc::new(Mutex::new(Vec::new()));
    let _monitor = spawn_device(
        SimulatedBusMonitor::with_log(Arc::clone(&shared_log)),
        Arc::clone(&bus),
    );

    // Bridge all three protocol channels -> TUI frame channel. Each bridge
    // wraps its frames into the tagged `WireFrame` so the TUI can route them
    // to the right tab.
    let (frame_tx, frame_rx) = mpsc::unbounded_channel();
    let _legacy_bridge = tokio::spawn({
        let bus = Arc::clone(&bus);
        let frame_tx = frame_tx.clone();
        async move {
            let mut rx = bus.subscribe();
            while let Ok(frame) = rx.recv().await {
                if frame_tx.send(WireFrame::Mil1553b(frame)).is_err() {
                    break;
                }
            }
        }
    });
    let _arinc_bridge = tokio::spawn({
        let bus = Arc::clone(&bus);
        let frame_tx = frame_tx.clone();
        async move {
            let mut rx = bus.subscribe_wire(ProtocolId::Arinc429);
            while let Ok(frame) = rx.recv().await {
                if frame_tx.send(frame).is_err() {
                    break;
                }
            }
        }
    });
    let _link_bridge = tokio::spawn({
        let bus = Arc::clone(&bus);
        let frame_tx = frame_tx.clone();
        async move {
            let mut rx = bus.subscribe_wire(ProtocolId::Link16);
            while let Ok(frame) = rx.recv().await {
                if frame_tx.send(frame).is_err() {
                    break;
                }
            }
        }
    });
    // Drop the original sender so the TUI feed ends once every bridge closes.
    drop(frame_tx);

    // TUI -> bus outbound channel (parsed hex injection bytes).
    let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let _outbound = tokio::spawn({
        let bus = Arc::clone(&bus);
        async move {
            while let Some(bytes) = outbound_rx.recv().await {
                if bytes.len() >= 2 {
                    let word = u16::from_be_bytes([bytes[0], bytes[1]]);
                    let frame = Frame {
                        sequence: bus.next_sequence(),
                        word_type: WordType::Command,
                        word,
                        source: BUS_CONTROLLER_ADDRESS,
                        corrupted: false,
                    };
                    let _ = bus.publish(frame);
                }
            }
        }
    });

    // Hand the frame stream + outbound sender to the TUI; blocks until quit.
    tui::run(frame_rx, outbound_tx).await?;

    if let Some(path) = capture_path {
        let frames = shared_log.lock().expect("monitor log poisoned").clone();
        let mut capture = Capture::new();
        for frame in frames {
            capture.record(frame);
        }
        capture.write_to(&path)?;
        eprintln!("captured {} frames to {}", capture.len(), path.display());
    }
    Ok(())
}
/// Run the full bus headlessly for `duration_ms`, capture every tagged
/// `WireFrame` from all three channels, and write them to `path` as JSON.
///
/// The bus runs the same 1 BC + N RTs scenario plus ARINC 429 sensors and
/// Link 16 nodes as `simulate`, minus the TUI. On exit the command prints a
/// per-protocol breakdown so the run is verifiable without rendering.
async fn wire_capture(
    rts: usize,
    interval_ms: u64,
    arinc: usize,
    link16: usize,
    duration_ms: u64,
    path: PathBuf,
) -> Result<(), Box<dyn Error>> {
    let bus = Arc::new(VirtualBus::new(1024));
    let config = SimulatorConfig::new(rts, Duration::from_millis(interval_ms), 0.0);

    let _bc = spawn_device(SimulatedBusController::new(config), Arc::clone(&bus));
    for address in 1..=rts as u8 {
        let _rt = spawn_device(SimulatedRemoteTerminal::new(address), Arc::clone(&bus));
    }
    for index in 0..arinc {
        let label = 10 + index as u8;
        let sensor = SimulatedArinc429Device::new(
            label,
            0,
            2000 + index as u32 * 100,
            3,
            Duration::from_millis(20),
            None,
        );
        let _sensor_task = spawn_device(sensor, Arc::clone(&bus));
    }
    for index in 0..link16 {
        let stn = 7 + index as u8;
        let node = SimulatedLink16Device::new_now(
            stn,
            3,
            2,
            vec![index as u8, 0xAA, 0xBB],
            Duration::from_millis(40),
            None,
        );
        let _node_task = spawn_device(node, Arc::clone(&bus));
    }

    let mut legacy_rx = bus.subscribe();
    let mut arinc_rx = bus.subscribe_wire(ProtocolId::Arinc429);
    let mut link_rx = bus.subscribe_wire(ProtocolId::Link16);

    let drain = tokio::time::timeout(Duration::from_millis(duration_ms.max(1)), async {
        let mut capture = WireCapture::new();
        loop {
            tokio::select! {
                biased;
                frame = legacy_rx.recv() => match frame {
                    Ok(frame) => capture.record(WireFrame::Mil1553b(frame)),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
                frame = arinc_rx.recv() => match frame {
                    Ok(frame) => capture.record(frame),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
                frame = link_rx.recv() => match frame {
                    Ok(frame) => capture.record(frame),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
            }
        }
        capture
    })
    .await
    .unwrap_or_else(|_| WireCapture::new());

    let counts = [
        (drain.frames_for(ProtocolId::Mil1553b).len(), "MIL-1553B"),
        (drain.frames_for(ProtocolId::Arinc429).len(), "ARINC 429"),
        (drain.frames_for(ProtocolId::Link16).len(), "Link 16"),
    ];
    match drain.write_to(&path) {
        Ok(()) => {
            println!(
                "wire capture complete: {} frames to {} ({} MIL-1553B, {} ARINC 429, {} Link 16)",
                drain.len(),
                path.display(),
                counts[0].0,
                counts[1].0,
                counts[2].0
            );
        }
        Err(err) => eprintln!("failed to write capture: {err}"),
    }
    for (count, label) in counts {
        println!("  {label}: {count}");
    }
    Ok(())
}

/// Run a single headless attack and print what the bus monitor observed.
async fn inject(kind: AttackKind, target: u8, duration_ms: u64) -> Result<(), Box<dyn Error>> {
    let bus = Arc::new(VirtualBus::new(1024));
    let log: Arc<Mutex<Vec<Frame>>> = Arc::new(Mutex::new(Vec::new()));
    let _monitor = spawn_device(
        SimulatedBusMonitor::with_log(Arc::clone(&log)),
        Arc::clone(&bus),
    );

    let attack_log = Arc::new(AttackLog::new());
    let attacker = Attacker::new(Arc::clone(&bus), Arc::clone(&attack_log));

    // Wait until the monitor has subscribed before injecting.
    wait_for_subscriber(&bus).await;

    let config = match kind {
        AttackKind::FakeCommand => AttackConfig::fake_command(target),
        AttackKind::Flood => AttackConfig::flood(target, Duration::from_millis(duration_ms)),
        AttackKind::CorruptStatus => AttackConfig::corrupt_status(target),
    };
    attacker.run_attack(&config).await?;

    // Give the monitor a moment to drain the broadcast channel.
    tokio::time::sleep(Duration::from_millis(5)).await;

    let frames = log.lock().expect("monitor log poisoned").clone();
    let records = attack_log.records().await;
    println!(
        "attack complete: {} frames observed, {} log entries",
        frames.len(),
        records.len()
    );
    for record in records {
        println!(
            "  {} -> RT {} at {:?}",
            record.attack_type.label(),
            record.target_address,
            record.timestamp
        );
    }
    Ok(())
}

/// Load a capture file and replay its frames through a fresh bus + monitor.
async fn replay(path: PathBuf) -> Result<(), Box<dyn Error>> {
    let capture = Capture::read_from(&path)?;
    println!("replaying {} frames from {}", capture.len(), path.display());

    let bus = Arc::new(VirtualBus::new(1024));
    let log: Arc<Mutex<Vec<Frame>>> = Arc::new(Mutex::new(Vec::new()));
    let _monitor = spawn_device(
        SimulatedBusMonitor::with_log(Arc::clone(&log)),
        Arc::clone(&bus),
    );
    wait_for_subscriber(&bus).await;

    for frame in capture.frames() {
        bus.publish(frame.clone())?;
    }
    tokio::task::yield_now().await;

    let seen = log.lock().expect("monitor log poisoned").clone();
    for frame in &seen {
        let detail = if frame.word_type == WordType::Command {
            let command = CommandWord::from_word(frame.word);
            format!("RT{} T/R={}", command.rt_address, command.transmit)
        } else {
            format!("{:#06x}", frame.word)
        };
        println!("  seq={} {:?} {}", frame.sequence, frame.word_type, detail);
    }
    println!("replay complete: {} frames re-injected", seen.len());
    Ok(())
}

/// Yield until at least one device has subscribed to the bus.
///
/// Spawned device tasks subscribe asynchronously, so on the multi-threaded
/// runtime a single `yield_now` does not guarantee the monitor has
/// subscribed yet. This spins (without blocking a worker) until it has.
async fn wait_for_subscriber(bus: &VirtualBus) {
    while bus.subscriber_count() == 0 {
        tokio::task::yield_now().await;
    }
}
