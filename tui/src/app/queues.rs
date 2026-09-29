use super::*;
use crate::app::{PendingConfirmationAction, confirmation_modal::ConfirmationModal};
use crate::effects::{ApiRequest, Effect};

const MAIN_QUEUE_ID: i64 = 1;

pub(crate) fn queue_start_message(stop: Option<chrono::DateTime<chrono::Utc>>) -> String {
    stop.map(|stop| {
        format!(
            "Queue started. Will pause at {}",
            stop.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
        )
    })
    .unwrap_or_else(|| "Queue started".into())
}

impl App {
    pub fn select_next_queue(&mut self) -> Vec<Effect> {
        let len = self.queues.len() + 1; // +1 for "All"
        self.selected_queue = (self.selected_queue + 1).min(len - 1);
        self.refresh()
    }

    pub fn select_prev_queue(&mut self) -> Vec<Effect> {
        self.selected_queue = self.selected_queue.saturating_sub(1);
        self.refresh()
    }

    pub fn current_queue(&self) -> Option<&Queue> {
        self.queues.get(self.selected_queue.saturating_sub(1))
    }

    pub fn can_delete_selected_queue(&self) -> bool {
        self.current_queue()
            .is_some_and(|queue| queue.id != MAIN_QUEUE_ID)
    }

    pub fn request_delete_selected_queue(&mut self) -> Vec<Effect> {
        if !self.can_delete_selected_queue() || self.has_open_modal() {
            return vec![];
        }

        if let Some(queue) = self.current_queue() {
            return self.delete_queue(queue.id, queue.name.clone(), false);
        }
        vec![]
    }

    pub fn confirm_delete_queue(&mut self, queue_id: i64) -> Vec<Effect> {
        let Some(queue_name) = self
            .queues
            .iter()
            .find(|queue| queue.id == queue_id)
            .map(|queue| queue.name.clone())
        else {
            return vec![];
        };
        self.delete_queue(queue_id, queue_name, true)
    }

    fn delete_queue(
        &self,
        queue_id: i64,
        queue_name: String,
        delete_downloads: bool,
    ) -> Vec<Effect> {
        vec![Effect::Api(ApiRequest::DeleteQueue {
            id: queue_id,
            name: queue_name,
            delete_downloads,
        })]
    }

    pub fn apply_queue_delete_result(
        &mut self,
        queue_id: i64,
        queue_name: String,
        result: anyhow::Result<crate::api::DeleteQueueOutcome>,
    ) -> Vec<Effect> {
        match result {
            Ok(crate::api::DeleteQueueOutcome::NeedsConfirmation) => {
                if !self.queues.iter().any(|queue| queue.id == queue_id) || self.has_open_modal() {
                    return vec![];
                }
                self.open_confirmation(
                    ConfirmationModal::new(
                        "Remove queue?",
                        format!(
                            "Remove \"{queue_name}\" and all of its download items? Downloaded files will be kept."
                        ),
                        "Remove",
                        "Cancel",
                    ),
                    PendingConfirmationAction::DeleteQueue { queue_id },
                );
                vec![]
            }
            Ok(crate::api::DeleteQueueOutcome::Deleted) => {
                let selected_id = self.current_queue().map(|queue| queue.id);
                let deleted_row = self
                    .queues
                    .iter()
                    .position(|queue| queue.id == queue_id)
                    .map(|index| index + 1);
                self.queues.retain(|queue| queue.id != queue_id);

                self.selected_queue = if selected_id == Some(queue_id) {
                    deleted_row.unwrap_or(0).min(self.queues.len())
                } else {
                    selected_id
                        .and_then(|id| self.queues.iter().position(|queue| queue.id == id))
                        .map(|index| index + 1)
                        .unwrap_or(0)
                };

                self.toasts.push(
                    format!("queue \"{queue_name}\" removed"),
                    ToastLevel::Success,
                );
                self.refresh()
            }
            Err(error) => {
                self.toasts.push(error.to_string(), ToastLevel::Error);
                vec![]
            }
        }
    }

    pub fn resume_selected_queue(&mut self) -> Vec<Effect> {
        if self.selected_queue == 0 {
            return vec![];
        }
        let Some(id) = self.current_queue().map(|d| d.id) else {
            return vec![];
        };
        vec![Effect::Api(ApiRequest::ResumeQueue(id))]
    }

    pub fn pause_selected_queue(&mut self) -> Vec<Effect> {
        if self.selected_queue == 0 {
            return vec![];
        }
        let Some(id) = self.current_queue().map(|d| d.id) else {
            return vec![];
        };
        vec![Effect::Api(ApiRequest::PauseQueue(id))]
    }

    pub fn remove_completed_downloads(&mut self) -> Vec<Effect> {
        let queue_id = if self.selected_queue == 0 {
            None
        } else {
            self.current_queue().map(|queue| queue.id)
        };

        // A stale queue selection cannot safely be interpreted as "All".
        if self.selected_queue != 0 && queue_id.is_none() {
            return vec![];
        }

        vec![Effect::Api(ApiRequest::DeleteCompleted(queue_id))]
    }
}

#[cfg(test)]
#[path = "queues_tests.rs"]
mod tests;
