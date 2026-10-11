use super::support::Fixture;
use reqwest::Method;
use serde_json::json;

#[tokio::test]
async fn invalid_requests_and_missing_ids_do_not_create_work() {
    let f = Fixture::new().await;
    for (method, path, body, status) in [
        (
            Method::POST,
            "/downloads",
            Some(json!({"inputs":[],"queue_id":1})),
            400,
        ),
        (
            Method::POST,
            "/downloads",
            Some(json!({"inputs":[{"Url":"http://fixture.invalid/a"}],"queue_id":999})),
            400,
        ),
        (Method::GET, "/downloads/999", None, 404),
        (Method::GET, "/queues/999", None, 404),
        (Method::POST, "/downloads/999/resume", None, 404),
        (Method::PUT, "/queues/999", Some(json!({})), 422),
    ] {
        assert_eq!(
            f.response(method, path, body).await.status(),
            status,
            "{path}"
        );
    }
    let r = f
        .http
        .post(format!("{}/downloads", f.base))
        .header("content-type", "application/json")
        .body("{broken")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    assert!(
        f.state
            .db
            .list_downloads(&Default::default())
            .unwrap()
            .is_empty()
    );
    assert_eq!(f.calls("aria2.addUri"), 0);
    f.close().await;
}

#[tokio::test]
async fn multipart_missing_malformed_and_oversized_uploads_leave_no_records() {
    let f = Fixture::new().await;
    for form in [
        reqwest::multipart::Form::new()
            .text("metadata", r#"{"queue_id":1,"start_immediately":false}"#),
        reqwest::multipart::Form::new()
            .text("metadata", "invalid")
            .part(
                "file",
                reqwest::multipart::Part::bytes(b"bad".to_vec()).file_name("bad.torrent"),
            ),
        reqwest::multipart::Form::new()
            .text("metadata", r#"{"queue_id":1}"#)
            .part(
                "file",
                reqwest::multipart::Part::bytes(b"not bencode".to_vec()).file_name("bad.torrent"),
            ),
    ] {
        assert_eq!(
            f.http
                .post(format!("{}/downloads/torrent", f.base))
                .multipart(form)
                .send()
                .await
                .unwrap()
                .status(),
            400
        );
    }
    let form = reqwest::multipart::Form::new()
        .text("metadata", r#"{"queue_id":1}"#)
        .part(
            "file",
            reqwest::multipart::Part::bytes(vec![0; 16 * 1024 * 1024 + 1])
                .file_name("large.torrent"),
        );
    assert_eq!(
        f.http
            .post(format!("{}/downloads/torrent", f.base))
            .multipart(form)
            .send()
            .await
            .unwrap()
            .status(),
        413
    );
    assert!(
        f.state
            .db
            .list_downloads(&Default::default())
            .unwrap()
            .is_empty()
    );
    assert_eq!(f.calls("aria2.addTorrent"), 0);
    f.close().await;
}
