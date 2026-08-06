use super::*;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::Span,
    widgets::{Block, Borders, List, ListState},
};

use crate::app::Focus;

pub fn draw_queues_list(f: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let focused = app.focus == Focus::Queues;

    let mut items: Vec<String> = app.queues.iter().map(|q| q.name.clone()).collect();
    items.insert(0, "All".to_string());

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
