pub mod category_list;
pub mod clipboard_import_modal;
pub mod confirmation_modal;
pub mod download_edit_modal;
pub mod downloads_table;
pub mod help_modal;
pub mod queue_list;
pub mod queue_modal;

use std::{sync::mpsc::Sender, thread};

use crate::app::clipboard_import_modal::ClipboardImportModal;
use crate::app::confirmation_modal::ConfirmationModal;
use crate::app::download_edit_modal::DownloadEditModal;
use crate::app::queue_modal::QueueModal;
use crate::icons::IconSet;
use crate::theme::Theme;
pub use crate::toast::ToastLevel;
use crate::toast::ToastStack;
use crate::{api, event::Event};
use common::download::{AddDownloadInput, AddDownloadsRequest, DownloadFilter, DownloadLiveStatus};
use common::enums::{AllocStrategy, DownloadStatus, FileCategory, StreamPieceSelector};
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
    Toast {
        message: String,
        level: ToastLevel,
    },
    DownloadFilesDeleted(anyhow::Result<common::download::DeleteDownloadFilesResult>),
    QueueDeleteResolved {
        queue_id: i64,
        queue_name: String,
        result: anyhow::Result<api::DeleteQueueOutcome>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Queues,
    Categories,
    Downloads,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PendingConfirmationAction {
    DeleteDownloadFiles { download_id: i64 },
    DeleteQueue { queue_id: i64 },
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

pub const ALL_CATEGORIES: [FileCategory; 6] = [
    FileCategory::Video,
    FileCategory::Music,
    FileCategory::Document,
    FileCategory::Archive,
    FileCategory::Program,
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
    pub icons: IconSet,
    pub modal: Option<ClipboardImportModal>,
    pub queue_modal: Option<QueueModal>,
    pub download_modal: Option<DownloadEditModal>,
    pub confirmation_modal: Option<ConfirmationModal>,
    pub help_modal: Option<help_modal::HelpModal>,
    pub toasts: ToastStack,
    /// When true, TUI may spawn/supervise ario_daemon for a local URL.
    manages_server: bool,
    event_sender: Sender<Event>,
    refresh_in_flight: bool,
    pending_confirmation_action: Option<PendingConfirmationAction>,
}

impl App {
    pub fn new(
        api_base: String,
        theme: Theme,
        icons: IconSet,
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
            icons,
            modal: None,
            queue_modal: None,
            download_modal: None,
            confirmation_modal: None,
            help_modal: None,
            event_sender,
            manages_server,
            refresh_in_flight: false,
            pending_confirmation_action: None,
            toasts: ToastStack::new(),
        }
    }

    pub fn has_open_modal(&self) -> bool {
        self.modal.is_some()
            || self.queue_modal.is_some()
            || self.download_modal.is_some()
            || self.confirmation_modal.is_some()
            || self.help_modal.is_some()
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

    pub fn apply_toast(&mut self, message: String, level: ToastLevel) {
        self.toasts.push(message, level);
    }

    pub fn apply_download_files_deleted(
        &mut self,
        result: anyhow::Result<common::download::DeleteDownloadFilesResult>,
    ) {
        match result {
            Ok(result) if result.missing_payloads > 0 || !result.metadata_complete => {
                let mut message = if result.missing_payloads > 0 {
                    format!(
                        "download removed; {} file(s) were already missing",
                        result.missing_payloads
                    )
                } else {
                    "download and known files removed".to_string()
                };
                if !result.metadata_complete {
                    message.push_str(
                        "; legacy file metadata was incomplete, so untracked files may remain",
                    );
                }
                self.toasts.push(message, ToastLevel::Warning);
            }
            Ok(result) => self.toasts.push(
                format!("download removed with {} file(s)", result.removed_payloads),
                ToastLevel::Success,
            ),
            Err(error) => self.toasts.push(error.to_string(), ToastLevel::Error),
        }
        self.refresh();
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

#[cfg(test)]
mod tests {
    use super::*;
    use common::download::DeleteDownloadFilesResult;
    use std::sync::mpsc;

    fn app() -> App {
        let (sender, _receiver) = mpsc::channel();
        App::new(
            "http://127.0.0.1:1".into(),
            Theme::default_dark(),
            crate::icons::IconSet::new(crate::icons::GlyphMode::Unicode),
            sender,
            false,
        )
    }

    #[test]
    fn missing_file_result_becomes_a_warning_toast() {
        let mut app = app();
        app.apply_download_files_deleted(Ok(DeleteDownloadFilesResult {
            removed_payloads: 0,
            missing_payloads: 1,
            metadata_complete: true,
        }));

        let toast = app.toasts.iter().last().unwrap();
        assert_eq!(toast.level, ToastLevel::Warning);
        assert!(toast.message.contains("already missing"));
    }

    #[test]
    fn fully_removed_result_becomes_a_success_toast() {
        let mut app = app();
        app.apply_download_files_deleted(Ok(DeleteDownloadFilesResult {
            removed_payloads: 2,
            missing_payloads: 0,
            metadata_complete: true,
        }));

        let toast = app.toasts.iter().last().unwrap();
        assert_eq!(toast.level, ToastLevel::Success);
    }
}
