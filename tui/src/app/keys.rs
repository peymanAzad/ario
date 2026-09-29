use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    app::{App, Focus, ModalTab, queue_modal::QueueModalTab},
    msg::{Action, Msg},
};

pub fn route_key(app: &mut App, key_event: KeyEvent) -> Option<Msg> {
    if app.confirmation_modal.is_some() {
        return handle_confirmation_key(app, key_event);
    }

    if key_event.modifiers == KeyModifiers::CONTROL
        && matches!(key_event.code, KeyCode::Char('c') | KeyCode::Char('C'))
    {
        return Some(Msg::Action(Action::Quit));
    }

    if app.help_modal.is_some() {
        handle_help_modal_key(app, key_event);
        return None;
    }

    if app.queue_modal.is_some() {
        return handle_queue_modal_key(app, key_event);
    }

    if app.torrent_modal.is_some() {
        return handle_torrent_modal_key(app, key_event);
    }

    if app.modal.is_some() {
        return handle_clipboard_modal_key(app, key_event);
    }

    if app.download_modal.is_some() {
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
    if app.torrent_modal.is_some() {
        app.paste_torrent_path(text);
    } else if app
        .queue_modal
        .as_ref()
        .is_some_and(|modal| modal.editing_text)
    {
        for character in text.chars().filter(|character| !character.is_control()) {
            app.queue_modal_text_input(character);
        }
    } else if let Some(modal) = &mut app.help_modal
        && modal.editing_search
    {
        modal
            .query
            .extend(text.chars().filter(|character| !character.is_control()));
        modal.scroll = 0;
    }
}

fn handle_torrent_modal_key(app: &mut App, key_event: KeyEvent) -> Option<Msg> {
    let editing = app
        .torrent_modal
        .as_ref()
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
            if app.torrent_modal.as_ref().is_some_and(|modal| {
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

fn handle_confirmation_key(app: &mut App, key_event: KeyEvent) -> Option<Msg> {
    match key_event.code {
        KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
            app.take_confirm_action().map(|action| Msg::Action(Action::Confirm(action)))
        }
        KeyCode::Esc
        | KeyCode::Char('n')
        | KeyCode::Char('N')
        | KeyCode::Char('c')
        | KeyCode::Char('C') => {
            app.cancel_confirmation();
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
        KeyCode::Char(' ') if app.modal.as_ref().map(|m| m.tab) == Some(ModalTab::Urls) => {
            app.modal_toggle_selected_url();
            None
        }
        KeyCode::Char('a') if app.modal.as_ref().map(|m| m.tab) == Some(ModalTab::Urls) => {
            app.modal_select_all();
            None
        }
        KeyCode::Char('n') if app.modal.as_ref().map(|m| m.tab) == Some(ModalTab::Urls) => {
            app.modal_select_none();
            None
        }
        _ => None,
    }
}

fn handle_queue_modal_key(app: &mut App, key_event: KeyEvent) -> Option<Msg> {
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
        return None;
    }

    let on_items_tab = app
        .queue_modal
        .as_ref()
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
