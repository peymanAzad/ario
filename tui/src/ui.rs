use crate::app::{ALL_CATEGORIES, App, ClipboardImportModal, Focus, ModalTab};
use common::enums::{AllocStrategy, DownloadStatus, FileCategory, StreamPieceSelector};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table, TableState,
    },
};

pub fn render(app: &mut App, f: &mut Frame) {
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // status bar
            Constraint::Min(0),    // body
            Constraint::Length(1), // footer / keybindings
        ])
        .split(f.area());
    let body_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(20), Constraint::Percentage(80)])
        .split(main_layout[1]);
    let left_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(8)])
        .split(body_layout[0]);

    draw_status_bar(f, app, main_layout[0]);
    draw_queues_list(f, app, left_layout[0]);
    draw_categories_list(f, app, left_layout[1]);
    draw_downloads_table(f, app, body_layout[1]);
    draw_footer(f, app, main_layout[2]);

    if let Some(modal) = &app.modal {
        draw_clipboard_import_modal(f, app, modal);
    }
}

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;

    let aria2_indicator = if app.aria2_reachable {
        Span::styled(
            " aria2: up ",
            Style::default().fg(theme.selected_fg).bg(theme.status_ok),
        )
    } else {
        Span::styled(
            " aria2: down ",
            Style::default().fg(theme.foreground).bg(theme.status_error),
        )
    };

    let mut spans = vec![
        Span::styled(
            " Ario ",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        aria2_indicator,
    ];

    if let Some(label) = filter_indicator_label(app) {
        spans.push(Span::styled(
            format!(" {label} "),
            Style::default().fg(theme.selected_fg).bg(theme.accent),
        ));
    }

    if let Some(err) = &app.last_error {
        spans.push(Span::styled(
            format!("  {err}"),
            Style::default().fg(theme.status_error),
        ));
    }

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn filter_indicator_label(app: &App) -> Option<String> {
    let queue_part = if app.selected_queue != 0 {
        app.queues
            .get(app.selected_queue - 1)
            .map(|q| q.name.clone())
    } else {
        None
    };

    let category_part = if app.selected_category != 0 {
        ALL_CATEGORIES
            .get(app.selected_category - 1)
            .map(category_label)
    } else {
        None
    };

    match (queue_part, category_part) {
        (None, None) => None,
        (Some(q), None) => Some(format!("Filter: {q}")),
        (None, Some(c)) => Some(format!("Filter: {c}")),
        (Some(q), Some(c)) => Some(format!("Filter: {q} · {c}")),
    }
}

fn border_style(theme: &crate::theme::Theme, focused: bool) -> Style {
    if focused {
        Style::default().fg(theme.border_focused)
    } else {
        Style::default().fg(theme.border)
    }
}

fn highlight_style(theme: &crate::theme::Theme, focused: bool) -> Style {
    if focused {
        Style::default()
            .bg(theme.selected_bg)
            .fg(theme.selected_fg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.accent)
    }
}

fn draw_downloads_table(f: &mut Frame, app: &App, area: Rect) {
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

fn draw_queues_list(f: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let focused = app.focus == Focus::Queues;

    let mut items: Vec<String> = app.queues.iter().map(|q| q.name.clone()).collect();
    items.insert(0, "All".to_string());

    let mut state = ListState::default();
    state.select(Some(app.selected_queue));

    let list = List::new(items)
        .style(Style::default().fg(theme.foreground))
        .highlight_style(highlight_style(theme, focused))
        .highlight_symbol("> ")
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style(theme, focused))
                .title(Span::styled(
                    " [1] Queues ",
                    Style::default().fg(theme.foreground),
                )),
        );

    f.render_stateful_widget(list, area, &mut state);
}

fn draw_categories_list(f: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let focused = app.focus == Focus::Categories;

    let mut items: Vec<String> = ALL_CATEGORIES.iter().map(category_label).collect();
    items.insert(0, "All".to_string());

    let mut state = ListState::default();
    state.select(Some(app.selected_category));

    let list = List::new(items)
        .style(Style::default().fg(theme.foreground))
        .highlight_style(highlight_style(theme, focused))
        .highlight_symbol("> ")
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style(theme, focused))
                .title(Span::styled(
                    " [2] Categories ",
                    Style::default().fg(theme.foreground),
                )),
        );

    f.render_stateful_widget(list, area, &mut state);
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let help = match app.focus {
        Focus::Downloads => {
            "1/2/3 or Tab: switch pane   j/k ↑/↓: navigate   p: pause   r: resume   d: delete   v: import clipboard   q: quit"
        }
        Focus::Queues | Focus::Categories => {
            "1/2/3 or Tab: switch pane   j/k ↑/↓: navigate   v: import clipboard   q: quit"
        }
    };
    f.render_widget(
        Paragraph::new(help).style(Style::default().fg(app.theme.text_muted)),
        area,
    );
}

