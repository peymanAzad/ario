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
mod tests {
    use super::*;
    use crate::theme::Theme;
    use chrono::Utc;
    use common::{
        enums::{QueueStatus, Recurrence},
        finetune::FineTune,
        queue::QueueSettings,
        scheduler::Scheduler,
    };

    fn queue(id: i64, name: &str) -> Queue {
        Queue {
            scheduled_stop_at: None,
            id,
            name: name.into(),
            position: id as i32,
            settings: QueueSettings {
                max_concurrent_downloads: 1,
                max_retries: 3,
                retry_wait_seconds: 5,
                default_finetune: FineTune::default(),
            },
            scheduler: Scheduler {
                enabled: false,
                recurrence: Recurrence::Once {
                    start: Utc::now(),
                    end: Utc::now(),
                },
                run_missed_on_startup: false,
            },
            status: QueueStatus::Paused,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn start_message_includes_local_stop_date_and_time() {
        let stop = chrono::DateTime::parse_from_rfc3339("2030-01-07T17:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let message = queue_start_message(Some(stop));
        assert!(
            message.contains(
                &stop
                    .with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M")
                    .to_string()
            )
        );
        assert!(message.starts_with("Queue started. Will pause at "));
        assert_eq!(queue_start_message(None), "Queue started");
    }

    fn app() -> App {
        let mut app = App::new(
            Theme::default_dark(),
            crate::icons::IconSet::new(crate::icons::GlyphMode::Unicode),
            false,
        );
        app.queues = vec![
            queue(1, "Main Queue"),
            queue(2, "Second"),
            queue(3, "Third"),
        ];
        app
    }

    #[test]
    fn all_and_main_queue_are_not_deletable() {
        let mut app = app();
        assert!(!app.can_delete_selected_queue());
        assert!(app.request_delete_selected_queue().is_empty());

        app.selected_queue = 1;
        assert!(!app.can_delete_selected_queue());
        assert!(app.request_delete_selected_queue().is_empty());
    }

    #[test]
    fn populated_queue_result_opens_confirmation_with_retention_copy() {
        let mut app = app();
        let _ = app.apply_queue_delete_result(
            2,
            "Second".into(),
            Ok(crate::api::DeleteQueueOutcome::NeedsConfirmation),
        );

        let modal = app.confirmation_modal.as_ref().unwrap();
        assert_eq!(modal.title, "Remove queue?");
        assert!(modal.message.contains("Second"));
        assert!(modal.message.contains("Downloaded files will be kept"));
        assert_eq!(
            app.pending_confirmation_action,
            Some(PendingConfirmationAction::DeleteQueue { queue_id: 2 })
        );

        app.cancel_confirmation();
        assert!(app.confirmation_modal.is_none());
        assert_eq!(app.queues.len(), 3);
    }

    #[test]
    fn late_queue_confirmation_does_not_replace_help() {
        let mut app = app();
        app.open_help_modal();
        let _ = app.apply_queue_delete_result(
            2,
            "Second".into(),
            Ok(crate::api::DeleteQueueOutcome::NeedsConfirmation),
        );
        assert!(app.help_modal.is_some());
        assert!(app.confirmation_modal.is_none());
        assert!(app.pending_confirmation_action.is_none());
    }

    #[test]
    fn successful_delete_selects_the_next_queue_or_previous_at_end() {
        let mut app = app();
        app.selected_queue = 2;
        let _ = app.apply_queue_delete_result(
            2,
            "Second".into(),
            Ok(crate::api::DeleteQueueOutcome::Deleted),
        );
        assert_eq!(
            app.queues.iter().map(|queue| queue.id).collect::<Vec<_>>(),
            vec![1, 3]
        );
        assert_eq!(app.current_queue().map(|queue| queue.id), Some(3));
        assert!(app.toasts.iter().any(|toast| {
            toast.level == ToastLevel::Success && toast.message.contains("Second")
        }));

        // Avoid a second refresh while the first test refresh is in flight.
        app.selected_queue = 2;
        let _ = app.apply_queue_delete_result(
            3,
            "Third".into(),
            Ok(crate::api::DeleteQueueOutcome::Deleted),
        );
        assert_eq!(app.current_queue().map(|queue| queue.id), Some(1));
    }

    #[test]
    fn delete_error_is_shown_as_a_toast() {
        let mut app = app();
        let _ = app.apply_queue_delete_result(2, "Second".into(), Err(anyhow::anyhow!("boom")));
        assert!(
            app.toasts
                .iter()
                .any(|toast| toast.level == ToastLevel::Error && toast.message == "boom")
        );
    }
}
