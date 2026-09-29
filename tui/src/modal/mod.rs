pub mod confirmation;
pub mod help;

use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};
use common::finetune::Aria2GlobalOptions;
use common::queue::Queue;
use crate::app::{
    clipboard_import_modal::ClipboardImportModal,
    download_edit_modal::DownloadEditModal,
    queue_modal::QueueModal,
    torrent_file_modal::TorrentFileModal,
};
use crate::icons::IconSet;
use crate::msg::Action;
use crate::theme::Theme;

pub use confirmation::ConfirmationModal;
pub use help::HelpModal;

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
}

#[allow(dead_code)]
pub struct Ctx<'a> {
    pub queues: &'a [Queue],
    pub selected_queue: usize,
    pub aria2_global_options: Option<&'a Aria2GlobalOptions>,
    pub theme: &'a Theme,
    pub icons: &'a IconSet,
}

#[allow(dead_code)]
pub trait Component {
    fn handle_key(&mut self, key: KeyEvent, ctx: &Ctx<'_>) -> ModalOutcome;
    fn handle_paste(&mut self, _text: &str) {}
    fn render(&mut self, f: &mut Frame, area: Rect, ctx: &Ctx<'_>);
    fn hints(&self) -> Vec<(&'static str, &'static str)> {
        vec![]
    }
}

impl Modal {
    pub fn handle_key(&mut self, key: KeyEvent, ctx: &Ctx<'_>) -> ModalOutcome {
        match self {
            Modal::Confirmation(m) => m.handle_key(key, ctx),
            Modal::Help(m) => m.handle_key(key, ctx),
            // Phase 4a: remaining modals still use app/keys handlers.
            Modal::Queue(_)
            | Modal::TorrentFile(_)
            | Modal::ClipboardImport(_)
            | Modal::DownloadEdit(_) => ModalOutcome::Continue,
        }
    }

    pub fn handle_paste(&mut self, text: &str) {
        match self {
            Modal::Help(m) => m.handle_paste(text),
            Modal::Confirmation(_)
            | Modal::Queue(_)
            | Modal::TorrentFile(_)
            | Modal::ClipboardImport(_)
            | Modal::DownloadEdit(_) => {}
        }
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect, ctx: &Ctx<'_>) {
        match self {
            Modal::Confirmation(m) => m.render(f, area, ctx),
            Modal::Help(m) => m.render(f, area, ctx),
            Modal::Queue(_)
            | Modal::TorrentFile(_)
            | Modal::ClipboardImport(_)
            | Modal::DownloadEdit(_) => {}
        }
    }
}
