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
        // Enter: on the name field or a Once date field, starts text
        // editing; everywhere else on the Common/Scheduler tabs it's
        // unused, and on Download Items it's likewise a no-op.
        KeyCode::Enter => app.queue_modal_start_text_edit(),
        KeyCode::Char('s') if !on_items_tab => app.save_queue_modal(),
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
    fn confirmation_modal_consumes_cancel_before_global_quit() {
        let (sender, _receiver) = mpsc::channel();
        let mut app = App::new(
            "http://127.0.0.1:1".into(),
            Theme::default_dark(),
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
