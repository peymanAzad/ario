use super::support::Fixture;
use common::{
    download::{AddDownloadInput, AddDownloadsRequest, DownloadLiveStatus, TorrentUploadMetadata},
    enums::{DownloadStatus, FileCategory},
    finetune::FineTune,
};
use serde_json::json;

fn torrent() -> Vec<u8> {
    let mut bytes =
        b"d4:infod6:lengthi100e4:name10:sample.mp412:piece lengthi100e6:pieces20:".to_vec();
    bytes.extend([0; 20]);
    bytes.extend(b"ee");
    bytes
}

#[tokio::test]
async fn torrent_multipart_save_restart_and_start_uses_persisted_metainfo() {
    let mut f = Fixture::new().await;
    let bytes = torrent();
    let metadata = TorrentUploadMetadata {
        queue_id: 1,
        finetune_override: None,
        start_immediately: false,
    };
    let form = reqwest::multipart::Form::new()
        .text("metadata", serde_json::to_string(&metadata).unwrap())
        .part(
            "file",
            reqwest::multipart::Part::bytes(bytes.clone()).file_name("sample.torrent"),
        );
    let r = f
        .http
        .post(format!("{}/downloads/torrent", f.base))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());
    let item: DownloadLiveStatus = r.json().await.unwrap();
    assert_eq!(item.download.category, FileCategory::Video);
    assert_eq!(f.calls("aria2.addTorrent"), 0);
    f.restart().await;
    assert_eq!(
        f.state
            .db
            .get_download_torrent_data(item.download.id)
            .unwrap(),
        Some(bytes)
    );
    f.post(&format!("/downloads/{}/resume", item.download.id))
        .await;
    assert_eq!(f.calls("aria2.addTorrent"), 1);
    f.transfer(item.download.id, "complete", 100).await;
    f.tick().await;
    f.status(item.download.id, DownloadStatus::Completed).await;
    f.close().await;
}

#[tokio::test]
async fn magnet_metadata_handoff_adopts_child_and_routes_without_duplicate_records() {
    let mut f = Fixture::new().await;
    let mut config = (*f.state.config).clone();
    let video = f.root.join("video");
    config
        .settings
        .category_locations
        .insert(FileCategory::Video, video.to_string_lossy().into());
    f.reconfigure(config).await;
    let req = AddDownloadsRequest {
        inputs: vec![AddDownloadInput::Url(
            "magnet:?xt=urn:btih:0123456789012345678901234567890123456789".into(),
        )],
        queue_id: 1,
        finetune_override: None,
        start_immediately: true,
    };
    let items: Vec<DownloadLiveStatus> = serde_json::from_value(
        f.json(reqwest::Method::POST, "/downloads", Some(json!(req)))
            .await,
    )
    .unwrap();
    let id = items[0].download.id;
    let parent = items[0].download.aria2_gid.clone();
    let child = f.resolve_magnet(id).await;
    f.tick().await;
    f.tick().await;
    let item = f.get(id).await;
    assert_ne!(item.download.aria2_gid, parent);
    assert_eq!(item.download.aria2_gid, Some(child));
    assert_eq!(item.download.filename.as_deref(), Some("resolved.mp4"));
    assert_eq!(item.download.category, FileCategory::Video);
    assert_eq!(item.download.destination_path, video.to_str().unwrap());
    assert_eq!(item.download.size, Some(100));
    assert_eq!(f.calls("aria2.changeOption"), 1);
    assert_eq!(f.calls("aria2.unpause"), 1);
    assert_eq!(
        f.state
            .db
            .list_downloads(&Default::default())
            .unwrap()
            .len(),
        1
    );
    f.transfer(id, "complete", 100).await;
    f.tick().await;
    f.status(id, DownloadStatus::Completed).await;
    assert!(video.join("resolved.mp4").exists());
    f.close().await;
}

#[tokio::test]
async fn signed_url_category_and_finetune_inheritance_reach_rpc_and_persist() {
    let mut f = Fixture::new().await;
    let mut config = (*f.state.config).clone();
    let video = f.root.join("video");
    config
        .settings
        .category_locations
        .insert(FileCategory::Video, video.to_string_lossy().into());
    f.reconfigure(config).await;
    let mut q = f.get_queue(1).await;
    q.settings.max_retries = 3;
    q.settings.retry_wait_seconds = 7;
    q.settings.default_finetune.connections_per_download = Some(4);
    q.settings.default_finetune.max_connections_per_server = Some(2);
    f.edit(&q).await;
    let req = AddDownloadsRequest {
        inputs: vec![AddDownloadInput::Url(
            "http://fixture.invalid/movie.MKV?token=secret#fragment".into(),
        )],
        queue_id: 1,
        finetune_override: Some(FineTune {
            connections_per_download: Some(6),
            ..Default::default()
        }),
        start_immediately: true,
    };
    let items: Vec<DownloadLiveStatus> = serde_json::from_value(
        f.json(reqwest::Method::POST, "/downloads", Some(json!(req)))
            .await,
    )
    .unwrap();
    let id = items[0].download.id;
    let calls = f.mock.lock().unwrap().calls.clone();
    let call = calls
        .iter()
        .find(|c| c["method"] == "aria2.addUri")
        .unwrap();
    let options = &call["params"][2];
    assert_eq!(options["dir"], video.to_str().unwrap());
    assert_eq!(options["split"], "6");
    assert_eq!(options["max-connection-per-server"], "2");
    assert_eq!(options["max-tries"], "4");
    assert_eq!(options["retry-wait"], "7");
    f.tick().await;
    let item = f.get(id).await;
    assert_eq!(item.download.category, FileCategory::Video);
    assert_eq!(item.download.filename.as_deref(), Some("movie.MKV"));
    f.json(
        reqwest::Method::PUT,
        &format!("/downloads/{id}/finetune"),
        Some(json!({"connections_per_download":8})),
    )
    .await;
    f.restart().await;
    assert_eq!(
        f.get(id).await.download.finetune.connections_per_download,
        Some(8)
    );
    assert_eq!(
        f.get(id).await.download.finetune.max_connections_per_server,
        Some(2)
    );
    f.close().await;
}
