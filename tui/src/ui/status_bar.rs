use crate::app::{ALL_CATEGORIES, App, LifecycleState, SPEED_HISTORY_LEN};
use crate::icons::GlyphMode;
use common::enums::DownloadStatus;
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
    let show_speed_cluster = has_download_activity(app);
    let show_sparkline = show_speed_cluster && app.icons.glyph_mode() != GlyphMode::Ascii;
    let speed_label_text = format!("{}/s ", super::format_bytes(app.displayed_download_speed()));
    let speed_label_width = speed_label_text.len() as u16 + u16::from(show_sparkline);
    let chunks = if show_sparkline {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(0),
                Constraint::Length(SPEED_HISTORY_LEN as u16),
                Constraint::Length(speed_label_width),
            ])
            .split(area)
    } else if show_speed_cluster {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(0), Constraint::Length(speed_label_width)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(0)])
            .split(area)
    };

    let status = server_status(app);
    let server_background = match status {
        ServerStatus::Up => theme.status_ok,
        ServerStatus::Starting | ServerStatus::Retrying => theme.status_warning,
        ServerStatus::Down | ServerStatus::Failed => theme.status_error,
    };
    let server_indicator = Span::styled(
        format!(" server: {} ", status.label()),
        Style::default().fg(theme.selected_fg).bg(server_background),
    );

    let aria2_indicator = if app.aria2_reachable {
        Span::styled(
            " aria2: up ",
            Style::default().fg(theme.selected_fg).bg(theme.status_ok),
        )
    } else {
        Span::styled(
            " aria2: down ",
            Style::default().fg(theme.foreground).bg(theme.status_error),
        )
    };

    let mut spans = vec![
        Span::styled(
            " Ario ",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        server_indicator,
        aria2_indicator,
    ];

    if let Some(label) = filter_indicator_label(app) {
        spans.push(Span::styled(
            format!(" {label} "),
            Style::default().fg(theme.selected_fg).bg(theme.accent),
        ));
    }

    if let Some(err) = &app.last_error {
        spans.push(Span::styled(
            format!("  {err}"),
            Style::default().fg(theme.status_error),
        ));
    }

    if let Some(DownloadStatus::Error(message)) = app
        .downloads
        .get(app.selected_download)
        .map(|download| &download.download.status)
    {
        spans.push(Span::styled(
            format!("  Download: {message}"),
            Style::default().fg(theme.status_error),
        ));
    }

    f.render_widget(Paragraph::new(Line::from(spans)), chunks[0]);

    if !show_speed_cluster {
        return;
    }

    let speed_label = Paragraph::new(speed_label_text)
        .style(Style::default().fg(theme.accent))
        .right_aligned();

    if show_sparkline {
        let sparkline = Sparkline::default()
            .data(sparkline_bars(app.speed_history.iter().copied()))
            .max(app.speed_chart_max())
            .bar_set(SPARKLINE_BARS)
            .absent_value_symbol(" ")
            .direction(RenderDirection::LeftToRight)
            .style(theme.accent);
        f.render_widget(sparkline, chunks[1]);
        f.render_widget(speed_label, chunks[2]);
    } else {
        f.render_widget(speed_label, chunks[1]);
    }
}

fn has_download_activity(app: &App) -> bool {
    app.active_downloads > 0
}

fn sparkline_bars(history: impl IntoIterator<Item = u64>) -> Vec<Option<u64>> {
    let samples: Vec<u64> = history.into_iter().collect();
    let pad = SPEED_HISTORY_LEN.saturating_sub(samples.len());
    std::iter::repeat_n(None, pad)
        .chain(samples.into_iter().map(Some))
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

fn filter_indicator_label(app: &App) -> Option<String> {
    let queue_part = if app.selected_queue != 0 {
        app.queues
            .get(app.selected_queue - 1)
            .map(|q| q.name.clone())
    } else {
        None
    };

    let category_part = if app.selected_category != 0 {
        ALL_CATEGORIES
            .get(app.selected_category - 1)
            .map(super::category_label)
    } else {
        None
    };

    match (queue_part, category_part) {
        (None, None) => None,
        (Some(q), None) => Some(format!("Filter: {q}")),
        (None, Some(c)) => Some(format!("Filter: {c}")),
        (Some(q), Some(c)) => Some(format!("Filter: {q} · {c}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        icons::{GlyphMode, IconSet},
        theme::Theme,
    };
    use ratatui::{Terminal, backend::TestBackend};
    use std::sync::mpsc;

    fn app(managed: bool) -> App {
        app_with_glyphs(managed, GlyphMode::Ascii)
    }

    fn app_with_glyphs(managed: bool, mode: GlyphMode) -> App {
        let (sender, _receiver) = mpsc::channel();
        App::new(
            "http://127.0.0.1:1".into(),
            Theme::default_dark(),
            IconSet::new(mode),
            sender,
            managed,
        )
    }

    fn rendered_status_bar(app: &App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(80, 1)).unwrap();
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
        assert!(rendered.contains("server: up"));
        assert!(rendered.contains("aria2: down"));
        assert!(!rendered.contains("connected"));
        assert!(!rendered.contains("0.0 B/s"));
        assert!(!rendered.contains("▁"));
        assert!(!rendered.contains("█"));
    }

    #[test]
    fn idle_status_bar_hides_speed_cluster_so_errors_use_the_full_width() {
        let mut app = app_with_glyphs(true, GlyphMode::Unicode);
        app.apply_lifecycle(LifecycleState::Connected);
        app.last_error = Some("can't reach server: connection refused".into());
        app.speed_history.extend([1024, 512, 0]);
        let rendered = rendered_status_bar(&app);
        assert!(rendered.contains("can't reach server: connection refused"));
        assert!(!rendered.contains("0.0 B/s"));
        assert!(!rendered.contains("▁"));
        assert!(!rendered.contains("█"));
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
        let bars = sparkline_bars([1, 2]);
        assert_eq!(bars.len(), SPEED_HISTORY_LEN);
        assert!(bars.iter().take(SPEED_HISTORY_LEN - 2).all(Option::is_none));
        assert_eq!(bars[SPEED_HISTORY_LEN - 2], Some(1));
        assert_eq!(bars[SPEED_HISTORY_LEN - 1], Some(2));
    }

    #[test]
    fn unsampled_history_is_blank_while_zero_samples_draw_a_baseline() {
        let mut app = app_with_glyphs(true, GlyphMode::Unicode);
        app.total_download_speed = 100;
        app.active_downloads = 1;
        app.speed_history.push_back(0);
        let rendered = rendered_status_bar(&app);
        let graph_start = rendered.find('▁').unwrap();
        assert!(rendered[..graph_start].ends_with("              "));
    }
}
