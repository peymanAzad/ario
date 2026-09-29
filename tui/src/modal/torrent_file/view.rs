use crate::{
    modal::{
        Ctx,
        clipboard_import::view::draw_finetuning_fields,
        torrent_file::{TorrentFileModal, TorrentFileModalTab},
    },
    ui::centered_rect,
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

pub fn draw_torrent_file_modal(f: &mut Frame, modal: &TorrentFileModal, ctx: &Ctx<'_>) {
    let theme = ctx.theme;
    let area = centered_rect(70, 55, f.area());
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

    draw_tabs(f, ctx, modal, layout[0]);
    match modal.tab {
        TorrentFileModalTab::Torrent => draw_torrent_tab(f, ctx, modal, layout[1]),
        TorrentFileModalTab::FineTuning => draw_finetuning_fields(
            f,
            theme,
            &modal.finetune,
            modal.finetune_cursor,
            layout[1],
        ),
    }
    draw_buttons(f, ctx, layout[2]);
}

fn draw_tabs(f: &mut Frame, ctx: &Ctx<'_>, modal: &TorrentFileModal, area: Rect) {
    let theme = ctx.theme;
    let active = |selected| {
        if selected {
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

fn draw_torrent_tab(f: &mut Frame, ctx: &Ctx<'_>, modal: &TorrentFileModal, area: Rect) {
    let theme = ctx.theme;
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
            .style(Style::default().fg(theme.foreground)),
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
        Style::default().fg(theme.text_muted)
    } else if modal.resolved_path.is_some() {
        Style::default().fg(theme.status_ok)
    } else {
        Style::default().fg(theme.foreground)
    };
    f.render_widget(
        Paragraph::new(value).style(path_style).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(if modal.editing_path {
                    theme.border_focused
                } else {
                    theme.border
                }))
                .title(" .torrent path "),
        ),
        layout[1],
    );

    let queue_name = ctx
        .queues
        .get(modal.queue_cursor)
        .map(|queue| queue.name.as_str())
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
        layout[2],
    );
    let hint = if modal.editing_path {
        "Enter: validate path   Tab: fine tuning   Esc: unfocus"
    } else {
        "Enter: edit path   h/l: select queue   Tab: fine tuning"
    };
    f.render_widget(
        Paragraph::new(hint).style(Style::default().fg(theme.text_muted)),
        layout[3],
    );
}

fn draw_buttons(f: &mut Frame, ctx: &Ctx<'_>, area: Rect) {
    let theme = ctx.theme;
    let spans = vec![
        Span::styled(
            " [s] Start Now ",
            Style::default()
                .fg(theme.selected_fg)
                .bg(theme.status_ok),
        ),
        Span::raw("  "),
        Span::styled(
            " [w] Save For Later ",
            Style::default()
                .fg(theme.selected_fg)
                .bg(theme.accent),
        ),
        Span::raw("  "),
        Span::styled(
            " [c/Esc] Cancel ",
            Style::default()
                .fg(theme.foreground)
                .bg(theme.border),
        ),
    ];
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}
