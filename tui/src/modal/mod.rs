// Optional re-exports later; for Phase 3 keep structs in app/
// pub mod confirmation;

use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};
use common::finetune::Aria2GlobalOptions;
use common::queue::Queue;
use crate::app::{
    clipboard_import_modal::ClipboardImportModal,
    confirmation_modal::ConfirmationModal,
    download_edit_modal::DownloadEditModal,
    help_modal::HelpModal,
    queue_modal::QueueModal,
    torrent_file_modal::TorrentFileModal,
    PendingConfirmationAction,
};
use crate::icons::IconSet;
use crate::msg::Action;
use crate::theme::Theme;

#[derive(Debug)]
pub enum Modal {
    Confirmation {
        modal: ConfirmationModal,
        action: PendingConfirmationAction,
    },
    Help(HelpModal),
    Queue(QueueModal),
    TorrentFile(TorrentFileModal),
    ClipboardImport(ClipboardImportModal),
    DownloadEdit(DownloadEditModal),
}

#[allow(dead_code)]
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
