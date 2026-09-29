use crate::{
    modal::{
        Ctx,
        clipboard_import::{ClipboardImportModal, ModalTab},
        widgets::FineTuneEditor,
    },
    theme::Theme,
    ui::centered_rect,
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

pub fn draw_clipboard_import_modal(f: &mut Frame, modal: &ClipboardImportModal, ctx: &Ctx<'_>) {
    let theme = ctx.theme;
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
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(inner);

    draw_modal_tab_bar(f, theme, modal, layout[0]);
    match modal.tab {
        ModalTab::Urls => draw_modal_urls_tab(f, ctx, modal, layout[1]),
        ModalTab::FineTuning => {
            draw_finetuning_fields(f, theme, &modal.finetune_editor, layout[1])
        }
    }
    draw_modal_buttons(f, theme, layout[2]);
}

fn draw_modal_tab_bar(f: &mut Frame, theme: &Theme, modal: &ClipboardImportModal, area: Rect) {
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

fn draw_modal_urls_tab(f: &mut Frame, ctx: &Ctx<'_>, modal: &ClipboardImportModal, area: Rect) {
    let theme = ctx.theme;

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    let queue_name = ctx
        .queues
        .get(modal.queue_picker.cursor)
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

pub(crate) fn draw_finetuning_fields(
    f: &mut Frame,
    theme: &Theme,
    editor: &FineTuneEditor,
    area: Rect,
) {
    let fields = editor.rows(None);

    let items: Vec<ListItem> = fields
        .iter()
        .enumerate()
        .map(|(i, (label, value))| {
            let style = if i == editor.cursor {
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

fn draw_modal_buttons(f: &mut Frame, theme: &Theme, area: Rect) {
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
