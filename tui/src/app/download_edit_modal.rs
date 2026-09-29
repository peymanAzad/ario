use std::path::PathBuf;

use common::{enums::DownloadStatus, finetune::FineTune};

use crate::{
    app::{App, adjust_finetune_field},
    effects::Effect,
    msg::Action,
};

pub struct DownloadEditModal {
    pub download_id: i64,
    pub finetune: FineTune,
    pub cursor: usize,
    pub queue_cursor: usize,
    pub original_queue_id: i64,
    pub error: Option<String>,
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
            self.download_modal = Some(DownloadEditModal {
                download_id,
                finetune,
                cursor: 0,
                queue_cursor,
                original_queue_id,
                error: None,
            });
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

    pub fn cancel_download_modal(&mut self) {
        self.download_modal = None;
    }

    pub fn download_modal_move_down(&mut self) {
        if let Some(m) = &mut self.download_modal {
            m.cursor = (m.cursor + 1).min(6);
        }
    }

    pub fn download_modal_move_up(&mut self) {
        if let Some(m) = &mut self.download_modal {
            m.cursor = m.cursor.saturating_sub(1);
        }
    }

    pub fn download_modal_adjust_left(&mut self) {
        self.download_modal_adjust(false);
    }

    pub fn download_modal_adjust_right(&mut self) {
        self.download_modal_adjust(true);
    }

    fn download_modal_adjust(&mut self, forward: bool) {
        if let Some(m) = &mut self.download_modal {
            if m.cursor == 6 {
                if !self.queues.is_empty() {
                    let len = self.queues.len();
                    m.queue_cursor = if forward {
                        (m.queue_cursor + 1) % len
                    } else {
                        (m.queue_cursor + len - 1) % len
                    };
                }
            } else {
                adjust_finetune_field(&mut m.finetune, m.cursor, forward);
            }
        }
    }

    pub fn take_save_download_edit_action(&mut self) -> Option<Action> {
        let mut modal = self.download_modal.take()?;
        let Some(queue_id) = self.queues.get(modal.queue_cursor).map(|queue| queue.id) else {
            modal.error = Some("No queue is available".into());
            self.download_modal = Some(modal);
            return None;
        };
        Some(Action::SaveDownloadEdit {
            id: modal.download_id,
            finetune: modal.finetune,
            queue_id,
            original_queue_id: modal.original_queue_id,
        })
    }
}
