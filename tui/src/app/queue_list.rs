use super::*;

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
