use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, Focus, ModalTab, queue_modal::QueueModalTab};

pub fn update(app: &mut App, key_event: KeyEvent) {
    if app.confirmation_modal.is_some() {
        handle_confirmation_key(app, key_event);
        return;
    }

    if key_event.modifiers == KeyModifiers::CONTROL
        && matches!(key_event.code, KeyCode::Char('c') | KeyCode::Char('C'))
    {
        app.quit();
        return;
    }

    if app.help_modal.is_some() {
        handle_help_modal_key(app, key_event);
        return;
    }

    if app.queue_modal.is_some() {
        handle_queue_modal_key(app, key_event);
        return;
    }

    if app.modal.is_some() {
        handle_clipboard_modal_key(app, key_event);
        return;
    }

    if app.download_modal.is_some() {
        handle_download_modal_key(app, key_event);
        return;
    }

    match key_event.code {
        KeyCode::Char('?') => {
            app.open_help_modal();
            return;
        }
        KeyCode::Esc | KeyCode::Char('q') => {
            app.quit();
            return;
        }
        KeyCode::Char('1') => {
            app.focus = Focus::Queues;
            return;
        }
        KeyCode::Char('2') => {
            app.focus = Focus::Categories;
            return;
        }
        KeyCode::Char('3') => {
            app.focus = Focus::Downloads;
            return;
        }
        KeyCode::Tab => {
            app.focus = app.focus.next();
            return;
        }
        KeyCode::BackTab => {
            app.focus = app.focus.prev();
            return;
        }
        KeyCode::Char('v') => {
            app.open_clipboard_import();
            return;
        }
        _ => {}
    }

    if is_remove_completed_key(app.focus, key_event.code) {
        app.remove_completed_downloads();
        return;
    }
    if is_delete_files_key(app.focus, key_event.code) {
        app.request_delete_selected_files();
        return;
    }
    if is_delete_queue_key(app.focus, key_event.code) {
        app.request_delete_selected_queue();
        return;
    }

    match app.focus {
        Focus::Queues => match key_event.code {
            KeyCode::Down | KeyCode::Char('j') => app.select_next_queue(),
            KeyCode::Up | KeyCode::Char('k') => app.select_prev_queue(),
            KeyCode::Char('n') => app.open_create_queue_modal(),
            KeyCode::Enter => app.open_edit_queue_modal(),
            KeyCode::Char('p') => app.pause_selected_queue(),
            KeyCode::Char('r') => app.resume_selected_queue(),
            _ => {}
        },
        Focus::Categories => match key_event.code {
            KeyCode::Down | KeyCode::Char('j') => app.select_next_category(),
            KeyCode::Up | KeyCode::Char('k') => app.select_prev_category(),
            _ => {}
        },
        Focus::Downloads => match key_event.code {
            KeyCode::Down | KeyCode::Char('j') => app.select_next_download(),
            KeyCode::Up | KeyCode::Char('k') => app.select_prev_download(),
            KeyCode::Enter => app.activate_selected_download(),
            KeyCode::Char('f') => app.open_selected_download_folder(),
            KeyCode::Char('p') => app.pause_selected(),
            KeyCode::Char('r') => app.resume_selected(),
            KeyCode::Char('d') => app.delete_selected(),
            _ => {}
        },
    }
}

