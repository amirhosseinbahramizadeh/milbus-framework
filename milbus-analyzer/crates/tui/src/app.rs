//! Application state for the milbus-analyzer terminal interface.

use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent};
use protocol_core::hex::{format_hex, parse_hex};
use simulator::frame::{Frame as SimFrame, WordType};
use simulator::wire::WireFrame;
use tokio::sync::mpsc::UnboundedSender;

/// The protocol tabs shown across the top of the interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    /// MIL-STD-1553B command/status/data words.
    Mil1553b,
    /// ARINC 429 labelled 32-bit words.
    Arinc429,
    /// Link 16 J-series message traffic.
    Link16,
}

impl Tab {
    /// Tabs in left-to-right order.
    pub const ALL: [Tab; 3] = [Tab::Mil1553b, Tab::Arinc429, Tab::Link16];

    /// Human-readable tab label.
    pub fn title(self) -> &'static str {
        match self {
            Tab::Mil1553b => "MIL-1553B",
            Tab::Arinc429 => "ARINC 429",
            Tab::Link16 => "Link 16",
        }
    }

    /// The next tab to the right (wrapping).
    pub fn next(self) -> Self {
        match self {
            Tab::Mil1553b => Tab::Arinc429,
            Tab::Arinc429 => Tab::Link16,
            Tab::Link16 => Tab::Mil1553b,
        }
    }
}

/// Whether the analyzer is attached to the (in-process) simulator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    /// Frames are flowing from the simulator.
    Connected,
    /// No simulator traffic is being received.
    Disconnected,
}

impl ConnectionStatus {
    /// Short label for the status bar.
    pub fn label(self) -> &'static str {
        match self {
            ConnectionStatus::Connected => "CONNECTED",
            ConnectionStatus::Disconnected => "DISCONNECTED",
        }
    }
}

/// Whether a frame was received from or injected into the bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Frames observed on the bus (simulator or other devices).
    Rx,
    /// Frames the user injected through the injection box.
    Tx,
}

impl Direction {
    /// Short label for the direction column.
    pub fn label(self) -> &'static str {
        match self {
            Direction::Rx => "RX",
            Direction::Tx => "TX",
        }
    }
}

/// One displayed row in the active protocol's frame table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameRow {
    /// Elapsed time since the interface started.
    pub timestamp: String,
    /// Whether the row shows received or injected traffic.
    pub direction: Direction,
    /// Protocol-specific summary (word or label), e.g. `0x0820`.
    pub command: String,
    /// Payload bytes or decoded value.
    pub data: String,
    /// Status detail (OK/CORRUPT plus protocol-specific state).
    pub status: String,
    /// Which protocol tab the row belongs to.
    pub protocol: Tab,
}

/// Root application state driving the terminal UI.
pub struct App {
    /// Currently selected protocol tab.
    pub active_tab: Tab,
    /// Received (and injected) frames, oldest first.
    pub frames: Vec<FrameRow>,
    /// Text typed into the hex injection box.
    pub injection_buffer: String,
    /// Whether the injection input box is currently open.
    pub injecting: bool,
    /// Attachment status to the simulator.
    pub connection: ConnectionStatus,
    /// Whether the simulator task is running.
    pub simulator_running: bool,
    /// Set to true when the user asks to quit.
    pub should_quit: bool,
    /// Transient message shown in the status bar.
    pub status_message: String,
    /// Parsed frames injected via Enter are forwarded here for the caller to
    /// publish onto the virtual bus.
    outbound: UnboundedSender<Vec<u8>>,
    started_at: Instant,
    total_frames: usize,
}

impl App {
    /// Create a fresh application. Injected frames are sent on `outbound`.
    pub fn new(outbound: UnboundedSender<Vec<u8>>) -> Self {
        Self {
            active_tab: Tab::Mil1553b,
            frames: Vec::new(),
            injection_buffer: String::new(),
            injecting: false,
            connection: ConnectionStatus::Disconnected,
            simulator_running: false,
            should_quit: false,
            status_message: "Tab: switch | i: inject | s: start/stop | q: quit".to_string(),
            outbound,
            started_at: Instant::now(),
            total_frames: 0,
        }
    }

