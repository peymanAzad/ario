use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Frame, layout::Rect};
use unicode_segmentation::UnicodeSegmentation;

use crate::app::App;
use crate::keymap;
use crate::modal::clipboard_import::HELP as CLIPBOARD_HELP;
use crate::modal::confirmation::HELP as CONFIRMATION_HELP;
use crate::modal::download_edit::HELP as DOWNLOAD_EDIT_HELP;
use crate::modal::queue::{HELP as QUEUE_HELP, HELP_TEXT_EDITING};
use crate::modal::{Component, Ctx, Modal, ModalOutcome};

mod view;

pub use view::wrap_text;

pub struct KeybindingSection {
    pub title: &'static str,
    pub bindings: &'static [(&'static str, &'static str)],
}

impl Clone for KeybindingSection {
    fn clone(&self) -> Self {
        *self
    }
}

impl Copy for KeybindingSection {}

/// Help section for the help modal itself.
pub const HELP: &[(&str, &str)] = &[
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
];

/// Modal help sections composed onto the BINDINGS-derived main-screen sections.
const MODAL_SECTIONS: &[KeybindingSection] = &[
    KeybindingSection {
        title: "Clipboard Import",
        bindings: CLIPBOARD_HELP,
    },
    KeybindingSection {
        title: "Queue Editor",
        bindings: QUEUE_HELP,
    },
    KeybindingSection {
        title: "Text Editing",
        bindings: HELP_TEXT_EDITING,
    },
    KeybindingSection {
        title: "Download Editor",
        bindings: DOWNLOAD_EDIT_HELP,
    },
    KeybindingSection {
        title: "Confirmations",
        bindings: CONFIRMATION_HELP,
    },
    KeybindingSection {
        title: "Help",
        bindings: HELP,
    },
];

/// All help sections (main-screen from keymap BINDINGS + modal HELP constants).
fn all_sections() -> &'static [KeybindingSection] {
    use std::sync::OnceLock;
    static SECTIONS: OnceLock<Vec<KeybindingSection>> = OnceLock::new();
    SECTIONS.get_or_init(|| {
        let mut sections: Vec<KeybindingSection> = keymap::main_help_sections()
            .into_iter()
            .map(|(title, bindings)| KeybindingSection {
                title,
                bindings: Box::leak(bindings.into_boxed_slice()),
            })
            .collect();
        sections.extend(MODAL_SECTIONS.iter().copied());
        sections
    })
}

pub fn filtered_sections(query: &str) -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
    let query = query.to_lowercase();
    all_sections()
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

#[derive(Debug, Default)]
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

impl Component for HelpModal {
    fn handle_key(&mut self, key: KeyEvent, _ctx: &Ctx<'_>) -> ModalOutcome {
        if self.editing_search {
            match key.code {
                KeyCode::Esc => {
                    self.query.clear();
                    self.scroll = 0;
                    self.editing_search = false;
                    return ModalOutcome::Continue;
                }
                KeyCode::Enter => {
                    self.editing_search = false;
                    return ModalOutcome::Continue;
                }
                KeyCode::Backspace => {
                    if let Some((index, _)) = self.query.grapheme_indices(true).next_back() {
                        self.query.truncate(index);
                    }
                    self.scroll = 0;
                    return ModalOutcome::Continue;
                }
                KeyCode::Char(c) => {
                    if !c.is_control()
                        && !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    {
                        self.query.push(c);
                        self.scroll = 0;
                    }
                    return ModalOutcome::Continue;
                }
                _ => {}
            }
        }
        match key.code {
            KeyCode::Char('/') => {
                self.editing_search = true;
                ModalOutcome::Continue
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll_down(1);
                ModalOutcome::Continue
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_up(1);
                ModalOutcome::Continue
            }
            KeyCode::PageDown => {
                self.scroll_down(self.viewport_height);
                ModalOutcome::Continue
            }
            KeyCode::PageUp => {
                self.scroll_up(self.viewport_height);
                ModalOutcome::Continue
            }
            KeyCode::Home => {
                self.scroll = 0;
                ModalOutcome::Continue
            }
            KeyCode::End => {
                self.scroll = self.max_scroll();
                ModalOutcome::Continue
            }
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => ModalOutcome::Close,
            _ => ModalOutcome::Continue,
        }
    }

    fn handle_paste(&mut self, text: &str) {
        if self.editing_search {
            self.query
                .extend(text.chars().filter(|character| !character.is_control()));
            self.scroll = 0;
        }
    }

    fn render(&mut self, f: &mut Frame, area: Rect, ctx: &Ctx<'_>) {
        view::draw_help_modal(f, self, ctx.theme, area);
    }

    fn hints(&self) -> Vec<(&'static str, &'static str)> {
        if self.editing_search {
            vec![
                ("Enter", "Keep"),
                ("Esc", "Clear"),
                ("Backspace", "Erase"),
            ]
        } else {
            vec![
                ("Esc/q/?", "Close"),
                ("/", "Search"),
                ("j/k", "Scroll"),
            ]
        }
    }
}

impl App {
    pub fn open_help_modal(&mut self) {
        if !self.has_open_modal() {
            self.modal = Some(Modal::Help(HelpModal::default()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_matches_keys_descriptions_and_whole_sections() {
        let sections = all_sections();
        assert_eq!(filtered_sections("").len(), sections.len());
        assert_eq!(
            sections
                .iter()
                .take(4)
                .map(|section| section.title)
                .collect::<Vec<_>>(),
            ["Main Navigation", "Downloads", "Queues", "Categories"]
        );
        let filtered = filtered_sections("cAtEgOrIeS");
        let category = filtered
            .iter()
            .find(|(title, _)| *title == "Categories")
            .unwrap();
        assert_eq!(category.1, sections[3].bindings);
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
