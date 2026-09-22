mod category_list;
mod clipboard_import_modal;
mod confirmation_modal;
mod download_edit_modal;
mod downloads_table;
mod footer;
mod help_modal;
mod queue_list;
mod queue_modal;
mod status_bar;
mod toast_popup;

use crate::{
    app::App,
    ui::{
        category_list::draw_categories_list, clipboard_import_modal::draw_clipboard_import_modal,
        confirmation_modal::draw_confirmation_modal, download_edit_modal::draw_download_modal,
        downloads_table::draw_downloads_table, footer::draw_footer, queue_list::draw_queues_list,
        queue_modal::draw_queue_modal, status_bar::draw_status_bar, toast_popup::draw_toasts,
    },
};
use common::enums::FileCategory;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
};

pub fn render(app: &mut App, f: &mut Frame) {
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // status bar
            Constraint::Min(0),    // body
            Constraint::Length(1), // footer / keybindings
        ])
        .split(f.area());
    let body_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
        .split(main_layout[1]);
    let left_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(9)])
        .split(body_layout[0]);

    draw_status_bar(f, app, main_layout[0]);
    draw_queues_list(f, app, left_layout[0]);
    draw_categories_list(f, app, left_layout[1]);
    draw_downloads_table(f, app, body_layout[1]);

    if let Some(modal) = &app.confirmation_modal {
        draw_confirmation_modal(f, app, modal);
    } else if let Some(modal) = &app.queue_modal {
        draw_queue_modal(f, app, modal);
    } else if let Some(modal) = &app.download_modal {
        draw_download_modal(f, app, modal);
    } else if let Some(modal) = &app.modal {
        draw_clipboard_import_modal(f, app, modal);
    }

    draw_toasts(f, app);
    if let Some(modal) = &mut app.help_modal {
        help_modal::draw_help_modal(f, modal, &app.theme, main_layout[1]);
    }
    draw_footer(f, app, main_layout[2]);
}

fn border_style(theme: &crate::theme::Theme, focused: bool) -> Style {
    if focused {
        Style::default().fg(theme.border_focused)
    } else {
        Style::default().fg(theme.border)
    }
}

fn highlight_style(theme: &crate::theme::Theme, focused: bool) -> Style {
    if focused {
        Style::default()
            .bg(theme.selected_bg)
            .fg(theme.selected_fg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.accent)
    }
}

fn category_label(c: &FileCategory) -> String {
    match c {
        FileCategory::Video => "Video",
        FileCategory::Music => "Music",
        FileCategory::Document => "Document",
        FileCategory::Archive => "Archive",
        FileCategory::Program => "Program",
        FileCategory::Other => "Other",
    }
    .to_string()
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
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

fn format_speed(bytes_per_sec: u64) -> String {
    if bytes_per_sec == 0 {
        return "-".to_string();
    }
    format!("{}/s", format_bytes(bytes_per_sec))
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;
    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }
    format!("{size:.1} {}", UNITS[unit_idx])
}

fn format_eta(seconds: u64) -> String {
    let h = seconds / 3600;
    let m = (seconds % 3600) / 60;
    let s = seconds % 60;
    if h > 0 {
        format!("{h}h{m}m")
    } else if m > 0 {
        format!("{m}m{s}s")
    } else {
        format!("{s}s")
    }
}

fn field_style(theme: &crate::theme::Theme, active: bool) -> Style {
    if active {
        Style::default()
            .bg(theme.selected_bg)
            .fg(theme.selected_fg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.foreground)
    }
}

#[cfg(test)]
mod tests {
    use super::{format_bytes, render};
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
    use std::sync::mpsc;

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

    fn rendered_app(mode: GlyphMode) -> (String, usize) {
        let (sender, _receiver) = mpsc::channel();
        let icons = IconSet::new(mode);
        let mut app = App::new(
            "http://127.0.0.1:1".into(),
            Theme::default_dark(),
            icons,
            sender,
            false,
        );
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
        app.queues.push(Queue {
            id: 1,
            name: "Main Queue".into(),
            position: 0,
            settings: QueueSettings {
                max_concurrent_downloads: 1,
                max_retries: 3,
                retry_wait_seconds: 5,
                default_finetune: FineTune::default(),
            },
            scheduler: Scheduler {
                enabled: true,
                recurrence: Recurrence::Once {
                    start: Utc::now(),
                    end: Utc::now(),
                },
                run_missed_on_startup: false,
            },
            created_at: Utc::now(),
            status: QueueStatus::Paused,
        });

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
            let (output, status_column) = rendered_app(mode);
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
            assert!(output.contains(&format!(
                "Main Queue {} {}",
                icons.queue_status(&QueueStatus::Paused),
                icons.scheduler()
            )));
            assert!(output.contains("Download: checksum mismatch"));
            assert!(!output.contains("[paused]"));
            assert!(!output.contains("[scheduled]"));
            assert!(!output.contains("Error: checksum mismatch"));
        }
        assert!(
            status_columns
                .windows(2)
                .all(|columns| columns[0] == columns[1])
        );
    }
}
