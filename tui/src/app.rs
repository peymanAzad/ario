pub mod category_list;
pub mod clipboard_import_modal;
pub mod confirmation_modal;
pub mod download_edit_modal;
pub mod downloads_table;
pub mod help_modal;
pub mod queue_list;
pub mod queue_modal;

use std::{
    collections::VecDeque,
    sync::mpsc::Sender,
    thread,
    time::{Duration, Instant},
};

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
use common::finetune::{Aria2GlobalOptions, FineTune};
use common::queue::Queue;

#[derive(Debug)]
pub enum AppEvent {
    Lifecycle(LifecycleState),
    Refreshed {
        downloads: anyhow::Result<Vec<DownloadLiveStatus>>,
        queues: anyhow::Result<Vec<Queue>>,
        server_reachable: bool,
        aria2_reachable: bool,
        download_speed: u64,
        active_downloads: u64,
        aria2_global_options: Option<Aria2GlobalOptions>,
        lifecycle_revision: u64,
    },
    QueueDownloadsLoaded {
        queue_id: i64,
        result: anyhow::Result<Vec<DownloadLiveStatus>>,
    },
    QueueSaved(anyhow::Result<()>),
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LifecycleState {
    Starting,
    Retrying,
    Connected,
    Failed(String),
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

pub const SPEED_HISTORY_LEN: usize = 15;
pub(crate) const SPEED_SAMPLE_INTERVAL: Duration = Duration::from_secs(2);

fn speed_scale_target(history: impl IntoIterator<Item = u64>) -> u64 {
    let peak = history.into_iter().max().unwrap_or(0);
    let headroom = peak / 5 + u64::from(peak % 5 != 0);
    peak.saturating_add(headroom).max(1)
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
    pub aria2_global_options: Option<Aria2GlobalOptions>,
    pub total_download_speed: u64,
    pub active_downloads: u64,
    pub speed_history: VecDeque<u64>,
    smoothed_download_speed: Option<u64>,
    speed_chart_max: u64,
    pub lifecycle: LifecycleState,
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
    lifecycle_revision: u64,
    pending_confirmation_action: Option<PendingConfirmationAction>,
    last_speed_sample_at: Option<Instant>,
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
            aria2_global_options: None,
            total_download_speed: 0,
            active_downloads: 0,
            speed_history: VecDeque::new(),
            smoothed_download_speed: None,
            speed_chart_max: 1,
            lifecycle: LifecycleState::Starting,
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
            lifecycle_revision: 0,
            pending_confirmation_action: None,
            last_speed_sample_at: None,
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
        let lifecycle_revision = self.lifecycle_revision;

        thread::spawn(move || {
            let health = api::health(&api_base);
            let (
                server_reachable,
                aria2_reachable,
                download_speed,
                active_downloads,
                aria2_global_options,
            ) = match health {
                Ok(h) => (
                    true,
                    h.aria2_reachable,
                    h.download_speed,
                    h.active_downloads,
                    h.aria2_global_options,
                ),
                Err(_) => (false, false, 0, 0, None),
            };

            let downloads = api::list_downloads(&api_base, &filter);
            let queues = api::list_queues(&api_base);
            // A managed daemon may have been restarted by the supervisor between requests.
            let (
                server_reachable,
                aria2_reachable,
                download_speed,
                active_downloads,
                aria2_global_options,
            ) = if !server_reachable && manages_server {
                match api::health(&api_base) {
                    Ok(h) => (
                        true,
                        h.aria2_reachable,
                        h.download_speed,
                        h.active_downloads,
                        h.aria2_global_options,
                    ),
                    Err(_) => (false, false, 0, 0, None),
                }
            } else {
                (
                    server_reachable,
                    aria2_reachable,
                    download_speed,
                    active_downloads,
                    aria2_global_options,
                )
            };

            let _ = sender.send(Event::App(AppEvent::Refreshed {
                downloads,
                queues,
                server_reachable,
                aria2_reachable,
                download_speed,
                active_downloads,
                aria2_global_options,
                lifecycle_revision,
            }));
        });
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
        if self.manages_server && !server_reachable && lifecycle_revision != self.lifecycle_revision
        {
            return;
        }

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
        self.total_download_speed = if active_downloads == 0 {
            0
        } else {
            download_speed
        };
        self.active_downloads = active_downloads;
        if server_reachable {
            self.smoothed_download_speed = Some(if active_downloads == 0 {
                0
            } else {
                self.smoothed_download_speed
                    .map_or(download_speed, |previous| {
                        ((u128::from(previous) * 3 + u128::from(download_speed)) / 4) as u64
                    })
            });
            if active_downloads > 0 && self.should_record_speed_sample() {
                self.push_speed_sample(download_speed);
                self.last_speed_sample_at = Some(Instant::now());
            }
            self.apply_lifecycle(LifecycleState::Connected);
        } else {
            self.smoothed_download_speed = None;
        }
    }

    fn should_record_speed_sample(&self) -> bool {
        self.last_speed_sample_at
            .is_none_or(|at| at.elapsed() >= SPEED_SAMPLE_INTERVAL)
    }

    pub(crate) fn push_speed_sample(&mut self, speed: u64) {
        if self.speed_history.len() == SPEED_HISTORY_LEN {
            self.speed_history.pop_front();
        }
        self.speed_history.push_back(speed);

        let target = speed_scale_target(self.speed_history.iter().copied());
        self.speed_chart_max = if target >= self.speed_chart_max {
            target
        } else {
            let decay = self.speed_chart_max / 10 + u64::from(self.speed_chart_max % 10 != 0);
            self.speed_chart_max.saturating_sub(decay).max(target)
        };
    }

    pub(crate) fn displayed_download_speed(&self) -> u64 {
        self.smoothed_download_speed
            .unwrap_or(self.total_download_speed)
    }

    pub(crate) fn speed_chart_max(&self) -> u64 {
        self.speed_chart_max
            .max(speed_scale_target(self.speed_history.iter().copied()))
    }

    pub fn apply_lifecycle(&mut self, state: LifecycleState) {
        self.lifecycle_revision += 1;
        if matches!(
            state,
            LifecycleState::Starting | LifecycleState::Retrying | LifecycleState::Connected
        ) {
            self.last_error = None;
        }
        if let LifecycleState::Failed(ref message) = state {
            self.last_error = Some(message.clone());
        }
        if matches!(state, LifecycleState::Connected) {
            self.server_reachable = true;
        } else {
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
mod tests {
    use super::*;
    use common::download::DeleteDownloadFilesResult;
    use std::sync::mpsc;
    use std::time::Instant;

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
    fn optional_retry_values_include_default_zero_and_bounded_values() {
        assert_eq!(adjust_opt_u32_including_zero(None, true, 20), Some(0));
        assert_eq!(adjust_opt_u32_including_zero(Some(0), false, 20), None);
        assert_eq!(adjust_opt_u32_including_zero(Some(20), true, 20), Some(20));
        assert_eq!(adjust_opt_u32_including_zero(Some(1), false, 20), Some(0));
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

    #[test]
    fn lifecycle_progress_and_stale_failed_refresh() {
        let mut app = app();
        app.manages_server = true;
        assert_eq!(app.lifecycle, LifecycleState::Starting);
        app.apply_refresh(
            Err(anyhow::anyhow!("offline")),
            Err(anyhow::anyhow!("offline")),
            false,
            false,
            0,
            0,
            None,
            0,
        );
        assert!(app.last_error.is_none());
        app.apply_lifecycle(LifecycleState::Retrying);
        app.apply_lifecycle(LifecycleState::Connected);
        let revision = app.lifecycle_revision;
        app.apply_refresh(Ok(vec![]), Ok(vec![]), true, true, 0, 0, None, revision);
        assert!(app.server_reachable);
        app.apply_refresh(
            Err(anyhow::anyhow!("old")),
            Err(anyhow::anyhow!("old")),
            false,
            false,
            0,
            0,
            None,
            0,
        );
        assert!(app.server_reachable);
        assert!(app.last_error.is_none());
        app.apply_lifecycle(LifecycleState::Failed("daemon exited".into()));
        assert_eq!(app.last_error.as_deref(), Some("daemon exited"));
        app.apply_refresh(
            Err(anyhow::anyhow!("offline")),
            Err(anyhow::anyhow!("offline")),
            false,
            false,
            0,
            0,
            None,
            app.lifecycle_revision,
        );
        assert_eq!(app.last_error.as_deref(), Some("daemon exited"));
        app.apply_refresh(
            Ok(vec![]),
            Ok(vec![]),
            true,
            true,
            0,
            0,
            None,
            app.lifecycle_revision,
        );
        assert_eq!(app.lifecycle, LifecycleState::Connected);
        assert!(app.last_error.is_none());
    }

    #[test]
    fn reachable_refresh_records_speed_history_and_caps_it() {
        let mut app = app();
        app.apply_refresh(Ok(vec![]), Ok(vec![]), true, true, 100, 1, None, 0);
        assert_eq!(app.total_download_speed, 100);
        assert_eq!(app.speed_history.iter().copied().collect::<Vec<_>>(), [100]);

        app.apply_refresh(Ok(vec![]), Ok(vec![]), false, false, 50, 0, None, 0);
        assert_eq!(app.total_download_speed, 0);
        assert_eq!(app.speed_history.iter().copied().collect::<Vec<_>>(), [100]);

        for speed in 1..=SPEED_HISTORY_LEN as u64 {
            app.push_speed_sample(speed);
        }
        assert_eq!(app.speed_history.len(), SPEED_HISTORY_LEN);
        assert_eq!(app.speed_history.front(), Some(&1));
        assert_eq!(app.speed_history.back(), Some(&(SPEED_HISTORY_LEN as u64)));
        app.push_speed_sample(SPEED_HISTORY_LEN as u64 + 1);
        assert_eq!(app.speed_history.len(), SPEED_HISTORY_LEN);
        assert_eq!(app.speed_history.front(), Some(&2));
        assert_eq!(
            app.speed_history.back(),
            Some(&(SPEED_HISTORY_LEN as u64 + 1))
        );
    }

    #[test]
    fn successful_health_refresh_caches_aria2_global_options() {
        let mut app = app();
        let options = Aria2GlobalOptions {
            connections_per_download: Some(5),
            max_connections_per_server: Some(1),
            alloc_strategy: Some(AllocStrategy::Prealloc),
            stream_piece_selector: Some(StreamPieceSelector::Default),
        };

        app.apply_refresh(
            Ok(vec![]),
            Ok(vec![]),
            true,
            true,
            0,
            0,
            Some(options.clone()),
            0,
        );

        assert_eq!(app.aria2_global_options, Some(options));
    }

    #[test]
    fn reachable_refresh_throttles_speed_samples() {
        let mut app = app();
        app.apply_refresh(Ok(vec![]), Ok(vec![]), true, true, 100, 1, None, 0);
        app.apply_refresh(Ok(vec![]), Ok(vec![]), true, true, 200, 1, None, 0);
        assert_eq!(app.total_download_speed, 200);
        assert_eq!(app.displayed_download_speed(), 125);
        assert_eq!(app.speed_history.iter().copied().collect::<Vec<_>>(), [100]);

        app.last_speed_sample_at = Some(Instant::now() - SPEED_SAMPLE_INTERVAL);
        app.apply_refresh(Ok(vec![]), Ok(vec![]), true, true, 300, 1, None, 0);
        assert_eq!(app.displayed_download_speed(), 168);
        assert_eq!(
            app.speed_history.iter().copied().collect::<Vec<_>>(),
            [100, 300]
        );
    }

    #[test]
    fn active_zero_speed_stays_smoothed_but_idle_resets_immediately() {
        let mut app = app();
        app.apply_refresh(Ok(vec![]), Ok(vec![]), true, true, 100, 1, None, 0);
        app.apply_refresh(Ok(vec![]), Ok(vec![]), true, true, 0, 1, None, 0);
        assert_eq!(app.total_download_speed, 0);
        assert_eq!(app.displayed_download_speed(), 75);

        app.apply_refresh(Ok(vec![]), Ok(vec![]), true, true, 100, 0, None, 0);
        assert_eq!(app.total_download_speed, 0);
        assert_eq!(app.displayed_download_speed(), 0);
        assert_eq!(app.active_downloads, 0);
    }

    #[test]
    fn speed_chart_uses_twenty_percent_headroom_and_decays_gradually() {
        assert_eq!(speed_scale_target(std::iter::empty()), 1);
        assert_eq!(speed_scale_target([1]), 2);
        assert_eq!(speed_scale_target([100]), 120);
        assert_eq!(speed_scale_target([u64::MAX]), u64::MAX);

        let mut app = app();
        app.push_speed_sample(100);
        assert_eq!(app.speed_chart_max(), 120);

        app.speed_history.clear();
        app.push_speed_sample(0);
        assert_eq!(app.speed_chart_max(), 108);
        app.speed_history.clear();
        app.push_speed_sample(0);
        assert_eq!(app.speed_chart_max(), 97);
    }
}
