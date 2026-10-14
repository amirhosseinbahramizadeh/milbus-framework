//! Simulated remote terminal: responds to command words addressed to it.

use std::future::Future;

use tokio::sync::broadcast::error::RecvError;

use crate::bus::VirtualBus;
use crate::device::SimulatedDevice;
use crate::frame::{status_word, CommandWord, Frame, WordType, BUS_CONTROLLER_ADDRESS};

/// A remote terminal with a fixed 5-bit address.
///
/// When it observes a non-corrupted command word addressed to it that
/// requests a transmission (T/R bit set), it replies with a status word
/// followed by the requested number of data words.
pub struct SimulatedRemoteTerminal {
    address: u8,
    data_counter: u16,
    responses_sent: u64,
}

impl SimulatedRemoteTerminal {
    /// Create a remote terminal at `address` (0..=30; 31 is reserved for the
    /// bus controller).
    pub fn new(address: u8) -> Self {
        assert!(
            address < BUS_CONTROLLER_ADDRESS,
            "RT address must be within 0..=30"
        );
        Self {
            address,
            data_counter: 0,
            responses_sent: 0,
        }
    }

    /// Address of this remote terminal.
    pub fn address(&self) -> u8 {
        self.address
    }

    /// Number of command words this terminal has answered so far.
    pub fn responses_sent(&self) -> u64 {
        self.responses_sent
    }

    fn next_data_word(&mut self) -> u16 {
        let word = self.data_counter;
        self.data_counter = self.data_counter.wrapping_add(1);
        word
    }

    /// If the frame is a non-corrupted command from the bus controller that
    /// targets this terminal with a transmit request, decode it.
    fn responds_to(&self, frame: &Frame) -> Option<CommandWord> {
        if frame.corrupted
            || frame.word_type != WordType::Command
            || frame.source != BUS_CONTROLLER_ADDRESS
        {
            return None;
        }
        let command = CommandWord::from_word(frame.word);
        (command.rt_address == self.address && command.transmit).then_some(command)
    }
}

impl SimulatedDevice for SimulatedRemoteTerminal {
    fn run(&mut self, bus: &VirtualBus) -> impl Future<Output = ()> + Send {
        async move {
            let mut rx = bus.subscribe();
            loop {
                let frame = match rx.recv().await {
                    Ok(frame) => frame,
                    // The sender outran this receiver; skip ahead.
                    Err(RecvError::Lagged(_)) => continue,
                    // The bus is gone; shut down.
                    Err(RecvError::Closed) => break,
                };

                let Some(command) = self.responds_to(&frame) else {
                    continue;
                };

                let _ = bus.send_word(WordType::Status, status_word(self.address), self.address);
                for _ in 0..command.word_count {
                    let word = self.next_data_word();
                    let _ = bus.send_word(WordType::Data, word, self.address);
                }
                self.responses_sent += 1;
            }
        }
    }
}
