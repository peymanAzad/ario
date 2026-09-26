use crate::app::App;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

pub fn draw_toasts(f: &mut Frame, app: &App) {
    if app.toasts.is_empty() {
        return;
    }
    let theme = &app.theme;
    let screen = f.area();

    let toast_width = 42.min(screen.width.saturating_sub(2));
    if toast_width < 3 || screen.height < 4 {
        return;
    }
    let gap = 1;
    let mut y = screen.y.saturating_add(1);

    for toast in app.toasts.iter() {
        let mut lines: Vec<Line<'static>> = toast
            .message
            .lines()
            .flat_map(|line| {
                let wrapped =
                    super::help_modal::wrap_text(line, toast_width.saturating_sub(2) as usize);
                if wrapped.is_empty() {
                    vec![Line::default()]
                } else {
                    wrapped.into_iter().map(Line::from).collect()
                }
            })
            .collect();
        if lines.is_empty() {
            lines.push(Line::default());
        }
        let available = screen.bottom().saturating_sub(y);
        let toast_height =
            (lines.len().saturating_add(2).min(u16::MAX as usize) as u16).min(available);
        if toast_height < 3 {
            break;
        }

        let x = screen.right().saturating_sub(toast_width + 1);
        let area = Rect {
            x,
            y,
            width: toast_width,
            height: toast_height,
        };
        f.render_widget(Clear, area);

        let (color, label) = match toast.level {
            crate::toast::ToastLevel::Error => (theme.status_error, "Error"),
            crate::toast::ToastLevel::Success => (theme.status_ok, "Success"),
            crate::toast::ToastLevel::Info => (theme.accent, "Info"),
            crate::toast::ToastLevel::Warning => (theme.status_warning, "Warning"),
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(color))
            .title(Span::styled(
                format!(" {label} "),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));

        let paragraph = Paragraph::new(lines)
            .style(Style::default().fg(theme.foreground))
            .block(block);

        f.render_widget(paragraph, area);
        y = y.saturating_add(toast_height + gap);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        icons::{GlyphMode, IconSet},
        theme::Theme,
        toast::ToastLevel,
    };
    use ratatui::{Terminal, backend::TestBackend};
    use std::sync::mpsc;

    fn render(width: u16, height: u16, messages: &[&str]) -> Vec<String> {
        let (sender, _receiver) = mpsc::channel();
        let mut app = App::new(
            "http://127.0.0.1:1".into(),
            Theme::default_dark(),
            IconSet::new(GlyphMode::Ascii),
            sender,
            false,
        );
        for message in messages {
            app.toasts.push(*message, ToastLevel::Success);
        }
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw_toasts(frame, &app)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn scheduled_stop_date_and_time_are_both_visible() {
        let lines = render(
            80,
            20,
            &["Queue started. Will pause at 2030-01-07 17:30 +04"],
        );
        let rendered = lines.join("\n");
        assert!(rendered.contains("2030-01-07"));
        assert!(rendered.contains("17:30 +04"));
    }

    #[test]
    fn wrapped_toasts_stack_without_hiding_each_other() {
        let lines = render(
            80,
            20,
            &[
                "This queue’s schedule has ended. Disable the scheduler or update its end time before starting.",
                "Second toast",
            ],
        );
        let first_end = lines
            .iter()
            .position(|line| line.contains("before starting."))
            .unwrap();
        let second = lines
            .iter()
            .position(|line| line.contains("Second toast"))
            .unwrap();
        assert!(second > first_end + 1);
    }

    #[test]
    fn narrow_and_tiny_screens_render_without_overflow() {
        for (width, height) in [(1, 1), (2, 4), (5, 6), (20, 12)] {
            let lines = render(
                width,
                height,
                &[
                    "Queue started. Will pause at 2030-01-07 17:30 +04",
                    "Another message",
                ],
            );
            assert_eq!(lines.len(), height as usize);
        }
        assert!(
            render(20, 12, &["Stop at 17:30"])
                .join("\n")
                .contains("17:30")
        );
        assert!(
            render(20, 4, &["Stop at 17:30"])
                .join("\n")
                .contains("17:30")
        );
    }
}
