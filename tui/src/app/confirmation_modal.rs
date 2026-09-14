use super::{App, PendingConfirmationAction};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfirmationModal {
    pub title: String,
    pub message: String,
    pub confirm_label: String,
    pub cancel_label: String,
}

impl ConfirmationModal {
    pub fn new(
        title: impl Into<String>,
        message: impl Into<String>,
        confirm_label: impl Into<String>,
        cancel_label: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            confirm_label: confirm_label.into(),
            cancel_label: cancel_label.into(),
        }
    }
}

impl App {
    pub fn open_confirmation(
        &mut self,
        modal: ConfirmationModal,
        action: PendingConfirmationAction,
    ) {
        self.confirmation_modal = Some(modal);
        self.pending_confirmation_action = Some(action);
    }

    pub fn cancel_confirmation(&mut self) {
        self.confirmation_modal = None;
        self.pending_confirmation_action = None;
    }

    pub fn confirm_confirmation(&mut self) {
        let action = self.pending_confirmation_action.take();
        self.confirmation_modal = None;
        match action {
            Some(PendingConfirmationAction::DeleteDownloadFiles { download_id }) => {
                self.delete_download_files(download_id)
            }
            Some(PendingConfirmationAction::DeleteQueue { queue_id }) => {
                self.confirm_delete_queue(queue_id)
            }
            None => {}
        }
    }
}
