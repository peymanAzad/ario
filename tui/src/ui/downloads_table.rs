use super::*;
use crate::app::{App, Focus};
use common::{download::DownloadLiveStatus, enums::DownloadStatus};
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

fn download_name(d: &DownloadLiveStatus) -> &str {
    d.download.filename.as_deref().unwrap_or(&d.download.url)
}

fn progress(d: &DownloadLiveStatus) -> Option<String> {
    if d.download.status == DownloadStatus::Completed {
        return None;
    }
    d.download.size.filter(|&size| size > 0).map(|size| {
        let percent = (d.completed_length as f64 / size as f64 * 100.0).clamp(0.0, 100.0);
        format!("{percent:.1}%")
    })
}

// Explicit widths keep filename truncation and the table's actual cell layout in agreement.
fn column_widths(width: u16) -> Vec<u16> {
    let mut metrics = vec![9, 10, 12, 9];
    while metrics.len() > 1 && width < 12 + metrics.iter().sum::<u16>() + metrics.len() as u16 {
        metrics.pop();
    }
    if width < 10 {
        return vec![0, width.saturating_sub(1)];
    }
    let name = width.saturating_sub(metrics.iter().sum::<u16>() + metrics.len() as u16);
    let mut widths = vec![name];
    widths.extend(metrics);
    widths
}

fn middle_truncate(text: &str, width: usize, ellipsis: &str) -> String {
    if text.width() <= width {
        return text.to_owned();
    }
    let marker_width = ellipsis.width();
    if width < marker_width {
        return ellipsis.chars().take(width).collect();
    }
    let remaining = width - marker_width;
    let head_budget = remaining / 2;
    let tail_budget = remaining - head_budget;
    let mut head = String::new();
    let mut used = 0;
    for grapheme in text.graphemes(true) {
        if used + grapheme.width() > head_budget {
            break;
        }
        head.push_str(grapheme);
        used += grapheme.width();
    }
    let mut tail = Vec::new();
    used = 0;
    for grapheme in text.graphemes(true).rev() {
        if used + grapheme.width() > tail_budget {
            break;
        }
        tail.push(grapheme);
        used += grapheme.width();
    }
    format!(
        "{head}{ellipsis}{}",
        tail.into_iter().rev().collect::<String>()
    )
}

// Character wrapping preserves spaces and also works for filenames without word boundaries.
fn wrap_name(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }
    let mut lines = vec![String::new()];
    let mut used = 0;
    for grapheme in text.graphemes(true) {
        // A terminal narrower than a single grapheme cannot display that grapheme.
        let grapheme = if grapheme.width() > width {
            "?"
        } else {
            grapheme
        };
        let cells = grapheme.width();
        if used + cells > width {
            lines.push(String::new());
            used = 0;
        }
        lines.last_mut().unwrap().push_str(grapheme);
        used += cells;
    }
    lines
}

fn detail_lines(text: &str, width: usize, height: usize, ellipsis: &str) -> Vec<String> {
    if width == 0 || height == 0 {
        return Vec::new();
    }
    let full = wrap_name(text, width);
    if full.len() <= height {
        return full;
    }
    // Wide graphemes can leave unused cells at line ends, so fit the wrapped result too.
    let mut budget = width * height;
    loop {
        let shortened = middle_truncate(text, budget, ellipsis);
        let lines = wrap_name(&shortened, width);
        if lines.len() <= height || budget == 0 {
            return lines;
        }
        budget -= 1;
    }
}

pub fn draw_downloads_table(f: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let focused = app.focus == Focus::Downloads;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style(theme, focused))
        .title(Span::styled(
            " [3] Downloads ",
            Style::default().fg(theme.foreground),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let ellipsis = app.icons.ellipsis();
    let max_detail_height = (inner.height / 3).min(inner.height.saturating_sub(3));
    let details = app.current_download().map_or_else(Vec::new, |d| {
        detail_lines(
            download_name(d),
            inner.width as usize,
            max_detail_height as usize,
            ellipsis,
        )
    });
    let detail_height = details.len() as u16;
    let table_area = Rect {
        height: inner.height.saturating_sub(if detail_height > 0 {
            detail_height + 1
        } else {
            0
        }),
        ..inner
    };
    if detail_height > 0 {
        let divider = Rect::new(inner.x, table_area.bottom(), inner.width, 1);
        // ASCII mode also uses an ASCII separator in the detail area.
        let separator = if ellipsis == "..." { "-" } else { "─" };
        f.render_widget(
            Paragraph::new(separator.repeat(inner.width as usize))
                .style(border_style(theme, focused)),
            divider,
        );
        let detail_area = Rect::new(inner.x, divider.bottom(), inner.width, detail_height);
        f.render_widget(
            Paragraph::new(details.into_iter().map(Line::from).collect::<Vec<_>>())
                .style(Style::default().fg(theme.text_muted)),
            detail_area,
        );
    }

    let widths = column_widths(inner.width);
    let header = Row::new(
        ["Name", "Status", "Size", "Speed", "ETA"]
            .into_iter()
            .take(widths.len()),
    )
    .style(
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD),
    );
    let rows: Vec<Row> = app
        .downloads
        .iter()
        .map(|d| {
            let prefix = format!("{} ", app.icons.category(&d.download.category));
            let name_width = widths[0] as usize;
            let name = if name_width < prefix.width() {
                middle_truncate(&prefix, name_width, ellipsis)
            } else {
                format!(
                    "{prefix}{}",
                    middle_truncate(download_name(d), name_width - prefix.width(), ellipsis)
                )
            };
            let mut status = vec![Span::styled(
                app.icons.download_status(&d.download.status),
                Style::default().fg(app.icons.download_status_color(&d.download.status, theme)),
            )];
            if let Some(percent) = progress(d) {
                status.push(Span::raw(format!(" {percent}")));
            }
            let completed = d.download.status == DownloadStatus::Completed;
            let mut cells = vec![
                Cell::from(name),
                Cell::from(Line::from(status)),
                Cell::from(
                    d.download
                        .size
                        .map(format_bytes)
                        .unwrap_or_else(|| "-".to_string()),
                ),
                Cell::from(if completed {
                    "-".to_string()
                } else {
                    format_speed(d.download_speed)
                }),
                Cell::from(if completed {
                    "-".to_string()
                } else {
                    d.eta_seconds
                        .map(format_eta)
                        .unwrap_or_else(|| "-".to_string())
                }),
            ];
            cells.truncate(widths.len());
            Row::new(cells)
        })
        .collect();

    let mut state = TableState::default();
    if app.current_download().is_some() {
        state.select(Some(app.selected_download));
    }
    let table = Table::new(rows, widths.into_iter().map(Constraint::Length))
        .column_spacing(1)
        .header(header)
        .row_highlight_style(highlight_style(theme, focused));
    f.render_stateful_widget(table, table_area, &mut state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        icons::{GlyphMode, IconSet},
        theme::Theme,
    };
    use chrono::Utc;
    use common::{download::Download, enums::SourceType, finetune::FineTune};
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};
    use std::sync::mpsc;

    fn app(mode: GlyphMode) -> App {
        let (sender, _) = mpsc::channel();
        let mut app = App::new(
            "http://127.0.0.1:1".into(),
            Theme::default_dark(),
            IconSet::new(mode),
            sender,
            false,
        );
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
}
