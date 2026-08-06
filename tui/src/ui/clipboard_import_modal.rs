use crate::app::{App, ClipboardImportModal, ModalTab};
use common::enums::{AllocStrategy, StreamPieceSelector};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

pub fn draw_clipboard_import_modal(f: &mut Frame, app: &App, modal: &ClipboardImportModal) {
    let theme = &app.theme;
    let area = super::centered_rect(70, 70, f.area());

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
