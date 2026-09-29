use super::{App, PendingConfirmationAction};
use crate::effects::Effect;
use crate::modal::Modal;

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
        self.modal = Some(Modal::Confirmation { modal, action });
    }

    pub fn cancel_confirmation(&mut self) {
        if matches!(self.modal, Some(Modal::Confirmation { .. })) {
            self.modal = None;
        }
    }

    pub fn take_confirm_action(&mut self) -> Option<PendingConfirmationAction> {
        match self.modal.take() {
            Some(Modal::Confirmation { action, .. }) => Some(action),
            other => {
                self.modal = other;
                None
            }
        }
    }

    pub fn execute_confirmation(&mut self, action: PendingConfirmationAction) -> Vec<Effect> {
        match action {
            PendingConfirmationAction::DeleteDownloadFiles { download_id } => {
                self.delete_download_files(download_id)
            }
            PendingConfirmationAction::DeleteQueue { queue_id } => {
                self.confirm_delete_queue(queue_id)
            }
        }
    }

    #[allow(dead_code)]
    pub fn confirm_confirmation(&mut self) -> Vec<Effect> {
        match self.take_confirm_action() {
            Some(action) => self.execute_confirmation(action),
            None => vec![],
        }
    }
}
