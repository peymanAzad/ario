use super::*;
use crate::app::{
    App,
    queue_modal::{QueueModal, QueueModalMode, QueueModalTab, RecurrenceKind},
};
use common::enums::{AllocStrategy, StreamPieceSelector};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

pub fn draw_queue_modal(f: &mut Frame, app: &App, modal: &QueueModal) {
    let theme = &app.theme;
    let area = centered_rect(75, 80, f.area());
    f.render_widget(Clear, area);

    let title = match modal.mode {
        QueueModalMode::Create => " Create Queue ".to_string(),
        QueueModalMode::Edit { .. } => format!(" Edit Queue: {} ", modal.name),
    };

    let outer = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_focused))
        .title(Span::styled(
            title,
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // tab bar
            Constraint::Min(0),    // tab content
            Constraint::Length(1), // error line (blank if none)
            Constraint::Length(1), // buttons
        ])
        .split(inner);

    draw_queue_modal_tab_bar(f, theme, modal, layout[0]);
    match modal.tab {
        QueueModalTab::Common => draw_queue_modal_common_tab(f, app, modal, layout[1]),
        QueueModalTab::Scheduler => draw_queue_modal_scheduler_tab(f, theme, modal, layout[1]),
        QueueModalTab::DownloadItems => draw_queue_modal_items_tab(f, theme, modal, layout[1]),
    }

    if let Some(err) = &modal.error {
        f.render_widget(
            Paragraph::new(format!(" {err}")).style(Style::default().fg(theme.status_error)),
            layout[2],
        );
    }

    draw_queue_modal_buttons(f, theme, modal, layout[3]);
}

fn draw_queue_modal_tab_bar(
    f: &mut Frame,
    theme: &crate::theme::Theme,
    modal: &QueueModal,
    area: Rect,
) {
    let tab_style = |active: bool| {
        if active {
            Style::default()
                .fg(theme.selected_fg)
                .bg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text_muted)
        }
    };

    let mut spans = vec![
        Span::styled(" Common ", tab_style(modal.tab == QueueModalTab::Common)),
        Span::raw("  "),
        Span::styled(
            " Scheduler ",
            tab_style(modal.tab == QueueModalTab::Scheduler),
        ),
    ];

    if matches!(modal.mode, QueueModalMode::Edit { .. }) {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!(" Download Items ({}) ", modal.items.len()),
            tab_style(modal.tab == QueueModalTab::DownloadItems),
        ));
    }

    spans.push(Span::raw("   (Tab to switch)"));

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_queue_modal_common_tab(f: &mut Frame, app: &App, modal: &QueueModal, area: Rect) {
    let theme = &app.theme;
    let name_display = if modal.editing_text && modal.common_cursor == 0 {
        format!("{}▏", modal.text_buffer) // trailing block cursor while editing
    } else {
        modal.name.clone()
    };

    let alloc_label = match &modal.finetune.alloc_strategy {
        None => aria2_global_label(
            app.aria2_global_options
                .as_ref()
                .and_then(|options| options.alloc_strategy.as_ref())
                .map(alloc_strategy_label),
        ),
        Some(AllocStrategy::None) => "none".to_string(),
        Some(AllocStrategy::Prealloc) => "prealloc".to_string(),
        Some(AllocStrategy::Falloc) => "falloc".to_string(),
        Some(AllocStrategy::Trunc) => "trunc".to_string(),
    };
    let selector_label = match &modal.finetune.stream_piece_selector {
        None => aria2_global_label(
            app.aria2_global_options
                .as_ref()
                .and_then(|options| options.stream_piece_selector.as_ref())
                .map(stream_piece_selector_label),
        ),
        Some(StreamPieceSelector::Default) => "default".to_string(),
        Some(StreamPieceSelector::InOrder) => "inorder".to_string(),
        Some(StreamPieceSelector::Random) => "random".to_string(),
        Some(StreamPieceSelector::Geom) => "geom".to_string(),
    };

    let rows: [(String, String); 8] = [
        ("Name".to_string(), name_display),
        (
            "Max concurrent downloads".to_string(),
            format!("◀ {} ▶", modal.max_concurrent_downloads),
        ),
        (
            "Max retries".to_string(),
            format!("◀ {} ▶", modal.max_retries),
        ),
        (
            "Retry wait (seconds)".to_string(),
            format!("◀ {} ▶", modal.retry_wait_seconds),
        ),
        (
            "Connections per download".to_string(),
            format!(
                "◀ {} ▶",
                modal
                    .finetune
                    .connections_per_download
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| {
                        aria2_global_label(
                            app.aria2_global_options
                                .as_ref()
                                .and_then(|options| options.connections_per_download)
                                .map(|value| value.to_string()),
                        )
                    })
            ),
        ),
        (
            "Max connections per server".to_string(),
            format!(
                "◀ {} ▶",
                modal
                    .finetune
                    .max_connections_per_server
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| {
                        aria2_global_label(
                            app.aria2_global_options
                                .as_ref()
                                .and_then(|options| options.max_connections_per_server)
                                .map(|value| value.to_string()),
                        )
                    })
            ),
        ),
        ("File allocation".to_string(), format!("◀ {alloc_label} ▶")),
        (
            "Stream piece selector".to_string(),
            format!("◀ {selector_label} ▶"),
        ),
    ];

    let items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(i, (label, value))| {
            ListItem::new(format!("{label:<28} {value}"))
                .style(field_style(theme, i == modal.common_cursor))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(Span::styled(
                " j/k: field   h/l: adjust   Enter: edit name ",
                Style::default().fg(theme.text_muted),
            )),
    );

    f.render_widget(list, area);
}

