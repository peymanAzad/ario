use super::*;
use crate::app::{ALL_CATEGORIES, App, Focus};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::Span,
    widgets::{Block, Borders, List},
};

pub fn draw_categories_list(f: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let focused = app.focus == Focus::Categories;

    let mut items: Vec<String> = ALL_CATEGORIES
        .iter()
        .map(|category| {
            format!(
                "{} {}",
                app.icons.category(category),
                category_label(category)
            )
        })
        .collect();
    items.insert(0, format!("{} All", app.icons.all()));

    let list = List::new(items)
        .style(Style::default().fg(theme.foreground))
        .highlight_style(highlight_style(theme, focused))
        .highlight_symbol(LIST_SELECTION_MARKER)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style(theme, focused))
                .title(Span::styled(
                    CATEGORIES_TITLE,
                    Style::default().fg(theme.foreground),
                )),
        );

    let inner = ratatui::widgets::Block::default()
        .borders(Borders::ALL)
        .inner(area);
    app.interactions
        .borrow_mut()
        .add(area, crate::mouse::Target::Focus(Focus::Categories));
    let offset = crate::mouse::list(
        f,
        &app.interactions,
        crate::mouse::ScrollTarget::Categories,
        list,
        (area, inner),
        app.selected_category,
        ALL_CATEGORIES.len() + 1,
    );
    for row in 0..inner.height {
        let index = offset + row as usize;
        if index > ALL_CATEGORIES.len() {
            break;
        }
        let id = None;
        app.interactions.borrow_mut().add(
            ratatui::layout::Rect::new(inner.x, inner.y + row, inner.width, 1),
            crate::mouse::Target::Row(Focus::Categories, index, id),
        );
    }
}
