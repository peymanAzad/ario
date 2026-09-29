use super::*;
use crate::app::{App, LifecycleState};
use crate::icons::{GlyphMode, IconSet};
use crate::theme::Theme;
use common::enums::{AllocStrategy, StreamPieceSelector};
use common::finetune::Aria2GlobalOptions;
use std::time::Instant;

fn app() -> App {
    App::new(Theme::default_dark(), IconSet::new(GlyphMode::Unicode), false)
}

#[test]
fn speed_history_resets_on_connection_loss_and_caps_retained_samples() {
    let mut app = app();
    app.apply_refresh(Ok(vec![]), Ok(vec![]), true, true, 100, 1, None, 0);
    assert_eq!(app.total_download_speed, 100);
    assert_eq!(
        app.speed.history.iter().copied().collect::<Vec<_>>(),
        [100]
    );

    app.apply_refresh(Ok(vec![]), Ok(vec![]), false, false, 50, 0, None, 0);
    assert_eq!(app.total_download_speed, 0);
    assert!(app.speed.history.is_empty());
    assert!(app.speed.last_sample_at.is_none());
    assert_eq!(app.speed_chart_max(), 1);

    for speed in 1..=MAX_SPEED_HISTORY_SAMPLES as u64 {
        app.push_speed_sample(speed);
    }
    assert_eq!(app.speed.history.len(), MAX_SPEED_HISTORY_SAMPLES);
    assert_eq!(app.speed.history.front(), Some(&1));
    assert_eq!(
        app.speed.history.back(),
        Some(&(MAX_SPEED_HISTORY_SAMPLES as u64))
    );
    app.push_speed_sample(MAX_SPEED_HISTORY_SAMPLES as u64 + 1);
    assert_eq!(app.speed.history.len(), MAX_SPEED_HISTORY_SAMPLES);
    assert_eq!(app.speed.history.front(), Some(&2));
    assert_eq!(
        app.speed.history.back(),
        Some(&(MAX_SPEED_HISTORY_SAMPLES as u64 + 1))
    );
}

#[test]
fn reachable_refresh_throttles_speed_samples() {
    let mut app = app();
    app.apply_refresh(Ok(vec![]), Ok(vec![]), true, true, 100, 1, None, 0);
    app.apply_refresh(Ok(vec![]), Ok(vec![]), true, true, 200, 1, None, 0);
    assert_eq!(app.total_download_speed, 200);
    assert_eq!(app.displayed_download_speed(), 125);
    assert_eq!(
        app.speed.history.iter().copied().collect::<Vec<_>>(),
        [100]
    );

    app.speed.last_sample_at = Some(Instant::now() - SPEED_SAMPLE_INTERVAL);
    app.apply_refresh(Ok(vec![]), Ok(vec![]), true, true, 300, 1, None, 0);
    assert_eq!(app.displayed_download_speed(), 168);
    assert_eq!(
        app.speed.history.iter().copied().collect::<Vec<_>>(),
        [100, 300]
    );
}

#[test]
fn active_zero_speed_stays_smoothed_but_idle_resets_immediately() {
    let mut app = app();
    app.apply_refresh(Ok(vec![]), Ok(vec![]), true, true, 100, 1, None, 0);
    app.apply_refresh(Ok(vec![]), Ok(vec![]), true, true, 0, 1, None, 0);
    assert_eq!(app.total_download_speed, 0);
    assert_eq!(app.displayed_download_speed(), 75);

    app.apply_refresh(Ok(vec![]), Ok(vec![]), true, true, 100, 0, None, 0);
    assert_eq!(app.total_download_speed, 0);
    assert_eq!(app.displayed_download_speed(), 0);
    assert_eq!(app.active_downloads, 0);
    assert!(app.speed.history.is_empty());
    assert!(app.speed.last_sample_at.is_none());
}

#[test]
fn speed_chart_uses_twenty_percent_headroom_and_decays_gradually() {
    assert_eq!(speed_scale_target(std::iter::empty()), 1);
    assert_eq!(speed_scale_target([1]), 2);
    assert_eq!(speed_scale_target([100]), 120);
    assert_eq!(speed_scale_target([u64::MAX]), u64::MAX);

    let mut app = app();
    app.push_speed_sample(100);
    assert_eq!(app.speed_chart_max(), 120);

    app.speed.history.clear();
    app.push_speed_sample(0);
    assert_eq!(app.speed_chart_max(), 108);
    app.speed.history.clear();
    app.push_speed_sample(0);
    assert_eq!(app.speed_chart_max(), 97);
}

#[test]
fn successful_health_refresh_caches_aria2_global_options() {
    let mut app = app();
    let options = Aria2GlobalOptions {
        connections_per_download: Some(5),
        max_connections_per_server: Some(1),
        alloc_strategy: Some(AllocStrategy::Prealloc),
        stream_piece_selector: Some(StreamPieceSelector::Default),
    };

    app.apply_refresh(
        Ok(vec![]),
        Ok(vec![]),
        true,
        true,
        0,
        0,
        Some(options.clone()),
        0,
    );

    assert_eq!(app.aria2_global_options, Some(options));
}

#[test]
fn lifecycle_progress_and_stale_failed_refresh() {
    let mut app = app();
    app.manages_server = true;
    assert_eq!(app.lifecycle, LifecycleState::Starting);
    app.apply_refresh(
        Err(anyhow::anyhow!("offline")),
        Err(anyhow::anyhow!("offline")),
        false,
        false,
        0,
        0,
        None,
        0,
    );
    assert!(app.last_error.is_none());
    app.apply_lifecycle(LifecycleState::Retrying);
    app.apply_lifecycle(LifecycleState::Connected);
    let revision = app.lifecycle_revision;
    app.apply_refresh(Ok(vec![]), Ok(vec![]), true, true, 0, 0, None, revision);
    assert!(app.server_reachable);
    app.apply_refresh(
        Err(anyhow::anyhow!("old")),
        Err(anyhow::anyhow!("old")),
        false,
        false,
        0,
        0,
        None,
        0,
    );
    assert!(app.server_reachable);
    assert!(app.last_error.is_none());
    app.apply_lifecycle(LifecycleState::Failed("daemon exited".into()));
    assert_eq!(app.last_error.as_deref(), Some("daemon exited"));
    app.apply_refresh(
        Err(anyhow::anyhow!("offline")),
        Err(anyhow::anyhow!("offline")),
        false,
        false,
        0,
        0,
        None,
        app.lifecycle_revision,
    );
    assert_eq!(app.last_error.as_deref(), Some("daemon exited"));
    app.apply_refresh(
        Ok(vec![]),
        Ok(vec![]),
        true,
        true,
        0,
        0,
        None,
        app.lifecycle_revision,
    );
    assert_eq!(app.lifecycle, LifecycleState::Connected);
    assert!(app.last_error.is_none());
}