fn aria2_global_label(value: Option<String>) -> String {
    value.map_or_else(
        || "(aria2 global)".to_string(),
        |value| format!("{value} (aria2 global)"),
    )
}

fn alloc_strategy_label(value: &AllocStrategy) -> String {
    match value {
        AllocStrategy::None => "none",
        AllocStrategy::Prealloc => "prealloc",
        AllocStrategy::Falloc => "falloc",
        AllocStrategy::Trunc => "trunc",
    }
    .to_string()
}

fn stream_piece_selector_label(value: &StreamPieceSelector) -> String {
    match value {
        StreamPieceSelector::Default => "default",
        StreamPieceSelector::InOrder => "inorder",
        StreamPieceSelector::Random => "random",
        StreamPieceSelector::Geom => "geom",
    }
    .to_string()
}

fn draw_queue_modal_scheduler_tab(
    f: &mut Frame,
    theme: &crate::theme::Theme,
    modal: &QueueModal,
    area: Rect,
) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(0)])
        .split(area);

    // Enabled + recurrence-kind toggle, always shown regardless of kind.
    let enabled_style = field_style(theme, modal.scheduler_cursor == 0);
    let kind_style = field_style(theme, modal.scheduler_cursor == 1);
    let top_lines = vec![
        Line::from(Span::styled(
            format!(
                "Scheduler enabled{:<10} ◀ {} ▶",
                "",
                if modal.scheduler_enabled { "yes" } else { "no" }
            ),
            enabled_style,
        )),
        Line::from(Span::styled(
            format!(
                "Recurrence{:<19} ◀ {} ▶",
                "",
                match modal.recurrence_kind {
                    RecurrenceKind::Weekly => "weekly",
                    RecurrenceKind::Once => "one-time",
                }
            ),
            kind_style,
        )),
    ];
    f.render_widget(Paragraph::new(top_lines), layout[0]);

    match modal.recurrence_kind {
        RecurrenceKind::Weekly => draw_weekly_fields(f, theme, modal, layout[1]),
        RecurrenceKind::Once => draw_once_fields(f, theme, modal, layout[1]),
    }
}

