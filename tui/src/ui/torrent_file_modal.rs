use crate::app::{
    App,
    torrent_file_modal::{TorrentFileModal, TorrentFileModalTab},
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

pub fn draw_torrent_file_modal(f: &mut Frame, app: &App, modal: &TorrentFileModal) {
    let theme = &app.theme;
    let area = super::centered_rect(70, 55, f.area());
    f.render_widget(Clear, area);

    let outer = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_focused))
        .title(Span::styled(
            " Add Torrent File ",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = outer.inner(area);
    f.render_widget(outer, area);
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(inner);

    draw_tabs(f, app, modal, layout[0]);
    match modal.tab {
        TorrentFileModalTab::Torrent => draw_torrent_tab(f, app, modal, layout[1]),
        TorrentFileModalTab::FineTuning => super::clipboard_import_modal::draw_finetuning_fields(
            f,
            app,
            &modal.finetune,
            modal.finetune_cursor,
            layout[1],
        ),
    }
    draw_buttons(f, app, layout[2]);
}

fn draw_tabs(f: &mut Frame, app: &App, modal: &TorrentFileModal, area: Rect) {
    let active = |selected| {
        if selected {
            Style::default()
                .fg(app.theme.selected_fg)
                .bg(app.theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(app.theme.text_muted)
        }
    };
    let spans = vec![
        Span::styled(
            " Torrent File ",
            active(modal.tab == TorrentFileModalTab::Torrent),
        ),
        Span::raw("  "),
        Span::styled(
            " Fine Tuning ",
            active(modal.tab == TorrentFileModalTab::FineTuning),
        ),
        Span::raw("   (Tab to switch)"),
    ];
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_torrent_tab(f: &mut Frame, app: &App, modal: &TorrentFileModal, area: Rect) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(area);
    f.render_widget(
        Paragraph::new("Enter or drag and drop a .torrent file path\nMaximum file size: 16 MiB")
            .style(Style::default().fg(app.theme.foreground)),
        layout[0],
    );

    let value = if modal.path_input.is_empty() {
        "~/Downloads/example.torrent".to_string()
    } else if modal.editing_path {
        format!("{}▏", modal.path_input)
    } else {
        modal.path_input.clone()
    };
    let path_style = if modal.path_input.is_empty() {
        Style::default().fg(app.theme.text_muted)
    } else if modal.resolved_path.is_some() {
        Style::default().fg(app.theme.status_ok)
    } else {
        Style::default().fg(app.theme.foreground)
    };
    f.render_widget(
        Paragraph::new(value).style(path_style).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(if modal.editing_path {
                    app.theme.border_focused
                } else {
                    app.theme.border
                }))
                .title(" .torrent path "),
        ),
        layout[1],
    );

    let queue_name = app
        .queues
        .get(modal.queue_cursor)
        .map(|queue| queue.name.as_str())
        .unwrap_or("(no queues available)");
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Queue:  ", Style::default().fg(app.theme.foreground)),
            Span::styled(
                format!("◀ {queue_name} ▶"),
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
        ])),
        layout[2],
    );
    let hint = if modal.editing_path {
        "Enter: validate path   Tab: fine tuning   Esc: unfocus"
    } else {
        "Enter: edit path   h/l: select queue   Tab: fine tuning"
    };
    f.render_widget(
        Paragraph::new(hint).style(Style::default().fg(app.theme.text_muted)),
        layout[3],
    );
}

fn draw_buttons(f: &mut Frame, app: &App, area: Rect) {
    let spans = vec![
        Span::styled(
            " [s] Start Now ",
            Style::default()
                .fg(app.theme.selected_fg)
                .bg(app.theme.status_ok),
        ),
        Span::raw("  "),
        Span::styled(
            " [w] Save For Later ",
            Style::default()
                .fg(app.theme.selected_fg)
                .bg(app.theme.accent),
        ),
        Span::raw("  "),
        Span::styled(
            " [c/Esc] Cancel ",
            Style::default()
                .fg(app.theme.foreground)
                .bg(app.theme.border),
        ),
    ];
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}
