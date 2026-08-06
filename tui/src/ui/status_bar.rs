use crate::app::{ALL_CATEGORIES, App};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

pub fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
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
            .map(super::category_label)
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
