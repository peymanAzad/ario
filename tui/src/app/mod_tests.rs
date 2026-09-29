use super::*;
use common::download::DeleteDownloadFilesResult;

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
