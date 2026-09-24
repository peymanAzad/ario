use std::{fs, path::PathBuf, thread};

use anyhow::{Context, ensure};
use common::{download::TorrentUploadMetadata, finetune::FineTune};
use unicode_segmentation::UnicodeSegmentation;

use super::*;

pub const MAX_TORRENT_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TorrentFileModalTab {
    Torrent,
    FineTuning,
}

pub struct TorrentFileModal {
    pub tab: TorrentFileModalTab,
    pub path_input: String,
    pub resolved_path: Option<PathBuf>,
    pub editing_path: bool,
    pub queue_cursor: usize,
    pub finetune: FineTune,
    pub finetune_cursor: usize,
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
        self.torrent_modal = Some(TorrentFileModal {
            tab: TorrentFileModalTab::Torrent,
            path_input: String::new(),
            resolved_path: None,
            editing_path: true,
            queue_cursor,
            finetune: FineTune::default(),
            finetune_cursor: 0,
        });
    }

    pub fn cancel_torrent_file_modal(&mut self) {
        self.torrent_modal = None;
    }

    pub fn torrent_modal_stop_path_editing(&mut self) {
        if let Some(modal) = &mut self.torrent_modal {
            modal.editing_path = false;
        }
    }

    pub fn torrent_modal_next_tab(&mut self) {
        if let Some(modal) = &mut self.torrent_modal {
            modal.editing_path = false;
            modal.tab = match modal.tab {
                TorrentFileModalTab::Torrent => TorrentFileModalTab::FineTuning,
                TorrentFileModalTab::FineTuning => TorrentFileModalTab::Torrent,
            };
        }
    }

    pub fn torrent_modal_text_input(&mut self, character: char) {
        if let Some(modal) = &mut self.torrent_modal
            && modal.editing_path
            && !character.is_control()
        {
            modal.path_input.push(character);
            modal.resolved_path = None;
        }
    }

    pub fn torrent_modal_text_backspace(&mut self) {
        if let Some(modal) = &mut self.torrent_modal
            && modal.editing_path
            && let Some((index, _)) = modal.path_input.grapheme_indices(true).next_back()
        {
            modal.path_input.truncate(index);
            modal.resolved_path = None;
        }
    }

    pub fn paste_torrent_path(&mut self, text: &str) {
        if let Some(modal) = &mut self.torrent_modal
            && modal.editing_path
        {
            modal.path_input = text.trim().to_string();
            modal.resolved_path = None;
        }
    }

    pub fn torrent_modal_commit_path(&mut self) {
        let Some(modal) = &mut self.torrent_modal else {
            return;
        };
        if !modal.editing_path {
            modal.editing_path = true;
            return;
        }
        match normalize_torrent_path(&modal.path_input) {
            Ok(path) => {
                modal.path_input = path.to_string_lossy().into_owned();
                modal.resolved_path = Some(path);
                modal.editing_path = false;
            }
            Err(error) => self.toasts.push(error.to_string(), ToastLevel::Error),
        }
    }

    pub fn torrent_modal_move_down(&mut self) {
        if let Some(modal) = &mut self.torrent_modal
            && modal.tab == TorrentFileModalTab::FineTuning
        {
            modal.finetune_cursor = (modal.finetune_cursor + 1).min(5);
        }
    }

    pub fn torrent_modal_move_up(&mut self) {
        if let Some(modal) = &mut self.torrent_modal
            && modal.tab == TorrentFileModalTab::FineTuning
        {
            modal.finetune_cursor = modal.finetune_cursor.saturating_sub(1);
        }
    }

    fn torrent_modal_adjust(&mut self, forward: bool) {
        let queue_count = self.queues.len();
        if let Some(modal) = &mut self.torrent_modal {
            match modal.tab {
                TorrentFileModalTab::Torrent if !modal.editing_path && queue_count > 0 => {
                    modal.queue_cursor = if forward {
                        (modal.queue_cursor + 1).min(queue_count - 1)
                    } else {
                        modal.queue_cursor.saturating_sub(1)
                    };
                }
                TorrentFileModalTab::FineTuning => {
                    adjust_finetune_field(&mut modal.finetune, modal.finetune_cursor, forward)
                }
                _ => {}
            }
        }
    }

    pub fn torrent_modal_adjust_left(&mut self) {
        self.torrent_modal_adjust(false);
    }

    pub fn torrent_modal_adjust_right(&mut self) {
        self.torrent_modal_adjust(true);
    }

    fn submit_torrent_file(&mut self, start_immediately: bool) {
        let Some(modal) = &mut self.torrent_modal else {
            return;
        };
        let path = match normalize_torrent_path(&modal.path_input) {
            Ok(path) => path,
            Err(error) => {
                self.toasts.push(error.to_string(), ToastLevel::Error);
                return;
            }
        };
        modal.resolved_path = Some(path.clone());
        let modal = self.torrent_modal.take().expect("torrent modal is open");
        let queue_id = self
            .queues
            .get(modal.queue_cursor)
            .map(|queue| queue.id)
            .unwrap_or(1);
        let finetune_override = (modal.finetune != FineTune::default()).then_some(modal.finetune);
        let metadata = TorrentUploadMetadata {
            queue_id,
            finetune_override,
            start_immediately,
        };
        let api_base = self.api_base.clone();
        let sender = self.event_sender.clone();
        self.toasts.push("adding torrent…", ToastLevel::Info);
        thread::spawn(move || {
            let result = (|| -> anyhow::Result<DownloadLiveStatus> {
                let data = fs::read(&path)
                    .with_context(|| format!("failed to read {}", path.display()))?;
                ensure!(!data.is_empty(), "torrent file is empty");
                ensure!(
                    data.len() as u64 <= MAX_TORRENT_BYTES,
                    "torrent file exceeds the 16 MiB limit"
                );
                let filename = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .context("torrent filename is not valid UTF-8")?;
                api::add_torrent(&api_base, filename, data, &metadata)
            })();
            let _ = sender.send(Event::App(AppEvent::TorrentAdded(result)));
        });
    }

    pub fn start_torrent_now(&mut self) {
        self.submit_torrent_file(true);
    }

    pub fn save_torrent_for_later(&mut self) {
        self.submit_torrent_file(false);
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
    fs::File::open(&path)
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
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sample.TORRENT");
        fs::write(&path, b"torrent").unwrap();

        let quoted = format!("'{}'", path.display());
        assert_eq!(normalize_torrent_path(&quoted).unwrap(), path);
        let escaped = path.to_string_lossy().replace(' ', "\\ ");
        assert_eq!(normalize_torrent_path(&escaped).unwrap(), path);

        fs::remove_file(path).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn rejects_directories_wrong_extensions_and_empty_files() {
        let dir = temp_path("torrent-validation");
        fs::create_dir_all(&dir).unwrap();
        let wrong = dir.join("sample.txt");
        let empty = dir.join("empty.torrent");
        fs::write(&wrong, b"data").unwrap();
        fs::write(&empty, b"").unwrap();

        assert!(normalize_torrent_path(dir.to_str().unwrap()).is_err());
        assert!(normalize_torrent_path(wrong.to_str().unwrap()).is_err());
        assert!(normalize_torrent_path(empty.to_str().unwrap()).is_err());

        fs::remove_file(wrong).unwrap();
        fs::remove_file(empty).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn rejects_torrent_files_over_the_size_limit() {
        let path = temp_path("oversized.torrent");
        let file = fs::File::create(&path).unwrap();
        file.set_len(MAX_TORRENT_BYTES + 1).unwrap();
        drop(file);

        assert!(normalize_torrent_path(path.to_str().unwrap()).is_err());

        fs::remove_file(path).unwrap();
    }
}