fn handle_help_modal_key(app: &mut App, key_event: KeyEvent) {
    let modal = app.help_modal.as_mut().expect("help is open");
    if modal.editing_search {
        match key_event.code {
            KeyCode::Esc => {
                modal.query.clear();
                modal.scroll = 0;
                modal.editing_search = false;
                return;
            }
            KeyCode::Enter => {
                modal.editing_search = false;
                return;
            }
            KeyCode::Backspace => {
                use unicode_segmentation::UnicodeSegmentation;
                if let Some((index, _)) = modal.query.grapheme_indices(true).next_back() {
                    modal.query.truncate(index);
                }
                modal.scroll = 0;
                return;
            }
            KeyCode::Char(c) => {
                if !c.is_control()
                    && !key_event
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                {
                    modal.query.push(c);
                    modal.scroll = 0;
                }
                return;
            }
            _ => {}
        }
    }
    match key_event.code {
        KeyCode::Char('/') => modal.editing_search = true,
        KeyCode::Down | KeyCode::Char('j') => modal.scroll_down(1),
        KeyCode::Up | KeyCode::Char('k') => modal.scroll_up(1),
        KeyCode::PageDown => modal.scroll_down(modal.viewport_height),
        KeyCode::PageUp => modal.scroll_up(modal.viewport_height),
        KeyCode::Home => modal.scroll = 0,
        KeyCode::End => modal.scroll = modal.max_scroll(),
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => app.help_modal = None,
        _ => {}
    }
}

fn handle_confirmation_key(app: &mut App, key_event: KeyEvent) {
    match key_event.code {
        KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => app.confirm_confirmation(),
        KeyCode::Esc
        | KeyCode::Char('n')
        | KeyCode::Char('N')
        | KeyCode::Char('c')
        | KeyCode::Char('C') => app.cancel_confirmation(),
        _ => {}
    }
}

fn is_remove_completed_key(focus: Focus, key_code: KeyCode) -> bool {
    focus == Focus::Queues && key_code == KeyCode::Char('x')
}

fn is_delete_files_key(focus: Focus, key_code: KeyCode) -> bool {
    focus == Focus::Downloads && key_code == KeyCode::Char('D')
}

fn is_delete_queue_key(focus: Focus, key_code: KeyCode) -> bool {
    focus == Focus::Queues && key_code == KeyCode::Char('d')
}

fn handle_clipboard_modal_key(app: &mut App, key_event: KeyEvent) {
    match key_event.code {
        KeyCode::Esc | KeyCode::Char('c') => app.cancel_modal(),
        KeyCode::Tab => app.modal_next_tab(),
        KeyCode::BackTab => app.modal_prev_tab(),
        KeyCode::Char('s') => app.start_modal_now(),
        KeyCode::Char('w') => app.save_modal_for_later(),
        KeyCode::Down | KeyCode::Char('j') => app.modal_move_down(),
        KeyCode::Up | KeyCode::Char('k') => app.modal_move_up(),
        KeyCode::Left | KeyCode::Char('h') => app.modal_adjust_left(),
        KeyCode::Right | KeyCode::Char('l') => app.modal_adjust_right(),
        KeyCode::Char(' ') if app.modal.as_ref().map(|m| m.tab) == Some(ModalTab::Urls) => {
            app.modal_toggle_selected_url()
        }
        KeyCode::Char('a') if app.modal.as_ref().map(|m| m.tab) == Some(ModalTab::Urls) => {
            app.modal_select_all()
        }
        KeyCode::Char('n') if app.modal.as_ref().map(|m| m.tab) == Some(ModalTab::Urls) => {
            app.modal_select_none()
        }
        _ => {}
    }
}

