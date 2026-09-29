mod category_list;
mod clipboard_import_modal;
mod confirmation_modal;
mod download_edit_modal;
mod downloads_table;
pub(crate) mod format;
mod footer;
mod help_modal;
mod queue_list;
mod queue_modal;
mod status_bar;
pub(crate) mod style;
mod toast_popup;
mod torrent_file_modal;

pub(crate) use format::{
    detail_lines, format_bytes, format_eta, format_speed, middle_truncate,
};
pub(crate) use style::{border_style, centered_rect, field_style, highlight_style};

use crate::{
    app::{ALL_CATEGORIES, App},
    ui::{
        category_list::draw_categories_list, clipboard_import_modal::draw_clipboard_import_modal,
        confirmation_modal::draw_confirmation_modal, download_edit_modal::draw_download_modal,
        downloads_table::draw_downloads_table, footer::draw_footer, queue_list::draw_queues_list,
        queue_modal::draw_queue_modal, status_bar::draw_status_bar, toast_popup::draw_toasts,
        torrent_file_modal::draw_torrent_file_modal,
    },
};
use common::enums::FileCategory;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
};
use unicode_width::UnicodeWidthStr;

const SIDEBAR_MIN_WIDTH: u16 = 26;
const SIDEBAR_MAX_WIDTH: u16 = 32;
const SIDEBAR_MAX_BODY_PERCENT: u16 = 33;
const SIDEBAR_CONTENT_RIGHT_PADDING: usize = 1;
const LIST_SELECTION_MARKER: &str = "> ";
const QUEUES_TITLE: &str = " [1] Queues ";
const CATEGORIES_TITLE: &str = " [2] Categories ";

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
        .constraints([
            Constraint::Length(sidebar_width(app, main_layout[1].width)),
            Constraint::Min(0),
        ])
        .split(main_layout[1]);
    let left_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(9)])
        .split(body_layout[0]);

    draw_status_bar(f, app, main_layout[0]);
    draw_queues_list(f, app, left_layout[0]);
    draw_categories_list(f, app, left_layout[1]);
    draw_downloads_table(f, app, body_layout[1]);

    match &app.modal {
        Some(crate::modal::Modal::Confirmation { modal, .. }) => {
            draw_confirmation_modal(f, app, modal);
        }
        Some(crate::modal::Modal::Queue(modal)) => draw_queue_modal(f, app, modal),
        Some(crate::modal::Modal::TorrentFile(modal)) => draw_torrent_file_modal(f, app, modal),
        Some(crate::modal::Modal::ClipboardImport(modal)) => {
            draw_clipboard_import_modal(f, app, modal)
        }
        Some(crate::modal::Modal::DownloadEdit(modal)) => draw_download_modal(f, app, modal),
        Some(crate::modal::Modal::Help(_)) | None => {}
    }

    draw_toasts(f, app);
    if let Some(crate::modal::Modal::Help(modal)) = &mut app.modal {
        help_modal::draw_help_modal(f, modal, &app.theme, main_layout[1]);
    }
    draw_footer(f, app, main_layout[2]);
}

fn sidebar_width(app: &App, body_width: u16) -> u16 {
    let preferred = preferred_sidebar_width(app);
    let narrow_terminal_cap = body_width.saturating_mul(SIDEBAR_MAX_BODY_PERCENT) / 100;
    preferred.min(narrow_terminal_cap)
}

fn preferred_sidebar_width(app: &App) -> u16 {
    // Both lists reserve space for the selection marker and their two borders.
    let list_chrome = LIST_SELECTION_MARKER.width() + 2 + SIDEBAR_CONTENT_RIGHT_PADDING;
    let longest_queue = std::iter::once("All".width())
        .chain(app.queues.iter().map(|queue| {
            queue.name.width()
                + usize::from(queue.status == common::enums::QueueStatus::Active)
                    * (1 + app.icons.queue_running().width())
                + usize::from(queue.scheduler.enabled) * (1 + app.icons.scheduler().width())
        }))
        .max()
        .unwrap_or(0)
        + list_chrome;
    let longest_category = std::iter::once(app.icons.all().width() + 1 + "All".width())
        .chain(ALL_CATEGORIES.iter().map(|category| {
            app.icons.category(category).width() + 1 + category_label(category).width()
        }))
        .max()
        .unwrap_or(0)
        + list_chrome;
    let widest_title = QUEUES_TITLE.width().max(CATEGORIES_TITLE.width()) + 2;

    u16::try_from(longest_queue.max(longest_category).max(widest_title))
        .unwrap_or(u16::MAX)
        .clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH)
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


#[cfg(test)]
mod tests;
