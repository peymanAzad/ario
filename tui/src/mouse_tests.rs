use super::*;
use crate::{
    app::{PendingConfirmationAction, update},
    effects::{ApiRequest, Effect},
    icons::{GlyphMode, IconSet},
    modal::{
        ClipboardImportModal, ConfirmationModal, DownloadEditModal, HelpModal, QueueModal,
        QueueModalTab, TorrentFileModal,
    },
    theme::Theme,
};
use common::{download::DownloadLiveStatus, enums::FileCategory, finetune::FineTune, queue::Queue};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::json;

fn app() -> App {
    App::new(
        Theme::default_dark(),
        IconSet::new(GlyphMode::Unicode),
        false,
    )
}
fn download(id: i64) -> DownloadLiveStatus {
    serde_json::from_value(json!({"download":{"id":id,"aria2_gid":null,"url":format!("https://example.test/{id}"),"filename":format!("file-{id}.bin"),"destination_path":"/tmp","source_type":"Http","category":"Other","status":"Pending","paused_by_scheduler":false,"manually_started":false,"size":100,"completed_length":0,"queue_id":1,"position_in_queue":id,"finetune":{},"created_at":"2030-01-07T12:00:00Z","started_at":null,"completed_at":null},"completed_length":0,"download_speed":0,"eta_seconds":null})).unwrap()
}
fn queue(id: i64) -> Queue {
    serde_json::from_value(json!({"id":id,"name":format!("Queue {id}"),"position":id,"status":"Paused","scheduled_stop_at":null,"settings":{"max_concurrent_downloads":1,"max_retries":3,"retry_wait_seconds":5,"default_finetune":{}},"scheduler":{"enabled":false,"recurrence":{"Weekly":{"days":["Mon"],"start_time":"02:00:00","end_time":"06:00:00"}},"run_missed_on_startup":false},"created_at":"2030-01-07T12:00:00Z"})).unwrap()
}
fn draw(app: &mut App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| crate::ui::render(app, f)).unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect()
}
fn event(kind: MouseEventKind, area: Rect) -> MouseEvent {
    MouseEvent {
        kind,
        column: area.x,
        row: area.y,
        modifiers: KeyModifiers::NONE,
    }
}
fn send(app: &mut App, event: MouseEvent) -> Vec<Effect> {
    route_mouse(app, event)
        .map(|msg| update::update(app, msg))
        .unwrap_or_default()
}
fn hit(app: &App, predicate: impl Fn(&Target) -> bool) -> Rect {
    app.interactions
        .borrow()
        .hits
        .iter()
        .rev()
        .find(|hit| predicate(&hit.target))
        .expect("visible target")
        .area
}
fn intent(app: &App, wanted: ModalIntent) -> Rect {
    hit(app, |t| matches!(t, Target::Modal(i) if *i == wanted))
}
fn click(app: &mut App, wanted: ModalIntent) -> Vec<Effect> {
    let area = intent(app, wanted);
    send(app, event(MouseEventKind::Down(MouseButton::Left), area))
}
fn wheel(app: &mut App, key: ScrollTarget, down: bool) -> Vec<Effect> {
    let area = app
        .interactions
        .borrow()
        .scrolls
        .iter()
        .find(|(_, k)| *k == key)
        .unwrap()
        .0;
    send(
        app,
        event(
            if down {
                MouseEventKind::ScrollDown
            } else {
                MouseEventKind::ScrollUp
            },
            area,
        ),
    )
}
fn refreshed(app: &App, revision: u64, downloads: Vec<DownloadLiveStatus>) -> Msg {
    Msg::Api(crate::msg::ApiResult::Refreshed {
        filter_revision: revision,
        downloads: Ok(downloads),
        queues: Ok(app.queues.clone()),
        server_reachable: true,
        aria2_reachable: true,
        download_speed: 0,
        active_downloads: 0,
        aria2_global_options: None,
        lifecycle_revision: app.lifecycle_revision,
    })
}

