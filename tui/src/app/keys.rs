use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    app::{App, Focus, ModalTab, queue_modal::QueueModalTab},
    modal::{Modal, ModalOutcome},
    msg::{Action, Msg},
};

pub fn route_key(app: &mut App, key_event: KeyEvent) -> Option<Msg> {
    // Confirmation cancels before global Ctrl+C quit; other modals still quit on Ctrl+C.
    if matches!(app.modal, Some(Modal::Confirmation(_))) {
        return dispatch_component_modal_key(app, key_event);
    }

    if key_event.modifiers == KeyModifiers::CONTROL
        && matches!(key_event.code, KeyCode::Char('c') | KeyCode::Char('C'))
    {
        return Some(Msg::Action(Action::Quit));
    }

    if matches!(app.modal, Some(Modal::Help(_))) {
        let _ = dispatch_component_modal_key(app, key_event);
        return None;
    }
    if matches!(app.modal, Some(Modal::Queue(_))) {
        return handle_queue_modal_key(app, key_event);
    }
    if matches!(app.modal, Some(Modal::TorrentFile(_))) {
        return handle_torrent_modal_key(app, key_event);
    }
    if matches!(app.modal, Some(Modal::ClipboardImport(_))) {
        return handle_clipboard_modal_key(app, key_event);
    }
    if matches!(app.modal, Some(Modal::DownloadEdit(_))) {
        return handle_download_modal_key(app, key_event);
    }

    match key_event.code {
        KeyCode::Char('?') => return Some(Msg::Action(Action::OpenHelp)),
        KeyCode::Esc | KeyCode::Char('q') => return Some(Msg::Action(Action::Quit)),
        KeyCode::Char('1') => return Some(Msg::Action(Action::Focus(Focus::Queues))),
        KeyCode::Char('2') => return Some(Msg::Action(Action::Focus(Focus::Categories))),
        KeyCode::Char('3') => return Some(Msg::Action(Action::Focus(Focus::Downloads))),
        KeyCode::Tab => return Some(Msg::Action(Action::FocusNext)),
        KeyCode::BackTab => return Some(Msg::Action(Action::FocusPrev)),
        KeyCode::Char('v') => return Some(Msg::Action(Action::OpenClipboardImport)),
        KeyCode::Char('a') => return Some(Msg::Action(Action::OpenTorrentFile)),
        _ => {}
    }

    if is_remove_completed_key(app.focus, key_event.code) {
        return Some(Msg::Action(Action::RemoveCompleted));
    }
    if is_delete_files_key(app.focus, key_event.code) {
        return Some(Msg::Action(Action::DeleteDownloadFiles));
    }
    if is_delete_queue_key(app.focus, key_event.code) {
        return Some(Msg::Action(Action::DeleteQueue));
    }

    match app.focus {
        Focus::Queues => match key_event.code {
            KeyCode::Down | KeyCode::Char('j') => Some(Msg::Action(Action::SelectNext)),
            KeyCode::Up | KeyCode::Char('k') => Some(Msg::Action(Action::SelectPrev)),
            KeyCode::Char('n') => Some(Msg::Action(Action::OpenCreateQueue)),
            KeyCode::Enter => Some(Msg::Action(Action::OpenEditQueue)),
            KeyCode::Char('p') => Some(Msg::Action(Action::PauseQueue)),
            KeyCode::Char('r') => Some(Msg::Action(Action::ResumeQueue)),
            _ => None,
        },
        Focus::Categories => match key_event.code {
            KeyCode::Down | KeyCode::Char('j') => Some(Msg::Action(Action::SelectNext)),
            KeyCode::Up | KeyCode::Char('k') => Some(Msg::Action(Action::SelectPrev)),
            _ => None,
        },
        Focus::Downloads => match key_event.code {
            KeyCode::Down | KeyCode::Char('j') => Some(Msg::Action(Action::SelectNext)),
            KeyCode::Up | KeyCode::Char('k') => Some(Msg::Action(Action::SelectPrev)),
            KeyCode::Enter => Some(Msg::Action(Action::ActivateDownload)),
            KeyCode::Char('f') => Some(Msg::Action(Action::OpenDownloadFolder)),
            KeyCode::Char('p') => Some(Msg::Action(Action::PauseDownload)),
            KeyCode::Char('r') => Some(Msg::Action(Action::ResumeDownload)),
            KeyCode::Char('d') => Some(Msg::Action(Action::DeleteDownload)),
            _ => None,
        },
    }
}

pub fn paste(app: &mut App, text: &str) {
    if matches!(app.modal, Some(Modal::TorrentFile(_))) {
        app.paste_torrent_path(text);
    } else if app.queue_modal().is_some_and(|modal| modal.editing_text) {
        for character in text.chars().filter(|character| !character.is_control()) {
            app.queue_modal_text_input(character);
        }
    } else if let Some(modal) = &mut app.modal {
        modal.handle_paste(text);
    }
}

fn dispatch_component_modal_key(app: &mut App, key_event: KeyEvent) -> Option<Msg> {
    let App {
        modal,
        queues,
        selected_queue,
        aria2_global_options,
        theme,
        icons,
        ..
    } = app;
    let outcome = {
        let Some(active) = modal.as_mut() else {
            return None;
        };
        let ctx = crate::modal::Ctx {
            queues,
            selected_queue: *selected_queue,
            aria2_global_options: aria2_global_options.as_ref(),
            theme,
            icons,
        };
        active.handle_key(key_event, &ctx)
    };
    match outcome {
        ModalOutcome::Continue => None,
        ModalOutcome::Close => {
            *modal = None;
            None
        }
        ModalOutcome::Emit(action) => {
            *modal = None;
            Some(Msg::Action(action))
        }
    }
}