    /// Total frames processed (RX + TX).
    pub fn frame_count(&self) -> usize {
        self.total_frames
    }

    /// Rows belonging to `tab`, newest last.
    pub fn rows_for_tab(&self, tab: Tab) -> impl Iterator<Item = &FrameRow> {
        self.frames.iter().filter(move |row| row.protocol == tab)
    }

    /// Current simulator state as shown in the status bar.
    pub fn simulator_state_label(&self) -> &'static str {
        if self.simulator_running {
            "RUNNING"
        } else {
            "STOPPED"
        }
    }

    fn timestamp(&self) -> String {
        let ms = self.started_at.elapsed().as_millis();
        format!("{ms:>7}ms")
    }

    /// Ingest a word broadcast by the simulator.
    ///
    /// The simulator currently speaks MIL-STD-1553B, so RX rows are tagged to
    /// that tab. A command word opens a new row; the status and data words
    /// that follow are folded into it, mirroring the BC -> RT -> status flow.
    pub fn ingest_simulator_frame(&mut self, frame: &SimFrame) {
        self.total_frames += 1;
        self.connection = ConnectionStatus::Connected;
        let status = if frame.corrupted {
            "CORRUPT".to_string()
        } else {
            "OK".to_string()
        };

        match frame.word_type {
            WordType::Command => {
                self.frames.push(FrameRow {
                    timestamp: self.timestamp(),
                    direction: Direction::Rx,
                    command: format!("{:#06X}", frame.word),
                    data: String::new(),
                    status,
                    protocol: Tab::Mil1553b,
                });
            }
            WordType::Status => {
                if let Some(row) = self
                    .frames
                    .iter_mut()
                    .rev()
                    .find(|row| row.protocol == Tab::Mil1553b)
                {
                    row.status = format!("{status} ST{:#06X}", frame.word);
                }
            }
            WordType::Data => {
                if let Some(row) = self
                    .frames
                    .iter_mut()
                    .rev()
                    .find(|row| row.protocol == Tab::Mil1553b)
                {
                    if !row.data.is_empty() {
                        row.data.push(' ');
                    }
                    row.data.push_str(&format!("{:#06X}", frame.word));
                }
            }
        }
    }

    /// Ingest a protocol-tagged frame from the multi-channel hub.
    ///
    /// MIL-STD-1553B frames reuse the folding logic in
    /// [`ingest_simulator_frame`](Self::ingest_simulator_frame); ARINC 429 and
    /// Link 16 frames each become their own row on the matching tab.
    pub fn ingest_wire_frame(&mut self, frame: &WireFrame) {
        match frame {
            WireFrame::Mil1553b(inner) => self.ingest_simulator_frame(inner),
            WireFrame::Arinc429(word) => {
                self.total_frames += 1;
                self.connection = ConnectionStatus::Connected;
                let decoded = word.decode();
                self.frames.push(FrameRow {
                    timestamp: self.timestamp(),
                    direction: Direction::Rx,
                    command: format!("L{}", decoded.octal_label()),
                    data: format!("0x{:05X}", decoded.data),
                    status: format!("SSM{}", decoded.ssm),
                    protocol: Tab::Arinc429,
                });
            }
            WireFrame::Link16(link) => {
                self.total_frames += 1;
                self.connection = ConnectionStatus::Connected;
                self.frames.push(FrameRow {
                    timestamp: self.timestamp(),
                    direction: Direction::Rx,
                    command: format!("ST{}", link.message.source_track_number),
                    data: format_hex(&link.message.payload),
                    status: "OK".to_string(),
                    protocol: Tab::Link16,
                });
            }
        }
    }

    /// Handle a keyboard event (a single key press).
    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.injecting {
            self.handle_injection_key(key);
            return;
        }

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('i') => {
                self.injecting = true;
                self.injection_buffer.clear();
                self.status_message = "Inject hex frame (Enter=send, Esc=cancel)".to_string();
            }
            KeyCode::Char('s') => {
                self.simulator_running = !self.simulator_running;
                self.connection = if self.simulator_running {
                    ConnectionStatus::Connected
                } else {
                    ConnectionStatus::Disconnected
                };
                self.status_message = format!("simulator {}", self.simulator_state_label());
            }
            KeyCode::Tab => {
                self.active_tab = self.active_tab.next();
                self.status_message = format!("tab: {}", self.active_tab.title());
            }
            _ => {}
        }
    }

    fn handle_injection_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.injecting = false;
                self.injection_buffer.clear();
                self.status_message = "injection cancelled".to_string();
            }
            KeyCode::Enter => self.submit_injection(),
            KeyCode::Backspace => {
                self.injection_buffer.pop();
            }
            KeyCode::Char(c) => self.injection_buffer.push(c),
            _ => {}
        }
    }

    fn submit_injection(&mut self) {
        let raw = self.injection_buffer.trim();
        if raw.is_empty() {
            self.status_message = "injection buffer is empty".to_string();
            return;
        }

        match parse_hex(raw) {
            Ok(bytes) => {
                if bytes.len() < 2 {
                    self.status_message =
                        "frame needs at least 2 bytes (a command word)".to_string();
                    return;
                }
                let command = format!("{:#06X}", u16::from_be_bytes([bytes[0], bytes[1]]));
                let data = if bytes.len() > 2 {
                    format_hex(&bytes[2..])
                } else {
                    String::new()
                };
                self.frames.push(FrameRow {
                    timestamp: self.timestamp(),
                    direction: Direction::Tx,
                    command,
                    data,
                    status: "SENT".to_string(),
                    protocol: self.active_tab,
                });
                self.total_frames += 1;
                // Hand the raw bytes to the caller so they can be published
                // onto the virtual bus.
                let _ = self.outbound.send(bytes);
                self.status_message = format!("injected onto {}", self.active_tab.title());
                self.injection_buffer.clear();
                self.injecting = false;
            }
            Err(err) => {
                self.status_message = format!("hex error: {err}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn new_app() -> (App, UnboundedReceiverVec) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (App::new(tx), rx)
    }

    // Small helper type so tests can inspect forwarded bytes.
    type UnboundedReceiverVec = tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>;

    #[test]
    fn tab_cycles_through_all_tabs() {
        let (mut app, _rx) = new_app();
        assert_eq!(app.active_tab, Tab::Mil1553b);
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.active_tab, Tab::Arinc429);
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.active_tab, Tab::Link16);
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.active_tab, Tab::Mil1553b);
    }

    #[test]
    fn ingest_wire_frame_routes_arinc_and_link16_to_their_tabs() {
        let (mut app, _rx) = new_app();

        // ARINC 429: label 10 (octal 012), data 2000 (0x07D0), SSM 3.
        let raw = protocol_core::Arinc429Frame {
            label: 10,
            sdi: 0,
            data: 2000,
            ssm: 3,
            parity: true,
        }
        .to_word()
        .unwrap();
        let word = simulator::wire::Arinc429Word::new(raw).unwrap();
        app.ingest_wire_frame(&WireFrame::Arinc429(word));

        // Link 16: STN 7, payload DE AD.
        let link = simulator::wire::Link16Frame::new(
            0,
            chrono::Utc::now(),
            protocol_core::Link16Message::new(3, 2, 7, vec![0xde, 0xad]),
        )
        .unwrap();
        app.ingest_wire_frame(&WireFrame::Link16(link));

        let arinc_rows: Vec<_> = app.rows_for_tab(Tab::Arinc429).collect();
        assert_eq!(arinc_rows.len(), 1);
        assert_eq!(arinc_rows[0].command, "L012");
        assert!(arinc_rows[0].data.contains("007D0"), "data {}", arinc_rows[0].data);
        assert_eq!(arinc_rows[0].status, "SSM3");

        let link_rows: Vec<_> = app.rows_for_tab(Tab::Link16).collect();
        assert_eq!(link_rows.len(), 1);
        assert_eq!(link_rows[0].command, "ST7");
        assert!(link_rows[0].data.contains("DEAD"), "data {}", link_rows[0].data);

        // The 1553B tab stays untouched by wire-protocol traffic.
        assert_eq!(app.rows_for_tab(Tab::Mil1553b).count(), 0);
        assert_eq!(app.frame_count(), 2);
        assert_eq!(app.connection, ConnectionStatus::Connected);
    }

    #[test]
    fn q_quits_only_when_not_injecting() {
        let (mut app, _rx) = new_app();
        app.handle_key(key(KeyCode::Char('q')));
        assert!(app.should_quit);

        let (mut app, _rx) = new_app();
        app.injecting = true;
        app.handle_key(key(KeyCode::Char('q')));
        assert!(!app.should_quit);
        assert_eq!(app.injection_buffer, "q");
    }

    #[test]
    fn s_toggles_simulator_and_connection() {
        let (mut app, _rx) = new_app();
        assert_eq!(app.connection, ConnectionStatus::Disconnected);
        app.handle_key(key(KeyCode::Char('s')));
        assert!(app.simulator_running);
        assert_eq!(app.connection, ConnectionStatus::Connected);
        app.handle_key(key(KeyCode::Char('s')));
        assert!(!app.simulator_running);
        assert_eq!(app.connection, ConnectionStatus::Disconnected);
    }

    #[test]
    fn injection_enter_parses_hex_and_forwards_bytes() {
        let (mut app, mut rx) = new_app();
        app.handle_key(key(KeyCode::Char('i')));
        assert!(app.injecting);
        for c in "80 00 02 3C".chars() {
            app.handle_key(key(KeyCode::Char(c)));
        }
        assert_eq!(app.injection_buffer, "80 00 02 3C");
        app.handle_key(key(KeyCode::Enter));

        assert!(!app.injecting);
        assert!(app.injection_buffer.is_empty());
        let sent = rx.try_recv().expect("bytes forwarded");
        assert_eq!(sent, vec![0x80, 0x00, 0x02, 0x3C]);

        let row = app.rows_for_tab(Tab::Mil1553b).last().expect("tx row");
        assert_eq!(row.direction, Direction::Tx);
        assert_eq!(row.command, "0x8000");
        assert_eq!(row.data, "023C");
        assert_eq!(row.status, "SENT");
    }

    #[test]
    fn injection_rejects_bad_hex_and_keeps_box_open() {
        let (mut app, mut rx) = new_app();
        app.handle_key(key(KeyCode::Char('i')));
        for c in "XYZ".chars() {
            app.handle_key(key(KeyCode::Char(c)));
        }
        app.handle_key(key(KeyCode::Enter));
        assert!(app.injecting, "box stays open on parse error");
        assert!(app.status_message.contains("hex error"));
        assert!(rx.try_recv().is_err(), "nothing forwarded on error");
    }

    #[test]
    fn ingest_folds_status_and_data_into_command_row() {
        let (mut app, _rx) = new_app();
        let cmd = SimFrame {
            sequence: 0,
            word_type: WordType::Command,
            word: 0x0820,
            source: 31,
            corrupted: false,
        };
        let status = SimFrame {
            sequence: 1,
            word_type: WordType::Status,
            word: 0x0800,
            source: 2,
            corrupted: false,
        };
        let data = SimFrame {
            sequence: 2,
            word_type: WordType::Data,
            word: 0xCAFE,
            source: 2,
            corrupted: false,
        };
        app.ingest_simulator_frame(&cmd);
        app.ingest_simulator_frame(&status);
        app.ingest_simulator_frame(&data);

        let rows: Vec<_> = app.rows_for_tab(Tab::Mil1553b).collect();
        assert_eq!(rows.len(), 1, "command opens one row, status/data fold in");
        assert_eq!(rows[0].direction, Direction::Rx);
        assert_eq!(rows[0].command, "0x0820");
        assert_eq!(rows[0].data, "0xCAFE");
        assert!(rows[0].status.contains("OK"));
        assert!(rows[0].status.contains("ST"));
        assert_eq!(app.frame_count(), 3);
    }
}
