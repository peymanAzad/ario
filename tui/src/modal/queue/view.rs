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
        QueueModalTab::Events => draw_queue_modal_events_tab(f, ctx, modal, layout[1]),
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

    let tabs = QueueModalTab::available(modal.mode);
    let items_label = format!(" Download Items ({}) ", modal.items.len());
    let compact_items_label = format!(" Items ({}) ", modal.items.len());
    let mut labels = [" Common ", " Scheduler ", " Events ", &items_label];
    let full_width = labels[..tabs.len()]
        .iter()
        .map(|label| label.len())
        .sum::<usize>()
        + (tabs.len() - 1) * 2;
    let compact = full_width > usize::from(area.width);
    if compact {
        labels[3] = &compact_items_label;
    }

    let mut spans = Vec::new();
    let mut targets = Vec::new();
    for (index, (&tab, label)) in tabs.iter().zip(labels).enumerate() {
        if index > 0 {
            spans.push(Span::raw(if compact { " " } else { "  " }));
        }
        targets.push((spans.len(), mouse::modal_target(I::Tab(index))));
        spans.push(Span::styled(label, tab_style(modal.tab == tab)));
    }
    spans.push(Span::raw("   (Tab to switch)"));
    mouse::spans(ctx.interactions, area, &spans, &targets);
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn text_value(modal: &QueueModal, selected: bool, value: &str, width: usize) -> String {
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;
    let editing = modal.text_input.editing && selected;
    let value = if editing {
        modal.text_input.buffer.as_str()
    } else {
        value
    };
    let value = if value.is_empty() && !editing {
        "(disabled)"
    } else {
        value
    };
    let mut displayed = if editing {
        format!("{value}▏")
    } else {
        value.to_string()
    };
    if displayed.width() > width {
        let mut used = 1;
        let tail = displayed
            .graphemes(true)
            .rev()
            .take_while(|g| {
                used += g.width();
                used <= width
            })
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<String>();
        displayed = format!("…{tail}");
    }
    displayed
}

fn event_result(result: Option<&common::queue::QueueEventResult>) -> String {
    use common::queue::QueueEventStatus as Status;
    let Some(result) = result else {
        return "never run".into();
    };
    let status = match result.status {
        Status::Queued => "queued",
        Status::Running => "running",
        Status::Succeeded => "succeeded",
        Status::Failed => "failed",
        Status::Interrupted => "interrupted",
    };
    let timestamp = result
        .finished_at
        .or(result.started_at)
        .unwrap_or(result.queued_at)
        .with_timezone(&chrono::Local)
        .format("%m-%d %H:%M")
        .to_string();
    let detail = result
        .message
        .as_deref()
        .map(|message| format!(" — {message}"))
        .unwrap_or_default();
    format!("{status} ({timestamp}){detail}")
}

fn field_row(label: &str, value: String, label_width: usize) -> String {
    let label: String = label.chars().take(label_width).collect();
    format!("{label:<label_width$} {value}")
}

fn draw_queue_modal_common_tab(f: &mut Frame, ctx: &Ctx<'_>, modal: &QueueModal, area: Rect) {
    let inner_width = usize::from(area.width.saturating_sub(2));
    let label_width = 28.min(inner_width / 2);
    let value_width = inner_width.saturating_sub(label_width + 1).max(1);
    let row = |label: &str, value| field_row(label, value, label_width);
    let mut rows = vec![
        row(
            "Name",
            text_value(modal, modal.common_cursor == 0, &modal.name, value_width),
        ),
        row(
            "Max concurrent downloads",
            format!("◀ {} ▶", modal.max_concurrent_downloads),
        ),
        row("Max retries", format!("◀ {} ▶", modal.max_retries)),
        row(
            "Retry wait (seconds)",
            format!("◀ {} ▶", modal.retry_wait_seconds),
        ),
    ];
    for (label, value) in modal
        .finetune_editor
        .rows(ctx.aria2_global_options)
        .into_iter()
        .take(4)
    {
        rows.push(row(&label, format!("◀ {value} ▶")));
    }
    let (inner, offset) = mouse::fields(
        f,
        ctx,
        S::Common,
        area,
        modal.common_cursor,
        &rows,
        " j/k: field   h/l: adjust   Enter: edit text ",
    );
    if offset == 0 && inner.height > 0 {
        ctx.interactions.borrow_mut().add(
            Rect::new(inner.x, inner.y, inner.width, 1),
            mouse::modal_target(I::Text),
        );
    }
}

fn draw_queue_modal_events_tab(f: &mut Frame, ctx: &Ctx<'_>, modal: &QueueModal, area: Rect) {
    use ratatui::widgets::{List, ListItem};
    let theme = ctx.theme;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(" j/k: field   Enter: edit command ");
    let inner = block.inner(area);
    let label_width = "On automatic stop".len().min(usize::from(inner.width) / 2);
    let value_width = usize::from(inner.width)
        .saturating_sub(label_width + 1)
        .max(1);
    let command_row = |index, label, command| {
        field_row(
            label,
            text_value(modal, modal.events.cursor == index, command, value_width),
            label_width,
        )
    };
    let bullet = if ctx.icons.glyph_mode() == crate::icons::GlyphMode::Ascii {
        "-"
    } else {
        "•"
    };
    let note = |text| format!("{bullet} {text}");
    let rows = [
        (None, "Commands".into()),
        (
            Some(0),
            command_row(0, "On success", &modal.events.on_success_command),
        ),
        (
            Some(1),
            command_row(1, "On automatic stop", &modal.events.on_stop_command),
        ),
        (None, String::new()),
        (None, "Latest results".into()),
        (
            Some(2),
            format!(
                "Success: {}",
                event_result(modal.events.results.on_success.as_ref())
            ),
        ),
        (
            Some(3),
            format!(
                "Stop: {}",
                event_result(modal.events.results.on_stop.as_ref())
            ),
        ),
        (None, String::new()),
        (None, "Notes".into()),
        (Some(4), note("Commands run on the server machine.")),
        (Some(5), note("Commands cannot prompt for input.")),
        (Some(6), note("Success runs both commands in order.")),
        (Some(7), note("Manual queue stops skip both commands.")),
    ];
    let selected = rows
        .iter()
        .position(|(field, _)| *field == Some(modal.events.cursor))
        .unwrap_or_default();
    let items = rows.iter().map(|(field, text)| {
        let style = match field {
            None => Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
            Some(index) if *index == modal.events.cursor || *index < 4 => {
                crate::ui::field_style(theme, *index == modal.events.cursor)
            }
            Some(_) => Style::default().fg(theme.text_muted),
        };
        ListItem::new(text.as_str()).style(style)
    });
    let offset = mouse::list(
        f,
        ctx.interactions,
        S::Events,
        List::new(items).block(block),
        (area, inner),
        selected,
        rows.len(),
    );
    for y in 0..inner.height {
        if let Some((Some(field @ 0..=1), _)) = rows.get(offset + usize::from(y)) {
            ctx.interactions.borrow_mut().add(
                Rect::new(inner.x, inner.y + y, inner.width, 1),
                mouse::modal_target(I::TextField(*field)),
            );
        }
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