fn handle_queue_modal_key(app: &mut App, key_event: KeyEvent) {
    let editing = app
        .queue_modal
        .as_ref()
        .map(|m| m.editing_text)
        .unwrap_or(false);

    if editing {
        match key_event.code {
            KeyCode::Enter => app.queue_modal_confirm_text_edit(),
            KeyCode::Esc => app.queue_modal_cancel_text_edit(),
            KeyCode::Backspace => app.queue_modal_text_backspace(),
            KeyCode::Char(c) => app.queue_modal_text_input(c),
            _ => {}
        }
        return;
    }

    let on_items_tab = app
        .queue_modal
        .as_ref()
        .map(|m| m.tab == QueueModalTab::DownloadItems)
        .unwrap_or(false);

    match key_event.code {
        KeyCode::Esc | KeyCode::Char('c') => app.cancel_queue_modal(),
        KeyCode::Tab => app.queue_modal_next_tab(),
        KeyCode::BackTab => app.queue_modal_prev_tab(),
        // Enter starts text editing on the queue name. Scheduler values use
        // left/right adjustment controls, and elsewhere Enter is a no-op.
        KeyCode::Enter => app.queue_modal_start_text_edit(),
        KeyCode::Char('s') => app.save_queue_modal(),
        // Reordering uses dedicated shifted keys rather than left/right,
        // since left/right has no natural meaning for moving an item up
        // or down a vertical list.
        KeyCode::Char('J') if on_items_tab => app.queue_modal_move_item_down(),
        KeyCode::Char('K') if on_items_tab => app.queue_modal_move_item_up(),
        KeyCode::Down | KeyCode::Char('j') => app.queue_modal_move_down(),
        KeyCode::Up | KeyCode::Char('k') => app.queue_modal_move_up(),
        KeyCode::Left | KeyCode::Char('h') => app.queue_modal_adjust_left(),
        KeyCode::Right | KeyCode::Char('l') => app.queue_modal_adjust_right(),
        // Space: toggles the highlighted day (Scheduler tab, Weekly days
        // row) — a no-op elsewhere, since the method itself checks context.
        KeyCode::Char(' ') => app.queue_modal_toggle_day(),
        _ => {}
    }
}

