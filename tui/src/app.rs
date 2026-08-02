use std::{sync::mpsc::Sender, thread};

use crate::theme::Theme;
use crate::{api, event::Event};
use common::{download::DownloadLiveStatus, queue::Queue};

#[derive(Debug)]
pub enum AppEvent {
    Refreshed {
        downloads: anyhow::Result<Vec<DownloadLiveStatus>>,
        queues: anyhow::Result<Vec<Queue>>,
        aria2_reachable: bool,
    },
}

pub struct App {
    pub api_base: String,
    pub downloads: Vec<DownloadLiveStatus>,
    pub queues: Vec<Queue>,
    pub selected: usize,
    pub aria2_reachable: bool,
    pub last_error: Option<String>,
    pub should_quit: bool,
    pub theme: Theme,
    event_sender: Sender<Event>,
    refresh_in_flight: bool,
}

impl App {
    pub fn new(api_base: String, theme: Theme, event_sender: Sender<Event>) -> Self {
        Self {
            api_base,
            downloads: Vec::new(),
            queues: Vec::new(),
            selected: 0,
            aria2_reachable: false,
            last_error: None,
            should_quit: false,
            theme,
            event_sender,
            refresh_in_flight: false,
        }
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    pub fn refresh(&mut self) {
        if self.refresh_in_flight {
            return;
        }
        self.refresh_in_flight = true;

        let api_base = self.api_base.clone();
        let sender = self.event_sender.clone();

        thread::spawn(move || {
            let downloads = api::list_downloads(&api_base);
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
                    self.selected = self.selected.min(self.downloads.len() - 1);
                } else {
                    self.selected = 0;
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

    pub fn select_next(&mut self) {
        if !self.downloads.is_empty() {
            self.selected = (self.selected + 1).min(self.downloads.len() - 1);
        }
    }

    pub fn select_prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn selected_download(&self) -> Option<&DownloadLiveStatus> {
        self.downloads.get(self.selected)
    }

    pub fn pause_selected(&mut self) {
        if let Some(id) = self.selected_download().map(|d| d.download.id) {
            let api_base = self.api_base.clone();
            thread::spawn(move || {
                let _ = api::pause_download(&api_base, id);
            });
        }
    }

    pub fn resume_selected(&mut self) {
        if let Some(id) = self.selected_download().map(|d| d.download.id) {
            let api_base = self.api_base.clone();
            thread::spawn(move || {
                let _ = api::resume_download(&api_base, id);
            });
        }
    }

    pub fn delete_selected(&mut self) {
        if let Some(id) = self.selected_download().map(|d| d.download.id) {
            let api_base = self.api_base.clone();
            thread::spawn(move || {
                let _ = api::delete_download(&api_base, id);
            });
        }
    }
}
