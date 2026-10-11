use super::{
    support::{Fixture, weekly},
    tui_api,
};
use common::{
    download::{AddDownloadInput, AddDownloadsRequest},
    finetune::FineTune,
    queue::CreateQueueRequest,
};

#[tokio::test]
async fn client_contract_real_routes_decode_success_errors_and_confirmation() {
    let f = Fixture::new().await;
    let base = f.base.clone();
    tokio::task::spawn_blocking(move || {
        assert!(tui_api::health(&base).unwrap().aria2_reachable);
        assert_eq!(tui_api::list_queues(&base).unwrap().len(), 1);
        let req = CreateQueueRequest {
            name: "client".into(),
            position: 1,
            max_concurrent_downloads: 1,
            max_retries: 0,
            retry_wait_seconds: 1,
            default_finetune: FineTune::default(),
            scheduler_enabled: false,
            recurrence: weekly(12, 13),
            run_missed_on_startup: false,
        };
        let q = tui_api::create_queue(&base, &req).unwrap();
        assert_eq!(
            tui_api::resume_queue(&base, q.id).unwrap(),
            tui_api::QueueResumeOutcome::NothingToStart
        );
        let items = tui_api::add_downloads(
            &base,
            &AddDownloadsRequest {
                inputs: vec![AddDownloadInput::Url(
                    "http://fixture.invalid/client.bin".into(),
                )],
                queue_id: q.id,
                finetune_override: None,
                start_immediately: false,
            },
        )
        .unwrap();
        let id = items[0].download.id;
        assert_eq!(
            tui_api::list_downloads(&base, &Default::default())
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            tui_api::delete_queue(&base, q.id, false).unwrap(),
            tui_api::DeleteQueueOutcome::NeedsConfirmation
        );
        assert_eq!(
            tui_api::resume_queue(&base, q.id).unwrap(),
            tui_api::QueueResumeOutcome::Started
        );
        tui_api::pause_queue(&base, q.id).unwrap();
        tui_api::resume_download(&base, id).unwrap();
        tui_api::pause_download(&base, id).unwrap();
        assert!(
            tui_api::resume_download(&base, 99999)
                .unwrap_err()
                .to_string()
                .contains("99999")
        );
        tui_api::delete_download(&base, id).unwrap();
        assert_eq!(
            tui_api::delete_queue(&base, q.id, false).unwrap(),
            tui_api::DeleteQueueOutcome::Deleted
        );
        let q = tui_api::create_queue(&base, &req).unwrap();
        tui_api::add_downloads(
            &base,
            &AddDownloadsRequest {
                inputs: vec![AddDownloadInput::Url(
                    "http://fixture.invalid/confirmed.bin".into(),
                )],
                queue_id: q.id,
                finetune_override: None,
                start_immediately: false,
            },
        )
        .unwrap();
        assert_eq!(
            tui_api::delete_queue(&base, q.id, true).unwrap(),
            tui_api::DeleteQueueOutcome::Deleted
        );
    })
    .await
    .unwrap();
    f.close().await;
}