fn category_label(c: &FileCategory) -> String {
    match c {
        FileCategory::Video => "Video",
        FileCategory::Music => "Music",
        FileCategory::Document => "Document",
        FileCategory::Archive => "Archive",
        FileCategory::Program => "Program",
        FileCategory::Other => "Other",
    }
    .to_string()
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

fn draw_clipboard_import_modal(f: &mut Frame, app: &App, modal: &ClipboardImportModal) {
    let theme = &app.theme;
    let area = centered_rect(70, 70, f.area());

    f.render_widget(Clear, area);

    let outer = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_focused))
        .title(Span::styled(
            " Import from Clipboard ",
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
            Constraint::Length(1), // buttons
        ])
        .split(inner);

    draw_modal_tab_bar(f, app, modal, layout[0]);
    match modal.tab {
        ModalTab::Urls => draw_modal_urls_tab(f, app, modal, layout[1]),
        ModalTab::FineTuning => draw_modal_finetuning_tab(f, app, modal, layout[1]),
    }
    draw_modal_buttons(f, app, layout[2]);
}

fn draw_modal_tab_bar(f: &mut Frame, app: &App, modal: &ClipboardImportModal, area: Rect) {
    let theme = &app.theme;

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

    let spans = vec![
        Span::styled(
            format!(" URLs ({}) ", modal.entries.len()),
            tab_style(modal.tab == ModalTab::Urls),
        ),
        Span::raw("  "),
        Span::styled(
            " Fine Tuning ",
            tab_style(modal.tab == ModalTab::FineTuning),
        ),
        Span::raw("   (Tab to switch)"),
    ];

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_modal_urls_tab(f: &mut Frame, app: &App, modal: &ClipboardImportModal, area: Rect) {
    let theme = &app.theme;

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // queue picker
            Constraint::Min(0),    // URL checklist
            Constraint::Length(1), // hint line
        ])
        .split(area);

    let queue_name = app
        .queues
        .get(modal.queue_cursor)
        .map(|q| q.name.as_str())
        .unwrap_or("(no queues available)");
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Queue:  ", Style::default().fg(theme.foreground)),
            Span::styled(
                format!("◀ {queue_name} ▶"),
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
        ])),
        layout[0],
    );

    let items: Vec<ListItem> = modal
        .entries
        .iter()
        .map(|entry| {
            let checkbox = if entry.selected { "[x] " } else { "[ ] " };
            let style = if entry.selected {
                Style::default().fg(theme.foreground)
            } else {
                Style::default().fg(theme.text_muted)
            };
            ListItem::new(format!("{checkbox}{}", entry.url)).style(style)
        })
        .collect();

    let mut state = ListState::default();
    if !modal.entries.is_empty() {
        state.select(Some(modal.url_cursor));
    }

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(theme.selected_bg)
                .fg(theme.selected_fg)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ")
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border)),
        );

    f.render_stateful_widget(list, layout[1], &mut state);

    f.render_widget(
        Paragraph::new("space: toggle   a: select all   n: select none")
            .style(Style::default().fg(theme.text_muted)),
        layout[2],
    );
}

fn draw_modal_finetuning_tab(f: &mut Frame, app: &App, modal: &ClipboardImportModal, area: Rect) {
    let theme = &app.theme;

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

    let fields: [(&str, String); 4] = [
        (
            "Connections per download",
            modal
                .finetune
                .connections_per_download
                .map(|v| v.to_string())
                .unwrap_or_else(|| "(default)".to_string()),
        ),
        (
            "Max connections per server",
            modal
                .finetune
                .max_connections_per_server
                .map(|v| v.to_string())
                .unwrap_or_else(|| "(default)".to_string()),
        ),
        ("File allocation", alloc_label),
        ("Stream piece selector", selector_label),
    ];

    let items: Vec<ListItem> = fields
        .iter()
        .enumerate()
        .map(|(i, (label, value))| {
            let style = if i == modal.finetune_cursor {
                Style::default()
                    .bg(theme.selected_bg)
                    .fg(theme.selected_fg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.foreground)
            };
            ListItem::new(format!("{label:<28} ◀ {value} ▶")).style(style)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(Span::styled(
                " j/k: select field   h/l: adjust value ",
                Style::default().fg(theme.text_muted),
            )),
    );

    f.render_widget(list, area);
}

fn draw_modal_buttons(f: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;

    let spans = vec![
        Span::styled(
            " [s] Start Now ",
            Style::default().fg(theme.selected_fg).bg(theme.status_ok),
        ),
        Span::raw("  "),
        Span::styled(
            " [w] Save For Later ",
            Style::default().fg(theme.selected_fg).bg(theme.accent),
        ),
        Span::raw("  "),
        Span::styled(
            " [c/Esc] Cancel ",
            Style::default().fg(theme.foreground).bg(theme.border),
        ),
    ];

    f.render_widget(Paragraph::new(Line::from(spans)), area);
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
