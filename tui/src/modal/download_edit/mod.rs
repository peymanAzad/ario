use std::path::PathBuf;

use common::enums::DownloadStatus;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{Frame, layout::Rect};

use crate::app::App;
use crate::effects::Effect;
use crate::modal::widgets::{FineTuneEditor, QueuePicker};
use crate::modal::{Component, Ctx, Modal, ModalOutcome};
use crate::msg::Action;

mod view;

#[derive(Debug)]
pub struct DownloadEditModal {
    pub download_id: i64,
    pub finetune_editor: FineTuneEditor,
    pub queue_picker: QueuePicker,
    /// When true, the Queue row (after the six finetune fields) is focused.
    pub focusing_queue: bool,
    pub original_queue_id: i64,
    pub error: Option<String>,
}

impl DownloadEditModal {
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
            original_queue_id: self.original_queue_id,
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
}

impl App {
    pub fn activate_selected_download(&mut self) -> Vec<Effect> {
        if self.has_open_modal() {
            return vec![];
        }
        let Some(live) = self.current_download() else {
            return vec![];
        };
        let is_completed = live.download.status == DownloadStatus::Completed;
        let download_id = live.download.id;
        let finetune = live.download.finetune.clone();
        let original_queue_id = live.download.queue_id;
        let destination_path = live.download.destination_path.clone();
        let filename = live.download.filename.clone();

        if is_completed {
            self.open_download_file(destination_path, filename)
        } else {
            let queue_cursor = self
                .queues
                .iter()
                .position(|queue| queue.id == original_queue_id)
                .unwrap_or(0);
            self.modal = Some(Modal::DownloadEdit(DownloadEditModal {
                download_id,
                finetune_editor: FineTuneEditor::new(finetune),
                queue_picker: QueuePicker {
                    cursor: queue_cursor,
                },
                focusing_queue: false,
                original_queue_id,
                error: None,
            }));
            vec![]
        }
    }

    fn open_download_file(
        &mut self,
        destination_path: String,
        filename: Option<String>,
    ) -> Vec<Effect> {
        let path = filename
            .map(|name| PathBuf::from(&destination_path).join(name))
            .unwrap_or_else(|| PathBuf::from(destination_path));
        vec![Effect::OpenPath(path)]
    }

    pub fn open_selected_download_folder(&mut self) -> Vec<Effect> {
        if self.has_open_modal() {
            return vec![];
        }
        let Some(live) = self.current_download() else {
            return vec![];
        };
        if live.download.status != DownloadStatus::Completed {
            return vec![];
        }

        vec![Effect::OpenPath(PathBuf::from(
            live.download.destination_path.clone(),
        ))]
    }

    #[allow(dead_code)]
    pub fn cancel_download_modal(&mut self) {
        if matches!(self.modal, Some(Modal::DownloadEdit(_))) {
            self.modal = None;
        }
    }
}
