use crate::app::App;
use common::enums::DownloadStatus;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
};

pub fn render(app: &mut App, f: &mut Frame) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // status bar
            Constraint::Min(0),    // downloads table
            Constraint::Length(1), // footer / keybindings
        ])
        .split(f.area());

    draw_status_bar(f, app, chunks[0]);
    draw_downloads_table(f, app, chunks[1]);
    draw_footer(f, chunks[2]);
}

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let aria2_indicator = if app.aria2_reachable {
        Span::styled(
            " aria2: up ",
            Style::default().fg(Color::Black).bg(Color::Green),
        )
    } else {
        Span::styled(
            " aria2: down ",
            Style::default().fg(Color::White).bg(Color::Red),
        )
    };

    let mut spans = vec![
        Span::styled(" Ario ", Style::default().add_modifier(Modifier::BOLD)),
        aria2_indicator,
    ];

    if let Some(err) = &app.last_error {
        spans.push(Span::styled(
            format!("  {err}"),
            Style::default().fg(Color::Red),
        ));
    }

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_downloads_table(f: &mut Frame, app: &App, area: Rect) {
    let header = Row::new(vec!["Name", "Status", "Progress", "Speed", "ETA"])
        .style(Style::default().add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = app
        .downloads
        .iter()
        .map(|d| {
            let name = d
                .download
                .filename
                .clone()
                .unwrap_or_else(|| d.download.url.clone());

            let progress = match d.download.size {
                Some(total) if total > 0 => {
                    format!("{:.1}%", (d.completed_length as f64 / total as f64) * 100.0)
                }
                _ => "-".to_string(),
            };

            Row::new(vec![
                Cell::from(name),
                Cell::from(format_status(&d.download.status)),
                Cell::from(progress),
                Cell::from(format_speed(d.download_speed)),
                Cell::from(
                    d.eta_seconds
                        .map(format_eta)
                        .unwrap_or_else(|| "-".to_string()),
                ),
            ])
        })
        .collect();

    let widths = [
        Constraint::Percentage(40),
        Constraint::Percentage(15),
        Constraint::Percentage(15),
        Constraint::Percentage(15),
        Constraint::Percentage(15),
    ];

    let mut state = TableState::default();
    if !app.downloads.is_empty() {
        state.select(Some(app.selected));
    }

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(" Downloads "))
        .row_highlight_style(
            Style::default()
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(table, area, &mut state);
}

fn draw_footer(f: &mut Frame, area: Rect) {
    let help = "q: quit   j/k ↑/↓: navigate   p: pause   r: resume   d: delete";
    f.render_widget(
        Paragraph::new(help).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}

fn format_status(status: &DownloadStatus) -> String {
    match status {
        DownloadStatus::Pending => "Pending".to_string(),
        DownloadStatus::Active => "Active".to_string(),
        DownloadStatus::Paused => "Paused".to_string(),
        DownloadStatus::Completed => "Completed".to_string(),
        DownloadStatus::Error(msg) => format!("Error: {msg}"),
        DownloadStatus::Removed => "Removed".to_string(),
    }
}

fn format_speed(bytes_per_sec: u64) -> String {
    if bytes_per_sec == 0 {
        return "-".to_string();
    }
    format!("{}/s", format_bytes(bytes_per_sec))
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;
    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }
    format!("{size:.1} {}", UNITS[unit_idx])
}

fn format_eta(seconds: u64) -> String {
    let h = seconds / 3600;
    let m = (seconds % 3600) / 60;
    let s = seconds % 60;
    if h > 0 {
        format!("{h}h{m}m")
    } else if m > 0 {
        format!("{m}m{s}s")
    } else {
        format!("{s}s")
    }
}
