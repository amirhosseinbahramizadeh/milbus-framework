//! Drawing routines for the milbus-analyzer terminal interface.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Tabs};
use ratatui::Frame;

use crate::app::{App, Direction as Dir, Tab};

/// Draw the entire interface for the current `app` state onto `frame`.
pub fn render(app: &App, frame: &mut Frame) {
    let area = frame.area();

    // Outer split: a large body area plus a one-line status bar.
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);

    // Body split: the tab header plus the frame table.
    let body = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(outer[0]);

    render_tabs(app, frame, body[0]);
    render_table(app, frame, body[1]);
    render_status_bar(app, frame, outer[1]);

    if app.injecting {
        render_injection_box(app, frame, body[1]);
    }
}

fn render_tabs(app: &App, frame: &mut Frame, area: Rect) {
    let titles: Vec<_> = Tab::ALL.iter().map(|t| t.title()).collect();
    let selected = Tab::ALL
        .iter()
        .position(|t| *t == app.active_tab)
        .unwrap_or(0);
    let tabs = Tabs::new(titles)
        .select(selected)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("milbus-analyzer"),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .divider("\u{2502}"); // box-drawing vertical bar
    frame.render_widget(tabs, area);
}

fn render_table(app: &App, frame: &mut Frame, area: Rect) {
    let header = Row::new(["Timestamp", "Dir", "Command", "Data", "Status"])
        .style(Style::default().add_modifier(Modifier::BOLD));

    let widths = [
        Constraint::Length(11),
        Constraint::Length(4),
        Constraint::Length(11),
        Constraint::Min(16),
        Constraint::Length(16),
    ];

    let rows: Vec<Row> = app
        .rows_for_tab(app.active_tab)
        .map(|row| {
            let dir_style = match row.direction {
                Dir::Rx => Style::default().fg(Color::Green),
                Dir::Tx => Style::default().fg(Color::Yellow),
            };
            Row::new(vec![
                Cell::from(row.timestamp.clone()),
                Cell::from(row.direction.label()).style(dir_style),
                Cell::from(row.command.clone()),
                Cell::from(row.data.clone()),
                Cell::from(row.status.clone()),
            ])
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(app.active_tab.title());
    let table = Table::new(rows, widths).header(header).block(block);
    frame.render_widget(table, area);
}

fn render_status_bar(app: &App, frame: &mut Frame, area: Rect) {
    let line = format!(
        " {} | sim {} | frames {} | {} | {}",
        app.connection.label(),
        app.simulator_state_label(),
        app.frame_count(),
        app.active_tab.title(),
        app.status_message,
    );
    let paragraph =
        Paragraph::new(line).style(Style::default().bg(Color::DarkGray).fg(Color::White));
    frame.render_widget(paragraph, area);
}

fn render_injection_box(app: &App, frame: &mut Frame, area: Rect) {
    let box_area = centered_rect(60, 5, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Inject hex (Enter=send, Esc=cancel)");
    let paragraph = Paragraph::new(app.injection_buffer.as_str())
        .block(block)
        .style(Style::default().fg(Color::Cyan));
    frame.render_widget(paragraph, box_area);
}

/// A rectangle centered within `area`, sized as a percentage of it.
fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{ConnectionStatus, Direction, FrameRow};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn test_app() -> App {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        App::new(tx)
    }

    fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
        let buffer = terminal.backend().buffer();
        let area = *buffer.area();
        let mut out = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                out.push_str(buffer[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn render_draws_tabs_table_and_status_bar() {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = test_app();
        app.simulator_running = true;
        app.connection = ConnectionStatus::Connected;
        app.frames.push(FrameRow {
            timestamp: "      10ms".into(),
            direction: Direction::Rx,
            command: "0x0820".into(),
            data: "0xCAFE".into(),
            status: "OK".into(),
            protocol: Tab::Mil1553b,
        });

        terminal.draw(|f| render(&app, f)).unwrap();
        let text = buffer_text(&terminal);
        assert!(text.contains("MIL-1553B"));
        assert!(text.contains("ARINC 429"));
        assert!(text.contains("Link 16"));
        assert!(text.contains("Timestamp"));
        assert!(text.contains("Command"));
        assert!(text.contains("CONNECTED"));
        assert!(text.contains("RUNNING"));
        assert!(text.contains("0x0820"));
    }

    #[test]
    fn injection_box_renders_only_when_open() {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = test_app();
        terminal.draw(|f| render(&app, f)).unwrap();
        assert!(!buffer_text(&terminal).contains("Inject hex"));

        app.injecting = true;
        app.injection_buffer.push_str("DEAD");
        terminal.draw(|f| render(&app, f)).unwrap();
        let text = buffer_text(&terminal);
        assert!(text.contains("Inject hex"));
        assert!(text.contains("DEAD"));
    }
}
