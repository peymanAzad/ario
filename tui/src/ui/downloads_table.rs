use super::*;
use crate::app::{App, Focus};
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Cell, Row, Table, TableState},
};

pub fn draw_downloads_table(f: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let focused = app.focus == Focus::Downloads;

    let header = Row::new(vec!["Name", "Status", "Progress", "Speed", "ETA"]).style(
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD),
    );

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
        state.select(Some(app.selected_download));
    }

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style(theme, focused))
                .title(Span::styled(
                    " [3] Downloads ",
                    Style::default().fg(theme.foreground),
                )),
        )
        .row_highlight_style(highlight_style(theme, focused));

    f.render_stateful_widget(table, area, &mut state);
}
