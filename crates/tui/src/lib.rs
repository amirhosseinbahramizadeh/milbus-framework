//! Ratatui-based terminal interface for milbus-analyzer.
//!
//! The interface shows three protocol tabs (MIL-1553B, ARINC 429, Link 16),
//! a live table of received/injected frames, and a status bar. It multiplexes
//! keyboard input and incoming simulator frames with [`tokio::select!`].

pub mod app;
pub mod render;

pub use app::{App, ConnectionStatus, Direction, FrameRow, Tab};
pub use render::render;

use std::io::{self, Stdout};

use crossterm::event::{Event, EventStream, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use simulator::wire::WireFrame;
use tokio::sync::mpsc;

/// A Ratatui terminal backed by crossterm on stdout.
pub type Tui = Terminal<CrosstermBackend<Stdout>>;

/// Run the terminal UI until the user quits or the simulator feed ends.
///
/// * `frames` receives protocol-tagged frames broadcast by the simulator (RX).
/// * `outbound` carries parsed bytes for frames the user injects (TX); the
///   caller is responsible for publishing them onto the virtual bus.
pub async fn run(
    frames: mpsc::UnboundedReceiver<WireFrame>,
    outbound: mpsc::UnboundedSender<Vec<u8>>,
) -> io::Result<()> {
    let mut terminal = init_terminal()?;
    let result = run_loop(&mut terminal, frames, outbound).await;
    restore_terminal(&mut terminal)?;
    result
}

/// Enter raw mode + the alternate screen.
pub fn init_terminal() -> io::Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

/// Leave raw mode + the alternate screen.
pub fn restore_terminal(terminal: &mut Tui) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

async fn run_loop(
    terminal: &mut Tui,
    mut frames: mpsc::UnboundedReceiver<WireFrame>,
    outbound: mpsc::UnboundedSender<Vec<u8>>,
) -> io::Result<()> {
    let mut app = App::new(outbound);
    let mut events = EventStream::new();

    loop {
        terminal.draw(|frame| render(&app, frame))?;

        tokio::select! {
            // Keyboard input.
            maybe_event = events.next() => {
                match maybe_event {
                    Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                        app.handle_key(key);
                    }
                    Some(Ok(_)) => {}
                    Some(Err(err)) => return Err(io::Error::other(err)),
                    None => return Ok(()),
                }
            }
            // Frames streamed in from the simulator.
            maybe_frame = frames.recv() => {
                match maybe_frame {
                    Some(frame) => app.ingest_wire_frame(&frame),
                    None => return Ok(()),
                }
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{Direction, Tab};

    #[tokio::test]
    async fn run_loop_ingests_simulator_frames() {
        let (frame_tx, mut frame_rx) = mpsc::unbounded_channel();
        let (outbound_tx, _outbound_rx) = mpsc::unbounded_channel();

        // We cannot drive the real terminal here, so exercise the App path
        // directly to confirm the wiring types line up.
        let mut app = App::new(outbound_tx);
        let frame = simulator::Frame {
            sequence: 0,
            word_type: simulator::WordType::Command,
            word: 0x0820,
            source: 31,
            corrupted: false,
        };
        frame_tx
            .send(simulator::wire::WireFrame::Mil1553b(frame))
            .unwrap();
        let received = frame_rx.recv().await.unwrap();
        app.ingest_wire_frame(&received);

        let rows: Vec<_> = app.rows_for_tab(Tab::Mil1553b).collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].direction, Direction::Rx);
        assert_eq!(rows[0].command, "0x0820");
    }
}
