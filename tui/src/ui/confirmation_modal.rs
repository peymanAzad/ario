use super::*;
use crate::app::{App, confirmation_modal::ConfirmationModal};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

pub fn draw_confirmation_modal(f: &mut Frame, app: &App, modal: &ConfirmationModal) {
    let theme = &app.theme;
    let area = centered_rect(58, 28, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.status_error))
        .title(Span::styled(
            format!(" {} ", modal.title),
            Style::default()
                .fg(theme.status_error)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    f.render_widget(
        Paragraph::new(modal.message.as_str())
            .style(Style::default().fg(theme.foreground))
            .wrap(Wrap { trim: true }),
        layout[0],
    );
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" Enter/y: {} ", modal.confirm_label),
                Style::default()
                    .fg(theme.status_error)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                format!("Esc/n/c: {}", modal.cancel_label),
                Style::default().fg(theme.text_muted),
            ),
        ])),
        layout[1],
    );
}
