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
mod tests {
    use super::*;
    use crate::{
        app::{PendingConfirmationAction, confirmation_modal::ConfirmationModal, update::update},
        theme::Theme,
    };

    fn test_app() -> App {
        App::new(
            Theme::default_dark(),
            crate::icons::IconSet::new(crate::icons::GlyphMode::Unicode),
            false,
        )
    }

    fn press(app: &mut App, code: KeyCode) {
        if let Some(msg) = route_key(app, KeyEvent::new(code, KeyModifiers::NONE)) {
            let _ = update(app, msg);
        }
    }

    #[test]
    fn help_opens_from_every_pane_and_consumes_main_screen_actions() {
        for focus in [Focus::Queues, Focus::Categories, Focus::Downloads] {
            for close in [KeyCode::Esc, KeyCode::Char('q'), KeyCode::Char('?')] {
                let mut app = test_app();
                app.focus = focus;
                // Terminals can report '?' with Shift set.
                if let Some(msg) =
                    route_key(&mut app, KeyEvent::new(KeyCode::Char('?'), KeyModifiers::SHIFT))
                {
                    let _ = update(&mut app, msg);
                }
                assert!(app.help_modal.is_some());
                for code in [
                    KeyCode::Char('1'),
                    KeyCode::Tab,
                    KeyCode::Char('n'),
                    KeyCode::Char('a'),
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
                assert!(app.torrent_modal.is_none());
                assert!(app.toasts.is_empty());
                press(&mut app, close);
                assert!(app.help_modal.is_none());
                assert!(!app.should_quit);
            }
        }
    }

    #[test]
    fn add_shortcut_opens_torrent_modal_and_accepts_paste() {
        let mut app = test_app();
        press(&mut app, KeyCode::Char('a'));
        assert!(app.torrent_modal.as_ref().unwrap().editing_path);

        paste(&mut app, "  ~/Downloads/example.torrent  ");
        assert_eq!(
            app.torrent_modal.as_ref().unwrap().path_input,
            "~/Downloads/example.torrent"
        );

        press(&mut app, KeyCode::Esc);
        assert!(app.torrent_modal.is_some());
        assert!(!app.torrent_modal.as_ref().unwrap().editing_path);
        press(&mut app, KeyCode::Esc);
        assert!(app.torrent_modal.is_none());
        assert!(!app.should_quit);
    }

    #[test]
    fn torrent_path_focus_can_move_to_tabs_and_back_without_validation() {
        use crate::app::torrent_file_modal::TorrentFileModalTab;

        let mut app = test_app();
        press(&mut app, KeyCode::Char('a'));

        press(&mut app, KeyCode::Tab);
        let modal = app.torrent_modal.as_ref().unwrap();
        assert!(!modal.editing_path);
        assert_eq!(modal.tab, TorrentFileModalTab::FineTuning);

        press(&mut app, KeyCode::BackTab);
        let modal = app.torrent_modal.as_ref().unwrap();
        assert!(!modal.editing_path);
        assert_eq!(modal.tab, TorrentFileModalTab::Torrent);

        press(&mut app, KeyCode::Enter);
        assert!(app.torrent_modal.as_ref().unwrap().editing_path);
        press(&mut app, KeyCode::Esc);
        assert!(app.torrent_modal.is_some());
        assert!(!app.torrent_modal.as_ref().unwrap().editing_path);
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
        if let Some(msg) = route_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        ) {
            let _ = update(&mut app, msg);
        }
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
        let mut app = App::new(
            Theme::default_dark(),
            crate::icons::IconSet::new(crate::icons::GlyphMode::Unicode),
            false,
        );
        app.open_confirmation(
            ConfirmationModal::new("Confirm", "Message", "Yes", "No"),
            PendingConfirmationAction::DeleteDownloadFiles { download_id: 1 },
        );

        if let Some(msg) = route_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
        ) {
            let _ = update(&mut app, msg);
        }
        assert!(app.confirmation_modal.is_none());
        assert!(!app.should_quit);
    }
}
