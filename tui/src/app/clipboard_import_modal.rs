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
                selected: true, // all selected by default, per spec
            })
            .collect();

        let main_queue_cursor = self.queues.iter().position(|q| q.name == "Main Queue");
        let queue_cursor =
            clipboard_queue_cursor(self.selected_queue, self.queues.len(), main_queue_cursor);

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
        let sender = self.event_sender.clone();
        thread::spawn(move || {
            if let Err(e) = api::add_downloads(&api_base, &request) {
                let _ = sender.send(Event::App(AppEvent::Toast {
                    message: e.to_string(),
                    level: ToastLevel::Error,
                }));
            }
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

fn clipboard_queue_cursor(
    selected_queue: usize,
    queue_count: usize,
    main_queue_cursor: Option<usize>,
) -> usize {
    selected_queue
        .checked_sub(1)
        .filter(|&queue_cursor| queue_cursor < queue_count)
        .or(main_queue_cursor)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::clipboard_queue_cursor;

    #[test]
    fn defaults_to_the_current_queue() {
        assert_eq!(clipboard_queue_cursor(3, 4, Some(0)), 2);
    }

    #[test]
    fn all_queues_selection_defaults_to_main_queue() {
        assert_eq!(clipboard_queue_cursor(0, 4, Some(2)), 2);
    }

    #[test]
    fn missing_main_queue_defaults_to_the_first_queue() {
        assert_eq!(clipboard_queue_cursor(0, 4, None), 0);
    }

    #[test]
    fn empty_and_stale_queue_selections_are_safe() {
        assert_eq!(clipboard_queue_cursor(0, 0, None), 0);
        assert_eq!(clipboard_queue_cursor(5, 2, Some(1)), 1);
        assert_eq!(clipboard_queue_cursor(5, 2, None), 0);
    }
}
