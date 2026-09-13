use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DownloadAction {
    Start,
    Resume,
    Pause,
    Retry,
    Restart,
}

impl DownloadAction {
    pub fn key(self) -> char {
        match self {
            Self::Pause => 'p',
            Self::Start | Self::Resume | Self::Retry | Self::Restart => 'r',
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::Start => "   r: start",
            Self::Resume => "   r: resume",
            Self::Pause => "   p: pause",
            Self::Retry => "   r: retry",
            Self::Restart => "   r: restart",
        }
    }
}

pub fn download_action(status: &DownloadStatus) -> DownloadAction {
    match status {
        DownloadStatus::Pending => DownloadAction::Start,
        DownloadStatus::Paused => DownloadAction::Resume,
        DownloadStatus::Active => DownloadAction::Pause,
        DownloadStatus::Error(_) | DownloadStatus::Removed => DownloadAction::Retry,
        DownloadStatus::Completed => DownloadAction::Restart,
    }
}

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

    pub fn current_download_action(&self) -> Option<DownloadAction> {
        self.current_download()
            .map(|download| download_action(&download.download.status))
    }

    pub fn pause_selected(&mut self) {
        if self.current_download_action() != Some(DownloadAction::Pause) {
            return;
        }
        let Some(download) = self.current_download() else {
            return;
        };
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
        let Some(action) = self.current_download_action() else {
            return;
        };
        if action.key() != 'r' {
            return;
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use common::{
        download::Download,
        enums::{FileCategory, SourceType},
        finetune::FineTune,
    };
    use std::{sync::mpsc, time::Duration};

    fn app_with_status(status: DownloadStatus) -> (App, mpsc::Receiver<Event>) {
        let (sender, receiver) = mpsc::channel();
        let mut app = App::new(
            "http://127.0.0.1:1".into(),
            Theme::default_dark(),
            sender,
            false,
        );
        app.downloads.push(DownloadLiveStatus {
            download: Download {
                id: 1,
                aria2_gid: Some("gid".into()),
                url: "https://example.test/file".into(),
                filename: Some("file".into()),
                destination_path: "/tmp".into(),
                source_type: SourceType::Http,
                category: FileCategory::Other,
                status,
                paused_by_scheduler: false,
                manually_started: false,
                size: None,
                queue_id: 1,
                position_in_queue: 0,
                finetune: FineTune::default(),
                created_at: Utc::now(),
                started_at: None,
                completed_at: None,
            },
            completed_length: 0,
            download_speed: 0,
            eta_seconds: None,
        });
        (app, receiver)
    }

    #[test]
    fn download_actions_match_every_status() {
        assert_eq!(
            download_action(&DownloadStatus::Pending),
            DownloadAction::Start
        );
        assert_eq!(
            download_action(&DownloadStatus::Paused),
            DownloadAction::Resume
        );
        assert_eq!(
            download_action(&DownloadStatus::Active),
            DownloadAction::Pause
        );
        assert_eq!(
            download_action(&DownloadStatus::Error("failed".into())),
            DownloadAction::Retry
        );
        assert_eq!(
            download_action(&DownloadStatus::Completed),
            DownloadAction::Restart
        );
        assert_eq!(
            download_action(&DownloadStatus::Removed),
            DownloadAction::Retry
        );
    }

    #[test]
    fn pause_does_not_issue_a_request_for_non_active_downloads() {
        for status in [
            DownloadStatus::Pending,
            DownloadStatus::Paused,
            DownloadStatus::Error("failed".into()),
            DownloadStatus::Completed,
            DownloadStatus::Removed,
        ] {
            let (mut app, receiver) = app_with_status(status);
            app.pause_selected();
            assert!(receiver.recv_timeout(Duration::from_millis(50)).is_err());
        }
    }

    #[test]
    fn resume_does_not_issue_a_request_for_active_downloads() {
        let (mut app, receiver) = app_with_status(DownloadStatus::Active);
        app.resume_selected();
        assert!(receiver.recv_timeout(Duration::from_millis(50)).is_err());
    }
}