#[test]
fn table_clicks_use_the_rendered_offset_and_ignore_headers_and_details() {
    let mut app = app();
    app.downloads = (1..=60).map(download).collect();
    app.selected_download = 40;
    draw(&mut app, 100, 15);
    let map = app.interactions.borrow();
    let offset = map.table.offset();
    assert!(offset > 0);
    let row = map
        .hits
        .iter()
        .find(|h| matches!(h.target, Target::Row(Focus::Downloads, index, _) if index == offset))
        .unwrap()
        .area;
    let table_body = map
        .scrolls
        .iter()
        .find(|(_, k)| *k == ScrollTarget::Downloads)
        .unwrap()
        .0;
    drop(map);
    send(
        &mut app,
        event(MouseEventKind::Down(MouseButton::Left), row),
    );
    assert_eq!(app.selected_download, offset);
    send(
        &mut app,
        event(
            MouseEventKind::Down(MouseButton::Left),
            Rect::new(table_body.x, table_body.y - 1, 1, 1),
        ),
    );
    assert_eq!(app.selected_download, offset);
    assert!(app.modal.is_none());
}
#[test]
fn main_wheel_focuses_hovered_pane_and_clamps_without_wrapping() {
    let mut app = app();
    app.downloads = vec![download(1), download(2)];
    draw(&mut app, 100, 24);
    wheel(&mut app, ScrollTarget::Downloads, true);
    assert_eq!(app.selected_download, 1);
    wheel(&mut app, ScrollTarget::Downloads, true);
    assert_eq!(app.selected_download, 1);
    wheel(&mut app, ScrollTarget::Downloads, false);
    assert_eq!(app.selected_download, 0);
    app.focus = Focus::Downloads;
    let effects = wheel(&mut app, ScrollTarget::Categories, true);
    assert_eq!(app.focus, Focus::Categories);
    assert_eq!(app.selected_category, 1);
    assert_eq!(effects.len(), 1);
    assert!(app.downloads.is_empty());
}
#[test]
fn queue_click_and_wheel_change_filters_and_same_selection_is_a_noop() {
    let mut app = app();
    app.queues = vec![queue(1), queue(2)];
    draw(&mut app, 100, 24);
    let row = hit(&app, |t| matches!(t, Target::Row(Focus::Queues, 1, _)));
    let effects = send(
        &mut app,
        event(MouseEventKind::Down(MouseButton::Left), row),
    );
    assert_eq!(app.selected_queue, 1);
    assert_eq!(effects.len(), 1);
    let version = app.filter_revision;
    update::update(
        &mut app,
        Msg::Action(Action::SelectAt {
            focus: Focus::Queues,
            index: 1,
        }),
    );
    assert_eq!(app.filter_revision, version);
    wheel(&mut app, ScrollTarget::Queues, true);
    assert_eq!(app.selected_queue, 2);
    wheel(&mut app, ScrollTarget::Queues, true);
    assert_eq!(app.selected_queue, 2);
    assert_eq!(app.filter_revision, version + 1);
}
#[test]
fn download_double_click_is_timed_and_runs_only_once() {
    let mut app = app();
    app.downloads = vec![download(1)];
    draw(&mut app, 100, 24);
    let row = hit(&app, |t| matches!(t, Target::Row(Focus::Downloads, _, _)));
    let now = Instant::now();
    let msg = route_mouse_at(
        &mut app,
        event(MouseEventKind::Down(MouseButton::Left), row),
        now,
    )
    .unwrap();
    update::update(&mut app, msg);
    assert!(app.modal.is_none());
    assert!(
        route_mouse_at(
            &mut app,
            event(MouseEventKind::Up(MouseButton::Left), row),
            now
        )
        .is_none()
    );
    let msg = route_mouse_at(
        &mut app,
        event(MouseEventKind::Down(MouseButton::Left), row),
        now + Duration::from_millis(400),
    )
    .unwrap();
    assert!(matches!(msg, Msg::Action(Action::ActivateDownload)));
    update::update(&mut app, msg);
    assert!(matches!(app.modal, Some(Modal::DownloadEdit(_))));
    assert!(app.interactions.borrow().last_click.is_none());
}
#[test]
fn double_click_expires_and_keyboard_or_scroll_resets_it() {
    let mut app = app();
    app.downloads = vec![download(1), download(2)];
    draw(&mut app, 100, 24);
    let row = hit(&app, |t| matches!(t, Target::Row(Focus::Downloads, 0, _)));
    let now = Instant::now();
    route_mouse_at(
        &mut app,
        event(MouseEventKind::Down(MouseButton::Left), row),
        now,
    );
    let msg = route_mouse_at(
        &mut app,
        event(MouseEventKind::Down(MouseButton::Left), row),
        now + Duration::from_millis(401),
    );
    assert!(matches!(msg, Some(Msg::Action(Action::SelectAt { .. }))));
    crate::keymap::route_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE),
    );
    assert!(app.interactions.borrow().last_click.is_none());
    route_mouse_at(
        &mut app,
        event(MouseEventKind::Down(MouseButton::Left), row),
        now,
    );
    wheel(&mut app, ScrollTarget::Downloads, true);
    assert!(app.interactions.borrow().last_click.is_none());
}
#[test]
fn double_click_requires_the_same_identity_and_focus() {
    let mut app = app();
    app.downloads = vec![download(1), download(2)];
    draw(&mut app, 100, 24);
    let first = hit(&app, |t| matches!(t, Target::Row(Focus::Downloads, 0, _)));
    let second = hit(&app, |t| matches!(t, Target::Row(Focus::Downloads, 1, _)));
    let now = Instant::now();
    route_mouse_at(
        &mut app,
        event(MouseEventKind::Down(MouseButton::Left), first),
        now,
    );
    assert!(matches!(
        route_mouse_at(
            &mut app,
            event(MouseEventKind::Down(MouseButton::Left), second),
            now
        ),
        Some(Msg::Action(Action::SelectAt { index: 1, .. }))
    ));
    app.focus = Focus::Queues;
    assert!(matches!(
        route_mouse_at(
            &mut app,
            event(MouseEventKind::Down(MouseButton::Left), second),
            now
        ),
        Some(Msg::Action(Action::SelectAt { .. }))
    ));
}
#[test]
fn queue_double_click_opens_edit_and_all_row_never_activates() {
    let mut app = app();
    app.queues = vec![queue(1)];
    draw(&mut app, 100, 24);
    let row = hit(&app, |t| matches!(t, Target::Row(Focus::Queues, 1, _)));
    let now = Instant::now();
    let msg = route_mouse_at(
        &mut app,
        event(MouseEventKind::Down(MouseButton::Left), row),
        now,
    )
    .unwrap();
    update::update(&mut app, msg);
    let msg = route_mouse_at(
        &mut app,
        event(MouseEventKind::Down(MouseButton::Left), row),
        now,
    )
    .unwrap();
    assert!(matches!(msg, Msg::Action(Action::OpenEditQueue)));
    let effects = update::update(&mut app, msg);
    assert!(matches!(
        effects.as_slice(),
        [Effect::Api(ApiRequest::ListQueueDownloads { queue_id: 1 })]
    ));
    app.modal = None;
    draw(&mut app, 100, 24);
    let all = hit(&app, |t| matches!(t, Target::Row(Focus::Queues, 0, _)));
    route_mouse_at(
        &mut app,
        event(MouseEventKind::Down(MouseButton::Left), all),
        now,
    );
    assert!(matches!(
        route_mouse_at(
            &mut app,
            event(MouseEventKind::Down(MouseButton::Left), all),
            now
        ),
        Some(Msg::Action(Action::SelectAt { .. }))
    ));
}
#[test]
fn modal_blocks_background_buttons_clicks_and_wheel() {
    let mut app = app();
    app.downloads = vec![download(1)];
    app.modal = Some(Modal::Queue(QueueModal::create()));
    draw(&mut app, 120, 40);
    assert!(
        app.interactions
            .borrow()
            .hits
            .iter()
            .all(|h| matches!(h.target, Target::Modal(_)))
    );
    send(
        &mut app,
        event(
            MouseEventKind::Down(MouseButton::Left),
            Rect::new(0, 0, 1, 1),
        ),
    );
    send(
        &mut app,
        event(MouseEventKind::ScrollDown, Rect::new(0, 5, 1, 1)),
    );
    assert_eq!(app.selected_download, 0);
    assert_eq!(app.focus, Focus::Downloads);
    assert!(app.modal.is_some());
}
#[test]
fn url_checkboxes_are_separate_from_row_selection_and_use_scrolled_indices() {
    let mut app = app();
    app.modal = Some(Modal::ClipboardImport(ClipboardImportModal::from_urls(
        (0..40)
            .map(|i| format!("https://example.test/{i}"))
            .collect(),
        0,
        &[],
    )));
    if let Some(Modal::ClipboardImport(m)) = &mut app.modal {
        m.url_cursor = 30;
    }
    draw(&mut app, 100, 20);
    let offset = app.interactions.borrow().lists[&ScrollTarget::Urls].offset();
    assert!(offset > 0);
    let row = intent(&app, ModalIntent::Field(offset));
    send(
        &mut app,
        event(
            MouseEventKind::Down(MouseButton::Left),
            Rect::new(row.x + 8, row.y, 1, 1),
        ),
    );
    if let Some(Modal::ClipboardImport(m)) = &app.modal {
        assert_eq!(m.url_cursor, offset);
        assert!(m.entries[offset].selected);
    }
    click(&mut app, ModalIntent::ToggleUrl(offset));
    if let Some(Modal::ClipboardImport(m)) = &app.modal {
        assert!(!m.entries[offset].selected);
    }
    wheel(&mut app, ScrollTarget::Urls, true);
    if let Some(Modal::ClipboardImport(m)) = &app.modal {
        assert_eq!(m.url_cursor, offset + 1);
        assert!(m.entries[offset + 1].selected);
    }
    click(&mut app, ModalIntent::Command(Command::None));
    if let Some(Modal::ClipboardImport(m)) = &app.modal {
        assert!(m.entries.iter().all(|e| !e.selected));
    }
    click(&mut app, ModalIntent::Command(Command::All));
    if let Some(Modal::ClipboardImport(m)) = &app.modal {
        assert!(m.entries.iter().all(|e| e.selected));
    }
}
#[test]
fn clipboard_tabs_fields_adjustments_queue_selector_and_submit_work() {
    let mut app = app();
    app.queues = vec![queue(1), queue(2)];
    app.modal = Some(Modal::ClipboardImport(ClipboardImportModal::from_urls(
        vec!["https://example.test/1".into()],
        0,
        &app.queues,
    )));
    draw(&mut app, 120, 40);
    click(
        &mut app,
        ModalIntent::Adjust {
            field: None,
            forward: true,
        },
    );
    if let Some(Modal::ClipboardImport(m)) = &app.modal {
        assert_eq!(m.queue_picker.cursor, 1);
    }
    click(&mut app, ModalIntent::Tab(1));
    draw(&mut app, 120, 40);
    click(
        &mut app,
        ModalIntent::Adjust {
            field: Some(0),
            forward: true,
        },
    );
    wheel(&mut app, ScrollTarget::Fields, true);
    if let Some(Modal::ClipboardImport(m)) = &app.modal {
        assert_eq!(m.finetune_editor.finetune.connections_per_download, Some(1));
        assert_eq!(m.finetune_editor.cursor, 1);
    }
    let effects = click(&mut app, ModalIntent::Command(Command::Later));
    assert!(effects.iter().any(|e| matches!(e, Effect::Api(ApiRequest::AddDownloads(r)) if !r.start_immediately && r.queue_id == 2 && r.finetune_override.as_ref().unwrap().connections_per_download == Some(1))));
    assert!(app.modal.is_none());
}
#[test]
fn queue_name_draft_survives_tab_and_field_changes_and_save_uses_it() {
    let mut app = app();
    app.modal = Some(Modal::Queue(QueueModal::create()));
    draw(&mut app, 120, 40);
    click(&mut app, ModalIntent::Text);
    crate::keymap::paste(&mut app, "My queue 界");
    click(&mut app, ModalIntent::Tab(1));
    if let Some(Modal::Queue(m)) = &app.modal {
        assert_eq!(m.name, "My queue 界");
        assert!(!m.name_input.editing);
    }
    draw(&mut app, 120, 40);
    click(&mut app, ModalIntent::Tab(0));
    draw(&mut app, 120, 40);
    click(&mut app, ModalIntent::Text);
    crate::keymap::paste(&mut app, " new");
    let effects = click(&mut app, ModalIntent::Command(Command::Save));
    assert!(
        matches!(effects.as_slice(), [Effect::Api(ApiRequest::CreateQueue(r))] if r.name == "My queue 界 new")
    );
    assert!(app.modal.is_none());
}
#[test]
fn queue_invalid_save_stays_open_and_name_discard_and_cancel_work() {
    let mut app = app();
    app.modal = Some(Modal::Queue(QueueModal::create()));
    draw(&mut app, 120, 40);
    assert!(click(&mut app, ModalIntent::Command(Command::Save)).is_empty());
    if let Some(Modal::Queue(m)) = &app.modal {
        assert!(m.error.is_some());
    }
    click(&mut app, ModalIntent::Text);
    crate::keymap::paste(&mut app, "draft");
    draw(&mut app, 120, 40);
    click(&mut app, ModalIntent::Command(Command::Discard));
    if let Some(Modal::Queue(m)) = &app.modal {
        assert!(m.name.is_empty());
    }
    draw(&mut app, 120, 40);
    click(&mut app, ModalIntent::Command(Command::Cancel));
    assert!(app.modal.is_none());
}
#[test]
fn scheduler_controls_and_wheel_select_without_changing_values() {
    let mut app = app();
    let mut modal = QueueModal::create();
    modal.tab = QueueModalTab::Scheduler;
    app.modal = Some(Modal::Queue(modal));
    draw(&mut app, 120, 40);
    click(
        &mut app,
        ModalIntent::Adjust {
            field: Some(0),
            forward: true,
        },
    );
    click(&mut app, ModalIntent::Day(3));
    wheel(&mut app, ScrollTarget::Scheduler, true);
    if let Some(Modal::Queue(m)) = &app.modal {
        assert!(m.scheduler_enabled);
        assert!(m.weekly_days[3]);
        assert_eq!(m.scheduler_cursor, 3);
        assert_eq!(m.weekly_start.to_string(), "02:00:00");
    }
    click(
        &mut app,
        ModalIntent::Adjust {
            field: Some(3),
            forward: true,
        },
    );
    if let Some(Modal::Queue(m)) = &app.modal {
        assert_ne!(m.weekly_start.to_string(), "02:00:00");
    }
    click(
        &mut app,
        ModalIntent::Adjust {
            field: Some(1),
            forward: true,
        },
    );
    draw(&mut app, 120, 40);
    click(
        &mut app,
        ModalIntent::Adjust {
            field: Some(2),
            forward: true,
        },
    );
    if let Some(Modal::Queue(m)) = &app.modal {
        assert_eq!(m.recurrence_kind, crate::modal::RecurrenceKind::Once);
    }
}
#[test]
fn queue_items_scroll_and_reorder_only_when_requested() {
    let mut app = app();
    let mut modal = QueueModal::edit(&queue(1));
    modal.tab = QueueModalTab::DownloadItems;
    modal.set_items((1..=40).map(|id| download(id).download).collect());
    modal.item_cursor = 30;
    app.modal = Some(Modal::Queue(modal));
    draw(&mut app, 120, 20);
    assert!(app.interactions.borrow().lists[&ScrollTarget::Items].offset() > 0);
    wheel(&mut app, ScrollTarget::Items, true);
    if let Some(Modal::Queue(m)) = &app.modal {
        assert_eq!(m.item_cursor, 31);
        assert_eq!(m.items[31].id, 32);
    }
    click(&mut app, ModalIntent::Command(Command::ItemUp));
    if let Some(Modal::Queue(m)) = &app.modal {
        assert_eq!(m.item_cursor, 30);
        assert_eq!(m.items[30].id, 32);
    }
    click(&mut app, ModalIntent::Command(Command::ItemDown));
    if let Some(Modal::Queue(m)) = &app.modal {
        assert_eq!(m.item_cursor, 31);
        assert_eq!(m.items[31].id, 32);
    }
}
#[test]
fn download_editor_can_select_adjust_and_save_queue_and_fields() {
    let mut app = app();
    app.queues = vec![queue(1), queue(2)];
    app.modal = Some(Modal::DownloadEdit(DownloadEditModal::editing(
        7,
        FineTune::default(),
        0,
    )));
    draw(&mut app, 120, 40);
    click(&mut app, ModalIntent::Field(3));
    click(
        &mut app,
        ModalIntent::Adjust {
            field: Some(0),
            forward: true,
        },
    );
    click(
        &mut app,
        ModalIntent::Adjust {
            field: Some(6),
            forward: true,
        },
    );
    if let Some(Modal::DownloadEdit(m)) = &app.modal {
        assert!(m.focusing_queue);
        assert_eq!(m.queue_picker.cursor, 1);
        assert_eq!(m.finetune_editor.finetune.connections_per_download, Some(1));
    }
    let effects = click(&mut app, ModalIntent::Command(Command::Save));
    assert!(matches!(
        effects.as_slice(),
        [Effect::Api(ApiRequest::SaveDownloadEdit {
            id: 7,
            queue_id: 2,
            ..
        })]
    ));
}
#[test]
fn download_editor_validation_failure_keeps_dialog_open() {
    let mut app = app();
    app.modal = Some(Modal::DownloadEdit(DownloadEditModal::editing(
        7,
        FineTune::default(),
        0,
    )));
    draw(&mut app, 120, 40);
    assert!(click(&mut app, ModalIntent::Command(Command::Save)).is_empty());
    if let Some(Modal::DownloadEdit(m)) = &app.modal {
        assert!(m.error.is_some());
    } else {
        panic!("dialog closed");
    }
}
#[test]
fn torrent_draft_survives_tabs_and_buttons_validate_while_editing() {
    let mut app = app();
    app.queues = vec![queue(1), queue(2)];
    app.modal = Some(Modal::TorrentFile(TorrentFileModal::open(0, &app.queues)));
    draw(&mut app, 120, 40);
    crate::keymap::paste(&mut app, "/tmp/no-such-ario-mouse-test.torrent");
    click(&mut app, ModalIntent::Tab(1));
    draw(&mut app, 120, 40);
    click(
        &mut app,
        ModalIntent::Adjust {
            field: Some(0),
            forward: true,
        },
    );
    click(&mut app, ModalIntent::Tab(0));
    draw(&mut app, 120, 40);
    click(&mut app, ModalIntent::Text);
    if let Some(Modal::TorrentFile(m)) = &app.modal {
        assert!(m.path_input.editing);
        assert_eq!(m.path_input.buffer, "/tmp/no-such-ario-mouse-test.torrent");
    }
    let effects = click(&mut app, ModalIntent::Command(Command::Start));
    assert!(effects.is_empty());
    assert!(app.modal.is_some());
    assert!(!app.toasts.is_empty());
    click(&mut app, ModalIntent::Command(Command::Cancel));
    assert!(app.modal.is_none());
}
#[test]
fn help_scroll_search_and_close_are_clickable_and_keep_query() {
    let mut app = app();
    app.modal = Some(Modal::Help(HelpModal::default()));
    draw(&mut app, 100, 15);
    wheel(&mut app, ScrollTarget::Help, true);
    if let Some(Modal::Help(m)) = &app.modal {
        assert_eq!(m.scroll, 1);
    }
    click(&mut app, ModalIntent::Text);
    crate::keymap::paste(&mut app, "download");
    draw(&mut app, 100, 15);
    click(&mut app, ModalIntent::Command(Command::Accept));
    if let Some(Modal::Help(m)) = &app.modal {
        assert_eq!(m.query, "download");
        assert!(!m.editing_search);
    }
    draw(&mut app, 100, 15);
    click(&mut app, ModalIntent::Command(Command::Cancel));
    assert!(app.modal.is_none());
}
#[test]
fn confirmation_requires_its_own_button_and_emits_the_captured_id() {
    let mut app = app();
    app.modal = Some(Modal::Confirmation(ConfirmationModal::new(
        "Delete",
        "Delete files?",
        "Delete",
        "Cancel",
        PendingConfirmationAction::DeleteDownloadFiles { download_id: 77 },
    )));
    draw(&mut app, 120, 40);
    send(
        &mut app,
        event(
            MouseEventKind::Down(MouseButton::Left),
            Rect::new(0, 0, 1, 1),
        ),
    );
    assert!(app.modal.is_some());
    let effects = click(&mut app, ModalIntent::Command(Command::Confirm));
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::Api(ApiRequest::DeleteDownloadFiles(77))))
    );
    assert!(app.modal.is_none());
}
#[test]
fn footer_has_distinct_delete_and_navigation_targets_and_ignores_spacing() {
    let mut app = app();
    app.downloads = vec![download(1)];
    let text = draw(&mut app, 240, 24);
    assert!(text.contains("d:Delete"));
    assert!(text.contains("D:Delete+files"));
    assert!(text.contains("j:Down"));
    assert!(text.contains("k:Up"));
    let area = hit(&app, |t| {
        matches!(t, Target::Action(Action::DeleteDownloadFiles))
    });
    send(
        &mut app,
        event(
            MouseEventKind::Down(MouseButton::Left),
            Rect::new(area.x - 1, area.y, 1, 1),
        ),
    );
    assert!(app.modal.is_none());
    send(
        &mut app,
        event(MouseEventKind::Down(MouseButton::Left), area),
    );
    assert!(matches!(app.modal, Some(Modal::Confirmation(_))));
}
#[test]
fn ignored_mouse_events_do_not_change_state_or_emit_actions() {
    let mut app = app();
    app.downloads = vec![download(1), download(2)];
    draw(&mut app, 100, 24);
    let row = hit(&app, |t| matches!(t, Target::Row(Focus::Downloads, 1, _)));
    for kind in [
        MouseEventKind::Up(MouseButton::Left),
        MouseEventKind::Down(MouseButton::Right),
        MouseEventKind::Down(MouseButton::Middle),
        MouseEventKind::Drag(MouseButton::Left),
        MouseEventKind::Moved,
        MouseEventKind::ScrollLeft,
        MouseEventKind::ScrollRight,
    ] {
        assert!(send(&mut app, event(kind, row)).is_empty());
        assert_eq!(app.selected_download, 0);
        assert!(app.modal.is_none());
    }
}
#[test]
fn rapid_filters_discard_old_results_and_fetch_the_latest_immediately() {
    let mut app = app();
    let original = app.filter_revision;
    app.refresh();
    app.select_next_category();
    app.select_next_category();
    app.select_next_category();
    assert!(app.downloads.is_empty());
    let msg = refreshed(&app, original, vec![download(1)]);
    let effects = update::update(&mut app, msg);
    assert!(app.downloads.is_empty());
    assert!(
        matches!(effects.as_slice(), [Effect::Api(ApiRequest::Refresh { filter_revision, filter, .. })] if *filter_revision == app.filter_revision && filter.category == Some(FileCategory::Document))
    );
    let msg = refreshed(&app, app.filter_revision, vec![download(2)]);
    assert!(update::update(&mut app, msg).is_empty());
    assert_eq!(app.downloads[0].download.id, 2);
}
#[test]
fn refresh_preserves_selection_by_id_when_rows_reorder() {
    let mut app = app();
    app.downloads = vec![download(1), download(2), download(3)];
    app.selected_download = 1;
    app.queues = vec![queue(1), queue(2)];
    app.selected_queue = 2;
    app.apply_refresh(
        Ok(vec![download(2), download(3), download(1)]),
        Ok(vec![queue(2), queue(1)]),
        crate::app::RefreshStatus {
            server_reachable: true,
            aria2_reachable: true,
            download_speed: 0,
            active_downloads: 0,
            aria2_global_options: None,
        },
        0,
    );
    assert_eq!(app.selected_download, 0);
    assert_eq!(app.selected_queue, 1);
}
#[test]
fn spans_use_unicode_cell_widths_and_clip_without_making_spacing_clickable() {
    let map = RefCell::new(Interactions::default());
    let spans = [Span::raw("界é"), Span::raw("  "), Span::raw("◀ long ▶")];
    super::spans(
        &map,
        Rect::new(4, 2, 8, 1),
        &spans,
        &[
            (0, Target::Focus(Focus::Queues)),
            (2, Target::Focus(Focus::Downloads)),
        ],
    );
    let map = map.borrow();
    assert_eq!(map.hits[0].area, Rect::new(4, 2, 3, 1));
    assert_eq!(map.hits[1].area, Rect::new(9, 2, 3, 1));
    assert!(
        !map.hits
            .iter()
            .any(|h| h.area.contains(Position::new(7, 2)))
    );
}
#[test]
fn every_view_handles_tiny_sizes_and_only_registers_visible_controls() {
    for modal in 0..9 {
        for (width, height) in [(1, 1), (2, 2), (8, 5), (30, 12), (120, 40)] {
            let mut app = app();
            app.downloads = (1..=40).map(download).collect();
            app.queues = (1..=40).map(queue).collect();
            app.selected_download = 30;
            app.modal = match modal {
                0 => None,
                1 => Some(Modal::ClipboardImport(ClipboardImportModal::from_urls(
                    vec!["https://example.test".into()],
                    0,
                    &app.queues,
                ))),
                2 => Some(Modal::TorrentFile(TorrentFileModal::open(0, &app.queues))),
                3 => Some(Modal::DownloadEdit(DownloadEditModal::editing(
                    1,
                    FineTune::default(),
                    0,
                ))),
                4 => Some(Modal::Help(HelpModal::default())),
                5 => Some(Modal::Confirmation(ConfirmationModal::new(
                    "Delete",
                    "Confirm",
                    "Delete",
                    "Cancel",
                    PendingConfirmationAction::DeleteQueue { queue_id: 2 },
                ))),
                index => {
                    let mut m = QueueModal::edit(&queue(1));
                    m.tab = match index {
                        6 => QueueModalTab::Common,
                        7 => QueueModalTab::Scheduler,
                        _ => QueueModalTab::DownloadItems,
                    };
                    m.set_items((1..=30).map(|id| download(id).download).collect());
                    m.item_cursor = 25;
                    m.scheduler_cursor = 5;
                    Some(Modal::Queue(m))
                }
            };
            draw(&mut app, width, height);
            let map = app.interactions.borrow();
            let screen = Rect::new(0, 0, width, height);
            assert!(
                map.hits
                    .iter()
                    .all(|h| h.area.intersection(screen) == h.area),
                "modal {modal}, {width}x{height}"
            );
            assert!(
                map.scrolls
                    .iter()
                    .all(|(area, _)| area.intersection(screen) == *area)
            );
        }
    }
}
#[test]
fn resize_and_removal_clamp_offsets_and_keep_hit_indices_valid() {
    let mut app = app();
    app.downloads = (1..=60).map(download).collect();
    app.selected_download = 59;
    draw(&mut app, 100, 10);
    assert!(app.interactions.borrow().table.offset() > 0);
    app.downloads.truncate(2);
    app.selected_download = 1;
    draw(&mut app, 100, 40);
    assert!(app.interactions.borrow().table.offset() < 2);
    assert!(
        app.interactions.borrow().hits.iter().all(
            |hit| !matches!(hit.target, Target::Row(Focus::Downloads, index, _) if index >= 2)
        )
    );
    app.downloads.clear();
    draw(&mut app, 100, 40);
    wheel(&mut app, ScrollTarget::Downloads, true);
    assert_eq!(app.selected_download, 0);
}

