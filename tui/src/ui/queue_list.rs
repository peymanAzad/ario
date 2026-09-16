use super::*;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::app::Focus;

pub fn draw_queues_list(f: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let focused = app.focus == Focus::Queues;

    let mut items = vec![ListItem::new("All")];
    items.extend(app.queues.iter().map(|queue| {
        let mut spans = vec![Span::styled(
            queue.name.clone(),
            Style::default().fg(theme.foreground),
        )];
        spans.push(Span::styled(
            format!(" {}", app.icons.queue_status(&queue.status)),
            Style::default().fg(app.icons.queue_status_color(&queue.status, theme)),
        ));
        if queue.scheduler.enabled {
            spans.push(Span::styled(
                format!(" {}", app.icons.scheduler()),
                Style::default().fg(theme.accent),
            ));
        }
        ListItem::new(Line::from(spans))
    }));

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
