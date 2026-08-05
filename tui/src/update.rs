use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, Focus, ModalTab};

pub fn update(app: &mut App, key_event: KeyEvent) {
    if key_event.modifiers == KeyModifiers::CONTROL
        && matches!(key_event.code, KeyCode::Char('c') | KeyCode::Char('C'))
    {
        app.quit();
        return;
    }

    if app.modal.is_some() {
        handle_modal_key(app, key_event);
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

    match app.focus {
        Focus::Queues => match key_event.code {
            KeyCode::Down | KeyCode::Char('j') => app.select_next_queue(),
            KeyCode::Up | KeyCode::Char('k') => app.select_prev_queue(),
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
            KeyCode::Char('p') => app.pause_selected(),
            KeyCode::Char('r') => app.resume_selected(),
            KeyCode::Char('d') => app.delete_selected(),
            _ => {}
        },
    }
}

fn handle_modal_key(app: &mut App, key_event: KeyEvent) {
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
