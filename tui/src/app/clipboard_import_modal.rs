use super::*;

pub struct ClipboardImportModal {
    pub tab: ModalTab,
    pub entries: Vec<ImportUrlEntry>,
    pub url_cursor: usize,
    pub queue_cursor: usize,
    pub finetune: FineTune,
    pub finetune_cursor: usize,
}

impl App {
    pub fn open_clipboard_import(&mut self) {
        let urls = crate::clipboard::scan_clipboard_for_urls();
        if urls.is_empty() {
            eprintln!("clipboard is empty");
            return;
        }

        let entries = urls
            .into_iter()
            .map(|url| ImportUrlEntry {
                url,
                selected: true, // all selected by default, per spec
            })
            .collect();

        let queue_cursor = self
            .queues
            .iter()
            .position(|q| q.name == "Main Queue")
            .unwrap_or(0);

        self.modal = Some(ClipboardImportModal {
            tab: ModalTab::Urls,
            entries,
            url_cursor: 0,
            queue_cursor,
            finetune: FineTune::default(),
            finetune_cursor: 0,
        });
    }

    pub fn cancel_modal(&mut self) {
        self.modal = None;
    }

    pub fn modal_next_tab(&mut self) {
        if let Some(m) = &mut self.modal {
            m.tab = match m.tab {
                ModalTab::Urls => ModalTab::FineTuning,
                ModalTab::FineTuning => ModalTab::Urls,
            };
        }
    }

    pub fn modal_prev_tab(&mut self) {
        self.modal_next_tab();
    }

    pub fn modal_move_down(&mut self) {
        if let Some(m) = &mut self.modal {
            match m.tab {
                ModalTab::Urls => {
                    if !m.entries.is_empty() {
                        m.url_cursor = (m.url_cursor + 1).min(m.entries.len() - 1);
                    }
                }
                ModalTab::FineTuning => {
                    m.finetune_cursor = (m.finetune_cursor + 1).min(3);
                }
            }
        }
    }

    pub fn modal_move_up(&mut self) {
        if let Some(m) = &mut self.modal {
            match m.tab {
                ModalTab::Urls => m.url_cursor = m.url_cursor.saturating_sub(1),
                ModalTab::FineTuning => m.finetune_cursor = m.finetune_cursor.saturating_sub(1),
            }
        }
    }

    fn modal_adjust(&mut self, forward: bool) {
        let queues_len = self.queues.len();
        if let Some(m) = &mut self.modal {
            match m.tab {
                ModalTab::Urls => {
                    if queues_len == 0 {
                        return;
                    }
                    if forward {
                        m.queue_cursor = (m.queue_cursor + 1).min(queues_len - 1);
                    } else {
                        m.queue_cursor = m.queue_cursor.saturating_sub(1);
                    }
                }
                ModalTab::FineTuning => {
                    adjust_finetune_field(&mut m.finetune, m.finetune_cursor, forward)
                }
            }
        }
    }

    pub fn modal_adjust_left(&mut self) {
        self.modal_adjust(false);
    }

    pub fn modal_adjust_right(&mut self) {
        self.modal_adjust(true);
    }

    pub fn modal_toggle_selected_url(&mut self) {
        if let Some(m) = &mut self.modal {
            if let Some(entry) = m.entries.get_mut(m.url_cursor) {
                entry.selected = !entry.selected;
            }
        }
    }

    pub fn modal_select_all(&mut self) {
        if let Some(m) = &mut self.modal {
            for e in &mut m.entries {
                e.selected = true;
            }
        }
    }

    pub fn modal_select_none(&mut self) {
        if let Some(m) = &mut self.modal {
            for e in &mut m.entries {
                e.selected = false;
            }
        }
    }

    fn submit_modal(&mut self, start_immediately: bool) {
        let Some(modal) = self.modal.take() else {
            return;
        };

        let inputs: Vec<AddDownloadInput> = modal
            .entries
            .into_iter()
            .filter(|e| e.selected)
            .map(|e| AddDownloadInput::Url(e.url))
            .collect();

        if inputs.is_empty() {
            return;
        }

        let queue_id = self
            .queues
            .get(modal.queue_cursor)
            .map(|q| q.id)
            .unwrap_or(1);

        let finetune_override = if modal.finetune == FineTune::default() {
            None
        } else {
            Some(modal.finetune)
        };

        let request = AddDownloadsRequest {
            inputs,
            queue_id,
            finetune_override,
            start_immediately,
        };

        let api_base = self.api_base.clone();
        thread::spawn(move || {
            let _ = api::add_downloads(&api_base, &request);
        });

        self.refresh();
    }

    pub fn start_modal_now(&mut self) {
        self.submit_modal(true);
    }

    pub fn save_modal_for_later(&mut self) {
        self.submit_modal(false);
    }
}
