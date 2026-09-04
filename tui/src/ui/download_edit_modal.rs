use super::*;
use crate::app::{App, download_edit_modal::DownloadEditModal};
use common::enums::{AllocStrategy, StreamPieceSelector};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

pub fn draw_download_modal(f: &mut Frame, app: &App, modal: &DownloadEditModal) {
    let theme = &app.theme;
    let area = centered_rect(55, 45, f.area());
    f.render_widget(Clear, area);

    let outer = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_focused))
        .title(Span::styled(
            " Edit Download ",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let alloc_label = match &modal.finetune.alloc_strategy {
        None => "(default)".to_string(),
        Some(AllocStrategy::None) => "none".to_string(),
        Some(AllocStrategy::Prealloc) => "prealloc".to_string(),
        Some(AllocStrategy::Falloc) => "falloc".to_string(),
        Some(AllocStrategy::Trunc) => "trunc".to_string(),
    };
    let selector_label = match &modal.finetune.stream_piece_selector {
        None => "(default)".to_string(),
        Some(StreamPieceSelector::Default) => "default".to_string(),
        Some(StreamPieceSelector::InOrder) => "inorder".to_string(),
        Some(StreamPieceSelector::Random) => "random".to_string(),
        Some(StreamPieceSelector::Geom) => "geom".to_string(),
    };

    let queue_label = app
        .queues
        .get(modal.queue_cursor)
        .map(|queue| queue.name.clone())
        .unwrap_or_else(|| "(unavailable)".into());
    let rows: [(&str, String); 5] = [
        (
            "Connections per download",
            modal
                .finetune
                .connections_per_download
                .map(|v| v.to_string())
                .unwrap_or_else(|| "(default)".into()),
        ),
        (
            "Max connections per server",
            modal
                .finetune
                .max_connections_per_server
                .map(|v| v.to_string())
                .unwrap_or_else(|| "(default)".into()),
        ),
        ("File allocation", alloc_label),
        ("Stream piece selector", selector_label),
        ("Queue", queue_label),
    ];

    let items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(i, (label, value))| {
            ListItem::new(format!("{label:<28} ◀ {value} ▶"))
                .style(field_style(theme, i == modal.cursor))
        })
        .collect();

    f.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border))
                .title(Span::styled(
                    " j/k: field   h/l: adjust ",
                    Style::default().fg(theme.text_muted),
                )),
        ),
        layout[0],
    );

    if let Some(err) = &modal.error {
        f.render_widget(
            Paragraph::new(format!(" {err}")).style(Style::default().fg(theme.status_error)),
            layout[1],
        );
    }

    let spans = vec![
        Span::styled(
            " [s] Save ",
            Style::default().fg(theme.selected_fg).bg(theme.status_ok),
        ),
        Span::raw("  "),
        Span::styled(
            " [c/Esc] Cancel ",
            Style::default().fg(theme.foreground).bg(theme.border),
        ),
    ];
    f.render_widget(Paragraph::new(Line::from(spans)), layout[2]);
}
