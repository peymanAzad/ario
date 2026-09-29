use super::*;
use crate::{
    icons::{GlyphMode, IconSet},
    theme::Theme,
};
use chrono::Utc;
use common::{download::Download, enums::SourceType, finetune::FineTune};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use crate::ui::format::wrap_name;

fn app(mode: GlyphMode) -> App {
    let mut app = App::new(Theme::default_dark(), IconSet::new(mode), false);
    app.downloads.push(DownloadLiveStatus {
        download: Download {
            id: 1,
            aria2_gid: None,
            url: "https://example.test/archive.part03.rar".into(),
            filename: Some("archive.part03.rar".into()),
            destination_path: "/tmp".into(),
            source_type: SourceType::Http,
            category: FileCategory::Archive,
            status: DownloadStatus::Active,
            paused_by_scheduler: false,
            manually_started: false,
            size: Some(100),
            completed_length: Some(42),
            queue_id: 1,
            position_in_queue: 0,
            finetune: FineTune::default(),
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
        },
        completed_length: 42,
        download_speed: 1024,
        eta_seconds: Some(10),
    });
    app
}

fn render(app: &App, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| draw_downloads_table(frame, app, frame.area()))
        .unwrap();
    terminal.backend().buffer().clone()
}

fn line(buffer: &Buffer, y: u16) -> String {
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect()
}

fn cell_text(buffer: &Buffer, x: u16, width: u16, y: u16) -> String {
    (x..x + width)
        .map(|x| buffer[(x, y)].symbol())
        .collect::<String>()
        .trim()
        .to_owned()
}

fn text(buffer: &Buffer) -> String {
    (0..buffer.area.height)
        .map(|y| line(buffer, y))
        .collect::<Vec<_>>()
        .join("\n")
}

fn find_text(buffer: &Buffer, needle: &str) -> (u16, u16) {
    (0..buffer.area.height)
        .find_map(|y| {
            let row = line(buffer, y);
            row.find(needle)
                .map(|x| (row[..x].chars().count() as u16, y))
        })
        .unwrap_or_else(|| panic!("{needle:?} not found in rendered buffer"))
}

#[test]
fn completed_status_overrides_zero_bytes_and_stale_transfer_metrics() {
    for mode in [GlyphMode::Ascii, GlyphMode::Unicode, GlyphMode::NerdFont] {
        let mut app = app(mode);
        app.downloads[0].download.status = DownloadStatus::Completed;
        app.downloads[0].completed_length = 0;
        let buffer = render(&app, 100, 12);
        let widths = column_widths(98);
        let status_x = 1 + widths[0] + 1;
        assert_eq!(
            cell_text(&buffer, status_x, 9, 2),
            app.icons.download_status(&DownloadStatus::Completed)
        );
        assert!(!line(&buffer, 1).contains("Progress"));
        assert!(!line(&buffer, 2).contains('%'));
        assert_eq!(cell_text(&buffer, status_x + 10 + 11, 12, 2), "-");
        assert_eq!(cell_text(&buffer, status_x + 10 + 11 + 13, 9, 2), "-");
    }
}

#[test]
fn incomplete_statuses_show_known_progress_only_and_keep_icon_color() {
    let mut app = app(GlyphMode::Unicode);
    // Inspect an unselected row: selection intentionally overrides foreground colors.
    app.downloads.push(app.downloads[0].clone());
    app.selected_download = 1;
    for status in [
        DownloadStatus::Active,
        DownloadStatus::Pending,
        DownloadStatus::Paused,
        DownloadStatus::Error("failed".into()),
        DownloadStatus::Removed,
    ] {
        app.downloads[0].download.status = status.clone();
        for (size, bytes, expected) in [
            (Some(100), 42, Some("42.0%")),
            (Some(100), 200, Some("100.0%")),
            (Some(100), 0, Some("0.0%")),
            (None, 42, None),
            (Some(0), 42, None),
        ] {
            app.downloads[0].download.size = size;
            app.downloads[0].completed_length = bytes;
            let buffer = render(&app, 100, 12);
            let x = 2 + column_widths(98)[0];
            let icon = app.icons.download_status(&status);
            let expected = expected.map_or_else(|| icon.to_owned(), |p| format!("{icon} {p}"));
            assert_eq!(cell_text(&buffer, x, 9, 2), expected);
            assert_eq!(
                buffer[(x, 2)].fg,
                app.icons.download_status_color(&status, &app.theme)
            );
            if progress(&app.downloads[0]).is_some() {
                assert_ne!(buffer[(x, 2)].fg, buffer[(x + 2, 2)].fg);
            }
        }
    }
}

