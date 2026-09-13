use super::*;

impl App {
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

    pub fn pause_selected(&mut self) {
        let Some(download) = self.current_download() else {
            return;
        };
        if download.download.status != DownloadStatus::Active {
            return;
        }
        let id = download.download.id;
        let api_base = self.api_base.clone();
        let sender = self.event_sender.clone();
        thread::spawn(move || {
            if let Err(e) = api::pause_download(&api_base, id) {
                let _ = sender.send(Event::App(AppEvent::Toast {
                    message: e.to_string(),
                    level: ToastLevel::Error,
                }));
            }
        });
    }

    pub fn resume_selected(&mut self) {
        if let Some(id) = self.current_download().map(|d| d.download.id) {
            let api_base = self.api_base.clone();
            let sender = self.event_sender.clone();
            thread::spawn(move || {
                if let Err(e) = api::resume_download(&api_base, id) {
                    let _ = sender.send(Event::App(AppEvent::Toast {
                        message: e.to_string(),
                        level: ToastLevel::Error,
                    }));
                }
            });
        }
    }

    pub fn delete_selected(&mut self) {
        if let Some(id) = self.current_download().map(|d| d.download.id) {
            let api_base = self.api_base.clone();
            let sender = self.event_sender.clone();
            thread::spawn(move || {
                if let Err(e) = api::delete_download(&api_base, id) {
                    let _ = sender.send(Event::App(AppEvent::Toast {
                        message: e.to_string(),
                        level: ToastLevel::Error,
                    }));
                }
            });
        }
    }
}
