use std::path::PathBuf;

use anyhow::{Context, ensure};
use common::{download::TorrentUploadMetadata, finetune::FineTune};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Frame, layout::Rect};

use crate::app::App;
use crate::modal::widgets::{FineTuneEditor, QueuePicker, TextInput};
use crate::modal::{Component, Ctx, Modal, ModalOutcome};
use crate::msg::Action;
use crate::toast::ToastLevel;

mod view;

pub const MAX_TORRENT_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TorrentFileModalTab {
    Torrent,
    FineTuning,
}

#[derive(Debug)]
pub struct TorrentFileModal {
    pub tab: TorrentFileModalTab,
    pub path_input: TextInput,
    pub resolved_path: Option<PathBuf>,
    pub queue_picker: QueuePicker,
    pub finetune_editor: FineTuneEditor,
    /// Toast to apply after handle_key returns Continue (path validation errors).
    pub pending_toast: Option<(String, ToastLevel)>,
}

impl TorrentFileModal {
    fn stop_path_editing(&mut self) {
        self.path_input.editing = false;
    }

    fn next_tab(&mut self) {
        self.path_input.editing = false;
        self.tab = match self.tab {
            TorrentFileModalTab::Torrent => TorrentFileModalTab::FineTuning,
            TorrentFileModalTab::FineTuning => TorrentFileModalTab::Torrent,
        };
    }

    fn commit_path(&mut self) {
        if !self.path_input.editing {
            self.path_input.editing = true;
            return;
        }
        match normalize_torrent_path(&self.path_input.buffer) {
            Ok(path) => {
                self.path_input.buffer = path.to_string_lossy().into_owned();
                self.resolved_path = Some(path);
                self.path_input.editing = false;
            }
            Err(error) => {
                self.pending_toast = Some((error.to_string(), ToastLevel::Error));
            }
        }
    }

    fn move_down(&mut self) {
        if self.tab == TorrentFileModalTab::FineTuning {
            self.finetune_editor.move_down();
        }
    }

    fn move_up(&mut self) {
        if self.tab == TorrentFileModalTab::FineTuning {
            self.finetune_editor.move_up();
        }
    }

    fn adjust(&mut self, forward: bool, queue_count: usize) {
        match self.tab {
            TorrentFileModalTab::Torrent if !self.path_input.editing => {
                self.queue_picker.move_by(forward, queue_count);
            }
            TorrentFileModalTab::FineTuning => self.finetune_editor.adjust(forward),
            _ => {}
        }
    }

    fn try_submit(&mut self, ctx: &Ctx<'_>, start_immediately: bool) -> ModalOutcome {
        let path = match normalize_torrent_path(&self.path_input.buffer) {
            Ok(path) => path,
            Err(error) => {
                self.pending_toast = Some((error.to_string(), ToastLevel::Error));
                return ModalOutcome::Continue;
            }
        };
        self.resolved_path = Some(path.clone());
        let queue_id = self
            .queue_picker
            .selected_id(ctx.queues)
            .unwrap_or(1);
        let finetune = &self.finetune_editor.finetune;
        let finetune_override = (*finetune != FineTune::default()).then_some(finetune.clone());
        ModalOutcome::Emit(Action::SubmitTorrent {
            path,
            metadata: TorrentUploadMetadata {
                queue_id,
                finetune_override,
                start_immediately,
            },
        })
    }

    pub fn take_pending_toast(&mut self) -> Option<(String, ToastLevel)> {
        self.pending_toast.take()
    }
}

impl Component for TorrentFileModal {
    fn handle_key(&mut self, key: KeyEvent, ctx: &Ctx<'_>) -> ModalOutcome {
        if self.path_input.editing {
            match key.code {
                KeyCode::Esc => {
                    self.stop_path_editing();
                    return ModalOutcome::Continue;
                }
                KeyCode::Tab | KeyCode::BackTab => {
                    self.next_tab();
                    return ModalOutcome::Continue;
                }
                KeyCode::Enter => {
                    self.commit_path();
                    return ModalOutcome::Continue;
                }
                KeyCode::Backspace => {
                    self.path_input.backspace();
                    self.resolved_path = None;
                    return ModalOutcome::Continue;
                }
                KeyCode::Char(character)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.path_input.input(character);
                    self.resolved_path = None;
                    return ModalOutcome::Continue;
                }
                _ => return ModalOutcome::Continue,
            }
        }

