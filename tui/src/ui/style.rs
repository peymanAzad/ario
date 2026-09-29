use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
};

pub(crate) fn border_style(theme: &crate::theme::Theme, focused: bool) -> Style {
    if focused {
        Style::default().fg(theme.border_focused)
    } else {
        Style::default().fg(theme.border)
    }
}

pub(crate) fn highlight_style(theme: &crate::theme::Theme, focused: bool) -> Style {
    if focused {
        Style::default()
            .bg(theme.selected_bg)
            .fg(theme.selected_fg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.accent)
    }
}

pub(crate) fn field_style(theme: &crate::theme::Theme, active: bool) -> Style {
    if active {
        Style::default()
            .bg(theme.selected_bg)
            .fg(theme.selected_fg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.foreground)
    }
}

pub(crate) fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}
