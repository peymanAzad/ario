use crate::app::{App, LifecycleState};
use crate::icons::GlyphMode;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Paragraph, RenderDirection, Sparkline},
};

const SPARKLINE_BARS: symbols::bar::Set = symbols::bar::Set {
    empty: "▁",
    ..symbols::bar::NINE_LEVELS
};

pub fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let show_speed_cluster = app.last_error.is_none() && has_download_activity(app);
    let show_sparkline = show_speed_cluster && app.icons.glyph_mode() != GlyphMode::Ascii;
    let speed_label_text = format!("{}/s ", super::format_bytes(app.displayed_download_speed()));
    let speed_label_width = speed_label_text.len() as u16 + u16::from(show_sparkline);

    let status = server_status(app);
    let server_color = match status {
        ServerStatus::Up => theme.status_ok,
        ServerStatus::Starting | ServerStatus::Retrying => theme.status_warning,
        ServerStatus::Down | ServerStatus::Failed => theme.status_error,
    };
    let (aria2_status, aria2_color) = if app.aria2_reachable {
        ("up", theme.status_ok)
    } else {
        ("down", theme.status_error)
    };

    let status_line = Line::from(vec![
        Span::styled(
            " Ario ",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" server:", Style::default().fg(theme.foreground)),
        Span::styled(status.label(), Style::default().fg(server_color)),
        Span::styled(" aria2:", Style::default().fg(theme.foreground)),
        Span::styled(aria2_status, Style::default().fg(aria2_color)),
        Span::raw(" "),
    ]);
    let status_width = status_line.width().min(u16::MAX as usize) as u16;

    if let Some(err) = &app.last_error {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(status_width), Constraint::Min(0)])
            .split(area);
        f.render_widget(Paragraph::new(status_line), chunks[0]);
        f.render_widget(
            Paragraph::new(format!("  {err}")).style(Style::default().fg(theme.status_error)),
            chunks[1],
        );
        return;
    }

    if !show_speed_cluster {
        f.render_widget(Paragraph::new(status_line), area);
        return;
    }

    let speed_label = Paragraph::new(speed_label_text)
        .style(Style::default().fg(theme.accent))
        .right_aligned();

    if show_sparkline {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(status_width),
                Constraint::Min(0),
                Constraint::Length(speed_label_width),
            ])
            .split(area);
        f.render_widget(Paragraph::new(status_line), chunks[0]);
        let sparkline = Sparkline::default()
            .data(sparkline_bars(
                app.speed.history.iter().copied(),
                chunks[1].width as usize,
            ))
            .max(app.speed_chart_max())
            .bar_set(SPARKLINE_BARS)
            .absent_value_symbol(" ")
            .direction(RenderDirection::LeftToRight)
            .style(theme.accent);
        f.render_widget(sparkline, chunks[1]);
        f.render_widget(speed_label, chunks[2]);
    } else {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(0), Constraint::Length(speed_label_width)])
            .split(area);
        f.render_widget(Paragraph::new(status_line), chunks[0]);
        f.render_widget(speed_label, chunks[1]);
    }
}

fn has_download_activity(app: &App) -> bool {
    app.server_reachable && app.active_downloads > 0
}

fn sparkline_bars(history: impl IntoIterator<Item = u64>, width: usize) -> Vec<Option<u64>> {
    let samples: Vec<u64> = history.into_iter().collect();
    let visible_start = samples.len().saturating_sub(width);
    let visible = &samples[visible_start..];
    let pad = width.saturating_sub(visible.len());
    std::iter::repeat_n(None, pad)
        .chain(visible.iter().copied().map(Some))
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ServerStatus {
    Starting,
    Retrying,
    Up,
    Down,
    Failed,
}

impl ServerStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Retrying => "retrying",
            Self::Up => "up",
            Self::Down => "down",
            Self::Failed => "failed",
        }
    }
}

fn server_status(app: &App) -> ServerStatus {
    if !app.manages_server() {
        return if app.server_reachable {
            ServerStatus::Up
        } else {
            ServerStatus::Down
        };
    }

    match &app.lifecycle {
        LifecycleState::Starting => ServerStatus::Starting,
        LifecycleState::Retrying => ServerStatus::Retrying,
        LifecycleState::Connected if app.server_reachable => ServerStatus::Up,
        LifecycleState::Connected => ServerStatus::Down,
        LifecycleState::Failed(_) => ServerStatus::Failed,
    }
}

#[cfg(test)]
#[path = "status_bar_tests.rs"]
mod tests;
