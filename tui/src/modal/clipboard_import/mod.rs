use common::download::{AddDownloadInput, AddDownloadsRequest};
use common::finetune::FineTune;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{Frame, layout::Rect};

use crate::app::App;
use crate::modal::widgets::{FineTuneEditor, QueuePicker};
use crate::modal::{Component, Ctx, Modal, ModalOutcome};
use crate::msg::Action;
use crate::toast::ToastLevel;

pub(crate) mod view;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModalTab {
    Urls,
    FineTuning,
}

#[derive(Clone, Debug)]
pub struct ImportUrlEntry {
    pub url: String,
    pub selected: bool,
}

#[derive(Debug)]
pub struct ClipboardImportModal {
    pub tab: ModalTab,
    pub entries: Vec<ImportUrlEntry>,
    pub url_cursor: usize,
    pub queue_picker: QueuePicker,
    pub finetune_editor: FineTuneEditor,
}

impl ClipboardImportModal {
    fn next_tab(&mut self) {
        self.tab = match self.tab {
            ModalTab::Urls => ModalTab::FineTuning,
            ModalTab::FineTuning => ModalTab::Urls,
        };
    }

    fn move_down(&mut self) {
        match self.tab {
            ModalTab::Urls => {
                if !self.entries.is_empty() {
                    self.url_cursor = (self.url_cursor + 1).min(self.entries.len() - 1);
                }
            }
            ModalTab::FineTuning => self.finetune_editor.move_down(),
        }
    }

    fn move_up(&mut self) {
        match self.tab {
            ModalTab::Urls => self.url_cursor = self.url_cursor.saturating_sub(1),
            ModalTab::FineTuning => self.finetune_editor.move_up(),
        }
    }

    fn adjust(&mut self, forward: bool, queue_count: usize) {
        match self.tab {
            ModalTab::Urls => self.queue_picker.move_by(forward, queue_count),
            ModalTab::FineTuning => self.finetune_editor.adjust(forward),
        }
    }

    fn toggle_selected_url(&mut self) {
        if let Some(entry) = self.entries.get_mut(self.url_cursor) {
            entry.selected = !entry.selected;
        }
    }

    fn select_all(&mut self) {
        for entry in &mut self.entries {
            entry.selected = true;
        }
    }

    fn select_none(&mut self) {
        for entry in &mut self.entries {
            entry.selected = false;
        }
    }

    fn try_submit(&self, ctx: &Ctx<'_>, start_immediately: bool) -> ModalOutcome {
        let inputs: Vec<AddDownloadInput> = self
            .entries
            .iter()
            .filter(|entry| entry.selected)
            .map(|entry| AddDownloadInput::Url(entry.url.clone()))
            .collect();

        if inputs.is_empty() {
            return ModalOutcome::Close;
        }

        let queue_id = self
            .queue_picker
            .selected_id(ctx.queues)
            .unwrap_or(1);

        let finetune = &self.finetune_editor.finetune;
        let finetune_override = if *finetune == FineTune::default() {
            None
        } else {
            Some(finetune.clone())
        };

        ModalOutcome::Emit(Action::SubmitDownloads(AddDownloadsRequest {
            inputs,
            queue_id,
            finetune_override,
            start_immediately,
        }))
    }
}

impl Component for ClipboardImportModal {
    fn handle_key(&mut self, key: KeyEvent, ctx: &Ctx<'_>) -> ModalOutcome {
        match key.code {
            KeyCode::Esc | KeyCode::Char('c') => ModalOutcome::Close,
            KeyCode::Tab | KeyCode::BackTab => {
                self.next_tab();
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
            KeyCode::Char(' ') if self.tab == ModalTab::Urls => {
                self.toggle_selected_url();
                ModalOutcome::Continue
            }
            KeyCode::Char('a') if self.tab == ModalTab::Urls => {
                self.select_all();
                ModalOutcome::Continue
            }
            KeyCode::Char('n') if self.tab == ModalTab::Urls => {
                self.select_none();
                ModalOutcome::Continue
            }
            _ => ModalOutcome::Continue,
        }
    }

    fn render(&mut self, f: &mut Frame, _area: Rect, ctx: &Ctx<'_>) {
        view::draw_clipboard_import_modal(f, self, ctx);
    }
}

impl App {
    pub fn open_clipboard_import(&mut self) {
        if self.has_open_modal() {
            return;
        }
        let urls = crate::clipboard::scan_clipboard_for_urls();
        if urls.is_empty() {
            self.toasts.push("clipboard is empty", ToastLevel::Info);
            return;
        }

        let entries = urls
            .into_iter()
            .map(|url| ImportUrlEntry {
                url,
                selected: true,
            })
            .collect();

        self.modal = Some(Modal::ClipboardImport(ClipboardImportModal {
            tab: ModalTab::Urls,
            entries,
            url_cursor: 0,
            queue_picker: QueuePicker::default_for(self.selected_queue, &self.queues),
            finetune_editor: FineTuneEditor::new(FineTune::default()),
        }));
    }

    #[allow(dead_code)]
    pub fn cancel_modal(&mut self) {
        if matches!(self.modal, Some(Modal::ClipboardImport(_))) {
            self.modal = None;
        }
    }
}
