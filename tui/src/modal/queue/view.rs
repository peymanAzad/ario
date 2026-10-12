use crate::mouse::{self, Command as C, ModalIntent as I, ScrollTarget as S};
use crate::{
    modal::{
        Ctx,
        queue::{QueueModal, QueueModalMode, QueueModalTab, RecurrenceKind},
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

pub fn draw_queue_modal(f: &mut Frame, modal: &QueueModal, ctx: &Ctx<'_>) {
    let theme = ctx.theme;
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
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);

    draw_queue_modal_tab_bar(f, ctx, modal, layout[0]);
    match modal.tab {
        QueueModalTab::Common => draw_queue_modal_common_tab(f, ctx, modal, layout[1]),
        QueueModalTab::Scheduler => draw_queue_modal_scheduler_tab(f, ctx, modal, layout[1]),
        QueueModalTab::DownloadItems => draw_queue_modal_items_tab(f, ctx, modal, layout[1]),
    }

    if let Some(err) = &modal.error {
        f.render_widget(
            Paragraph::new(format!(" {err}")).style(Style::default().fg(theme.status_error)),
            layout[2],
        );
    }

    draw_queue_modal_buttons(f, ctx, modal, layout[3]);
}

fn draw_queue_modal_tab_bar(f: &mut Frame, ctx: &Ctx<'_>, modal: &QueueModal, area: Rect) {
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

    mouse::spans(
        ctx.interactions,
        area,
        &spans,
        &[
            (0, mouse::modal_target(I::Tab(0))),
            (2, mouse::modal_target(I::Tab(1))),
            (4, mouse::modal_target(I::Tab(2))),
        ],
    );
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_queue_modal_common_tab(f: &mut Frame, ctx: &Ctx<'_>, modal: &QueueModal, area: Rect) {
    let name_display = if modal.name_input.editing && modal.common_cursor == 0 {
        format!("{}▏", modal.name_input.buffer)
    } else {
        modal.name.clone()
    };

    let mut rows = vec![
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
    ];
    // Queue common tab shows only the first four finetune fields (not retries).
    for (label, value) in modal
        .finetune_editor
        .rows(ctx.aria2_global_options)
        .into_iter()
        .take(4)
    {
        rows.push((label, format!("◀ {value} ▶")));
    }

    let rows = rows
        .into_iter()
        .map(|(label, value)| format!("{label:<28} {value}"))
        .collect::<Vec<_>>();
    let (inner, offset) = mouse::fields(
        f,
        ctx,
        S::Common,
        area,
        modal.common_cursor,
        &rows,
        " j/k: field   h/l: adjust   Enter: edit name ",
    );
    if offset == 0 && inner.height > 0 {
        ctx.interactions.borrow_mut().add(
            Rect::new(inner.x, inner.y, inner.width, 1),
            mouse::modal_target(I::Text),
        );
    }
}

fn draw_queue_modal_scheduler_tab(f: &mut Frame, ctx: &Ctx<'_>, modal: &QueueModal, area: Rect) {
    let day_spans = std::iter::once(Span::raw("Days:  "))
        .chain(
            ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
                .into_iter()
                .enumerate()
                .map(|(i, day)| {
                    let style = if modal.scheduler_cursor == 2 && modal.day_cursor == i {
                        Style::default()
                            .fg(ctx.theme.selected_fg)
                            .bg(ctx.theme.accent)
                            .add_modifier(Modifier::BOLD)
                    } else if modal.weekly_days[i] {
                        Style::default().fg(ctx.theme.status_ok)
                    } else {
                        Style::default().fg(ctx.theme.text_muted)
                    };
                    Span::styled(
                        format!(" {}{day} ", if modal.weekly_days[i] { "✓" } else { " " }),
                        style,
                    )
                }),
        )
        .collect::<Vec<_>>();
    let mut rows = vec![
        format!(
            "Scheduler enabled             ◀ {} ▶",
            if modal.scheduler_enabled { "yes" } else { "no" }
        ),
        format!(
            "Recurrence                    ◀ {} ▶",
            if modal.recurrence_kind == RecurrenceKind::Weekly {
                "weekly"
            } else {
                "one-time"
            }
        ),
    ];
    if modal.recurrence_kind == RecurrenceKind::Weekly {
        rows.push(
            day_spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>(),
        );
        rows.push(format!(
            "Start time                    ◀ {} ▶",
            modal.weekly_start.format("%H:%M")
        ));
        rows.push(format!(
            "End time                      ◀ {} ▶",
            modal.weekly_end.format("%H:%M")
        ));
    } else {
        rows.push(format!(
            "Start date                    ◀ {} ▶",
            modal.once_start_date.format("%Y-%m-%d")
        ));
        rows.push(format!(
            "Start time                    ◀ {} ▶",
            modal.once_start_time.format("%H:%M")
        ));
        rows.push(format!(
            "End date                      ◀ {} ▶",
            modal.once_end_date.format("%Y-%m-%d")
        ));
        rows.push(format!(
            "End time                      ◀ {} ▶",
            modal.once_end_time.format("%H:%M")
        ));
    }
    rows.push(format!(
        "Run missed on startup         ◀ {} ▶",
        if modal.run_missed_on_startup {
            "yes"
        } else {
            "no"
        }
    ));
    let (inner, offset) = mouse::fields(
        f,
        ctx,
        S::Scheduler,
        area,
        modal.scheduler_cursor,
        &rows,
        " j/k: field   h/l: adjust ",
    );
    for (index, _text) in rows
        .iter()
        .enumerate()
        .skip(offset)
        .take(inner.height as usize)
    {
        let row_area = Rect::new(inner.x, inner.y + (index - offset) as u16, inner.width, 1);
        if index == 0 || index == rows.len() - 1 {
            ctx.interactions.borrow_mut().add(
                row_area,
                mouse::modal_target(I::Adjust {
                    field: Some(index),
                    forward: true,
                }),
            );
        }
        if index == 2 && modal.recurrence_kind == RecurrenceKind::Weekly {
            f.render_widget(
                Paragraph::new(Line::from(day_spans.clone()))
                    .style(Style::default().bg(ratatui::style::Color::Reset)),
                row_area,
            );
            let targets = (0..7)
                .map(|i| (i + 1, mouse::modal_target(I::Day(i))))
                .collect::<Vec<_>>();
            mouse::spans(ctx.interactions, row_area, &day_spans, &targets);
        }
    }
}

fn draw_queue_modal_items_tab(f: &mut Frame, ctx: &Ctx<'_>, modal: &QueueModal, area: Rect) {
    let theme = ctx.theme;
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

    let rows = modal
        .items
        .iter()
        .enumerate()
        .map(|(i, d)| format!("{}. {}", i + 1, d.filename.as_deref().unwrap_or(&d.url)))
        .collect::<Vec<_>>();
    mouse::fields(
        f,
        ctx,
        S::Items,
        area,
        modal.item_cursor,
        &rows,
        " j/k: select   J/K: move item down/up ",
    );
}

fn draw_queue_modal_buttons(f: &mut Frame, ctx: &Ctx<'_>, modal: &QueueModal, area: Rect) {
    let theme = ctx.theme;
    let save_label = match modal.mode {
        QueueModalMode::Create => " [s] Create ",
        QueueModalMode::Edit { .. } => " [s] Save ",
    };

    let mut spans = vec![
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

    let mut targets = vec![(0, mouse::command(C::Save)), (2, mouse::command(C::Cancel))];
    if modal.tab == QueueModalTab::DownloadItems {
        spans.extend([
            Span::raw("  "),
            Span::styled(" [K] Move up ", Style::default().fg(theme.accent)),
            Span::raw("  "),
            Span::styled(" [J] Move down ", Style::default().fg(theme.accent)),
        ]);
        targets.extend([
            (4, mouse::command(C::ItemUp)),
            (6, mouse::command(C::ItemDown)),
        ]);
    }
    mouse::spans(ctx.interactions, area, &spans, &targets);
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}