#[test]
fn pausing_indicator_renders_in_every_glyph_mode() {
    for mode in [GlyphMode::Ascii, GlyphMode::Unicode, GlyphMode::NerdFont] {
        let mut app = app(mode);
        app.pause_selected();

        let buffer = render(&app, 100, 12);
        let widths = column_widths(98);
        let status_x = 1 + widths[0] + 1;
        let marker = app.icons.ellipsis().chars().next().unwrap_or('.');
        let expected = format!("pausing{marker}");
        assert_eq!(cell_text(&buffer, status_x, 10, 2), expected);
    }
}

#[test]
fn middle_truncation_preserves_suffixes_and_graphemes() {
    for marker in ["…", "..."] {
        assert_eq!(middle_truncate("short.rar", 9, marker), "short.rar");
        for suffix in ["part03.rar", ".r01", ".7z.003"] {
            let name = format!("Very.Long.Series.And.Episode.Name.{suffix}");
            let shortened = middle_truncate(&name, 25, marker);
            assert!(shortened.starts_with("Very.Long"));
            assert!(shortened.ends_with(suffix));
            assert!(shortened.contains(marker));
            assert!(shortened.width() <= 25);
        }
        let text = "界e\u{301}👩‍💻long filename with spaces界e\u{301}.rar";
        for width in 0..text.width() {
            let short = middle_truncate(text, width, marker);
            assert!(short.width() <= width);
            for grapheme in short.graphemes(true) {
                assert!(
                    text.graphemes(true).any(|g| g == grapheme) || marker.contains(grapheme)
                );
            }
        }
    }
}

#[test]
fn wrapping_keeps_spaces_and_caps_details_with_suffix_visible() {
    let text = "file  with   spaces.part03.rar";
    assert_eq!(wrap_name(text, 8).concat(), text);
    let short = detail_lines(&"界".repeat(100), 7, 3, "…");
    assert!(short.len() <= 3);
    assert!(short.iter().all(|s| s.width() <= 7));
    let short = detail_lines(&format!("{}.part03.rar", "long".repeat(50)), 20, 2, "…");
    assert_eq!(short.len(), 2);
    assert!(short.concat().contains('…'));
    assert!(short.concat().ends_with(".part03.rar"));
    assert!(detail_lines(text, 0, 3, "…").is_empty());
    assert!(detail_lines(text, 20, 0, "…").is_empty());
}

#[test]
fn details_follow_selection_and_fall_back_to_url() {
    let mut app = app(GlyphMode::Unicode);
    app.downloads.push(app.downloads[0].clone());
    app.downloads[1].download.filename = None;
    let first = render(&app, 100, 12);
    assert_eq!(cell_text(&first, 1, 98, 10), "archive.part03.rar");
    app.selected_download = 1;
    let second = render(&app, 100, 12);
    assert_eq!(cell_text(&second, 1, 98, 10), app.downloads[1].download.url);
    app.downloads.clear();
    let empty = render(&app, 100, 12);
    assert!(line(&empty, 10).trim_matches('│').trim().is_empty());
}

#[test]
fn empty_download_messages_follow_queue_and_category_filters() {
    let mut app = app(GlyphMode::Unicode);
    app.downloads.clear();

    let unfiltered = text(&render(&app, 80, 10));
    assert!(unfiltered.contains("Name"));
    assert!(unfiltered.contains("No downloads yet"));
    assert!(unfiltered.contains("v:Import Clipboard or a:Import Torrent File"));

    app.selected_queue = 1;
    let queue_filtered = text(&render(&app, 80, 10));
    assert!(queue_filtered.contains("No downloads match this view"));
    assert!(queue_filtered.contains("Select All queues and categories to view everything"));

    app.selected_queue = 0;
    app.selected_category = 1;
    let category_filtered = text(&render(&app, 80, 10));
    assert!(category_filtered.contains("No downloads match this view"));

    app.selected_queue = 1;
    let combined_filtered = text(&render(&app, 80, 10));
    assert!(combined_filtered.contains("No downloads match this view"));
    assert!(!combined_filtered.contains("v:Import Clipboard"));
}

#[test]
fn application_error_takes_precedence_in_an_empty_table() {
    let mut app = app(GlyphMode::Unicode);
    app.downloads.clear();
    app.selected_queue = 1;
    app.selected_category = 1;
    app.last_error = Some("can't reach server".into());

    let buffer = render(&app, 80, 10);
    let output = text(&buffer);
    assert!(output.contains("Downloads unavailable"));
    assert!(!output.contains("No downloads match this view"));
    assert!(!output.contains("Select All queues"));
    let (x, y) = find_text(&buffer, "Downloads unavailable");
    assert_eq!(buffer[(x, y)].fg, app.theme.status_error);
}

#[test]
fn download_rows_suppress_empty_state_even_when_an_error_is_present() {
    let mut app = app(GlyphMode::Unicode);
    app.last_error = Some("stale application error".into());

    let output = text(&render(&app, 80, 10));
    assert!(output.contains("archive.part03.rar"));
    assert!(!output.contains("No downloads"));
    assert!(!output.contains("Downloads unavailable"));
}

