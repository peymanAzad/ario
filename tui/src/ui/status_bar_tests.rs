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
    app.speed.history.extend([1024, 512, 0]);
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
    app.speed.history.extend([1, 2, 4, 8, 16, 12, 10, 14]);
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
    app.speed.history.extend([100, 0]);
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
    app.speed.history.extend(1..=150);

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
    app.speed.history.push_back(0);
    let rendered = rendered_status_bar(&app);
    let graph_start = rendered.find('▁').unwrap();
    assert!(rendered[..graph_start].ends_with("              "));
}
