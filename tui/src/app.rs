pub mod category_list;
pub mod clipboard_import_modal;
pub mod download_edit_modal;
pub mod downloads_table;
pub mod queue_list;
pub mod queue_modal;

use std::{sync::mpsc::Sender, thread};

use crate::app::clipboard_import_modal::ClipboardImportModal;
use crate::app::download_edit_modal::DownloadEditModal;
use crate::app::queue_modal::QueueModal;
use crate::theme::Theme;
use crate::toast::{ToastLevel, ToastStack};
use crate::{api, event::Event};
use common::download::{AddDownloadInput, AddDownloadsRequest, DownloadFilter, DownloadLiveStatus};
use common::enums::{AllocStrategy, FileCategory, StreamPieceSelector};
use common::finetune::FineTune;
use common::queue::Queue;

#[derive(Debug)]
pub enum AppEvent {
    Refreshed {
        downloads: anyhow::Result<Vec<DownloadLiveStatus>>,
        queues: anyhow::Result<Vec<Queue>>,
        server_reachable: bool,
        aria2_reachable: bool,
    },
    QueueDownloadsLoaded(anyhow::Result<Vec<DownloadLiveStatus>>),
    ActionFailed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Queues,
    Categories,
    Downloads,
}

impl Focus {
    pub fn next(self) -> Self {
        match self {
            Focus::Queues => Focus::Categories,
            Focus::Categories => Focus::Downloads,
            Focus::Downloads => Focus::Queues,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Focus::Queues => Focus::Downloads,
            Focus::Categories => Focus::Queues,
            Focus::Downloads => Focus::Categories,
        }
    }
}

