pub mod categories;
pub mod clipboard_import_modal;
pub mod confirmation_modal;
pub mod download_edit_modal;
pub mod downloads;
pub mod help_modal;
pub mod keys;
pub mod lifecycle;
pub mod queue_modal;
pub mod queues;
pub mod speed;
pub mod torrent_file_modal;
pub mod update;

pub use lifecycle::LifecycleState;
pub use speed::SpeedTracker;

use std::collections::HashSet;

use crate::app::lifecycle::{
    clears_last_error, is_stale_unreachable_refresh, marks_server_reachable,
};

use crate::app::clipboard_import_modal::ClipboardImportModal;
use crate::app::confirmation_modal::ConfirmationModal;
use crate::app::download_edit_modal::DownloadEditModal;
use crate::app::help_modal::HelpModal;
use crate::app::queue_modal::QueueModal;
use crate::app::torrent_file_modal::TorrentFileModal;
use crate::effects::{ApiRequest, Effect};
use crate::icons::IconSet;
use crate::modal::Modal;
use crate::theme::Theme;
pub use crate::toast::ToastLevel;
use crate::toast::ToastStack;
use common::download::{DownloadFilter, DownloadLiveStatus};
use common::enums::{AllocStrategy, DownloadStatus, FileCategory, StreamPieceSelector};
use common::finetune::{Aria2GlobalOptions, FineTune};
use common::queue::Queue;


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
    pub downloads: Vec<DownloadLiveStatus>,
    pub queues: Vec<Queue>,
    pub selected_download: usize,
    pub selected_queue: usize,
    pub selected_category: usize,
    pub focus: Focus,
    pub server_reachable: bool,
    pub aria2_reachable: bool,
    pub aria2_global_options: Option<Aria2GlobalOptions>,
    pub total_download_speed: u64,
    pub active_downloads: u64,
    pub speed: SpeedTracker,
    pub lifecycle: LifecycleState,
    pub last_error: Option<String>,
    pub should_quit: bool,
    pub theme: Theme,
    pub icons: IconSet,
    pub modal: Option<Modal>,
    pub toasts: ToastStack,
    /// When true, TUI may spawn/supervise ario_daemon for a local URL.
    pub(crate) manages_server: bool,
    refresh_in_flight: bool,
    pub(crate) lifecycle_revision: u64,
    pausing_downloads: HashSet<i64>,
}

impl App {
    pub fn new(theme: Theme, icons: IconSet, manages_server: bool) -> Self {
        Self {
            downloads: Vec::new(),
            queues: Vec::new(),
            selected_download: 0,
            selected_queue: 0,
            selected_category: 0,
            focus: Focus::Downloads,
            server_reachable: false,
            aria2_reachable: false,
            aria2_global_options: None,
            total_download_speed: 0,
            active_downloads: 0,
            speed: SpeedTracker::new(),
            lifecycle: LifecycleState::Starting,
            last_error: None,
            should_quit: false,
            theme,
            icons,
            modal: None,
            manages_server,
            refresh_in_flight: false,
            lifecycle_revision: 0,
            pausing_downloads: HashSet::new(),
            toasts: ToastStack::new(),
        }
    }

    pub fn has_open_modal(&self) -> bool {
        self.modal.is_some()
    }

    pub fn clipboard_modal(&self) -> Option<&ClipboardImportModal> {
        match &self.modal {
            Some(Modal::ClipboardImport(m)) => Some(m),
            _ => None,
        }
    }

    pub fn clipboard_modal_mut(&mut self) -> Option<&mut ClipboardImportModal> {
        match &mut self.modal {
            Some(Modal::ClipboardImport(m)) => Some(m),
            _ => None,
        }
    }

    pub fn torrent_modal(&self) -> Option<&TorrentFileModal> {
        match &self.modal {
            Some(Modal::TorrentFile(m)) => Some(m),
            _ => None,
        }
    }

    pub fn torrent_modal_mut(&mut self) -> Option<&mut TorrentFileModal> {
        match &mut self.modal {
            Some(Modal::TorrentFile(m)) => Some(m),
            _ => None,
        }
    }

