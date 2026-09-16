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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        icons::{GlyphMode, IconSet},
        theme::Theme,
    };
    use std::sync::mpsc;

    #[test]
    fn program_category_is_available_as_a_filter() {
        let (sender, _receiver) = mpsc::channel();
        let mut app = App::new(
            "http://127.0.0.1:1".into(),
            Theme::default_dark(),
            IconSet::new(GlyphMode::Unicode),
            sender,
            false,
        );
        app.selected_category = ALL_CATEGORIES
            .iter()
            .position(|category| *category == FileCategory::Program)
            .unwrap()
            + 1;

        assert_eq!(app.current_filter().category, Some(FileCategory::Program));
    }
}
