use crate::mouse::{self, Command as C, ModalIntent as I, ScrollTarget as S};
use crate::{
    modal::{
        Ctx,
        clipboard_import::{ClipboardImportModal, ModalTab},
        widgets::FineTuneEditor,
    },
    ui::centered_rect,
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
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

    draw_modal_tab_bar(f, ctx, modal, layout[0]);
    match modal.tab {
        ModalTab::Urls => draw_modal_urls_tab(f, ctx, modal, layout[1]),
        ModalTab::FineTuning => draw_finetuning_fields(f, ctx, &modal.finetune_editor, layout[1]),
    }
    draw_modal_buttons(f, ctx, layout[2]);
}

fn draw_modal_tab_bar(f: &mut Frame, ctx: &Ctx<'_>, modal: &ClipboardImportModal, area: Rect) {
    let theme = ctx.theme;
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

    mouse::spans(
        ctx.interactions,
        area,
        &spans,
        &[
            (0, mouse::modal_target(I::Tab(0))),
            (2, mouse::modal_target(I::Tab(1))),
        ],
    );
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

    let queue_text = format!("Queue:  ◀ {queue_name} ▶");
    mouse::selector(ctx.interactions, layout[0], &queue_text);
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

    let inner = Block::default().borders(Borders::ALL).inner(layout[1]);
    let offset = mouse::list(
        f,
        ctx.interactions,
        S::Urls,
        list,
        (layout[1], inner),
        modal.url_cursor,
        modal.entries.len(),
    );
    for row in 0..inner.height {
        let index = offset + row as usize;
        if index >= modal.entries.len() {
            break;
        }
        let area = Rect::new(inner.x, inner.y + row, inner.width, 1);
        ctx.interactions
            .borrow_mut()
            .add(area, mouse::modal_target(I::Field(index)));
        ctx.interactions.borrow_mut().add(
            Rect::new(
                inner.x.saturating_add(2),
                area.y,
                inner.width.saturating_sub(2).min(3),
                1,
            ),
            mouse::modal_target(I::ToggleUrl(index)),
        );
    }

    let actions = [
        Span::raw("space: toggle"),
        Span::raw("   "),
        Span::raw("a: select all"),
        Span::raw("   "),
        Span::raw("n: select none"),
    ];
    mouse::spans(
        ctx.interactions,
        layout[2],
        &actions,
        &[
            (0, mouse::modal_target(I::ToggleUrl(modal.url_cursor))),
            (2, mouse::command(C::All)),
            (4, mouse::command(C::None)),
        ],
    );
    f.render_widget(
        Paragraph::new("space: toggle   a: select all   n: select none")
            .style(Style::default().fg(theme.text_muted)),
        layout[2],
    );
}

pub(crate) fn draw_finetuning_fields(
    f: &mut Frame,
    ctx: &Ctx<'_>,
    editor: &FineTuneEditor,
    area: Rect,
) {
    let rows = editor
        .rows(None)
        .into_iter()
        .map(|(label, value)| format!("{label:<28} ◀ {value} ▶"))
        .collect::<Vec<_>>();
    mouse::fields(
        f,
        ctx,
        S::Fields,
        area,
        editor.cursor,
        &rows,
        " j/k: select field   h/l: adjust value ",
    );
}

fn draw_modal_buttons(f: &mut Frame, ctx: &Ctx<'_>, area: Rect) {
    let theme = ctx.theme;
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

    mouse::spans(
        ctx.interactions,
        area,
        &spans,
        &[
            (0, mouse::command(C::Start)),
            (2, mouse::command(C::Later)),
            (4, mouse::command(C::Cancel)),
        ],
    );
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}