#[test]
fn short_scheduler_and_field_lists_keep_selected_fields_visible() {
    let mut app = app();
    let mut modal = QueueModal::create();
    modal.tab = QueueModalTab::Scheduler;
    modal.scheduler_cursor = 5;
    app.modal = Some(Modal::Queue(modal));
    draw(&mut app, 100, 12);
    assert!(app.interactions.borrow().lists[&ScrollTarget::Scheduler].offset() > 0);
    assert!(
        !intent(
            &app,
            ModalIntent::Adjust {
                field: Some(5),
                forward: true
            }
        )
        .is_empty()
    );
    wheel(&mut app, ScrollTarget::Scheduler, false);
    draw(&mut app, 100, 12);
    assert!(!intent(&app, ModalIntent::Field(4)).is_empty());
    app.modal = Some(Modal::DownloadEdit(DownloadEditModal::editing(
        1,
        FineTune::default(),
        0,
    )));
    if let Some(Modal::DownloadEdit(m)) = &mut app.modal {
        m.focusing_queue = true;
    }
    draw(&mut app, 100, 12);
    assert!(app.interactions.borrow().lists[&ScrollTarget::Fields].offset() > 0);
    assert!(!intent(&app, ModalIntent::Field(6)).is_empty());
}

#[test]
fn disappearing_selected_queue_invalidates_download_filter_and_fetches_all() {
    let mut app = app();
    app.queues = vec![queue(1), queue(2)];
    app.selected_queue = 2;
    app.downloads = vec![download(1)];
    app.refresh();
    let msg = Msg::Api(crate::msg::ApiResult::Refreshed {
        filter_revision: app.filter_revision,
        downloads: Ok(vec![download(2)]),
        queues: Ok(vec![queue(1)]),
        server_reachable: true,
        aria2_reachable: true,
        download_speed: 0,
        active_downloads: 0,
        aria2_global_options: None,
        lifecycle_revision: app.lifecycle_revision,
    });
    let effects = update::update(&mut app, msg);
    assert_eq!(app.selected_queue, 0);
    assert!(app.downloads.is_empty());
    assert!(
        matches!(effects.as_slice(), [Effect::Api(ApiRequest::Refresh { filter, .. })] if filter.queue_id.is_none())
    );
}