fn handle_torrent_modal_key(app: &mut App, key_event: KeyEvent) -> Option<Msg> {
    let editing = app
        .torrent_modal()
        .is_some_and(|modal| modal.editing_path);
    if editing {
        match key_event.code {
            KeyCode::Esc => app.torrent_modal_stop_path_editing(),
            KeyCode::Tab | KeyCode::BackTab => app.torrent_modal_next_tab(),
            KeyCode::Enter => app.torrent_modal_commit_path(),
            KeyCode::Backspace => app.torrent_modal_text_backspace(),
            KeyCode::Char(character)
                if !key_event
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                app.torrent_modal_text_input(character)
            }
            _ => {}
        }
        return None;
    }

    match key_event.code {
        KeyCode::Esc | KeyCode::Char('c') => Some(Msg::Action(Action::CancelModal)),
        KeyCode::Tab | KeyCode::BackTab => {
            app.torrent_modal_next_tab();
            None
        }
        KeyCode::Enter
            if app.torrent_modal().is_some_and(|modal| {
                modal.tab == crate::app::torrent_file_modal::TorrentFileModalTab::Torrent
            }) =>
        {
            app.torrent_modal_commit_path();
            None
        }
        KeyCode::Char('s') => app
            .take_submit_torrent_action(true)
            .map(Msg::Action),
        KeyCode::Char('w') => app
            .take_submit_torrent_action(false)
            .map(Msg::Action),
        KeyCode::Down | KeyCode::Char('j') => {
            app.torrent_modal_move_down();
            None
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.torrent_modal_move_up();
            None
        }
        KeyCode::Left | KeyCode::Char('h') => {
            app.torrent_modal_adjust_left();
            None
        }
        KeyCode::Right | KeyCode::Char('l') => {
            app.torrent_modal_adjust_right();
            None
        }
        _ => None,
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

fn handle_clipboard_modal_key(app: &mut App, key_event: KeyEvent) -> Option<Msg> {
    match key_event.code {
        KeyCode::Esc | KeyCode::Char('c') => Some(Msg::Action(Action::CancelModal)),
        KeyCode::Tab => {
            app.modal_next_tab();
            None
        }
        KeyCode::BackTab => {
            app.modal_prev_tab();
            None
        }
        KeyCode::Char('s') => app.take_submit_downloads_action(true).map(Msg::Action),
        KeyCode::Char('w') => app.take_submit_downloads_action(false).map(Msg::Action),
        KeyCode::Down | KeyCode::Char('j') => {
            app.modal_move_down();
            None
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.modal_move_up();
            None
        }
        KeyCode::Left | KeyCode::Char('h') => {
            app.modal_adjust_left();
            None
        }
        KeyCode::Right | KeyCode::Char('l') => {
            app.modal_adjust_right();
            None
        }
        KeyCode::Char(' ') if app.clipboard_modal().map(|m| m.tab) == Some(ModalTab::Urls) => {
            app.modal_toggle_selected_url();
            None
        }
        KeyCode::Char('a') if app.clipboard_modal().map(|m| m.tab) == Some(ModalTab::Urls) => {
            app.modal_select_all();
            None
        }
        KeyCode::Char('n') if app.clipboard_modal().map(|m| m.tab) == Some(ModalTab::Urls) => {
            app.modal_select_none();
            None
        }
        _ => None,
    }
}

fn handle_queue_modal_key(app: &mut App, key_event: KeyEvent) -> Option<Msg> {
    let editing = app
        .queue_modal()
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
        return None;
    }

    let on_items_tab = app
        .queue_modal()
        .map(|m| m.tab == QueueModalTab::DownloadItems)
        .unwrap_or(false);

    match key_event.code {
        KeyCode::Esc | KeyCode::Char('c') => Some(Msg::Action(Action::CancelModal)),
        KeyCode::Tab => {
            app.queue_modal_next_tab();
            None
        }
        KeyCode::BackTab => {
            app.queue_modal_prev_tab();
            None
        }
        KeyCode::Enter => {
            app.queue_modal_start_text_edit();
            None
        }
        KeyCode::Char('s') => app.take_save_queue_action().map(Msg::Action),
        KeyCode::Char('J') if on_items_tab => {
            app.queue_modal_move_item_down();
            None
        }
        KeyCode::Char('K') if on_items_tab => {
            app.queue_modal_move_item_up();
            None
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.queue_modal_move_down();
            None
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.queue_modal_move_up();
            None
        }
        KeyCode::Left | KeyCode::Char('h') => {
            app.queue_modal_adjust_left();
            None
        }
        KeyCode::Right | KeyCode::Char('l') => {
            app.queue_modal_adjust_right();
            None
        }
        KeyCode::Char(' ') => {
            app.queue_modal_toggle_day();
            None
        }
        _ => None,
    }
}

fn handle_download_modal_key(app: &mut App, key_event: KeyEvent) -> Option<Msg> {
    match key_event.code {
        KeyCode::Esc | KeyCode::Char('c') => Some(Msg::Action(Action::CancelModal)),
        KeyCode::Char('s') => app.take_save_download_edit_action().map(Msg::Action),
        KeyCode::Down | KeyCode::Char('j') => {
            app.download_modal_move_down();
            None
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.download_modal_move_up();
            None
        }
        KeyCode::Left | KeyCode::Char('h') => {
            app.download_modal_adjust_left();
            None
        }
        KeyCode::Right | KeyCode::Char('l') => {
            app.download_modal_adjust_right();
            None
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "keys_tests.rs"]
mod tests;
