use super::*;
use crate::msg::{Action, ApiResult, Msg};
use common::download::DeleteDownloadFilesResult;
use common::finetune::FineTune;

fn app() -> App {
    App::new(
        Theme::default_dark(),
        crate::icons::IconSet::new(crate::icons::GlyphMode::Unicode),
        false,
    )
}

#[test]
fn missing_file_result_becomes_a_warning_toast() {
    let mut app = app();
    app.apply_download_files_deleted(Ok(DeleteDownloadFilesResult {
        removed_payloads: 0,
        missing_payloads: 1,
        metadata_complete: true,
    }));

    let toast = app.toasts.iter().last().unwrap();
    assert_eq!(toast.level, ToastLevel::Warning);
    assert!(toast.message.contains("already missing"));
}

#[test]
fn fully_removed_result_becomes_a_success_toast() {
    let mut app = app();
    app.apply_download_files_deleted(Ok(DeleteDownloadFilesResult {
        removed_payloads: 2,
        missing_payloads: 0,
        metadata_complete: true,
    }));

    let toast = app.toasts.iter().last().unwrap();
    assert_eq!(toast.level, ToastLevel::Success);
}

#[test]
fn download_edit_save_emits_one_request_without_refreshing_early() {
    let mut app = app();
    let finetune = FineTune::default();
    let effects = update::update(
        &mut app,
        Msg::Action(Action::SaveDownloadEdit {
            id: 7,
            queue_id: 2,
            finetune: finetune.clone(),
        }),
    );

    assert_eq!(
        effects,
        vec![Effect::Api(ApiRequest::SaveDownloadEdit {
            id: 7,
            queue_id: 2,
            finetune,
        })]
    );
    assert!(!app.refresh_in_flight);
}

#[test]
fn download_edit_result_refreshes_after_success_and_failure() {
    let mut success_app = app();
    let effects = update::update(
        &mut success_app,
        Msg::Api(ApiResult::DownloadEditSaved(Ok(()))),
    );
    assert!(matches!(
        effects.as_slice(),
        [Effect::Api(ApiRequest::Refresh { .. })]
    ));
    assert!(success_app.toasts.iter().next().is_none());

    let mut failure_app = app();
    let error = anyhow::anyhow!("queue missing").context("move download");
    let effects = update::update(
        &mut failure_app,
        Msg::Api(ApiResult::DownloadEditSaved(Err(error))),
    );
    assert!(matches!(
        effects.as_slice(),
        [Effect::Api(ApiRequest::Refresh { .. })]
    ));
    let toast = failure_app.toasts.iter().last().unwrap();
    assert_eq!(toast.level, ToastLevel::Error);
    assert_eq!(toast.message, "move download: queue missing");
}