#[test]
fn wide_glyph_clipping_does_not_create_invisible_targets() {
    let map = RefCell::new(Interactions::default());
    let line = [Span::raw("界"), Span::raw("x")];
    super::spans(
        &map,
        Rect::new(0, 0, 1, 1),
        &line,
        &[
            (0, Target::Focus(Focus::Queues)),
            (1, Target::Focus(Focus::Downloads)),
        ],
    );
    let map = map.borrow();
    assert_eq!(map.hits.len(), 1);
    assert!(matches!(
        map.hits[0].target,
        Target::Focus(Focus::Downloads)
    ));
}

#[test]
fn unicode_hit_targets_match_paragraph_clipping_at_span_boundaries() {
    use ratatui::{text::Line, widgets::Paragraph};
    for (width, labels, expected) in [
        (1, vec!["界", "x"], "x"),
        (2, vec!["a界", "x"], "a "),
        (3, vec!["a界", "x"], "a界"),
    ] {
        let map = RefCell::new(Interactions::default());
        let spans = labels.into_iter().map(Span::raw).collect::<Vec<_>>();
        let area = Rect::new(0, 0, width, 1);
        super::spans(
            &map,
            area,
            &spans,
            &[
                (0, Target::Focus(Focus::Queues)),
                (1, Target::Focus(Focus::Downloads)),
            ],
        );
        let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
        terminal
            .draw(|f| f.render_widget(Paragraph::new(Line::from(spans.clone())), area))
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert_eq!(text.trim_end(), expected.trim_end());
        if width == 2 {
            assert_eq!(map.borrow().hits.len(), 1);
            assert_eq!(map.borrow().hits[0].area.width, 1);
        }
    }
}

#[test]
fn torrent_start_and_save_use_the_draft_even_while_path_is_focused() {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "ario-mouse-submit-{}-{suffix}.torrent",
        std::process::id()
    ));
    std::fs::write(&path, b"d4:infodee").unwrap();
    let path = path.canonicalize().unwrap();
    for (command, start_immediately) in [(Command::Start, true), (Command::Later, false)] {
        let mut app = app();
        app.queues = vec![queue(1), queue(2)];
        app.modal = Some(Modal::TorrentFile(TorrentFileModal::open(2, &app.queues)));
        crate::keymap::paste(&mut app, path.to_str().unwrap());
        draw(&mut app, 120, 40);
        let effects = click(&mut app, ModalIntent::Command(command));
        assert!(
            matches!(effects.as_slice(), [Effect::Api(ApiRequest::AddTorrent { path: submitted, metadata })] if *submitted == path && metadata.queue_id == 2 && metadata.start_immediately == start_immediately)
        );
        assert!(app.modal.is_none());
    }
    std::fs::remove_file(path).unwrap();
}