        match key.code {
            KeyCode::Esc | KeyCode::Char('c') => ModalOutcome::Close,
            KeyCode::Tab | KeyCode::BackTab => {
                self.next_tab();
                ModalOutcome::Continue
            }
            KeyCode::Enter if self.tab == TorrentFileModalTab::Torrent => {
                self.commit_path();
                ModalOutcome::Continue
            }
            KeyCode::Char('s') => self.try_submit(ctx, true),
            KeyCode::Char('w') => self.try_submit(ctx, false),
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

    fn handle_paste(&mut self, text: &str) {
        if self.path_input.editing {
            self.path_input.buffer = text.trim().to_string();
            self.resolved_path = None;
        }
    }

    fn render(&mut self, f: &mut Frame, _area: Rect, ctx: &Ctx<'_>) {
        view::draw_torrent_file_modal(f, self, ctx);
    }
}

impl App {
    pub fn open_torrent_file_modal(&mut self) {
        if self.has_open_modal() {
            return;
        }
        let mut path_input = TextInput::default();
        path_input.start(String::new());
        self.modal = Some(Modal::TorrentFile(TorrentFileModal {
            tab: TorrentFileModalTab::Torrent,
            path_input,
            resolved_path: None,
            queue_picker: QueuePicker::default_for(self.selected_queue, &self.queues),
            finetune_editor: FineTuneEditor::new(FineTune::default()),
            pending_toast: None,
        }));
    }

    #[allow(dead_code)]
    pub fn cancel_torrent_file_modal(&mut self) {
        if matches!(self.modal, Some(Modal::TorrentFile(_))) {
            self.modal = None;
        }
    }
}

pub fn normalize_torrent_path(input: &str) -> anyhow::Result<PathBuf> {
    let mut value = input.trim().to_string();
    if value.len() >= 2 {
        let first = value.as_bytes()[0];
        let last = value.as_bytes()[value.len() - 1];
        if (first == b'\'' && last == b'\'') || (first == b'"' && last == b'"') {
            value = value[1..value.len() - 1].to_string();
        }
    }
    value = value.replace("\\ ", " ");
    ensure!(!value.is_empty(), "enter a .torrent file path");

    let mut path = PathBuf::from(&value);
    if let Some(remainder) = value.strip_prefix("~/") {
        let home = directories::BaseDirs::new().context("home directory is unavailable")?;
        path = home.home_dir().join(remainder);
    }
    let path = path
        .canonicalize()
        .with_context(|| format!("cannot open torrent file: {}", path.display()))?;
    let metadata = path
        .metadata()
        .with_context(|| format!("cannot inspect torrent file: {}", path.display()))?;
    ensure!(
        metadata.is_file(),
        "torrent path must point to a regular file"
    );
    ensure!(metadata.len() > 0, "torrent file is empty");
    ensure!(
        metadata.len() <= MAX_TORRENT_BYTES,
        "torrent file exceeds the 16 MiB limit"
    );
    ensure!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("torrent")),
        "file must have a .torrent extension"
    );
    std::fs::File::open(&path)
        .with_context(|| format!("torrent file is not readable: {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("ario-{name}-{}-{nonce}", std::process::id()))
    }

    #[test]
    fn normalizes_quoted_and_escaped_paths() {
        let dir = temp_path("torrent path with spaces");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sample.TORRENT");
        std::fs::write(&path, b"torrent").unwrap();

        let quoted = format!("'{}'", path.display());
        assert_eq!(normalize_torrent_path(&quoted).unwrap(), path);
        let escaped = path.to_string_lossy().replace(' ', "\\ ");
        assert_eq!(normalize_torrent_path(&escaped).unwrap(), path);

        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn rejects_directories_wrong_extensions_and_empty_files() {
        let dir = temp_path("torrent-validation");
        std::fs::create_dir_all(&dir).unwrap();
        let wrong = dir.join("sample.txt");
        let empty = dir.join("empty.torrent");
        std::fs::write(&wrong, b"data").unwrap();
        std::fs::write(&empty, b"").unwrap();

        assert!(normalize_torrent_path(dir.to_str().unwrap()).is_err());
        assert!(normalize_torrent_path(wrong.to_str().unwrap()).is_err());
        assert!(normalize_torrent_path(empty.to_str().unwrap()).is_err());

        std::fs::remove_file(wrong).unwrap();
        std::fs::remove_file(empty).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn rejects_torrent_files_over_the_size_limit() {
        let path = temp_path("oversized.torrent");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_TORRENT_BYTES + 1).unwrap();
        drop(file);

        assert!(normalize_torrent_path(path.to_str().unwrap()).is_err());

        std::fs::remove_file(path).unwrap();
    }
}
