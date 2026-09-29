use super::*;
use crate::{
    app::{Focus, PendingConfirmationAction, update::update},
    keymap,
    modal::{
        ClipboardImportModal, ConfirmationModal, DownloadEditModal, ModalTab, QueueModalMode,
        QueueModalTab, RecurrenceKind, TorrentFileModalTab,
    },
    msg::Action,
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
            assert!(app.help_modal().is_some());
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
            assert!(app.queue_modal().is_none());
            assert!(app.confirmation_modal().is_none());
            assert!(app.clipboard_modal().is_none());
            assert!(app.torrent_modal().is_none());
            assert!(app.toasts.is_empty());
            press(&mut app, close);
            assert!(app.help_modal().is_none());
            assert!(!app.should_quit);
        }
    }
}

#[test]
fn add_shortcut_opens_torrent_modal_and_accepts_paste() {
    let mut app = test_app();
    press(&mut app, KeyCode::Char('a'));
    assert!(app.torrent_modal().unwrap().path_input.editing);

    paste(&mut app, "  ~/Downloads/example.torrent  ");
    assert_eq!(
        app.torrent_modal().unwrap().path_input.buffer,
        "~/Downloads/example.torrent"
    );

    press(&mut app, KeyCode::Esc);
    assert!(app.torrent_modal().is_some());
    assert!(!app.torrent_modal().unwrap().path_input.editing);
    press(&mut app, KeyCode::Esc);
    assert!(app.torrent_modal().is_none());
    assert!(!app.should_quit);
}

#[test]
fn torrent_path_focus_can_move_to_tabs_and_back_without_validation() {
    let mut app = test_app();
    press(&mut app, KeyCode::Char('a'));

    press(&mut app, KeyCode::Tab);
    let modal = app.torrent_modal().unwrap();
    assert!(!modal.path_input.editing);
    assert_eq!(modal.tab, TorrentFileModalTab::FineTuning);

    press(&mut app, KeyCode::BackTab);
    let modal = app.torrent_modal().unwrap();
    assert!(!modal.path_input.editing);
    assert_eq!(modal.tab, TorrentFileModalTab::Torrent);

    press(&mut app, KeyCode::Enter);
    assert!(app.torrent_modal().unwrap().path_input.editing);
    press(&mut app, KeyCode::Esc);
    assert!(app.torrent_modal().is_some());
    assert!(!app.torrent_modal().unwrap().path_input.editing);
}

#[test]
fn existing_modals_block_help_and_text_editing_keeps_question_mark() {
    let mut app = test_app();
    app.open_create_queue_modal();
    press(&mut app, KeyCode::Char('?'));
    app.open_help_modal();
    assert!(app.help_modal().is_none());
    press(&mut app, KeyCode::Enter);
    let before = app.queue_modal().unwrap().name_input.buffer.clone();
    press(&mut app, KeyCode::Char('?'));
    assert_eq!(
        app.queue_modal().unwrap().name_input.buffer,
        format!("{before}?")
    );
    assert!(app.help_modal().is_none());
    app.cancel_queue_modal();

    app.modal = Some(crate::modal::Modal::ClipboardImport(ClipboardImportModal {
        tab: ModalTab::Urls,
        entries: vec![],
        url_cursor: 0,
        queue_picker: crate::modal::widgets::QueuePicker { cursor: 0 },
        finetune_editor: crate::modal::widgets::FineTuneEditor::new(Default::default()),
    }));
    press(&mut app, KeyCode::Char('?'));
    app.open_help_modal();
    assert!(app.help_modal().is_none());
    app.cancel_modal();

    app.modal = Some(crate::modal::Modal::DownloadEdit(DownloadEditModal {
        download_id: 1,
        finetune_editor: crate::modal::widgets::FineTuneEditor::new(Default::default()),
        queue_picker: crate::modal::widgets::QueuePicker { cursor: 0 },
        focusing_queue: false,
        original_queue_id: 1,
        error: None,
    }));
    press(&mut app, KeyCode::Char('?'));
    app.open_help_modal();
    assert!(app.help_modal().is_none());
    app.cancel_download_modal();

    app.open_confirmation(ConfirmationModal::new(
        "Confirm",
        "Message",
        "Yes",
        "No",
        PendingConfirmationAction::DeleteDownloadFiles { download_id: 1 },
    ));
    press(&mut app, KeyCode::Char('?'));
    app.open_help_modal();
    assert!(app.help_modal().is_none());
    assert!(app.confirmation_modal().is_some());
}

#[test]
fn queue_editor_can_save_from_download_items_tab() {
    let mut app = test_app();
    app.open_create_queue_modal();
    let modal = app.queue_modal_mut().unwrap();
    modal.mode = QueueModalMode::Edit { queue_id: 1 };
    modal.tab = QueueModalTab::DownloadItems;
    modal.name = "Queue".into();

    press(&mut app, KeyCode::Char('s'));

    assert!(app.queue_modal().is_none());
}

