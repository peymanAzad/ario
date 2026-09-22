use std::{
    path::PathBuf,
    process::{Command, Stdio},
    thread,
};

use common::{enums::DownloadStatus, finetune::FineTune};

use crate::{
    api,
    app::{App, AppEvent, ToastLevel, adjust_finetune_field},
    event::Event,
};

pub struct DownloadEditModal {
    pub download_id: i64,
    pub finetune: FineTune,
    pub cursor: usize,
    pub queue_cursor: usize,
    pub original_queue_id: i64,
    pub error: Option<String>,
}

fn silence_command_stdio(command: &mut Command) -> &mut Command {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
}

impl App {
    pub fn activate_selected_download(&mut self) {
        if self.has_open_modal() {
            return;
        }
        let Some(live) = self.current_download() else {
            return;
        };
        let is_completed = live.download.status == DownloadStatus::Completed;
        let download_id = live.download.id;
        let finetune = live.download.finetune.clone();
        let original_queue_id = live.download.queue_id;
        let destination_path = live.download.destination_path.clone();
        let filename = live.download.filename.clone();

        if is_completed {
            self.open_download_file(destination_path, filename);
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
        }
    }

    /// Spawns the OS's native "open this file" command in a background
    /// thread — fire-and-forget, same as every other action in this app.
    fn open_download_file(&mut self, destination_path: String, filename: Option<String>) {
        let path = filename
            .map(|name| PathBuf::from(&destination_path).join(name))
            .unwrap_or_else(|| PathBuf::from(destination_path));

        Self::open_path(path);
    }

    pub fn open_selected_download_folder(&mut self) {
        if self.has_open_modal() {
            return;
        }
        let Some(live) = self.current_download() else {
            return;
        };
        if live.download.status != DownloadStatus::Completed {
            return;
        }

        Self::open_path(PathBuf::from(live.download.destination_path.clone()));
    }

    /// Spawns the OS's native "open this path" command in a background thread.
    fn open_path(path: PathBuf) {
        thread::spawn(move || {
            #[cfg(target_os = "linux")]
            {
                let mut command = Command::new("xdg-open");
                command.arg(&path);
                let _ = silence_command_stdio(&mut command).spawn();
            }
            #[cfg(target_os = "macos")]
            {
                let mut command = Command::new("open");
                command.arg(&path);
                let _ = silence_command_stdio(&mut command).spawn();
            }
            #[cfg(target_os = "windows")]
            {
                let mut command = Command::new("cmd");
                command.args(["/C", "start", ""]).arg(&path);
                let _ = silence_command_stdio(&mut command).spawn();
            }
        });
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

    pub fn save_download_modal(&mut self) {
        let Some(mut modal) = self.download_modal.take() else {
            return;
        };
        let Some(queue_id) = self.queues.get(modal.queue_cursor).map(|queue| queue.id) else {
            modal.error = Some("No queue is available".into());
            self.download_modal = Some(modal);
            return;
        };
        let api_base = self.api_base.clone();
        let sender = self.event_sender.clone();
        thread::spawn(move || {
            let result: anyhow::Result<()> = (|| {
                if queue_id != modal.original_queue_id {
                    api::move_download_queue(&api_base, modal.download_id, queue_id)?;
                }
                api::update_finetune(&api_base, modal.download_id, &modal.finetune)?;
                Ok(())
            })();
            if let Err(e) = result {
                let _ = sender.send(Event::App(AppEvent::Toast {
                    message: e.to_string(),
                    level: ToastLevel::Error,
                }));
            }
        });
        self.refresh();
    }
}

#[cfg(test)]
mod tests {
    use super::silence_command_stdio;
    use std::process::Command;

    #[cfg(any(unix, windows))]
    #[test]
    fn external_command_stdio_is_silenced() {
        #[cfg(unix)]
        let mut command = {
            let mut command = Command::new("sh");
            command.args(["-c", "printf stdout; printf stderr >&2"]);
            command
        };

        #[cfg(windows)]
        let mut command = {
            let mut command = Command::new("cmd");
            command.args(["/C", "echo stdout & echo stderr 1>&2"]);
            command
        };

        let output = silence_command_stdio(&mut command)
            .output()
            .expect("test command should run");

        assert!(output.status.success());
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }
}
