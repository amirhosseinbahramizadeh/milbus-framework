//! Virtual MIL-STD-1553B bus simulation without physical hardware.
//!
//! * [`VirtualBus`] broadcasts every word to all connected devices.
//! * [`SimulatedBusController`] polls remote terminals on a fixed interval.
//! * [`SimulatedRemoteTerminal`] answers polls with status and data words.
//! * [`SimulatedBusMonitor`] records every frame it observes for inspection.
//! * [`SimulatorConfig`] tunes RT count, polling interval, and error injection.
//!
//! All device roles implement [`SimulatedDevice`] and are driven as async
//! tasks. Time-based behaviour is fully compatible with Tokio's paused clock
//! (`tokio::time::pause` / `tokio::time::advance`) for deterministic tests.

// Device roles spell `run` as `fn(...) -> impl Future<Output = ()> + Send`
// rather than `async fn`: that explicit form is the documented, stable shape of
// [`SimulatedDevice::run`] that every existing role and caller relies on, so
// clippy's `manual_async_fn` suggestion is intentionally not applied here.
#![allow(clippy::manual_async_fn)]

pub mod attack;
pub mod arinc429;
pub mod bus;
pub mod config;
pub mod controller;
pub mod device;
pub mod frame;
pub mod link16;
pub mod monitor;
pub mod remote_terminal;
pub mod wire;

pub use attack::{
    AttackCommand, AttackConfig, AttackError, AttackLog, AttackRecord, AttackType, Attacker,
};
pub use bus::VirtualBus;
pub use config::SimulatorConfig;
pub use controller::SimulatedBusController;
pub use device::SimulatedDevice;
pub use frame::{
    status_word, CommandWord, Frame, WordType, BUS_CONTROLLER_ADDRESS, DEFAULT_DATA_WORDS,
    MAX_WORD_COUNT,
};
pub use monitor::SimulatedBusMonitor;
pub use remote_terminal::SimulatedRemoteTerminal;

pub use arinc429::SimulatedArinc429Device;
pub use link16::SimulatedLink16Device;

use std::sync::Arc;

/// Spawn a device as an independent Tokio task.
///
/// The task owns the device and a shared handle to the bus, so it runs until
/// the bus is closed or the returned [`JoinHandle`](tokio::task::JoinHandle)
/// is aborted.
pub fn spawn_device<D>(device: D, bus: Arc<VirtualBus>) -> tokio::task::JoinHandle<()>
where
    D: SimulatedDevice + Send + 'static,
{
    tokio::spawn(async move {
        let mut device = device;
        device.run(&bus).await;
    })
}
