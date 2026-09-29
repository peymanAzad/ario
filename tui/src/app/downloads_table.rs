use super::*;
use crate::app::{PendingConfirmationAction, confirmation_modal::ConfirmationModal};
use crate::effects::{ApiRequest, Effect};

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
            .filter(|download| !self.pausing_downloads.contains(&download.download.id))
            .map(|download| download_action(&download.download.status))
    }

    pub(crate) fn is_download_pausing(&self, download_id: i64) -> bool {
        self.pausing_downloads.contains(&download_id)
    }

    pub fn pause_selected(&mut self) -> Vec<Effect> {
        if self.current_download_action() != Some(DownloadAction::Pause) {
            return vec![];
        }
        let Some(download) = self.current_download() else {
            return vec![];
        };
        let id = download.download.id;
        self.pausing_downloads.insert(id);
        vec![Effect::Api(ApiRequest::PauseDownload(id))]
    }

    pub fn resume_selected(&mut self) -> Vec<Effect> {
        let Some(action) = self.current_download_action() else {
            return vec![];
        };
        if action.key() != 'r' {
            return vec![];
        }
        let Some(id) = self.current_download().map(|d| d.download.id) else {
            return vec![];
        };
        vec![Effect::Api(ApiRequest::ResumeDownload(id))]
    }

    pub fn delete_selected(&mut self) -> Vec<Effect> {
        let Some(id) = self.current_download().map(|d| d.download.id) else {
            return vec![];
        };
        vec![Effect::Api(ApiRequest::DeleteDownload(id))]
    }

    pub fn request_delete_selected_files(&mut self) {
        if self.has_open_modal() {
            return;
        }
        let Some(download) = self.current_download() else {
            return;
        };
        let id = download.download.id;
        let name = download
            .download
            .filename
            .clone()
            .unwrap_or_else(|| download.download.url.clone());
        self.open_confirmation(
            ConfirmationModal::new(
                "Remove download and files?",
                format!(
                    "Remove \"{name}\", all downloaded data, and its aria2 control data from disk? This cannot be undone."
                ),
                "Remove",
                "Cancel",
            ),
            PendingConfirmationAction::DeleteDownloadFiles { download_id: id },
        );
    }

    pub fn delete_download_files(&mut self, download_id: i64) -> Vec<Effect> {
        vec![Effect::Api(ApiRequest::DeleteDownloadFiles(download_id))]
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

    fn app_with_status(status: DownloadStatus) -> App {
        let mut app = App::new(
            Theme::default_dark(),
            crate::icons::IconSet::new(crate::icons::GlyphMode::Unicode),
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
                completed_length: None,
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
        app
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
            let mut app = app_with_status(status);
            assert!(app.pause_selected().is_empty());
        }
    }

    #[test]
    fn pause_is_marked_pending_and_duplicate_requests_are_suppressed() {
        let mut app = app_with_status(DownloadStatus::Active);

        let effects = app.pause_selected();
        assert!(app.is_download_pausing(1));
        assert_eq!(app.current_download_action(), None);
        assert!(matches!(
            effects.as_slice(),
            [Effect::Api(ApiRequest::PauseDownload(1))]
        ));
        assert!(app.pause_selected().is_empty());
    }

    #[test]
    fn successful_pause_result_replaces_row_and_updates_totals() {
        let mut app = app_with_status(DownloadStatus::Active);
        app.pausing_downloads.insert(1);
        app.downloads[0].download_speed = 60;
        app.downloads[0].eta_seconds = Some(1);
        app.active_downloads = 1;
        app.total_download_speed = 60;
        app.smoothed_download_speed = Some(60);
        app.push_speed_sample(60);
        let mut paused = app.downloads[0].clone();
        paused.download.status = DownloadStatus::Paused;
        paused.download_speed = 0;
        paused.eta_seconds = None;

        app.apply_download_paused(1, Ok(paused));

        assert!(!app.is_download_pausing(1));
        assert_eq!(app.downloads[0].download.status, DownloadStatus::Paused);
        assert_eq!(app.downloads[0].download_speed, 0);
        assert_eq!(app.downloads[0].eta_seconds, None);
        assert_eq!(app.active_downloads, 0);
        assert_eq!(app.total_download_speed, 0);
        assert_eq!(app.displayed_download_speed(), 0);
        assert!(app.speed_history.is_empty());
    }

    #[test]
    fn accepted_pause_remains_pending_while_server_still_reports_active() {
        let mut app = app_with_status(DownloadStatus::Active);
        app.pausing_downloads.insert(1);
        app.downloads[0].download_speed = 60;
        app.active_downloads = 1;
        app.total_download_speed = 60;
        let active = app.downloads[0].clone();

        app.apply_download_paused(1, Ok(active));

        assert!(app.is_download_pausing(1));
        assert_eq!(app.current_download_action(), None);
        assert_eq!(app.active_downloads, 1);
        assert_eq!(app.total_download_speed, 60);
    }

    #[test]
    fn refresh_unlocks_resume_only_after_server_reports_paused() {
        let mut app = app_with_status(DownloadStatus::Active);
        app.pausing_downloads.insert(1);
        let mut paused = app.downloads[0].clone();
        paused.download.status = DownloadStatus::Paused;

        app.apply_refresh(
            Ok(vec![paused]),
            Err(anyhow::anyhow!("queues unavailable")),
            true,
            true,
            0,
            0,
            None,
            0,
        );

        assert!(!app.is_download_pausing(1));
        assert_eq!(app.current_download_action(), Some(DownloadAction::Resume));
    }

    #[test]
    fn failed_pause_result_restores_actions_and_reports_error() {
        let mut app = app_with_status(DownloadStatus::Active);
        app.pausing_downloads.insert(1);

        app.apply_download_paused(1, Err(anyhow::anyhow!("pause failed")));

        assert!(!app.is_download_pausing(1));
        assert_eq!(app.current_download_action(), Some(DownloadAction::Pause));
        let toast = app.toasts.iter().last().unwrap();
        assert_eq!(toast.level, ToastLevel::Error);
        assert!(toast.message.contains("pause failed"));
    }

    #[test]
    fn resume_does_not_issue_a_request_for_active_downloads() {
        let mut app = app_with_status(DownloadStatus::Active);
        assert!(app.resume_selected().is_empty());
    }

    #[test]
    fn destructive_delete_requires_confirmation() {
        let mut app = app_with_status(DownloadStatus::Completed);
        app.request_delete_selected_files();
        let modal = app.confirmation_modal.as_ref().unwrap();
        assert!(modal.title.contains("Remove"));
        assert!(modal.message.contains("cannot be undone"));

        app.cancel_confirmation();
        assert!(app.confirmation_modal.is_none());
    }

    #[test]
    fn destructive_delete_without_a_selection_is_a_no_op() {
        let mut app = App::new(
            Theme::default_dark(),
            crate::icons::IconSet::new(crate::icons::GlyphMode::Unicode),
            false,
        );
        app.request_delete_selected_files();
        assert!(app.confirmation_modal.is_none());
    }
}
