use super::*;
use crate::effects::Effect;

impl App {
    pub fn select_next_category(&mut self) -> Vec<Effect> {
        let len = ALL_CATEGORIES.len() + 1; // +1 for "All"
        self.selected_category = (self.selected_category + 1).min(len - 1);
        self.refresh()
    }

    pub fn select_prev_category(&mut self) -> Vec<Effect> {
        self.selected_category = self.selected_category.saturating_sub(1);
        self.refresh()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        icons::{GlyphMode, IconSet},
        theme::Theme,
    };

    #[test]
    fn program_category_is_available_as_a_filter() {
        let mut app = App::new(Theme::default_dark(), IconSet::new(GlyphMode::Unicode), false);
        app.selected_category = ALL_CATEGORIES
            .iter()
            .position(|category| *category == FileCategory::Program)
            .unwrap()
            + 1;

        assert_eq!(app.current_filter().category, Some(FileCategory::Program));
    }
}
