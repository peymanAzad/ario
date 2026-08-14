use std::thread;

use common::{enums::DownloadStatus, finetune::FineTune};

use crate::{
    api,
    app::{App, adjust_finetune_field},
};

pub struct DownloadEditModal {
    pub download_id: i64,
    pub finetune: FineTune,
    pub cursor: usize,
    pub error: Option<String>,
}

impl App {
    pub fn activate_selected_download(&mut self) {
        if self.modal.is_some() || self.queue_modal.is_some() || self.download_modal.is_some() {
            return;
        }
        let Some(live) = self.current_download() else {
            return;
        };
        let is_completed = live.download.status == DownloadStatus::Completed;
        let download_id = live.download.id;
        let finetune = live.download.finetune.clone();
        let destination_path = live.download.destination_path.clone();
        let filename = live.download.filename.clone();

        if is_completed {
            self.open_download_file(destination_path, filename);
        } else {
            self.download_modal = Some(DownloadEditModal {
                download_id,
                finetune,
                cursor: 0,
                error: None,
            });
        }
    }

    /// Spawns the OS's native "open this file" command in a background
    /// thread — fire-and-forget, same as every other action in this app.
    fn open_download_file(&mut self, destination_path: String, filename: Option<String>) {
        let path = match filename {
            Some(name) => format!("{}/{}", destination_path.trim_end_matches('/'), name),
            None => destination_path,
        };

        thread::spawn(move || {
            #[cfg(target_os = "linux")]
            {
                let _ = std::process::Command::new("xdg-open").arg(&path).spawn();
            }
            #[cfg(target_os = "macos")]
            {
                let _ = std::process::Command::new("open").arg(&path).spawn();
            }
            #[cfg(target_os = "windows")]
            {
                let _ = std::process::Command::new("cmd")
                    .args(["/C", "start", "", &path])
                    .spawn();
            }
        });
    }

    pub fn cancel_download_modal(&mut self) {
        self.download_modal = None;
    }

    pub fn download_modal_move_down(&mut self) {
        if let Some(m) = &mut self.download_modal {
            m.cursor = (m.cursor + 1).min(3);
        }
    }

    pub fn download_modal_move_up(&mut self) {
        if let Some(m) = &mut self.download_modal {
            m.cursor = m.cursor.saturating_sub(1);
        }
    }

    pub fn download_modal_adjust_left(&mut self) {
        if let Some(m) = &mut self.download_modal {
            adjust_finetune_field(&mut m.finetune, m.cursor, false);
        }
    }

    pub fn download_modal_adjust_right(&mut self) {
        if let Some(m) = &mut self.download_modal {
            adjust_finetune_field(&mut m.finetune, m.cursor, true);
        }
    }

    /// Saves via the existing `PUT /downloads/:id/finetune` — no new server
    /// endpoint needed since finetune is the only currently-editable field.
    pub fn save_download_modal(&mut self) {
        let Some(modal) = self.download_modal.take() else {
            return;
        };
        let api_base = self.api_base.clone();
        thread::spawn(move || {
            let _ = api::update_finetune(&api_base, modal.download_id, &modal.finetune);
        });
        self.refresh();
    }
}
