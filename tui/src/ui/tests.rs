use super::{
    SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH, format_bytes, preferred_sidebar_width, render,
    sidebar_width,
};
use crate::{
    app::App,
    icons::{GlyphMode, IconSet},
    theme::Theme,
};
use chrono::Utc;
use common::{
    download::{Download, DownloadLiveStatus},
    enums::{DownloadStatus, FileCategory, QueueStatus, Recurrence, SourceType},
    finetune::FineTune,
    queue::{Queue, QueueSettings},
    scheduler::Scheduler,
};
use ratatui::{Terminal, backend::TestBackend};

fn test_app(mode: GlyphMode) -> App {
    App::new(Theme::default_dark(), IconSet::new(mode), false)
}

fn queue(name: &str, status: QueueStatus, scheduled: bool) -> Queue {
    Queue {
        scheduled_stop_at: None,
        id: 1,
        name: name.into(),
        position: 0,
        settings: QueueSettings {
            max_concurrent_downloads: 1,
            max_retries: 3,
            retry_wait_seconds: 5,
            default_finetune: FineTune::default(),
        },
        scheduler: Scheduler {
            enabled: scheduled,
            recurrence: Recurrence::Once {
                start: Utc::now(),
                end: Utc::now(),
            },
            run_missed_on_startup: false,
        },
        created_at: Utc::now(),
        status,
    }
}

#[test]
fn format_bytes_uses_human_readable_binary_units() {
    assert_eq!(format_bytes(0), "0.0 B");
    assert_eq!(format_bytes(512), "512.0 B");
    assert_eq!(format_bytes(1023), "1023.0 B");
    assert_eq!(format_bytes(1024), "1.0 KB");
    assert_eq!(format_bytes(1536), "1.5 KB");
    assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
    assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0 GB");
    assert_eq!(format_bytes(1024_u64.pow(4)), "1.0 TB");
}

#[test]
fn sidebar_is_clamped_on_normal_and_wide_terminals() {
    let mut app = test_app(GlyphMode::Unicode);
    assert_eq!(preferred_sidebar_width(&app), SIDEBAR_MIN_WIDTH);

    app.queues
        .push(queue("1234567890123456789012", QueueStatus::Paused, false));
    assert_eq!(preferred_sidebar_width(&app), 27);
    for body_width in [79, 100, 120, 240] {
        let width = sidebar_width(&app, body_width);
        assert!((SIDEBAR_MIN_WIDTH..=SIDEBAR_MAX_WIDTH).contains(&width));
    }
}

#[test]
fn narrow_terminals_reserve_about_two_thirds_for_downloads() {
    let mut app = test_app(GlyphMode::Unicode);
    app.queues.push(queue(
        "a queue name long enough to reach the maximum",
        QueueStatus::Paused,
        false,
    ));

    for body_width in 0..79 {
        let width = sidebar_width(&app, body_width);
        assert!(u32::from(width) * 100 <= u32::from(body_width) * 33);
        assert!(body_width - width >= body_width.saturating_mul(67) / 100);
    }
}

#[test]
fn long_queue_names_expand_only_to_the_maximum_and_remeasure() {
    let mut app = test_app(GlyphMode::Unicode);
    assert_eq!(sidebar_width(&app, 120), SIDEBAR_MIN_WIDTH);

    app.queues.push(queue(
        "an exceptionally long queue name that Ratatui will clip",
        QueueStatus::Paused,
        false,
    ));
    assert_eq!(preferred_sidebar_width(&app), SIDEBAR_MAX_WIDTH);
    assert_eq!(sidebar_width(&app, 120), SIDEBAR_MAX_WIDTH);
    assert_eq!(sidebar_width(&app, 50), 16);
}

