use super::*;
use crate::app::{ALL_CATEGORIES, App, Focus};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::Span,
    widgets::{Block, Borders, List, ListState},
};

pub fn draw_categories_list(f: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let focused = app.focus == Focus::Categories;

    let mut items: Vec<String> = ALL_CATEGORIES.iter().map(category_label).collect();
    items.insert(0, "All".to_string());

    let mut state = ListState::default();
    state.select(Some(app.selected_category));

    let list = List::new(items)
        .style(Style::default().fg(theme.foreground))
        .highlight_style(highlight_style(theme, focused))
        .highlight_symbol("> ")
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style(theme, focused))
                .title(Span::styled(
                    " [2] Categories ",
                    Style::default().fg(theme.foreground),
                )),
        );

    f.render_stateful_widget(list, area, &mut state);
}
