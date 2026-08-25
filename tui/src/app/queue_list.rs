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
                    let _ = sender.send(Event::App(AppEvent::ActionFailed(e.to_string())));
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
                    let _ = sender.send(Event::App(AppEvent::ActionFailed(e.to_string())));
                }
            });
        }
    }
}
