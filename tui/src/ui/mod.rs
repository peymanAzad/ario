mod category_list;
mod downloads_table;
mod footer;
pub(crate) mod format;
mod queue_list;
mod status_bar;
pub(crate) mod style;
mod toast_popup;

pub(crate) use format::{detail_lines, format_bytes, format_eta, format_speed, middle_truncate};
pub(crate) use style::{border_style, centered_rect, field_style, highlight_style};

use crate::{
    app::{ALL_CATEGORIES, App},
    modal::Modal,
    ui::{
        category_list::draw_categories_list, downloads_table::draw_downloads_table,
        footer::draw_footer, queue_list::draw_queues_list, status_bar::draw_status_bar,
        toast_popup::draw_toasts,
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

    let help_open = matches!(app.modal, Some(Modal::Help(_)));

    // Non-help modals paint under toasts; Help paints after so search stays readable.
    if app.modal.is_some() && !help_open {
        let App {
            modal,
            queues,
            selected_queue,
            aria2_global_options,
            theme,
            icons,
            ..
        } = app;
        if let Some(m) = modal {
            let ctx = crate::modal::Ctx {
                queues,
                selected_queue: *selected_queue,
                aria2_global_options: aria2_global_options.as_ref(),
                theme,
                icons,
            };
            m.render(f, f.area(), &ctx);
        }
    }

    draw_toasts(f, app);
    if help_open {
        let body_area = main_layout[1];
        let App {
            modal,
            queues,
            selected_queue,
            aria2_global_options,
            theme,
            icons,
            ..
        } = app;
        if let Some(m) = modal {
            let ctx = crate::modal::Ctx {
                queues,
                selected_queue: *selected_queue,
                aria2_global_options: aria2_global_options.as_ref(),
                theme,
                icons,
            };
            m.render(f, body_area, &ctx);
        }
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