#[test]
fn indicators_unicode_and_every_glyph_mode_are_measured_in_cells() {
    for mode in [GlyphMode::NerdFont, GlyphMode::Unicode, GlyphMode::Ascii] {
        let empty = test_app(mode);
        assert_eq!(preferred_sidebar_width(&empty), SIDEBAR_MIN_WIDTH);

        let mut active = test_app(mode);
        active
            .queues
            .push(queue("12345678901234567890", QueueStatus::Active, false));
        assert_eq!(preferred_sidebar_width(&active), 27);

        let mut scheduled = test_app(mode);
        scheduled
            .queues
            .push(queue("12345678901234567890", QueueStatus::Paused, true));
        assert_eq!(preferred_sidebar_width(&scheduled), 27);

        let mut both = test_app(mode);
        both.queues
            .push(queue("12345678901234567890", QueueStatus::Active, true));
        assert_eq!(preferred_sidebar_width(&both), 29);

        let mut unicode = test_app(mode);
        unicode
            .queues
            .push(queue("下载队列下载队列下载", QueueStatus::Paused, false));
        assert_eq!(preferred_sidebar_width(&unicode), SIDEBAR_MIN_WIDTH);
    }
}

fn rendered_app(mode: GlyphMode, queue_status: QueueStatus) -> (String, usize) {
    let icons = IconSet::new(mode);
    let mut app = test_app(mode);
    app.downloads.push(DownloadLiveStatus {
        download: Download {
            id: 1,
            aria2_gid: None,
            url: "https://example.test/tool".into(),
            filename: Some("tool.bin".into()),
            destination_path: "/tmp".into(),
            source_type: SourceType::Http,
            category: FileCategory::Program,
            status: DownloadStatus::Error("checksum mismatch".into()),
            paused_by_scheduler: false,
            manually_started: false,
            size: Some(100),
            completed_length: Some(50),
            queue_id: 1,
            position_in_queue: 0,
            finetune: FineTune::default(),
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
        },
        completed_length: 50,
        download_speed: 0,
        eta_seconds: None,
    });
    app.queues.push(queue("Main Queue", queue_status, true));

    let width = 120;
    let backend = TestBackend::new(width, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(&mut app, frame)).unwrap();
    let buffer = terminal.backend().buffer();
    let output = buffer
        .content()
        .chunks(width as usize)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    let status_column = output
        .lines()
        .find(|line| line.contains("tool.bin"))
        .and_then(|line| {
            line.find(icons.download_status(&DownloadStatus::Error(String::new())))
                .map(|byte_index| line[..byte_index].chars().count())
        })
        .unwrap();
    (output, status_column)
}

#[test]
fn widgets_render_semantic_icons_in_every_mode_with_stable_columns() {
    let mut status_columns = Vec::new();
    for mode in [GlyphMode::NerdFont, GlyphMode::Unicode, GlyphMode::Ascii] {
        let icons = IconSet::new(mode);
        let (output, status_column) = rendered_app(mode, QueueStatus::Paused);
        status_columns.push(status_column);

        assert!(output.contains(&format!("{} All", icons.all())));
        assert!(output.contains(&format!(
            "{} Program",
            icons.category(&FileCategory::Program)
        )));
        assert!(output.contains(&format!(
            "{} tool.bin",
            icons.category(&FileCategory::Program)
        )));
        assert!(output.contains(&format!("Main Queue {}", icons.scheduler())));
        assert!(!output.contains(&format!(
            "Main Queue {} {}",
            icons.queue_running(),
            icons.scheduler()
        )));
        assert!(!output.contains("Download: checksum mismatch"));
        assert!(!output.contains("[paused]"));
        assert!(!output.contains("[scheduled]"));
        assert!(output.contains("Error: checksum mismatch"));
    }
    assert!(
        status_columns
            .windows(2)
            .all(|columns| columns[0] == columns[1])
    );
}

#[test]
fn running_queue_shows_play_icon() {
    for mode in [GlyphMode::NerdFont, GlyphMode::Unicode, GlyphMode::Ascii] {
        let icons = IconSet::new(mode);
        let (output, _) = rendered_app(mode, QueueStatus::Active);
        assert!(output.contains(&format!(
            "Main Queue {} {}",
            icons.queue_running(),
            icons.scheduler()
        )));
    }
}
