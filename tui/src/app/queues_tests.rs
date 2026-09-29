use super::*;
use crate::theme::Theme;
use chrono::Utc;
use common::{
    enums::{QueueStatus, Recurrence},
    finetune::FineTune,
    queue::QueueSettings,
    scheduler::Scheduler,
};

fn queue(id: i64, name: &str) -> Queue {
    Queue {
        scheduled_stop_at: None,
        id,
        name: name.into(),
        position: id as i32,
        settings: QueueSettings {
            max_concurrent_downloads: 1,
            max_retries: 3,
            retry_wait_seconds: 5,
            default_finetune: FineTune::default(),
        },
        scheduler: Scheduler {
            enabled: false,
            recurrence: Recurrence::Once {
                start: Utc::now(),
                end: Utc::now(),
            },
            run_missed_on_startup: false,
        },
        status: QueueStatus::Paused,
        created_at: Utc::now(),
    }
}

#[test]
fn start_message_includes_local_stop_date_and_time() {
    let stop = chrono::DateTime::parse_from_rfc3339("2030-01-07T17:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let message = queue_start_message(Some(stop));
    assert!(
        message.contains(
            &stop
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        )
    );
    assert!(message.starts_with("Queue started. Will pause at "));
    assert_eq!(queue_start_message(None), "Queue started");
}

fn app() -> App {
    let mut app = App::new(
        Theme::default_dark(),
        crate::icons::IconSet::new(crate::icons::GlyphMode::Unicode),
        false,
    );
    app.queues = vec![
        queue(1, "Main Queue"),
        queue(2, "Second"),
        queue(3, "Third"),
    ];
    app
}

#[test]
fn all_and_main_queue_are_not_deletable() {
    let mut app = app();
    assert!(!app.can_delete_selected_queue());
    assert!(app.request_delete_selected_queue().is_empty());

    app.selected_queue = 1;
    assert!(!app.can_delete_selected_queue());
    assert!(app.request_delete_selected_queue().is_empty());
}

#[test]
fn populated_queue_result_opens_confirmation_with_retention_copy() {
    let mut app = app();
    let _ = app.apply_queue_delete_result(
        2,
        "Second".into(),
        Ok(crate::api::DeleteQueueOutcome::NeedsConfirmation),
    );

    let modal = app.confirmation_modal().unwrap();
    assert_eq!(modal.title, "Remove queue?");
    assert!(modal.message.contains("Second"));
    assert!(modal.message.contains("Downloaded files will be kept"));
    assert_eq!(
        app.pending_confirmation_action(),
        Some(&PendingConfirmationAction::DeleteQueue { queue_id: 2 })
    );

    app.cancel_confirmation();
    assert!(app.confirmation_modal().is_none());
    assert_eq!(app.queues.len(), 3);
}

#[test]
fn late_queue_confirmation_does_not_replace_help() {
    let mut app = app();
    app.open_help_modal();
    let _ = app.apply_queue_delete_result(
        2,
        "Second".into(),
        Ok(crate::api::DeleteQueueOutcome::NeedsConfirmation),
    );
    assert!(app.help_modal().is_some());
    assert!(app.confirmation_modal().is_none());
    assert!(app.pending_confirmation_action().is_none());
}

#[test]
fn successful_delete_selects_the_next_queue_or_previous_at_end() {
    let mut app = app();
    app.selected_queue = 2;
    let _ = app.apply_queue_delete_result(
        2,
        "Second".into(),
        Ok(crate::api::DeleteQueueOutcome::Deleted),
    );
    assert_eq!(
        app.queues.iter().map(|queue| queue.id).collect::<Vec<_>>(),
        vec![1, 3]
    );
    assert_eq!(app.current_queue().map(|queue| queue.id), Some(3));
    assert!(app.toasts.iter().any(|toast| {
        toast.level == ToastLevel::Success && toast.message.contains("Second")
    }));

    // Avoid a second refresh while the first test refresh is in flight.
    app.selected_queue = 2;
    let _ = app.apply_queue_delete_result(
        3,
        "Third".into(),
        Ok(crate::api::DeleteQueueOutcome::Deleted),
    );
    assert_eq!(app.current_queue().map(|queue| queue.id), Some(1));
}

#[test]
fn delete_error_is_shown_as_a_toast() {
    let mut app = app();
    let _ = app.apply_queue_delete_result(2, "Second".into(), Err(anyhow::anyhow!("boom")));
    assert!(
        app.toasts
            .iter()
            .any(|toast| toast.level == ToastLevel::Error && toast.message == "boom")
    );
}