fn draw_weekly_fields(f: &mut Frame, theme: &crate::theme::Theme, modal: &QueueModal, area: Rect) {
    const DAY_LABELS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

    let days_row_active = modal.scheduler_cursor == 2;
    let mut day_spans = vec![Span::raw("Days:  ")];
    for (i, label) in DAY_LABELS.iter().enumerate() {
        let checked = modal.weekly_days[i];
        let is_cursor = days_row_active && modal.day_cursor == i;
        let text = format!(" {}{} ", if checked { "✓" } else { " " }, label);
        let style = if is_cursor {
            field_style(theme, true)
        } else if checked {
            Style::default().fg(theme.status_ok)
        } else {
            Style::default().fg(theme.text_muted)
        };
        day_spans.push(Span::styled(text, style));
    }

    let start_style = field_style(theme, modal.scheduler_cursor == 3);
    let end_style = field_style(theme, modal.scheduler_cursor == 4);
    let run_missed_style = field_style(theme, modal.scheduler_cursor == 5);

    let lines = vec![
        Line::from(day_spans),
        Line::from(""),
        Line::from(Span::styled(
            format!(
                "Start time            ◀ {} ▶",
                modal.weekly_start.format("%H:%M")
            ),
            start_style,
        )),
        Line::from(Span::styled(
            format!(
                "End time              ◀ {} ▶",
                modal.weekly_end.format("%H:%M")
            ),
            end_style,
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!(
                "Catch up in open window ◀ {} ▶",
                if modal.run_missed_on_startup {
                    "yes"
                } else {
                    "no"
                }
            ),
            run_missed_style,
        )),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(Span::styled(
            " j/k: field   h/l: adjust / move day   space: toggle day ",
            Style::default().fg(theme.text_muted),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_once_fields(f: &mut Frame, theme: &crate::theme::Theme, modal: &QueueModal, area: Rect) {
    let start_date_style = field_style(theme, modal.scheduler_cursor == 2);
    let start_time_style = field_style(theme, modal.scheduler_cursor == 3);
    let end_date_style = field_style(theme, modal.scheduler_cursor == 4);
    let end_time_style = field_style(theme, modal.scheduler_cursor == 5);
    let run_missed_style = field_style(theme, modal.scheduler_cursor == 6);

    let lines = vec![
        Line::from(Span::styled(
            format!(
                "Start date            ◀ {} ▶",
                modal.once_start_date.format("%Y-%m-%d")
            ),
            start_date_style,
        )),
        Line::from(Span::styled(
            format!(
                "Start time            ◀ {} ▶",
                modal.once_start_time.format("%H:%M")
            ),
            start_time_style,
        )),
        Line::from(Span::styled(
            format!(
                "End date              ◀ {} ▶",
                modal.once_end_date.format("%Y-%m-%d")
            ),
            end_date_style,
        )),
        Line::from(Span::styled(
            format!(
                "End time              ◀ {} ▶",
                modal.once_end_time.format("%H:%M")
            ),
            end_time_style,
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!(
                "Run missed on startup  ◀ {} ▶",
                if modal.run_missed_on_startup {
                    "yes"
                } else {
                    "no"
                }
            ),
            run_missed_style,
        )),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(Span::styled(
            " j/k: field   h/l: adjust date or time ",
            Style::default().fg(theme.text_muted),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_queue_modal_items_tab(
    f: &mut Frame,
    theme: &crate::theme::Theme,
    modal: &QueueModal,
    area: Rect,
) {
    if modal.items.is_empty() {
        f.render_widget(
            Paragraph::new("No downloads in this queue (or still loading…)")
                .style(Style::default().fg(theme.text_muted))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(theme.border)),
                ),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = modal
        .items
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let label = d.filename.clone().unwrap_or_else(|| d.url.clone());
            ListItem::new(format!("{}. {label}", i + 1))
                .style(field_style(theme, i == modal.item_cursor))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(Span::styled(
                " j/k: select   J/K: move item down/up ",
                Style::default().fg(theme.text_muted),
            )),
    );

    f.render_widget(list, area);
}

fn draw_queue_modal_buttons(
    f: &mut Frame,
    theme: &crate::theme::Theme,
    modal: &QueueModal,
    area: Rect,
) {
    let save_label = match modal.mode {
        QueueModalMode::Create => " [s] Create ",
        QueueModalMode::Edit { .. } => " [s] Save ",
    };

    let spans = vec![
        Span::styled(
            save_label,
            Style::default().fg(theme.selected_fg).bg(theme.status_ok),
        ),
        Span::raw("  "),
        Span::styled(
            " [c/Esc] Cancel ",
            Style::default().fg(theme.foreground).bg(theme.border),
        ),
    ];

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[cfg(test)]
mod tests {
    use super::{alloc_strategy_label, aria2_global_label, stream_piece_selector_label};
    use common::enums::{AllocStrategy, StreamPieceSelector};

    #[test]
    fn global_labels_include_effective_value_or_text_fallback() {
        assert_eq!(aria2_global_label(Some("5".into())), "5 (aria2 global)");
        assert_eq!(aria2_global_label(None), "(aria2 global)");
        assert_eq!(alloc_strategy_label(&AllocStrategy::Prealloc), "prealloc");
        assert_eq!(
            stream_piece_selector_label(&StreamPieceSelector::InOrder),
            "inorder"
        );
    }
}
