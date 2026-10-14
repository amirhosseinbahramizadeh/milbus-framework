//! Common interface implemented by every simulated bus participant.

use std::future::Future;

use crate::bus::VirtualBus;

/// A participant on the virtual bus.
///
/// Each device role is driven as an async task: the caller spawns
/// [`run`](SimulatedDevice::run) -- conveniently via [`crate::spawn_device`] --
/// and the device processes bus traffic until the bus is closed or the task
/// is aborted.
///
/// `run` is declared as a function returning a `Send` future, which is
/// semantically equivalent to `async fn run(&mut self, bus: &VirtualBus)` but
/// additionally guarantees the event loop can be spawned on Tokio runtimes
/// (`tokio::spawn` requires `Send` futures). The trait is not object-safe;
/// spawn concrete device types instead of `dyn SimulatedDevice`.
pub trait SimulatedDevice {
    /// Run the device's event loop against the given virtual bus.
    fn run(&mut self, bus: &VirtualBus) -> impl Future<Output = ()> + Send;
}
