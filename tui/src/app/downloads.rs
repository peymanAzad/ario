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
#[path = "downloads_tests.rs"]
mod tests;
