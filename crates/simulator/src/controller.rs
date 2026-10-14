//! Simulated bus controller: polls remote terminals on a fixed interval.

use std::future::Future;

use tokio::time::{interval, MissedTickBehavior};

use crate::bus::VirtualBus;
use crate::config::SimulatorConfig;
use crate::device::SimulatedDevice;
use crate::frame::{CommandWord, Frame, WordType, BUS_CONTROLLER_ADDRESS, DEFAULT_DATA_WORDS};

/// Subaddress used for simulated data transfers.
const POLL_SUBADDRESS: u8 = 1;

/// Small deterministic PRNG (Knuth MMIX LCG) so error injection is
/// reproducible across runs without pulling in a rand dependency.
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        // Force a non-zero state.
        Self(seed | 1)
    }

    /// Uniform-ish value in 0.0..=1.0.
    fn next_f64(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
    }
}

/// The bus controller (BC).
///
/// It polls every remote terminal in round-robin order, broadcasting one
/// command word per tick that asks the target RT to transmit
/// [`DEFAULT_DATA_WORDS`] data words. Tokio completes the first interval tick
/// immediately in real time (under a paused test clock the first tick fires on
/// the first `tokio::time::advance`); every subsequent command is emitted at
/// `config.polling_interval`.
pub struct SimulatedBusController {
    config: SimulatorConfig,
    poll_index: u64,
    rng: Lcg,
}

impl SimulatedBusController {
    /// Create a controller that polls according to `config`.
    pub fn new(config: SimulatorConfig) -> Self {
        Self {
            config,
            poll_index: 0,
            rng: Lcg::new(0x0001_553B),
        }
    }

    /// Number of command words emitted so far.
    pub fn polls(&self) -> u64 {
        self.poll_index
    }

    /// Round-robin target address for the current poll: RTs are addressed
    /// 1..=num_remote_terminals.
    fn next_rt_address(&self) -> u8 {
        let num_rts = self.config.num_remote_terminals.max(1) as u64;
        (self.poll_index % num_rts) as u8 + 1
    }

    /// Broadcast a single poll command word for the current round-robin
    /// slot, applying configured error injection.
    fn poll_once(&mut self, bus: &VirtualBus) {
        let rt_address = self.next_rt_address();
        self.poll_index += 1;

        let command = CommandWord::new(
            rt_address,
            true, // T/R = 1: the RT transmits to the bus controller
            POLL_SUBADDRESS,
            DEFAULT_DATA_WORDS,
        );
        let corrupted = self.rng.next_f64() < self.config.error_injection_rate;

        let frame = Frame {
            sequence: bus.next_sequence(),
            word_type: WordType::Command,
            word: command.to_word(),
            source: BUS_CONTROLLER_ADDRESS,
            corrupted,
        };
        // Ignore errors: no devices may be subscribed yet.
        let _ = bus.publish(frame);
    }
}

impl SimulatedDevice for SimulatedBusController {
    fn run(&mut self, bus: &VirtualBus) -> impl Future<Output = ()> + Send {
        async move {
            let mut ticker = interval(self.config.polling_interval);
            // Burst (Tokio default) keeps the poll cadence anchored to the
            // interval start. This also makes paused-clock tests
            // deterministic: each `tokio::time::advance(polling_interval)`
            // fires exactly one tick.
            ticker.set_missed_tick_behavior(MissedTickBehavior::Burst);
            loop {
                ticker.tick().await;
                self.poll_once(bus);
            }
        }
    }
}
