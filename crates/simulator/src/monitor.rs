//! Simulated bus monitor: records every frame it observes.

use std::future::Future;
use std::sync::{Arc, Mutex};

use tokio::sync::broadcast::error::RecvError;

use crate::bus::VirtualBus;
use crate::device::SimulatedDevice;
use crate::frame::Frame;

/// A passively listening monitor that records every frame broadcast on the
/// bus into a shared [`Arc<Mutex<Vec<Frame>>>"] log.
///
/// The log is also available through [`with_log`](Self::with_log) so tests
/// and the TUI can hold their own handle to the same underlying vector.
pub struct SimulatedBusMonitor {
    log: Arc<Mutex<Vec<Frame>>>,
}

impl Default for SimulatedBusMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl SimulatedBusMonitor {
    /// Create a monitor with a freshly allocated log.
    pub fn new() -> Self {
        Self {
            log: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Create a monitor that records into an externally owned log.
    pub fn with_log(log: Arc<Mutex<Vec<Frame>>>) -> Self {
        Self { log }
    }

    /// Shared handle to the monitor log.
    pub fn log(&self) -> Arc<Mutex<Vec<Frame>>> {
        Arc::clone(&self.log)
    }

    /// Snapshot of all frames recorded so far.
    pub fn frames(&self) -> Vec<Frame> {
        self.log()
            .lock()
            .expect("monitor log mutex poisoned")
            .clone()
    }
}

impl SimulatedDevice for SimulatedBusMonitor {
    fn run(&mut self, bus: &VirtualBus) -> impl Future<Output = ()> + Send {
        async move {
            let mut rx = bus.subscribe();
            loop {
                match rx.recv().await {
                    Ok(frame) => {
                        // Lock scope stays inside this statement; nothing is
                        // awaited while the mutex is held.
                        self.log()
                            .lock()
                            .expect("monitor log mutex poisoned")
                            .push(frame);
                    }
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => break,
                }
            }
        }
    }
}
