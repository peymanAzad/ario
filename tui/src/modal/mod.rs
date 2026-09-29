pub mod clipboard_import;
pub mod confirmation;
pub mod download_edit;
pub mod help;
pub mod queue;
pub mod torrent_file;
pub mod widgets;

use crate::icons::IconSet;
use crate::msg::Action;
use crate::theme::Theme;
use common::finetune::Aria2GlobalOptions;
use common::queue::Queue;
use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};

pub use clipboard_import::ClipboardImportModal;
#[allow(unused_imports)]
pub use clipboard_import::{ImportUrlEntry, ModalTab};
pub use confirmation::ConfirmationModal;
pub use download_edit::DownloadEditModal;
pub use help::HelpModal;
pub use queue::{QueueModal, QueueModalMode};
#[allow(unused_imports)]
pub use queue::{QueueModalTab, RecurrenceKind};
#[allow(unused_imports)]
pub use torrent_file::TorrentFileModalTab;
pub use torrent_file::{MAX_TORRENT_BYTES, TorrentFileModal};

#[derive(Debug)]
pub enum Modal {
    Confirmation(ConfirmationModal),
    Help(HelpModal),
    Queue(QueueModal),
    TorrentFile(TorrentFileModal),
    ClipboardImport(ClipboardImportModal),
    DownloadEdit(DownloadEditModal),
}

pub enum ModalOutcome {
    Continue,
    Close,
    Emit(Action),
    /// Keep the modal open and surface a toast through `Msg::Toast`.
    Notify {
        message: String,
        level: crate::toast::ToastLevel,
    },
}

#[allow(dead_code)]
pub struct Ctx<'a> {
    pub queues: &'a [Queue],
    pub selected_queue: usize,
    pub aria2_global_options: Option<&'a Aria2GlobalOptions>,
    pub theme: &'a Theme,
    pub icons: &'a IconSet,
}

pub trait Component {
    fn handle_key(&mut self, key: KeyEvent, ctx: &Ctx<'_>) -> ModalOutcome;
    fn handle_paste(&mut self, _text: &str) {}
    fn render(&mut self, f: &mut Frame, area: Rect, ctx: &Ctx<'_>);
    #[allow(dead_code)]
    fn hints(&self) -> Vec<(&'static str, &'static str)> {
        vec![]
    }
}

impl Modal {
    pub fn handle_key(&mut self, key: KeyEvent, ctx: &Ctx<'_>) -> ModalOutcome {
        match self {
            Modal::Confirmation(m) => m.handle_key(key, ctx),
            Modal::Help(m) => m.handle_key(key, ctx),
            Modal::Queue(m) => m.handle_key(key, ctx),
            Modal::TorrentFile(m) => m.handle_key(key, ctx),
            Modal::ClipboardImport(m) => m.handle_key(key, ctx),
            Modal::DownloadEdit(m) => m.handle_key(key, ctx),
        }
    }

    pub fn handle_paste(&mut self, text: &str) {
        match self {
            Modal::Help(m) => m.handle_paste(text),
            Modal::Queue(m) => m.handle_paste(text),
            Modal::TorrentFile(m) => m.handle_paste(text),
            Modal::Confirmation(_) | Modal::ClipboardImport(_) | Modal::DownloadEdit(_) => {}
        }
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect, ctx: &Ctx<'_>) {
        match self {
            Modal::Confirmation(m) => m.render(f, area, ctx),
            Modal::Help(m) => m.render(f, area, ctx),
            Modal::Queue(m) => m.render(f, area, ctx),
            Modal::TorrentFile(m) => m.render(f, area, ctx),
            Modal::ClipboardImport(m) => m.render(f, area, ctx),
            Modal::DownloadEdit(m) => m.render(f, area, ctx),
        }
    }

    pub fn hints(&self) -> Vec<(&'static str, &'static str)> {
        match self {
            Modal::Confirmation(m) => m.hints(),
            Modal::Help(m) => m.hints(),
            Modal::Queue(m) => m.hints(),
            Modal::TorrentFile(m) => m.hints(),
            Modal::ClipboardImport(m) => m.hints(),
            Modal::DownloadEdit(m) => m.hints(),
        }
    }
}