fn handle_download_modal_key(app: &mut App, key_event: KeyEvent) {
    match key_event.code {
        KeyCode::Esc | KeyCode::Char('c') => app.cancel_download_modal(),
        KeyCode::Char('s') => app.save_download_modal(),
        KeyCode::Down | KeyCode::Char('j') => app.download_modal_move_down(),
        KeyCode::Up | KeyCode::Char('k') => app.download_modal_move_up(),
        KeyCode::Left | KeyCode::Char('h') => app.download_modal_adjust_left(),
        KeyCode::Right | KeyCode::Char('l') => app.download_modal_adjust_right(),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{PendingConfirmationAction, confirmation_modal::ConfirmationModal},
        theme::Theme,
    };
    use std::sync::mpsc;

    fn test_app() -> App {
        let (sender, _receiver) = mpsc::channel();
        App::new(
            "http://127.0.0.1:1".into(),
            Theme::default_dark(),
            crate::icons::IconSet::new(crate::icons::GlyphMode::Unicode),
            sender,
            false,
        )
    }

    fn press(app: &mut App, code: KeyCode) {
        update(app, KeyEvent::new(code, KeyModifiers::NONE));
    }

    #[test]
    fn help_opens_from_every_pane_and_consumes_main_screen_actions() {
        for focus in [Focus::Queues, Focus::Categories, Focus::Downloads] {
            for close in [KeyCode::Esc, KeyCode::Char('q'), KeyCode::Char('?')] {
                let mut app = test_app();
                app.focus = focus;
                // Terminals can report '?' with Shift set.
                update(
                    &mut app,
                    KeyEvent::new(KeyCode::Char('?'), KeyModifiers::SHIFT),
                );
                assert!(app.help_modal.is_some());
                for code in [
                    KeyCode::Char('1'),
                    KeyCode::Tab,
                    KeyCode::Char('n'),
                    KeyCode::Char('v'),
                    KeyCode::Char('d'),
                    KeyCode::Char('D'),
                    KeyCode::Enter,
                    KeyCode::Char('j'),
                ] {
                    press(&mut app, code);
                }
                assert_eq!(app.focus, focus);
                assert_eq!(app.selected_queue, 0);
                assert_eq!(app.selected_category, 0);
                assert!(app.queue_modal.is_none());
                assert!(app.confirmation_modal.is_none());
                assert!(app.modal.is_none());
                assert!(app.toasts.is_empty());
                press(&mut app, close);
                assert!(app.help_modal.is_none());
                assert!(!app.should_quit);
            }
        }
    }

    #[test]
    fn existing_modals_block_help_and_text_editing_keeps_question_mark() {
        use crate::app::{
            clipboard_import_modal::ClipboardImportModal, download_edit_modal::DownloadEditModal,
        };
        let mut app = test_app();
        app.open_create_queue_modal();
        press(&mut app, KeyCode::Char('?'));
        app.open_help_modal();
        assert!(app.help_modal.is_none());
        press(&mut app, KeyCode::Enter);
        let before = app.queue_modal.as_ref().unwrap().text_buffer.clone();
        press(&mut app, KeyCode::Char('?'));
        assert_eq!(
            app.queue_modal.as_ref().unwrap().text_buffer,
            format!("{before}?")
        );
        assert!(app.help_modal.is_none());
        app.cancel_queue_modal();

        app.modal = Some(ClipboardImportModal {
            tab: ModalTab::Urls,
            entries: vec![],
            url_cursor: 0,
            queue_cursor: 0,
            finetune: Default::default(),
            finetune_cursor: 0,
        });
        press(&mut app, KeyCode::Char('?'));
        app.open_help_modal();
        assert!(app.help_modal.is_none());
        app.cancel_modal();

        app.download_modal = Some(DownloadEditModal {
            download_id: 1,
            finetune: Default::default(),
            cursor: 0,
            queue_cursor: 0,
            original_queue_id: 1,
            error: None,
        });
        press(&mut app, KeyCode::Char('?'));
        app.open_help_modal();
        assert!(app.help_modal.is_none());
        app.cancel_download_modal();

        app.open_confirmation(
            ConfirmationModal::new("Confirm", "Message", "Yes", "No"),
            PendingConfirmationAction::DeleteDownloadFiles { download_id: 1 },
        );
        press(&mut app, KeyCode::Char('?'));
        app.open_help_modal();
        assert!(app.help_modal.is_none());
        assert!(app.confirmation_modal.is_some());
    }

    #[test]
    fn queue_editor_can_save_from_download_items_tab() {
        let mut app = test_app();
        app.open_create_queue_modal();
        let modal = app.queue_modal.as_mut().unwrap();
        modal.mode = crate::app::queue_modal::QueueModalMode::Edit { queue_id: 1 };
        modal.tab = QueueModalTab::DownloadItems;
        modal.name = "Queue".into();

        press(&mut app, KeyCode::Char('s'));

        assert!(app.queue_modal.is_none());
    }

    #[test]
    fn one_time_schedule_uses_adjustable_date_and_time_fields() {
        use chrono::Duration as ChronoDuration;

        let mut app = test_app();
        app.open_create_queue_modal();
        let modal = app.queue_modal.as_mut().unwrap();
        modal.tab = QueueModalTab::Scheduler;
        modal.recurrence_kind = crate::app::queue_modal::RecurrenceKind::Once;
        modal.scheduler_cursor = 2;

        let start_date = modal.once_start_date;
        press(&mut app, KeyCode::Right);
        assert_eq!(
            app.queue_modal.as_ref().unwrap().once_start_date,
            start_date + ChronoDuration::days(1)
        );

        press(&mut app, KeyCode::Down);
        let start_time = app.queue_modal.as_ref().unwrap().once_start_time;
        press(&mut app, KeyCode::Right);
        assert_eq!(
            app.queue_modal.as_ref().unwrap().once_start_time,
            start_time + ChronoDuration::minutes(5)
        );

        press(&mut app, KeyCode::Enter);
        assert!(!app.queue_modal.as_ref().unwrap().editing_text);
    }

    #[test]
    fn help_search_accepts_shortcut_characters_and_clears_or_keeps_filter() {
        let mut app = test_app();
        app.open_help_modal();
        app.help_modal.as_mut().unwrap().set_dimensions(100, 10);
        press(&mut app, KeyCode::End);
        press(&mut app, KeyCode::Char('/'));
        for c in "q?j/ké".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        let modal = app.help_modal.as_ref().unwrap();
        assert_eq!(modal.query, "q?j/ké");
        assert_eq!(modal.scroll, 0);
        assert!(modal.editing_search);
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.help_modal.as_ref().unwrap().query, "q?j/k");
        press(&mut app, KeyCode::Enter);
        assert!(!app.help_modal.as_ref().unwrap().editing_search);
        assert_eq!(app.help_modal.as_ref().unwrap().query, "q?j/k");
        press(&mut app, KeyCode::Char('/'));
        press(&mut app, KeyCode::Esc);
        let modal = app.help_modal.as_ref().unwrap();
        assert!(!modal.editing_search);
        assert!(modal.query.is_empty());
        assert!(!app.should_quit);
        press(&mut app, KeyCode::Char('/'));
        press(&mut app, KeyCode::Char('x'));
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Esc);
        app.open_help_modal();
        let modal = app.help_modal.as_ref().unwrap();
        assert!(modal.query.is_empty());
        assert_eq!(modal.scroll, 0);
        assert!(!modal.editing_search);
    }

    #[test]
    fn help_navigation_uses_viewport_and_ctrl_c_still_quits() {
        let mut app = test_app();
        app.open_help_modal();
        app.help_modal.as_mut().unwrap().set_dimensions(50, 10);
        for (code, expected) in [
            (KeyCode::PageDown, 10),
            (KeyCode::Char('j'), 11),
            (KeyCode::Up, 10),
            (KeyCode::End, 40),
            (KeyCode::Down, 40),
            (KeyCode::PageUp, 30),
            (KeyCode::Home, 0),
            (KeyCode::Char('k'), 0),
        ] {
            press(&mut app, code);
            assert_eq!(app.help_modal.as_ref().unwrap().scroll, expected);
        }
        press(&mut app, KeyCode::Char('/'));
        update(
            &mut app,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        );
        assert!(app.should_quit);
    }

    #[test]
    fn remove_completed_key_is_scoped_to_queue_pane() {
        assert!(is_remove_completed_key(Focus::Queues, KeyCode::Char('x')));
        assert!(!is_remove_completed_key(
            Focus::Categories,
            KeyCode::Char('x')
        ));
        assert!(!is_remove_completed_key(
            Focus::Downloads,
            KeyCode::Char('x')
        ));
        assert!(!is_remove_completed_key(Focus::Queues, KeyCode::Char('X')));
    }

    #[test]
    fn destructive_delete_key_is_shifted_and_scoped_to_downloads() {
        assert!(is_delete_files_key(Focus::Downloads, KeyCode::Char('D')));
        assert!(!is_delete_files_key(Focus::Downloads, KeyCode::Char('d')));
        assert!(!is_delete_files_key(Focus::Queues, KeyCode::Char('D')));
        assert!(!is_delete_files_key(Focus::Categories, KeyCode::Char('D')));
    }

    #[test]
    fn queue_delete_key_is_lowercase_and_scoped_to_queues() {
        assert!(is_delete_queue_key(Focus::Queues, KeyCode::Char('d')));
        assert!(!is_delete_queue_key(Focus::Downloads, KeyCode::Char('d')));
        assert!(!is_delete_queue_key(Focus::Categories, KeyCode::Char('d')));
        assert!(!is_delete_queue_key(Focus::Queues, KeyCode::Char('D')));
    }

    #[test]
    fn confirmation_modal_consumes_cancel_before_global_quit() {
        let (sender, _receiver) = mpsc::channel();
        let mut app = App::new(
            "http://127.0.0.1:1".into(),
            Theme::default_dark(),
            crate::icons::IconSet::new(crate::icons::GlyphMode::Unicode),
            sender,
            false,
        );
        app.open_confirmation(
            ConfirmationModal::new("Confirm", "Message", "Yes", "No"),
            PendingConfirmationAction::DeleteDownloadFiles { download_id: 1 },
        );

        update(
            &mut app,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
        );
        assert!(app.confirmation_modal.is_none());
        assert!(!app.should_quit);
    }
}
