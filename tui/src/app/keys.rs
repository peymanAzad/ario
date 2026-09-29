use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    app::{App, Focus},
    modal::{Modal, ModalOutcome},
    msg::{Action, Msg},
};

pub fn route_key(app: &mut App, key_event: KeyEvent) -> Option<Msg> {
    let is_confirmation = matches!(app.modal, Some(Modal::Confirmation(_)));

    // Confirmation cancels on Ctrl+C before the global quit shortcut.
    if key_event.modifiers == KeyModifiers::CONTROL
        && matches!(key_event.code, KeyCode::Char('c') | KeyCode::Char('C'))
        && !is_confirmation
    {
        return Some(Msg::Action(Action::Quit));
    }

    if app.modal.is_some() {
        return dispatch_component_modal_key(app, key_event);
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
    if let Some(modal) = &mut app.modal {
        modal.handle_paste(text);
        if let Some((message, level)) = modal.take_pending_toast() {
            app.toasts.push(message, level);
        }
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
        toasts,
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
    if let Some(active) = modal.as_mut()
        && let Some((message, level)) = active.take_pending_toast()
    {
        toasts.push(message, level);
    }
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

fn is_remove_completed_key(focus: Focus, key_code: KeyCode) -> bool {
    focus == Focus::Queues && key_code == KeyCode::Char('x')
}

fn is_delete_files_key(focus: Focus, key_code: KeyCode) -> bool {
    focus == Focus::Downloads && key_code == KeyCode::Char('D')
}

fn is_delete_queue_key(focus: Focus, key_code: KeyCode) -> bool {
    focus == Focus::Queues && key_code == KeyCode::Char('d')
}

#[cfg(test)]
#[path = "keys_tests.rs"]
mod tests;
