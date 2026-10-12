use super::*;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
};

use crate::app::Focus;
use common::enums::QueueStatus;

pub fn draw_queues_list(f: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let focused = app.focus == Focus::Queues;

    let mut items = vec![ListItem::new("All")];
    items.extend(app.queues.iter().map(|queue| {
        let mut spans = vec![Span::styled(
            queue.name.clone(),
            Style::default().fg(theme.foreground),
        )];
        if queue.status == QueueStatus::Active {
            spans.push(Span::styled(
                format!(" {}", app.icons.queue_running()),
                Style::default().fg(theme.status_ok),
            ));
        }
        if queue.scheduler.enabled {
            spans.push(Span::styled(
                format!(" {}", app.icons.scheduler()),
                Style::default().fg(theme.accent),
            ));
        }
        ListItem::new(Line::from(spans))
    }));

    let list = List::new(items)
        .style(Style::default().fg(theme.foreground))
        .highlight_style(highlight_style(theme, focused))
        .highlight_symbol(LIST_SELECTION_MARKER)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style(theme, focused))
                .title(Span::styled(
                    QUEUES_TITLE,
                    Style::default().fg(theme.foreground),
                )),
        );

    let inner = ratatui::widgets::Block::default()
        .borders(Borders::ALL)
        .inner(area);
    app.interactions
        .borrow_mut()
        .add(area, crate::mouse::Target::Focus(Focus::Queues));
    let offset = crate::mouse::list(
        f,
        &app.interactions,
        crate::mouse::ScrollTarget::Queues,
        list,
        (area, inner),
        app.selected_queue,
        app.queues.len() + 1,
    );
    for row in 0..inner.height {
        let index = offset + row as usize;
        if index > app.queues.len() {
            break;
        }
        let id = index
            .checked_sub(1)
            .and_then(|i| app.queues.get(i))
            .map(|q| q.id);
        app.interactions.borrow_mut().add(
            ratatui::layout::Rect::new(inner.x, inner.y + row, inner.width, 1),
            crate::mouse::Target::Row(Focus::Queues, index, id),
        );
    }
}