pub const ALL_CATEGORIES: [FileCategory; 5] = [
    FileCategory::Video,
    FileCategory::Music,
    FileCategory::Document,
    FileCategory::Archive,
    FileCategory::Other,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModalTab {
    Urls,
    FineTuning,
}

#[derive(Clone, Debug)]
pub struct ImportUrlEntry {
    pub url: String,
    pub selected: bool,
}

pub struct App {
    pub api_base: String,
    pub downloads: Vec<DownloadLiveStatus>,
    pub queues: Vec<Queue>,
    pub selected_download: usize,
    pub selected_queue: usize,
    pub selected_category: usize,
    pub focus: Focus,
    pub server_reachable: bool,
    pub aria2_reachable: bool,
    pub last_error: Option<String>,
    pub should_quit: bool,
    pub theme: Theme,
    pub modal: Option<ClipboardImportModal>,
    pub queue_modal: Option<QueueModal>,
    pub download_modal: Option<DownloadEditModal>,
    pub toasts: ToastStack,
    /// When true, TUI may spawn/supervise ario_daemon for a local URL.
    manages_server: bool,
    event_sender: Sender<Event>,
    refresh_in_flight: bool,
}

impl App {
    pub fn new(
        api_base: String,
        theme: Theme,
        event_sender: Sender<Event>,
        manages_server: bool,
    ) -> Self {
        Self {
            api_base,
            downloads: Vec::new(),
            queues: Vec::new(),
            selected_download: 0,
            selected_queue: 0,
            selected_category: 0,
            focus: Focus::Downloads,
            server_reachable: false,
            aria2_reachable: false,
            last_error: None,
            should_quit: false,
            theme,
            modal: None,
            queue_modal: None,
            download_modal: None,
            event_sender,
            manages_server,
            refresh_in_flight: false,
            toasts: ToastStack::new(),
        }
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    fn current_filter(&self) -> DownloadFilter {
        let queue_id = if self.selected_queue == 0 {
            None
        } else {
            self.queues.get(self.selected_queue - 1).map(|q| q.id)
        };

        let category = if self.selected_category == 0 {
            None
        } else {
            ALL_CATEGORIES.get(self.selected_category - 1).cloned()
        };

        DownloadFilter {
            queue_id,
            category,
            status: None,
            sort_by: None,
            sort_desc: false,
        }
    }

    pub fn refresh(&mut self) {
        self.toasts.prune();

        if self.refresh_in_flight {
            return;
        }
        self.refresh_in_flight = true;

        let api_base = self.api_base.clone();
        let filter = self.current_filter();
        let sender = self.event_sender.clone();
        let manages_server = self.manages_server;

        thread::spawn(move || {
            let health = api::health(&api_base);
            let server_reachable = health.is_ok();
            let aria2_reachable = health.map(|h| h.aria2_reachable).unwrap_or(false);

            let downloads = api::list_downloads(&api_base, &filter);
            let queues = api::list_queues(&api_base);
            // A managed daemon may have been restarted by the supervisor between requests.
            let (server_reachable, aria2_reachable) = if !server_reachable && manages_server {
                match api::health(&api_base) {
                    Ok(h) => (true, h.aria2_reachable),
                    Err(_) => (false, false),
                }
            } else {
                (server_reachable, aria2_reachable)
            };

            let _ = sender.send(Event::App(AppEvent::Refreshed {
                downloads,
                queues,
                server_reachable,
                aria2_reachable,
            }));
        });
    }

    pub fn apply_refresh(
        &mut self,
        downloads: anyhow::Result<Vec<DownloadLiveStatus>>,
        queues: anyhow::Result<Vec<Queue>>,
        server_reachable: bool,
        aria2_reachable: bool,
    ) {
        self.refresh_in_flight = false;

        if !self.manages_server && self.server_reachable && !server_reachable {
            self.toasts.push("server is down", ToastLevel::Error);
        }

        match downloads {
            Ok(downloads) => {
                self.downloads = downloads;
                if !self.downloads.is_empty() {
                    self.selected_download = self.selected_download.min(self.downloads.len() - 1);
                } else {
                    self.selected_download = 0;
                }
                if server_reachable {
                    self.last_error = None;
                }
            }
            Err(e) => {
                self.last_error = Some(format!("can't reach server: {e}"));
            }
        }

        if let Ok(queues) = queues {
            self.queues = queues;
        }
        self.server_reachable = server_reachable;
        self.aria2_reachable = aria2_reachable;
    }

    pub fn apply_action_failed(&mut self, message: String) {
        self.toasts.push(message, ToastLevel::Error);
    }
}

fn adjust_finetune_field(f: &mut FineTune, cursor: usize, forward: bool) {
    match cursor {
        0 => f.connections_per_download = adjust_opt_u32(f.connections_per_download, forward, 16),
        1 => {
            f.max_connections_per_server = adjust_opt_u32(f.max_connections_per_server, forward, 16)
        }
        2 => f.alloc_strategy = cycle(&ALLOC_STRATEGY_ORDER, &f.alloc_strategy, forward),
        3 => {
            f.stream_piece_selector =
                cycle(&STREAM_SELECTOR_ORDER, &f.stream_piece_selector, forward)
        }
        _ => {}
    }
}

pub fn adjust_opt_u32(current: Option<u32>, forward: bool, max: u32) -> Option<u32> {
    let val = current.unwrap_or(0);
    let new_val = if forward {
        (val + 1).min(max)
    } else {
        val.saturating_sub(1)
    };
    if new_val == 0 { None } else { Some(new_val) }
}

const ALLOC_STRATEGY_ORDER: [Option<AllocStrategy>; 5] = [
    None,
    Some(AllocStrategy::None),
    Some(AllocStrategy::Prealloc),
    Some(AllocStrategy::Falloc),
    Some(AllocStrategy::Trunc),
];

const STREAM_SELECTOR_ORDER: [Option<StreamPieceSelector>; 5] = [
    None,
    Some(StreamPieceSelector::Default),
    Some(StreamPieceSelector::InOrder),
    Some(StreamPieceSelector::Random),
    Some(StreamPieceSelector::Geom),
];

fn cycle<T: PartialEq + Clone>(
    order: &[Option<T>],
    current: &Option<T>,
    forward: bool,
) -> Option<T> {
    let idx = order.iter().position(|v| v == current).unwrap_or(0);
    let len = order.len();
    let new_idx = if forward {
        (idx + 1) % len
    } else {
        (idx + len - 1) % len
    };
    order[new_idx].clone()
}
