use crate::mouse::{self, Command as C, ScrollTarget as S};
use crate::{
    modal::{Ctx, download_edit::DownloadEditModal},
    ui::centered_rect,
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

pub fn draw_download_modal(f: &mut Frame, modal: &DownloadEditModal, ctx: &Ctx<'_>) {
    let theme = ctx.theme;
    let area = centered_rect(70, 60, f.area());
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

    let queue_label = ctx
        .queues
        .get(modal.queue_picker.cursor)
        .map(|queue| queue.name.clone())
        .unwrap_or_else(|| "(unavailable)".into());

    let mut rows = modal.finetune_editor.rows(None);
    rows.push(("Queue".into(), queue_label));

    let selected = modal.selected_row();
    let rows = rows
        .into_iter()
        .map(|(label, value)| format!("{label:<28} ◀ {value} ▶"))
        .collect::<Vec<_>>();
    mouse::fields(
        f,
        ctx,
        S::Fields,
        layout[0],
        selected,
        &rows,
        " j/k: field   h/l: adjust ",
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
    mouse::spans(
        ctx.interactions,
        layout[2],
        &spans,
        &[(0, mouse::command(C::Save)), (2, mouse::command(C::Cancel))],
    );
    f.render_widget(Paragraph::new(Line::from(spans)), layout[2]);
}
