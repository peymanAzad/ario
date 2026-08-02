use crate::api;
use crate::theme::Theme;
use common::{download::DownloadLiveStatus, queue::Queue};

pub struct App {
    pub api_base: String,
    pub downloads: Vec<DownloadLiveStatus>,
    pub queues: Vec<Queue>,
    pub selected: usize,
    pub aria2_reachable: bool,
    pub last_error: Option<String>,
    pub should_quit: bool,
    pub theme: Theme,
}

impl App {
    pub fn new(api_base: String, theme: Theme) -> Self {
        Self {
            api_base,
            downloads: Vec::new(),
            queues: Vec::new(),
            selected: 0,
            aria2_reachable: false,
            last_error: None,
            should_quit: false,
            theme,
        }
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    pub fn refresh(&mut self) {
        match api::list_downloads(&self.api_base) {
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

        if let Ok(queues) = api::list_queues(&self.api_base) {
            self.queues = queues;
        }

        self.aria2_reachable = api::health(&self.api_base)
            .map(|h| h.aria2_reachable)
            .unwrap_or(false);
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
            let _ = api::pause_download(&self.api_base, id);
        }
    }

    pub fn resume_selected(&mut self) {
        if let Some(id) = self.selected_download().map(|d| d.download.id) {
            let _ = api::resume_download(&self.api_base, id);
        }
    }

    pub fn delete_selected(&mut self) {
        if let Some(id) = self.selected_download().map(|d| d.download.id) {
            let _ = api::delete_download(&self.api_base, id);
        }
    }
}
