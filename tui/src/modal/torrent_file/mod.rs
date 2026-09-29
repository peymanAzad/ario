use std::path::PathBuf;

use anyhow::{Context, ensure};
use common::{download::TorrentUploadMetadata, finetune::FineTune};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Frame, layout::Rect};
use unicode_segmentation::UnicodeSegmentation;

use crate::app::{App, adjust_finetune_field};
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
    pub path_input: String,
    pub resolved_path: Option<PathBuf>,
    pub editing_path: bool,
    pub queue_cursor: usize,
    pub finetune: FineTune,
    pub finetune_cursor: usize,
    /// Toast to apply after handle_key returns Continue (path validation errors).
    pub pending_toast: Option<(String, ToastLevel)>,
}

impl TorrentFileModal {
    fn stop_path_editing(&mut self) {
        self.editing_path = false;
    }

    fn next_tab(&mut self) {
        self.editing_path = false;
        self.tab = match self.tab {
            TorrentFileModalTab::Torrent => TorrentFileModalTab::FineTuning,
            TorrentFileModalTab::FineTuning => TorrentFileModalTab::Torrent,
        };
    }

    fn text_input(&mut self, character: char) {
        if self.editing_path && !character.is_control() {
            self.path_input.push(character);
            self.resolved_path = None;
        }
    }

    fn text_backspace(&mut self) {
        if self.editing_path
            && let Some((index, _)) = self.path_input.grapheme_indices(true).next_back()
        {
            self.path_input.truncate(index);
            self.resolved_path = None;
        }
    }

    fn commit_path(&mut self) {
        if !self.editing_path {
            self.editing_path = true;
            return;
        }
        match normalize_torrent_path(&self.path_input) {
            Ok(path) => {
                self.path_input = path.to_string_lossy().into_owned();
                self.resolved_path = Some(path);
                self.editing_path = false;
            }
            Err(error) => {
                self.pending_toast = Some((error.to_string(), ToastLevel::Error));
            }
        }
    }

    fn move_down(&mut self) {
        if self.tab == TorrentFileModalTab::FineTuning {
            self.finetune_cursor = (self.finetune_cursor + 1).min(5);
        }
    }

    fn move_up(&mut self) {
        if self.tab == TorrentFileModalTab::FineTuning {
            self.finetune_cursor = self.finetune_cursor.saturating_sub(1);
        }
    }

    fn adjust(&mut self, forward: bool, queue_count: usize) {
        match self.tab {
            TorrentFileModalTab::Torrent if !self.editing_path && queue_count > 0 => {
                self.queue_cursor = if forward {
                    (self.queue_cursor + 1).min(queue_count - 1)
                } else {
                    self.queue_cursor.saturating_sub(1)
                };
            }
            TorrentFileModalTab::FineTuning => {
                adjust_finetune_field(&mut self.finetune, self.finetune_cursor, forward)
            }
            _ => {}
        }
    }

    fn try_submit(&mut self, ctx: &Ctx<'_>, start_immediately: bool) -> ModalOutcome {
        let path = match normalize_torrent_path(&self.path_input) {
            Ok(path) => path,
            Err(error) => {
                self.pending_toast = Some((error.to_string(), ToastLevel::Error));
                return ModalOutcome::Continue;
            }
        };
        self.resolved_path = Some(path.clone());
        let queue_id = ctx
            .queues
            .get(self.queue_cursor)
            .map(|queue| queue.id)
            .unwrap_or(1);
        let finetune_override = (self.finetune != FineTune::default()).then_some(self.finetune.clone());
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
        if self.editing_path {
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
                    self.text_backspace();
                    return ModalOutcome::Continue;
                }
                KeyCode::Char(character)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.text_input(character);
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
        if self.editing_path {
            self.path_input = text.trim().to_string();
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
        let main_queue_cursor = self
            .queues
            .iter()
            .position(|queue| queue.name == "Main Queue");
        let queue_cursor = self
            .selected_queue
            .checked_sub(1)
            .filter(|cursor| *cursor < self.queues.len())
            .or(main_queue_cursor)
            .unwrap_or(0);
        self.modal = Some(Modal::TorrentFile(TorrentFileModal {
            tab: TorrentFileModalTab::Torrent,
            path_input: String::new(),
            resolved_path: None,
            editing_path: true,
            queue_cursor,
            finetune: FineTune::default(),
            finetune_cursor: 0,
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
