use super::App;

pub struct KeybindingSection {
    pub title: &'static str,
    pub bindings: &'static [(&'static str, &'static str)],
}

/// Reference for the context-sensitive handlers in update.rs. Aliases share a row.
pub const KEYBINDINGS: &[KeybindingSection] = &[
    KeybindingSection {
        title: "Main Navigation",
        bindings: &[
            ("?", "Open keybindings help (main screen only)"),
            ("1 / 2 / 3", "Focus Queues / Categories / Downloads"),
            ("Tab / Shift+Tab", "Focus next / previous pane"),
            ("v", "Import download URLs from clipboard"),
            ("q / Esc", "Quit from the main screen"),
            ("Ctrl+C", "Quit, except in confirmations where it cancels"),
        ],
    },
    KeybindingSection {
        title: "Queues",
        bindings: &[
            ("j / Down", "Select next queue"),
            ("k / Up", "Select previous queue"),
            ("n", "Create a queue"),
            ("Enter", "Edit selected queue (except All)"),
            ("p / r", "Pause / resume selected queue (except All)"),
            (
                "x",
                "Remove completed downloads from selected queue, or all queues in All",
            ),
            (
                "d",
                "Delete selected queue (except All and Main Queue); confirm if it contains downloads",
            ),
        ],
    },
    KeybindingSection {
        title: "Categories",
        bindings: &[
            ("j / Down", "Select next category"),
            ("k / Up", "Select previous category"),
        ],
    },
    KeybindingSection {
        title: "Downloads",
        bindings: &[
            ("j / Down", "Select next download"),
            ("k / Up", "Select previous download"),
            ("Enter", "Open completed file; edit other downloads"),
            ("f", "Open folder of completed download"),
            ("p", "Pause active download"),
            (
                "r",
                "Start pending, resume paused, retry failed/removed, or restart completed download",
            ),
            ("d", "Delete download, keeping downloaded files"),
            ("D", "Delete download and files after confirmation"),
        ],
    },
    KeybindingSection {
        title: "Clipboard Import",
        bindings: &[
            (
                "Tab / Shift+Tab",
                "Switch between URLs and Fine Tuning tabs",
            ),
            ("j / Down", "Select next URL or field"),
            ("k / Up", "Select previous URL or field"),
            ("h / Left", "Choose previous queue or decrease field value"),
            ("l / Right", "Choose next queue or increase field value"),
            ("Space", "Toggle selected URL (URLs tab)"),
            ("a / n", "Select all / none (URLs tab)"),
            ("s", "Start selected downloads now"),
            ("w", "Save selected downloads for later"),
            ("c / Esc", "Cancel import"),
        ],
    },
    KeybindingSection {
        title: "Queue Editor",
        bindings: &[
            ("Tab / Shift+Tab", "Switch to next / previous tab"),
            ("j / Down", "Select next field or download item"),
            ("k / Up", "Select previous field or download item"),
            ("h / Left", "Decrease value or select previous weekday"),
            ("l / Right", "Increase value or select next weekday"),
            ("Enter", "Edit queue name or a Once schedule date field"),
            (
                "Space",
                "Toggle highlighted weekday (Scheduler weekly days row)",
            ),
            (
                "J / K",
                "Move selected download item down / up (Download Items tab)",
            ),
            ("s", "Save queue (Common or Scheduler tab)"),
            ("c / Esc", "Cancel queue editing"),
        ],
    },
    KeybindingSection {
        title: "Text Editing",
        bindings: &[
            ("Characters", "Type queue name or Once schedule date"),
            ("Backspace", "Delete last character"),
            ("Enter", "Accept text edit"),
            ("Esc", "Discard text edit and return to queue editor"),
        ],
    },
    KeybindingSection {
        title: "Download Editor",
        bindings: &[
            ("j / Down", "Select next field"),
            ("k / Up", "Select previous field"),
            ("h / Left", "Decrease value or choose previous queue"),
            ("l / Right", "Increase value or choose next queue"),
            ("s", "Save download settings"),
            ("c / Esc", "Cancel download editing"),
        ],
    },
    KeybindingSection {
        title: "Confirmations",
        bindings: &[
            ("Enter / y / Y", "Confirm action"),
            ("Esc / n / N / c / C", "Cancel action (also Ctrl+C)"),
        ],
    },
    KeybindingSection {
        title: "Help",
        bindings: &[
            ("j / Down", "Scroll down (outside search editing)"),
            ("k / Up", "Scroll up (outside search editing)"),
            ("PageDown / PageUp", "Scroll down / up one page"),
            ("Home / End", "Jump to beginning / end"),
            (
                "/",
                "Edit search; filter keys, descriptions, and section titles as you type",
            ),
            ("Characters", "Type search query while search editing"),
            ("Backspace", "Delete last search character"),
            ("Enter", "Finish search editing and keep filter"),
            (
                "Esc",
                "Clear filter and exit search editing; otherwise close help",
            ),
            ("q / ?", "Close help (outside search editing)"),
        ],
    },
];

pub fn filtered_sections(query: &str) -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
    let query = query.to_lowercase();
    KEYBINDINGS
        .iter()
        .filter_map(|section| {
            let section_matches = section.title.to_lowercase().contains(&query);
            let bindings: Vec<_> = section
                .bindings
                .iter()
                .copied()
                .filter(|(keys, description)| {
                    section_matches
                        || keys.to_lowercase().contains(&query)
                        || description.to_lowercase().contains(&query)
                })
                .collect();
            (!bindings.is_empty()).then_some((section.title, bindings))
        })
        .collect()
}

#[derive(Default)]
pub struct HelpModal {
    pub query: String,
    pub editing_search: bool,
    pub scroll: usize,
    pub viewport_height: usize,
    content_height: usize,
}

impl HelpModal {
    pub fn set_dimensions(&mut self, content_height: usize, viewport_height: usize) {
        self.content_height = content_height;
        self.viewport_height = viewport_height;
        self.scroll = self.scroll.min(self.max_scroll());
    }

    pub fn max_scroll(&self) -> usize {
        self.content_height.saturating_sub(self.viewport_height)
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll = self.scroll.saturating_add(lines).min(self.max_scroll());
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.scroll = self.scroll.saturating_sub(lines);
    }
}

impl App {
    pub fn open_help_modal(&mut self) {
        if !self.has_open_modal() {
            self.help_modal = Some(HelpModal::default());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_matches_keys_descriptions_and_whole_sections() {
        assert_eq!(filtered_sections("").len(), KEYBINDINGS.len());
        let sections = filtered_sections("cAtEgOrIeS");
        let category = sections
            .iter()
            .find(|(title, _)| *title == "Categories")
            .unwrap();
        assert_eq!(category.1, KEYBINDINGS[2].bindings);
        assert!(
            filtered_sections("backspace")
                .iter()
                .all(|(_, bindings)| { bindings.iter().all(|(keys, _)| *keys == "Backspace") })
        );
        assert_eq!(filtered_sections("keeping downloaded files")[0].1[0].0, "d");
        assert!(filtered_sections("nonexistent shortcut").is_empty());
    }

    #[test]
    fn scrolling_clamps_at_boundaries_and_after_layout_changes() {
        let mut modal = HelpModal::default();
        modal.set_dimensions(100, 10);
        modal.scroll_down(usize::MAX);
        assert_eq!(modal.scroll, 90);
        modal.set_dimensions(100, 30);
        assert_eq!(modal.scroll, 70);
        modal.set_dimensions(3, 30);
        assert_eq!(modal.scroll, 0);
        modal.scroll_up(usize::MAX);
        assert_eq!(modal.scroll, 0);
    }
}
