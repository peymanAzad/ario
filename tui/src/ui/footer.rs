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
    let hints = clickable_hints(app);
    let accent = Style::default().fg(app.theme.accent);
    let muted = Style::default().fg(app.theme.text_muted);
    let available = area.width as usize;
    let mut used = 0;
    let mut spans = Vec::new();

    for (index, (hint, target)) in hints.into_iter().enumerate() {
        let separator_width = usize::from(index > 0) * HINT_SEPARATOR.width();
        let required = separator_width + hint.width();
        if index > 0 && used + required > available {
            break;
        }
        if index > 0 {
            spans.push(Span::styled(HINT_SEPARATOR, muted));
        }
        if let Some(target) = target {
            let x = used + separator_width;
            app.interactions.borrow_mut().add(
                Rect::new(
                    area.x.saturating_add(x as u16),
                    area.y,
                    (hint.width() as u16).min(area.width.saturating_sub(x as u16)),
                    area.height.min(1),
                ),
                target,
            );
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

fn clickable_hints(app: &App) -> Vec<(FooterHint, Option<crate::mouse::Target>)> {
    use crate::mouse::{self, Target};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut result = Vec::new();
    for hint in footer_hints(app) {
        let split = match hint.key {
            "d/D" => vec![
                FooterHint::new("d", "Delete"),
                FooterHint::new("D", "Delete+files"),
            ],
            "j/k" => vec![FooterHint::new("j", "Down"), FooterHint::new("k", "Up")],
            "h/l" => vec![
                FooterHint::new("h", "Decrease"),
                FooterHint::new("l", "Increase"),
            ],
            _ => vec![hint],
        };
        for hint in split {
            let target = if let Some(modal) = &app.modal {
                mouse::modal_hint(hint.key, modal).map(Target::Modal)
            } else {
                let code = match hint.key {
                    "Enter" => Some(KeyCode::Enter),
                    "Tab" => Some(KeyCode::Tab),
                    key if key.chars().count() == 1 => key.chars().next().map(KeyCode::Char),
                    _ => None,
                };
                code.and_then(|code| {
                    keymap::lookup(app.focus, KeyEvent::new(code, KeyModifiers::NONE))
                })
                .filter(|action| app.action_available(action))
                .map(Target::Action)
            };
            result.push((hint, target));
        }
    }
    result
}

#[cfg(test)]
#[path = "footer_tests.rs"]
mod tests;