#[test]
fn one_time_schedule_uses_adjustable_date_and_time_fields() {
    use chrono::Duration as ChronoDuration;

    let mut app = test_app();
    app.open_create_queue_modal();
    let modal = app.queue_modal_mut().unwrap();
    modal.tab = QueueModalTab::Scheduler;
    modal.recurrence_kind = RecurrenceKind::Once;
    modal.scheduler_cursor = 2;

    let start_date = modal.once_start_date;
    press(&mut app, KeyCode::Right);
    assert_eq!(
        app.queue_modal().unwrap().once_start_date,
        start_date + ChronoDuration::days(1)
    );

    press(&mut app, KeyCode::Down);
    let start_time = app.queue_modal().unwrap().once_start_time;
    press(&mut app, KeyCode::Right);
    assert_eq!(
        app.queue_modal().unwrap().once_start_time,
        start_time + ChronoDuration::minutes(5)
    );

    press(&mut app, KeyCode::Enter);
    assert!(!app.queue_modal().unwrap().name_input.editing);
}

#[test]
fn help_search_accepts_shortcut_characters_and_clears_or_keeps_filter() {
    let mut app = test_app();
    app.open_help_modal();
    app.help_modal_mut().unwrap().set_dimensions(100, 10);
    press(&mut app, KeyCode::End);
    press(&mut app, KeyCode::Char('/'));
    for c in "q?j/ké".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    let modal = app.help_modal().unwrap();
    assert_eq!(modal.query, "q?j/ké");
    assert_eq!(modal.scroll, 0);
    assert!(modal.editing_search);
    press(&mut app, KeyCode::Backspace);
    assert_eq!(app.help_modal().unwrap().query, "q?j/k");
    press(&mut app, KeyCode::Enter);
    assert!(!app.help_modal().unwrap().editing_search);
    assert_eq!(app.help_modal().unwrap().query, "q?j/k");
    press(&mut app, KeyCode::Char('/'));
    press(&mut app, KeyCode::Esc);
    let modal = app.help_modal().unwrap();
    assert!(!modal.editing_search);
    assert!(modal.query.is_empty());
    assert!(!app.should_quit);
    press(&mut app, KeyCode::Char('/'));
    press(&mut app, KeyCode::Char('x'));
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Esc);
    app.open_help_modal();
    let modal = app.help_modal().unwrap();
    assert!(modal.query.is_empty());
    assert_eq!(modal.scroll, 0);
    assert!(!modal.editing_search);
}

#[test]
fn help_navigation_uses_viewport_and_ctrl_c_still_quits() {
    let mut app = test_app();
    app.open_help_modal();
    app.help_modal_mut().unwrap().set_dimensions(50, 10);
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
        assert_eq!(app.help_modal().unwrap().scroll, expected);
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
    let x = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
    assert!(matches!(
        keymap::lookup(Focus::Queues, x),
        Some(Action::RemoveCompleted)
    ));
    assert!(!matches!(
        keymap::lookup(Focus::Categories, x),
        Some(Action::RemoveCompleted)
    ));
    assert!(!matches!(
        keymap::lookup(Focus::Downloads, x),
        Some(Action::RemoveCompleted)
    ));
    assert!(!matches!(
        keymap::lookup(Focus::Queues, KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE)),
        Some(Action::RemoveCompleted)
    ));
}

#[test]
fn destructive_delete_key_is_shifted_and_scoped_to_downloads() {
    let shift_d = KeyEvent::new(KeyCode::Char('D'), KeyModifiers::NONE);
    assert!(matches!(
        keymap::lookup(Focus::Downloads, shift_d),
        Some(Action::DeleteDownloadFiles)
    ));
    assert!(!matches!(
        keymap::lookup(
            Focus::Downloads,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)
        ),
        Some(Action::DeleteDownloadFiles)
    ));
    assert!(!matches!(
        keymap::lookup(Focus::Queues, shift_d),
        Some(Action::DeleteDownloadFiles)
    ));
    assert!(!matches!(
        keymap::lookup(Focus::Categories, shift_d),
        Some(Action::DeleteDownloadFiles)
    ));
}

#[test]
fn queue_delete_key_is_lowercase_and_scoped_to_queues() {
    let d = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE);
    assert!(matches!(
        keymap::lookup(Focus::Queues, d),
        Some(Action::DeleteQueue)
    ));
    assert!(!matches!(
        keymap::lookup(Focus::Downloads, d),
        Some(Action::DeleteQueue)
    ));
    assert!(!matches!(
        keymap::lookup(Focus::Categories, d),
        Some(Action::DeleteQueue)
    ));
    assert!(!matches!(
        keymap::lookup(Focus::Queues, KeyEvent::new(KeyCode::Char('D'), KeyModifiers::NONE)),
        Some(Action::DeleteQueue)
    ));
}

#[test]
fn confirmation_modal_consumes_cancel_before_global_quit() {
    let mut app = App::new(
        Theme::default_dark(),
        crate::icons::IconSet::new(crate::icons::GlyphMode::Unicode),
        false,
    );
    app.open_confirmation(ConfirmationModal::new(
        "Confirm",
        "Message",
        "Yes",
        "No",
        PendingConfirmationAction::DeleteDownloadFiles { download_id: 1 },
    ));

    if let Some(msg) = route_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
    ) {
        let _ = update(&mut app, msg);
    }
    assert!(app.confirmation_modal().is_none());
    assert!(!app.should_quit);
}