    pub fn queue_modal(&self) -> Option<&QueueModal> {
        match &self.modal {
            Some(Modal::Queue(m)) => Some(m),
            _ => None,
        }
    }

    pub fn queue_modal_mut(&mut self) -> Option<&mut QueueModal> {
        match &mut self.modal {
            Some(Modal::Queue(m)) => Some(m),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn download_modal(&self) -> Option<&DownloadEditModal> {
        match &self.modal {
            Some(Modal::DownloadEdit(m)) => Some(m),
            _ => None,
        }
    }

    pub fn download_modal_mut(&mut self) -> Option<&mut DownloadEditModal> {
        match &mut self.modal {
            Some(Modal::DownloadEdit(m)) => Some(m),
            _ => None,
        }
    }

    pub fn help_modal(&self) -> Option<&HelpModal> {
        match &self.modal {
            Some(Modal::Help(m)) => Some(m),
            _ => None,
        }
    }

    pub fn help_modal_mut(&mut self) -> Option<&mut HelpModal> {
        match &mut self.modal {
            Some(Modal::Help(m)) => Some(m),
            _ => None,
        }
    }

    pub fn confirmation_modal(&self) -> Option<&ConfirmationModal> {
        match &self.modal {
            Some(Modal::Confirmation { modal, .. }) => Some(modal),
            _ => None,
        }
    }

    pub fn pending_confirmation_action(&self) -> Option<&PendingConfirmationAction> {
        match &self.modal {
            Some(Modal::Confirmation { action, .. }) => Some(action),
            _ => None,
        }
    }

    pub fn manages_server(&self) -> bool {
        self.manages_server
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

    pub fn refresh(&mut self) -> Vec<Effect> {
        self.toasts.prune();
        if self.refresh_in_flight {
            return vec![];
        }
        self.refresh_in_flight = true;
        vec![Effect::Api(ApiRequest::Refresh {
            filter: self.current_filter(),
            manages_server: self.manages_server,
            lifecycle_revision: self.lifecycle_revision,
        })]
    }

    pub fn apply_refresh(
        &mut self,
        downloads: anyhow::Result<Vec<DownloadLiveStatus>>,
        queues: anyhow::Result<Vec<Queue>>,
        server_reachable: bool,
        aria2_reachable: bool,
        download_speed: u64,
        active_downloads: u64,
        aria2_global_options: Option<Aria2GlobalOptions>,
        lifecycle_revision: u64,
    ) {
        self.refresh_in_flight = false;

        // A failed request started before a newer worker event cannot undo it.
        if is_stale_unreachable_refresh(
            self.manages_server,
            server_reachable,
            lifecycle_revision,
            self.lifecycle_revision,
        ) {
            return;
        }

        if !self.manages_server && self.server_reachable && !server_reachable {
            self.toasts.push("server is down", ToastLevel::Error);
        }

        match downloads {
            Ok(downloads) => {
                self.pausing_downloads.retain(|download_id| {
                    downloads
                        .iter()
                        .find(|download| download.download.id == *download_id)
                        .is_none_or(|download| download.download.status == DownloadStatus::Active)
                });
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
            Err(e) if !self.manages_server => {
                self.last_error = Some(format!("can't reach server: {e}"));
            }
            Err(_) => {}
        }

        if let Ok(queues) = queues {
            self.queues = queues;
        }
        self.server_reachable = server_reachable;
        self.aria2_reachable = aria2_reachable;
        if server_reachable {
            self.aria2_global_options = aria2_global_options;
        }
        if !server_reachable || active_downloads == 0 {
            self.clear_speed_history();
        }
        self.total_download_speed = if active_downloads == 0 {
            0
        } else {
            download_speed
        };
        self.active_downloads = active_downloads;
        if server_reachable {
            self.speed.update_smoothed(download_speed, active_downloads);
            if active_downloads > 0 && self.speed.should_record() {
                self.push_speed_sample(download_speed);
            }
            self.apply_lifecycle(LifecycleState::Connected);
        } else {
            self.speed.clear_smoothed();
        }
    }

    pub(crate) fn push_speed_sample(&mut self, speed: u64) {
        self.speed.record_sample_now(speed);
    }

    pub(crate) fn displayed_download_speed(&self) -> u64 {
        self.speed.displayed(self.total_download_speed)
    }

    pub(crate) fn speed_chart_max(&self) -> u64 {
        self.speed.chart_max()
    }

    fn clear_speed_history(&mut self) {
        self.speed.clear();
    }

    pub fn apply_lifecycle(&mut self, state: LifecycleState) {
        self.lifecycle_revision += 1;
        if clears_last_error(&state) {
            self.last_error = None;
        }
        if let LifecycleState::Failed(ref message) = state {
            self.last_error = Some(message.clone());
        }
        if marks_server_reachable(&state) {
            self.server_reachable = true;
        } else {
            self.clear_speed_history();
            self.server_reachable = false;
            self.aria2_reachable = false;
        }
        self.lifecycle = state;
    }

    pub fn apply_toast(&mut self, message: String, level: ToastLevel) {
        self.toasts.push(message, level);
    }

    pub fn apply_download_files_deleted(
        &mut self,
        result: anyhow::Result<common::download::DeleteDownloadFilesResult>,
    ) -> Vec<Effect> {
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
        self.refresh()
    }

    pub fn apply_torrent_added(
        &mut self,
        result: anyhow::Result<DownloadLiveStatus>,
    ) -> Vec<Effect> {
        match result {
            Ok(result) if matches!(result.download.status, DownloadStatus::Error(_)) => {
                let DownloadStatus::Error(message) = result.download.status else {
                    unreachable!()
                };
                self.toasts.push(
                    format!("torrent was added but could not start: {message}"),
                    ToastLevel::Error,
                );
            }
            Ok(result) => self.toasts.push(
                format!(
                    "added {}",
                    result.download.filename.as_deref().unwrap_or("torrent")
                ),
                ToastLevel::Success,
            ),
            Err(error) => self.toasts.push(error.to_string(), ToastLevel::Error),
        }
        self.refresh()
    }

    pub fn apply_download_paused(
        &mut self,
        download_id: i64,
        result: anyhow::Result<DownloadLiveStatus>,
    ) {
        match result {
            Ok(download) => {
                let pause_finished = download.download.status != DownloadStatus::Active;
                if pause_finished {
                    self.pausing_downloads.remove(&download_id);
                }
                if let Some(existing) = self
                    .downloads
                    .iter_mut()
                    .find(|download| download.download.id == download_id)
                {
                    let was_active = existing.download.status == DownloadStatus::Active;
                    let previous_speed = existing.download_speed;
                    *existing = download;
                    if was_active && pause_finished {
                        self.active_downloads = self.active_downloads.saturating_sub(1);
                        self.total_download_speed = if self.active_downloads == 0 {
                            0
                        } else {
                            self.total_download_speed.saturating_sub(previous_speed)
                        };
                        self.speed.set_smoothed(Some(if self.active_downloads == 0 {
                            0
                        } else {
                            self.total_download_speed
                        }));
                        if self.active_downloads == 0 {
                            self.clear_speed_history();
                        }
                    }
                }
            }
            Err(error) => {
                self.pausing_downloads.remove(&download_id);
                self.toasts.push(error.to_string(), ToastLevel::Error);
            }
        }
    }
}

pub(crate) fn adjust_finetune_field(f: &mut FineTune, cursor: usize, forward: bool) {
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
        4 => f.max_retries = adjust_opt_u32_including_zero(f.max_retries, forward, 20),
        5 => {
            f.retry_wait_seconds = adjust_opt_u32_including_zero(f.retry_wait_seconds, forward, 300)
        }
        _ => {}
    }
}

fn adjust_opt_u32_including_zero(current: Option<u32>, forward: bool, max: u32) -> Option<u32> {
    match (current, forward) {
        (None, true) => Some(0),
        (None, false) => None,
        (Some(0), false) => None,
        (Some(value), true) => Some(value.saturating_add(1).min(max)),
        (Some(value), false) => Some(value - 1),
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
#[path = "mod_tests.rs"]
mod tests;