#[test]
fn empty_state_preserves_the_header_and_yields_explanation_on_short_panes() {
    let mut app = app(GlyphMode::Unicode);
    app.downloads.clear();

    let short = render(&app, 40, 4);
    assert!(line(&short, 1).contains("Name"));
    assert!(line(&short, 2).contains("No downloads yet"));
    assert!(!text(&short).contains("Import Clipboard"));

    for width in [1, 2, 5, 10, 20, 40] {
        for height in [1, 2, 3, 4, 5] {
            let buffer = render(&app, width, height);
            assert_eq!(buffer.area.width, width);
            assert_eq!(buffer.area.height, height);
        }
    }
}

#[test]
fn empty_state_shortcut_colors_match_the_footer() {
    let mut app = app(GlyphMode::Unicode);
    app.downloads.clear();
    let empty = render(&app, 80, 10);

    let mut terminal = Terminal::new(TestBackend::new(80, 1)).unwrap();
    terminal
        .draw(|frame| crate::ui::footer::draw_footer(frame, &app, frame.area()))
        .unwrap();
    let footer = terminal.backend().buffer();

    let (empty_v, empty_y) = find_text(&empty, "v:Import Clipboard");
    let (footer_v, footer_y) = find_text(footer, "v:Import Clipboard");
    assert_eq!(
        empty[(empty_v, empty_y)].fg,
        footer[(footer_v, footer_y)].fg
    );
    assert_eq!(
        empty[(empty_v + 1, empty_y)].fg,
        footer[(footer_v + 1, footer_y)].fg
    );

    let (empty_a, empty_a_y) = find_text(&empty, "a:Import Torrent File");
    let (footer_a, footer_a_y) = find_text(footer, "a:Torrent");
    assert_eq!(
        empty[(empty_a, empty_a_y)].fg,
        footer[(footer_a, footer_a_y)].fg
    );
}

#[test]
fn selected_download_error_wraps_beneath_filename_with_error_style() {
    let mut app = app(GlyphMode::Unicode);
    app.downloads[0].download.status = DownloadStatus::Error(
        "connection closed unexpectedly while receiving the archive from the remote server"
            .into(),
    );

    let buffer = render(&app, 60, 12);
    assert_eq!(cell_text(&buffer, 1, 58, 8), "archive.part03.rar");
    assert!(line(&buffer, 9).contains("Error: connection closed unexpectedly"));
    assert!(line(&buffer, 10).contains("remote server"));
    assert_eq!(buffer[(1, 9)].fg, app.theme.status_error);
}

#[test]
fn selected_download_error_follows_selection_and_yields_to_tiny_layouts() {
    let mut app = app(GlyphMode::Unicode);
    app.downloads[0].download.status = DownloadStatus::Error("failed".into());
    app.downloads.push(app.downloads[0].clone());
    app.downloads[1].download.filename = Some("healthy.iso".into());
    app.downloads[1].download.status = DownloadStatus::Active;

    let errored = render(&app, 100, 12);
    assert!(line(&errored, 10).contains("Error: failed"));

    app.selected_download = 1;
    let healthy = render(&app, 100, 12);
    assert!(line(&healthy, 10).contains("healthy.iso"));
    let healthy_text = (0..12)
        .map(|y| line(&healthy, y))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!healthy_text.contains("Error: failed"));

    app.selected_download = 0;
    let tiny = render(&app, 100, 6);
    assert!(line(&tiny, 4).contains("archive.part03.rar"));
    assert!(!line(&tiny, 4).contains("Error:"));
}

#[test]
fn responsive_layout_preserves_name_space_and_handles_tiny_terminals() {
    assert_eq!(column_widths(56), vec![12, 9, 10, 12, 9]);
    assert_eq!(column_widths(55), vec![21, 9, 10, 12]);
    assert_eq!(column_widths(45), vec![24, 9, 10]);
    assert_eq!(column_widths(32), vec![22, 9]);
    for mode in [GlyphMode::Ascii, GlyphMode::Unicode, GlyphMode::NerdFont] {
        let mut app = app(mode);
        app.downloads[0].download.filename =
            Some(format!("{}.part03.rar", "series".repeat(50)));
        for width in [1, 2, 5, 10, 24, 34, 47, 57, 58, 100, 160] {
            for height in [1, 2, 3, 5, 6, 12] {
                let buffer = render(&app, width, height);
                if width >= 58 && height >= 5 {
                    assert!(line(&buffer, 1).contains("ETA"));
                }
                if width == 100 && height == 12 {
                    assert!(line(&buffer, 2).contains("part03.rar"));
                    assert!(line(&buffer, 2).contains(app.icons.ellipsis()));
                    assert!(line(&buffer, 10).contains("part03.rar"));
                }
                if height == 5 && width == 100 {
                    assert!(line(&buffer, 2).contains("42.0%"));
                    assert!(!line(&buffer, 3).contains("part03.rar"));
                }
            }
        }
    }
}
