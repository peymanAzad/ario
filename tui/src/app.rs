use std::{sync::mpsc::Sender, thread};

use crate::theme::Theme;
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
        aria2_reachable: bool,
    },
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

pub struct ClipboardImportModal {
    pub tab: ModalTab,
    pub entries: Vec<ImportUrlEntry>,
    pub url_cursor: usize,
    pub queue_cursor: usize,
    pub finetune: FineTune,
    pub finetune_cursor: usize,
}

pub struct App {
    pub api_base: String,
    pub downloads: Vec<DownloadLiveStatus>,
    pub queues: Vec<Queue>,
    pub selected_download: usize,
    pub selected_queue: usize,
    pub selected_category: usize,
    pub focus: Focus,
    pub aria2_reachable: bool,
    pub last_error: Option<String>,
    pub should_quit: bool,
    pub theme: Theme,
    pub modal: Option<ClipboardImportModal>,
    event_sender: Sender<Event>,
    refresh_in_flight: bool,
}

impl App {
    pub fn new(api_base: String, theme: Theme, event_sender: Sender<Event>) -> Self {
        Self {
            api_base,
            downloads: Vec::new(),
            queues: Vec::new(),
            selected_download: 0,
            selected_queue: 0,
            selected_category: 0,
            focus: Focus::Downloads,
            aria2_reachable: false,
            last_error: None,
            should_quit: false,
            theme,
            modal: None,
            event_sender,
            refresh_in_flight: false,
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
        if self.refresh_in_flight {
            return;
        }
        self.refresh_in_flight = true;

        let api_base = self.api_base.clone();
        let filter = self.current_filter();
        let sender = self.event_sender.clone();

        thread::spawn(move || {
            let downloads = api::list_downloads(&api_base, &filter);
            let queues = api::list_queues(&api_base);
            let aria2_reachable = api::health(&api_base)
                .map(|h| h.aria2_reachable)
                .unwrap_or(false);

            let _ = sender.send(Event::App(AppEvent::Refreshed {
                downloads,
                queues,
                aria2_reachable,
            }));
        });
    }

    pub fn apply_refresh(
        &mut self,
        downloads: anyhow::Result<Vec<DownloadLiveStatus>>,
        queues: anyhow::Result<Vec<Queue>>,
        aria2_reachable: bool,
    ) {
        self.refresh_in_flight = false;

        match downloads {
            Ok(downloads) => {
                self.downloads = downloads;
                if !self.downloads.is_empty() {
                    self.selected_download = self.selected_download.min(self.downloads.len() - 1);
                } else {
                    self.selected_download = 0;
                }
                self.last_error = None;
            }
            Err(e) => {
                self.last_error = Some(format!("can't reach server: {e}"));
            }
        }

        if let Ok(queues) = queues {
            self.queues = queues;
        }

        self.aria2_reachable = aria2_reachable;
    }

    pub fn select_next_download(&mut self) {
        if !self.downloads.is_empty() {
            self.selected_download = (self.selected_download + 1).min(self.downloads.len() - 1);
        }
    }

    pub fn select_prev_download(&mut self) {
        self.selected_download = self.selected_download.saturating_sub(1);
    }

    pub fn current_download(&self) -> Option<&DownloadLiveStatus> {
        self.downloads.get(self.selected_download)
    }

    pub fn select_next_queue(&mut self) {
        let len = self.queues.len() + 1; // +1 for "All"
        self.selected_queue = (self.selected_queue + 1).min(len - 1);
        self.refresh();
    }

    pub fn select_prev_queue(&mut self) {
        self.selected_queue = self.selected_queue.saturating_sub(1);
        self.refresh();
    }

    pub fn select_next_category(&mut self) {
        let len = ALL_CATEGORIES.len() + 1; // +1 for "All"
        self.selected_category = (self.selected_category + 1).min(len - 1);
        self.refresh();
    }

    pub fn select_prev_category(&mut self) {
        self.selected_category = self.selected_category.saturating_sub(1);
        self.refresh();
    }

    pub fn pause_selected(&mut self) {
        if let Some(id) = self.current_download().map(|d| d.download.id) {
            let api_base = self.api_base.clone();
            thread::spawn(move || {
                let _ = api::pause_download(&api_base, id);
            });
        }
    }

    pub fn resume_selected(&mut self) {
        if let Some(id) = self.current_download().map(|d| d.download.id) {
            let api_base = self.api_base.clone();
            thread::spawn(move || {
                let _ = api::resume_download(&api_base, id);
            });
        }
    }

    pub fn delete_selected(&mut self) {
        if let Some(id) = self.current_download().map(|d| d.download.id) {
            let api_base = self.api_base.clone();
            thread::spawn(move || {
                let _ = api::delete_download(&api_base, id);
            });
        }
    }

    pub fn open_clipboard_import(&mut self) {
        let urls = crate::clipboard::scan_clipboard_for_urls();
        if urls.is_empty() {
            eprintln!("clipboard is empty");
            return;
        }

        let entries = urls
            .into_iter()
            .map(|url| ImportUrlEntry {
                url,
                selected: true, // all selected by default, per spec
            })
            .collect();

        let queue_cursor = self
            .queues
            .iter()
            .position(|q| q.name == "Main Queue")
            .unwrap_or(0);

        self.modal = Some(ClipboardImportModal {
            tab: ModalTab::Urls,
            entries,
            url_cursor: 0,
            queue_cursor,
            finetune: FineTune::default(),
            finetune_cursor: 0,
        });
    }

    pub fn cancel_modal(&mut self) {
        self.modal = None;
    }

    pub fn modal_next_tab(&mut self) {
        if let Some(m) = &mut self.modal {
            m.tab = match m.tab {
                ModalTab::Urls => ModalTab::FineTuning,
                ModalTab::FineTuning => ModalTab::Urls,
            };
        }
    }

    pub fn modal_prev_tab(&mut self) {
        self.modal_next_tab();
    }

    pub fn modal_move_down(&mut self) {
        if let Some(m) = &mut self.modal {
            match m.tab {
                ModalTab::Urls => {
                    if !m.entries.is_empty() {
                        m.url_cursor = (m.url_cursor + 1).min(m.entries.len() - 1);
                    }
                }
                ModalTab::FineTuning => {
                    m.finetune_cursor = (m.finetune_cursor + 1).min(3);
                }
            }
        }
    }

    pub fn modal_move_up(&mut self) {
        if let Some(m) = &mut self.modal {
            match m.tab {
                ModalTab::Urls => m.url_cursor = m.url_cursor.saturating_sub(1),
                ModalTab::FineTuning => m.finetune_cursor = m.finetune_cursor.saturating_sub(1),
            }
        }
    }

    fn modal_adjust(&mut self, forward: bool) {
        let queues_len = self.queues.len();
        if let Some(m) = &mut self.modal {
            match m.tab {
                ModalTab::Urls => {
                    if queues_len == 0 {
                        return;
                    }
                    if forward {
                        m.queue_cursor = (m.queue_cursor + 1).min(queues_len - 1);
                    } else {
                        m.queue_cursor = m.queue_cursor.saturating_sub(1);
                    }
                }
                ModalTab::FineTuning => {
                    adjust_finetune_field(&mut m.finetune, m.finetune_cursor, forward)
                }
            }
        }
    }

    pub fn modal_adjust_left(&mut self) {
        self.modal_adjust(false);
    }

    pub fn modal_adjust_right(&mut self) {
        self.modal_adjust(true);
    }

    pub fn modal_toggle_selected_url(&mut self) {
        if let Some(m) = &mut self.modal {
            if let Some(entry) = m.entries.get_mut(m.url_cursor) {
                entry.selected = !entry.selected;
            }
        }
    }

    pub fn modal_select_all(&mut self) {
        if let Some(m) = &mut self.modal {
            for e in &mut m.entries {
                e.selected = true;
            }
        }
    }

    pub fn modal_select_none(&mut self) {
        if let Some(m) = &mut self.modal {
            for e in &mut m.entries {
                e.selected = false;
            }
        }
    }

    fn submit_modal(&mut self, start_immediately: bool) {
        let Some(modal) = self.modal.take() else {
            return;
        };

        let inputs: Vec<AddDownloadInput> = modal
            .entries
            .into_iter()
            .filter(|e| e.selected)
            .map(|e| AddDownloadInput::Url(e.url))
            .collect();

        if inputs.is_empty() {
            return;
        }

        let queue_id = self
            .queues
            .get(modal.queue_cursor)
            .map(|q| q.id)
            .unwrap_or(1);

        let finetune_override = if modal.finetune == FineTune::default() {
            None
        } else {
            Some(modal.finetune)
        };

        let request = AddDownloadsRequest {
            inputs,
            queue_id,
            finetune_override,
            start_immediately,
        };

        let api_base = self.api_base.clone();
        thread::spawn(move || {
            let _ = api::add_downloads(&api_base, &request);
        });

        self.refresh();
    }

    pub fn start_modal_now(&mut self) {
        self.submit_modal(true);
    }

    pub fn save_modal_for_later(&mut self) {
        self.submit_modal(false);
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

fn adjust_opt_u32(current: Option<u32>, forward: bool, max: u32) -> Option<u32> {
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
