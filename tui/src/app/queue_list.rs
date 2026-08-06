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
}
