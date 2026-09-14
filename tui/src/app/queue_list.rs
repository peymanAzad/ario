use super::*;
use crate::app::{PendingConfirmationAction, confirmation_modal::ConfirmationModal};

const MAIN_QUEUE_ID: i64 = 1;

impl App {
    pub fn select_next_queue(&mut self) {
        let len = self.queues.len() + 1; // +1 for "All"
        self.selected_queue = (self.selected_queue + 1).min(len - 1);
        self.refresh();
    }

    pub fn select_prev_queue(&mut self) {
        self.selected_queue = self.selected_queue.saturating_sub(1);
        self.refresh();
    }

    pub fn current_queue(&self) -> Option<&Queue> {
        self.queues.get(self.selected_queue.saturating_sub(1))
    }

    pub fn can_delete_selected_queue(&self) -> bool {
        self.current_queue()
            .is_some_and(|queue| queue.id != MAIN_QUEUE_ID)
    }

    pub fn request_delete_selected_queue(&mut self) {
        if !self.can_delete_selected_queue()
            || self.confirmation_modal.is_some()
            || self.modal.is_some()
            || self.queue_modal.is_some()
            || self.download_modal.is_some()
        {
            return;
        }

        if let Some(queue) = self.current_queue() {
            self.delete_queue(queue.id, queue.name.clone(), false);
        }
    }

    pub fn confirm_delete_queue(&mut self, queue_id: i64) {
        let Some(queue_name) = self
            .queues
            .iter()
            .find(|queue| queue.id == queue_id)
            .map(|queue| queue.name.clone())
        else {
            return;
        };
        self.delete_queue(queue_id, queue_name, true);
    }

    fn delete_queue(&self, queue_id: i64, queue_name: String, delete_downloads: bool) {
        let api_base = self.api_base.clone();
        let sender = self.event_sender.clone();
        thread::spawn(move || {
            let result = api::delete_queue(&api_base, queue_id, delete_downloads);
            let _ = sender.send(Event::App(AppEvent::QueueDeleteResolved {
                queue_id,
                queue_name,
                result,
            }));
        });
    }

    pub fn apply_queue_delete_result(
        &mut self,
        queue_id: i64,
        queue_name: String,
        result: anyhow::Result<api::DeleteQueueOutcome>,
    ) {
        match result {
            Ok(api::DeleteQueueOutcome::NeedsConfirmation) => {
                if !self.queues.iter().any(|queue| queue.id == queue_id)
                    || self.confirmation_modal.is_some()
                    || self.modal.is_some()
                    || self.queue_modal.is_some()
                    || self.download_modal.is_some()
                {
                    return;
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
            }
            Ok(api::DeleteQueueOutcome::Deleted) => {
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
                self.refresh();
            }
            Err(error) => self.toasts.push(error.to_string(), ToastLevel::Error),
        }
    }

    pub fn resume_selected_queue(&mut self) {
        if self.selected_queue == 0 {
            return;
        }
        if let Some(id) = self.current_queue().map(|d| d.id) {
            let api_base = self.api_base.clone();
            let sender = self.event_sender.clone();
            thread::spawn(move || {
                if let Err(e) = api::resume_queue(&api_base, id) {
                    let _ = sender.send(Event::App(AppEvent::Toast {
                        message: e.to_string(),
                        level: ToastLevel::Error,
                    }));
                }
            });
        }
    }

    pub fn pause_selected_queue(&mut self) {
        if self.selected_queue == 0 {
            return;
        }
        if let Some(id) = self.current_queue().map(|d| d.id) {
            let api_base = self.api_base.clone();
            let sender = self.event_sender.clone();
            thread::spawn(move || {
                if let Err(e) = api::pause_queue(&api_base, id) {
                    let _ = sender.send(Event::App(AppEvent::Toast {
                        message: e.to_string(),
                        level: ToastLevel::Error,
                    }));
                }
            });
        }
    }

    pub fn remove_completed_downloads(&mut self) {
        let queue_id = if self.selected_queue == 0 {
            None
        } else {
            self.current_queue().map(|queue| queue.id)
        };

        // A stale queue selection cannot safely be interpreted as "All".
        if self.selected_queue != 0 && queue_id.is_none() {
            return;
        }

        let api_base = self.api_base.clone();
        let sender = self.event_sender.clone();
        thread::spawn(move || {
            if let Err(e) = api::delete_completed_downloads(&api_base, queue_id) {
                let _ = sender.send(Event::App(AppEvent::Toast {
                    message: e.to_string(),
                    level: ToastLevel::Error,
                }));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{event::Event, theme::Theme};
    use chrono::Utc;
    use common::{
        enums::{QueueStatus, Recurrence},
        finetune::FineTune,
        queue::QueueSettings,
        scheduler::Scheduler,
    };
    use std::{sync::mpsc, time::Duration};

    fn queue(id: i64, name: &str) -> Queue {
        Queue {
            id,
            name: name.into(),
            position: id as i32,
            settings: QueueSettings {
                max_concurrent_downloads: 1,
                max_retries: 3,
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

    fn app() -> (App, mpsc::Receiver<Event>) {
        let (sender, receiver) = mpsc::channel();
        let mut app = App::new(
            "http://127.0.0.1:1".into(),
            Theme::default_dark(),
            sender,
            false,
        );
        app.queues = vec![
            queue(1, "Main Queue"),
            queue(2, "Second"),
            queue(3, "Third"),
        ];
        (app, receiver)
    }

    #[test]
    fn all_and_main_queue_are_not_deletable() {
        let (mut app, receiver) = app();
        assert!(!app.can_delete_selected_queue());
        app.request_delete_selected_queue();

        app.selected_queue = 1;
        assert!(!app.can_delete_selected_queue());
        app.request_delete_selected_queue();

        assert!(receiver.recv_timeout(Duration::from_millis(50)).is_err());
    }

    #[test]
    fn populated_queue_result_opens_confirmation_with_retention_copy() {
        let (mut app, _receiver) = app();
        app.apply_queue_delete_result(
            2,
            "Second".into(),
            Ok(api::DeleteQueueOutcome::NeedsConfirmation),
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
    fn successful_delete_selects_the_next_queue_or_previous_at_end() {
        let (mut app, _receiver) = app();
        app.selected_queue = 2;
        app.apply_queue_delete_result(2, "Second".into(), Ok(api::DeleteQueueOutcome::Deleted));
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
        app.apply_queue_delete_result(3, "Third".into(), Ok(api::DeleteQueueOutcome::Deleted));
        assert_eq!(app.current_queue().map(|queue| queue.id), Some(1));
    }

    #[test]
    fn delete_error_is_shown_as_a_toast() {
        let (mut app, _receiver) = app();
        app.apply_queue_delete_result(2, "Second".into(), Err(anyhow::anyhow!("boom")));
        assert!(
            app.toasts
                .iter()
                .any(|toast| toast.level == ToastLevel::Error && toast.message == "boom")
        );
    }
}
