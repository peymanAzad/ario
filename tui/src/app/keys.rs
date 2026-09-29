use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    app::App,
    keymap,
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

    keymap::lookup(app.focus, key_event).map(Msg::Action)
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

#[cfg(test)]
#[path = "keys_tests.rs"]
mod tests;
