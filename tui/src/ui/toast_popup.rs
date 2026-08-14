use crate::app::App;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

pub fn draw_toasts(f: &mut Frame, app: &App) {
    if app.toasts.is_empty() {
        return;
    }
    let theme = &app.theme;
    let screen = f.area();

    let toast_width = 42.min(screen.width.saturating_sub(4)).max(10);
    let toast_height = 3;
    let gap = 1;
    let mut y = 1u16;

    for toast in app.toasts.iter() {
        if y + toast_height > screen.height {
            break; // out of vertical room — silently drop rather than overflow
        }

        let x = screen.width.saturating_sub(toast_width + 1);
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
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(color))
            .title(Span::styled(
                format!(" {label} "),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));

        let paragraph = Paragraph::new(toast.message.as_str())
            .style(Style::default().fg(theme.foreground))
            .wrap(Wrap { trim: true })
            .block(block);

        f.render_widget(paragraph, area);
        y += toast_height + gap;
    }
}
