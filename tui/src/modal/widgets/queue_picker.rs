use common::queue::Queue;

#[derive(Debug, Clone)]
pub struct QueuePicker {
    pub cursor: usize,
}

impl QueuePicker {
    /// Prefer `selected_queue - 1` when valid, else Main Queue by name, else 0.
    pub fn default_for(selected_queue: usize, queues: &[Queue]) -> Self {
        let cursor = selected_queue
            .checked_sub(1)
            .filter(|&queue_cursor| queue_cursor < queues.len())
            .or_else(|| queues.iter().position(|queue| queue.name == "Main Queue"))
            .unwrap_or(0);
        Self { cursor }
    }

    pub fn move_by(&mut self, forward: bool, len: usize) {
        if len == 0 {
            return;
        }
        if forward {
            self.cursor = (self.cursor + 1).min(len - 1);
        } else {
            self.cursor = self.cursor.saturating_sub(1);
        }
    }

    pub fn selected_id(&self, queues: &[Queue]) -> Option<i64> {
        queues.get(self.cursor).map(|queue| queue.id)
    }
}

#[cfg(test)]
mod tests {
    use super::QueuePicker;
    use chrono::Utc;
    use common::{
        enums::{QueueStatus, Recurrence},
        finetune::FineTune,
        queue::{Queue, QueueSettings},
        scheduler::Scheduler,
    };

    fn queue(name: &str, id: i64) -> Queue {
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
    fn defaults_to_the_current_queue() {
        let queues = vec![queue("A", 1), queue("B", 2), queue("C", 3), queue("D", 4)];
        assert_eq!(QueuePicker::default_for(3, &queues).cursor, 2);
    }

    #[test]
    fn all_queues_selection_defaults_to_main_queue() {
        let queues = vec![
            queue("A", 1),
            queue("B", 2),
            queue("Main Queue", 3),
            queue("D", 4),
        ];
        assert_eq!(QueuePicker::default_for(0, &queues).cursor, 2);
    }

    #[test]
    fn missing_main_queue_defaults_to_the_first_queue() {
        let queues = vec![queue("A", 1), queue("B", 2), queue("C", 3), queue("D", 4)];
        assert_eq!(QueuePicker::default_for(0, &queues).cursor, 0);
    }

    #[test]
    fn empty_and_stale_queue_selections_are_safe() {
        assert_eq!(QueuePicker::default_for(0, &[]).cursor, 0);
        let queues = vec![queue("A", 1), queue("Main Queue", 2)];
        assert_eq!(QueuePicker::default_for(5, &queues).cursor, 1);
        let queues = vec![queue("A", 1), queue("B", 2)];
        assert_eq!(QueuePicker::default_for(5, &queues).cursor, 0);
    }
}
