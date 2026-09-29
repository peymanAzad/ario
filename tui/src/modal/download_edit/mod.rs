use common::finetune::FineTune;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{Frame, layout::Rect};

use crate::modal::widgets::{FineTuneEditor, QueuePicker};
use crate::modal::{Component, Ctx, ModalOutcome};
use crate::msg::Action;

mod view;

/// Help section for the download editor modal.
pub const HELP: &[(&str, &str)] = &[
    ("j / Down", "Select next field"),
    ("k / Up", "Select previous field"),
    ("h / Left", "Decrease value or choose previous queue"),
    ("l / Right", "Increase value or choose next queue"),
    ("s", "Save download settings"),
    ("c / Esc", "Cancel download editing"),
];

#[derive(Debug)]
pub struct DownloadEditModal {
    pub download_id: i64,
    pub finetune_editor: FineTuneEditor,
    pub queue_picker: QueuePicker,
    /// When true, the Queue row (after the six finetune fields) is focused.
    pub focusing_queue: bool,
    pub error: Option<String>,
}

impl DownloadEditModal {
    pub(crate) fn editing(download_id: i64, finetune: FineTune, queue_cursor: usize) -> Self {
        Self {
            download_id,
            finetune_editor: FineTuneEditor::new(finetune),
            queue_picker: QueuePicker {
                cursor: queue_cursor,
            },
            focusing_queue: false,
            error: None,
        }
    }

    fn move_down(&mut self) {
        if self.focusing_queue {
            return;
        }
        if self.finetune_editor.cursor >= 5 {
            self.focusing_queue = true;
        } else {
            self.finetune_editor.move_down();
        }
    }

    fn move_up(&mut self) {
        if self.focusing_queue {
            self.focusing_queue = false;
        } else {
            self.finetune_editor.move_up();
        }
    }

    fn adjust(&mut self, forward: bool, queue_count: usize) {
        if self.focusing_queue {
            if queue_count == 0 {
                return;
            }
            // Preserve wrapping behavior for the download-edit queue row.
            self.queue_picker.cursor = if forward {
                (self.queue_picker.cursor + 1) % queue_count
            } else {
                (self.queue_picker.cursor + queue_count - 1) % queue_count
            };
        } else {
            self.finetune_editor.adjust(forward);
        }
    }

    pub(crate) fn selected_row(&self) -> usize {
        if self.focusing_queue {
            6
        } else {
            self.finetune_editor.cursor
        }
    }

    fn try_save(&mut self, ctx: &Ctx<'_>) -> ModalOutcome {
        let Some(queue_id) = self.queue_picker.selected_id(ctx.queues) else {
            self.error = Some("No queue is available".into());
            return ModalOutcome::Continue;
        };
        ModalOutcome::Emit(Action::SaveDownloadEdit {
            id: self.download_id,
            finetune: self.finetune_editor.finetune.clone(),
            queue_id,
        })
    }
}

impl Component for DownloadEditModal {
    fn handle_key(&mut self, key: KeyEvent, ctx: &Ctx<'_>) -> ModalOutcome {
        match key.code {
            KeyCode::Esc | KeyCode::Char('c') => ModalOutcome::Close,
            KeyCode::Char('s') => self.try_save(ctx),
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_down();
                ModalOutcome::Continue
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_up();
                ModalOutcome::Continue
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.adjust(false, ctx.queues.len());
                ModalOutcome::Continue
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.adjust(true, ctx.queues.len());
                ModalOutcome::Continue
            }
            _ => ModalOutcome::Continue,
        }
    }

    fn render(&mut self, f: &mut Frame, _area: Rect, ctx: &Ctx<'_>) {
        view::draw_download_modal(f, self, ctx);
    }

    fn hints(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("s", "Save"),
            ("c/Esc", "Cancel"),
            ("j/k", "Navigate"),
            ("h/l", "Adjust"),
        ]
    }
}
