use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::keymap::{self, FooterHint};

const HINT_SEPARATOR: &str = "   ";

pub fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let hints = footer_hints(app);
    let accent = Style::default().fg(app.theme.accent);
    let muted = Style::default().fg(app.theme.text_muted);
    let available = area.width as usize;
    let mut used = 0;
    let mut spans = Vec::new();

    for (index, hint) in hints.into_iter().enumerate() {
        let separator_width = usize::from(index > 0) * HINT_SEPARATOR.width();
        let required = separator_width + hint.width();
        if index > 0 && used + required > available {
            break;
        }
        if index > 0 {
            spans.push(Span::styled(HINT_SEPARATOR, muted));
        }
        spans.push(Span::styled(hint.key, accent));
        spans.push(Span::styled(":", muted));
        spans.push(Span::styled(hint.label, muted));
        used += required;
    }

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn footer_hints(app: &App) -> Vec<FooterHint> {
    if let Some(modal) = &app.modal {
        return modal
            .hints()
            .into_iter()
            .map(|(key, label)| FooterHint::new(key, label))
            .collect();
    }
    keymap::footer_hints(app)
}

#[cfg(test)]
#[path = "footer_tests.rs"]
mod tests;
