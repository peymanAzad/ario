use super::*;

impl App {
    pub fn select_next_category(&mut self) {
        let len = ALL_CATEGORIES.len() + 1; // +1 for "All"
        self.selected_category = (self.selected_category + 1).min(len - 1);
        self.refresh();
    }

    pub fn select_prev_category(&mut self) {
        self.selected_category = self.selected_category.saturating_sub(1);
        self.refresh();
    }
}
