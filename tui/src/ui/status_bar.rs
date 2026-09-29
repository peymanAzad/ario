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
                app.speed_history.iter().copied(),
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
mod tests {
    use super::*;
    use crate::{
        icons::{GlyphMode, IconSet},
        theme::Theme,
    };
    use ratatui::{Terminal, backend::TestBackend, style::Color};

    fn app(managed: bool) -> App {
        app_with_glyphs(managed, GlyphMode::Ascii)
    }

    fn app_with_glyphs(managed: bool, mode: GlyphMode) -> App {
        App::new(Theme::default_dark(), IconSet::new(mode), managed)
    }

    fn rendered_status_bar(app: &App) -> String {
        rendered_status_bar_at_width(app, 80)
    }

    fn rendered_status_bar_at_width(app: &App, width: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
        terminal
            .draw(|frame| draw_status_bar(frame, app, frame.area()))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn managed_server_indicator_follows_lifecycle_and_current_health() {
        let mut app = app(true);
        assert_eq!(server_status(&app), ServerStatus::Starting);
        app.apply_lifecycle(LifecycleState::Retrying);
        assert_eq!(server_status(&app), ServerStatus::Retrying);
        app.apply_lifecycle(LifecycleState::Failed("startup failed".into()));
        assert_eq!(server_status(&app), ServerStatus::Failed);
        app.apply_lifecycle(LifecycleState::Connected);
        assert_eq!(server_status(&app), ServerStatus::Up);
        assert!(app.server_reachable);
        assert!(!app.aria2_reachable);
        app.apply_refresh(
            Err(anyhow::anyhow!("server unreachable")),
            Err(anyhow::anyhow!("server unreachable")),
            false,
            false,
            0,
            0,
            None,
            3, // Retrying, Failed, then Connected.
        );
        assert_eq!(server_status(&app), ServerStatus::Down);
    }

    #[test]
    fn unmanaged_server_uses_reachability_and_aria2_stays_separate() {
        let mut unmanaged = app(false);
        assert_eq!(server_status(&unmanaged), ServerStatus::Down);
        unmanaged.server_reachable = true;
        assert_eq!(server_status(&unmanaged), ServerStatus::Up);

        let mut managed = app(true);
        managed.apply_lifecycle(LifecycleState::Connected);
        let rendered = rendered_status_bar(&managed);
        assert!(rendered.contains("server:up"));
        assert!(rendered.contains("aria2:down"));
        assert!(!rendered.contains("connected"));
        assert!(!rendered.contains("0.0 B/s"));
        assert!(!rendered.contains("▁"));
        assert!(!rendered.contains("█"));
    }

    #[test]
    fn status_values_use_semantic_foregrounds_without_backgrounds() {
        let mut app = app(true);
        app.apply_lifecycle(LifecycleState::Connected);

        let mut terminal = Terminal::new(TestBackend::new(80, 1)).unwrap();
        terminal
            .draw(|frame| draw_status_bar(frame, &app, frame.area()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rendered: String = buffer.content().iter().map(|cell| cell.symbol()).collect();
        let server_status_start = rendered.find("server:up").unwrap() + "server:".len();
        let aria2_status_start = rendered.find("aria2:down").unwrap() + "aria2:".len();

        for x in server_status_start..server_status_start + "up".len() {
            assert_eq!(buffer[(x as u16, 0)].fg, app.theme.status_ok);
            assert_eq!(buffer[(x as u16, 0)].bg, Color::Reset);
        }
        for x in aria2_status_start..aria2_status_start + "down".len() {
            assert_eq!(buffer[(x as u16, 0)].fg, app.theme.status_error);
            assert_eq!(buffer[(x as u16, 0)].bg, Color::Reset);
        }
    }

    #[test]
    fn application_error_takes_priority_over_active_speed_cluster() {
        let mut app = app_with_glyphs(true, GlyphMode::Unicode);
        app.apply_lifecycle(LifecycleState::Connected);
        app.last_error = Some("can't reach server: connection refused".into());
        app.total_download_speed = 1024;
        app.active_downloads = 1;
        app.speed_history.extend([1024, 512, 0]);
        let rendered = rendered_status_bar(&app);
        assert!(rendered.contains("can't reach server: connection refused"));
        assert!(!rendered.contains("1.0 KB/s"));
        assert!(!rendered.contains("▁"));
        assert!(!rendered.contains("█"));
    }

    #[test]
    fn unreachable_lifecycle_hides_stale_speed_state() {
        let mut app = app_with_glyphs(true, GlyphMode::Unicode);
        app.apply_lifecycle(LifecycleState::Connected);
        app.total_download_speed = 1024;
        app.active_downloads = 1;
        app.apply_lifecycle(LifecycleState::Retrying);

        let rendered = rendered_status_bar(&app);
        assert!(rendered.contains("server:retrying"));
        assert!(!rendered.contains("1.0 KB/s"));
        assert!(!rendered.contains("▁"));
    }

    #[test]
    fn status_bar_does_not_repeat_sidebar_filters() {
        let mut app = app(true);
        app.selected_category = 1;
        assert!(!rendered_status_bar(&app).contains("Filter:"));
    }

    #[test]
    fn unicode_status_bar_shows_sparkline_and_total_speed() {
        let mut app = app_with_glyphs(true, GlyphMode::Unicode);
        app.apply_lifecycle(LifecycleState::Connected);
        app.total_download_speed = 1024;
        app.active_downloads = 1;
        app.speed_history.extend([1, 2, 4, 8, 16, 12, 10, 14]);
        let rendered = rendered_status_bar(&app);
        assert!(rendered.contains("1.0 KB/s"));
        let label_start = rendered.find("1.0 KB/s").unwrap();
        let preceding: Vec<char> = rendered[..label_start].chars().rev().take(2).collect();
        assert_eq!(preceding[0], ' ');
        assert_ne!(preceding[1], ' ');
        assert!(rendered.ends_with("1.0 KB/s "));
        assert!(
            rendered.contains("▁")
                || rendered.contains("▂")
                || rendered.contains("▃")
                || rendered.contains("▄")
                || rendered.contains("▅")
                || rendered.contains("▆")
                || rendered.contains("▇")
                || rendered.contains("█")
        );
    }

    #[test]
    fn zero_speed_samples_draw_a_baseline() {
        let mut app = app_with_glyphs(true, GlyphMode::Unicode);
        app.apply_lifecycle(LifecycleState::Connected);
        app.active_downloads = 1;
        app.speed_history.extend([100, 0]);
        let rendered = rendered_status_bar(&app);
        assert!(rendered.contains("▁"));
    }

    #[test]
    fn sparkline_bars_pad_on_the_left_so_newest_sits_on_the_right() {
        let bars = sparkline_bars([1, 2], 5);
        assert_eq!(bars, [None, None, None, Some(1), Some(2)]);
    }

    #[test]
    fn sparkline_bars_keep_only_the_newest_samples_that_fit() {
        assert_eq!(
            sparkline_bars([1, 2, 3, 4, 5], 3),
            [Some(3), Some(4), Some(5)]
        );
        assert!(sparkline_bars([1, 2, 3], 0).is_empty());
    }

    #[test]
    fn sparkline_expands_and_contracts_with_available_width() {
        let mut app = app_with_glyphs(true, GlyphMode::Unicode);
        app.apply_lifecycle(LifecycleState::Connected);
        app.active_downloads = 1;
        app.speed_history.extend(1..=150);

        let narrow = rendered_status_bar_at_width(&app, 60);
        let wide = rendered_status_bar_at_width(&app, 100);
        let narrow_bars = narrow.chars().filter(|c| "▁▂▃▄▅▆▇█".contains(*c)).count();
        let wide_bars = wide.chars().filter(|c| "▁▂▃▄▅▆▇█".contains(*c)).count();
        assert!(wide_bars > narrow_bars);
    }

    #[test]
    fn unsampled_history_is_blank_while_zero_samples_draw_a_baseline() {
        let mut app = app_with_glyphs(true, GlyphMode::Unicode);
        app.apply_lifecycle(LifecycleState::Connected);
        app.total_download_speed = 100;
        app.active_downloads = 1;
        app.speed_history.push_back(0);
        let rendered = rendered_status_bar(&app);
        let graph_start = rendered.find('▁').unwrap();
        assert!(rendered[..graph_start].ends_with("              "));
    }
}
