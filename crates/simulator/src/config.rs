//! Configuration for the bus simulation.

use std::time::Duration;

/// Tuning knobs for the virtual bus simulation.
#[derive(Clone, Debug, PartialEq)]
pub struct SimulatorConfig {
    /// Number of remote terminals polled by the bus controller.
    pub num_remote_terminals: usize,
    /// Interval between bus controller polls.
    pub polling_interval: Duration,
    /// Probability (0.0..=1.0) that a given command word is transmitted
    /// corrupted.
    pub error_injection_rate: f64,
}

impl SimulatorConfig {
    /// Build a configuration, clamping values into sane ranges: at least one
    /// remote terminal, a non-zero polling interval, and an error-injection
    /// rate within 0.0..=1.0.
    pub fn new(
        num_remote_terminals: usize,
        polling_interval: Duration,
        error_injection_rate: f64,
    ) -> Self {
        Self {
            num_remote_terminals: num_remote_terminals.max(1),
            polling_interval: if polling_interval.is_zero() {
                Self::default().polling_interval
            } else {
                polling_interval
            },
            error_injection_rate: error_injection_rate.clamp(0.0, 1.0),
        }
    }
}

impl Default for SimulatorConfig {
    fn default() -> Self {
        Self {
            num_remote_terminals: 4,
            polling_interval: Duration::from_millis(10),
            error_injection_rate: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_clamps_values_into_sane_ranges() {
        let config = SimulatorConfig::new(0, Duration::ZERO, 3.5);
        assert_eq!(config.num_remote_terminals, 1);
        assert_eq!(config.polling_interval, Duration::from_millis(10));
        assert_eq!(config.error_injection_rate, 1.0);
    }

    #[test]
    fn default_is_four_rts_polling_every_ten_millis() {
        let config = SimulatorConfig::default();
        assert_eq!(config.num_remote_terminals, 4);
        assert_eq!(config.polling_interval, Duration::from_millis(10));
        assert_eq!(config.error_injection_rate, 0.0);
    }
}
