mod category_list;
mod clipboard_import_modal;
mod downloads_table;
mod footer;
mod queue_list;
mod queue_modal;
mod status_bar;

use crate::{
    app::App,
    ui::{
        category_list::draw_categories_list, clipboard_import_modal::draw_clipboard_import_modal,
        downloads_table::draw_downloads_table, footer::draw_footer, queue_list::draw_queues_list,
        queue_modal::draw_queue_modal, status_bar::draw_status_bar,
    },
};
use common::enums::{DownloadStatus, FileCategory};
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
        .constraints([Constraint::Min(5), Constraint::Length(8)])
        .split(body_layout[0]);

    draw_status_bar(f, app, main_layout[0]);
    draw_queues_list(f, app, left_layout[0]);
    draw_categories_list(f, app, left_layout[1]);
    draw_downloads_table(f, app, body_layout[1]);
    draw_footer(f, app, main_layout[2]);

    if let Some(modal) = &app.queue_modal {
        draw_queue_modal(f, app, modal);
    } else if let Some(modal) = &app.modal {
        draw_clipboard_import_modal(f, app, modal);
    }
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

fn format_status(status: &DownloadStatus) -> String {
    match status {
        DownloadStatus::Pending => "Pending".to_string(),
        DownloadStatus::Active => "Active".to_string(),
        DownloadStatus::Paused => "Paused".to_string(),
        DownloadStatus::Completed => "Completed".to_string(),
        DownloadStatus::Error(msg) => format!("Error: {msg}"),
        DownloadStatus::Removed => "Removed".to_string(),
    }
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
