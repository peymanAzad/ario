use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

fn mock_response(status: &str, body: &str) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            let count = stream.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..count]);
            if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..end]);
                let length = headers
                    .lines()
                    .filter_map(|line| line.split_once(':'))
                    .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                    .map(|(_, value)| value.trim().parse::<usize>().unwrap())
                    .unwrap_or(0);
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        stream.write_all(response.as_bytes()).unwrap();
        String::from_utf8_lossy(&request).into_owned()
    });
    (base, server)
}

#[test]
fn expired_start_errors_expose_server_message_for_queue_individual_and_immediate_add() {
    let message = "This queue’s schedule has ended. Disable the scheduler or update its end time before starting.";
    let body = serde_json::to_string(&ApiResponse::<()>::Error {
        message: message.into(),
    })
    .unwrap();
    for path in ["/queues/1/resume", "/downloads/2/resume", "/downloads"] {
        let (base, server) = mock_response("400 Bad Request", &body);
        let result = match path {
            "/queues/1/resume" => resume_queue(&base, 1),
            "/downloads/2/resume" => resume_download(&base, 2),
            _ => add_downloads(
                &base,
                &common::download::AddDownloadsRequest {
                    inputs: vec![common::download::AddDownloadInput::Url(
                        "https://example.test/a".into(),
                    )],
                    queue_id: 1,
                    finetune_override: None,
                    start_immediately: true,
                },
            )
            .map(|_| ()),
        };
        assert_eq!(result.unwrap_err().to_string(), message);
        assert!(
            server
                .join()
                .unwrap()
                .starts_with(&format!("POST {path} HTTP/1.1"))
        );
    }
}

#[test]
fn structured_not_found_error_is_not_mislabeled_as_expired_schedule() {
    let (base, server) = mock_response(
        "404 Not Found",
        r#"{"result":"error","message":"Queue 99 was not found."}"#,
    );
    assert_eq!(
        resume_queue(&base, 99).unwrap_err().to_string(),
        "Queue 99 was not found."
    );
    server.join().unwrap();
}

#[test]
fn unrecognized_error_bodies_fall_back_to_actual_status() {
    for body in [
        "",
        "<html>Not found</html>",
        r#"{"unexpected":true}"#,
        r#"{"result":"error","message":"  "}"#,
        r#"{"result":"ok","data":null}"#,
    ] {
        let (base, server) = mock_response("404 Not Found", body);
        assert_eq!(
            resume_queue(&base, 1).unwrap_err().to_string(),
            "Request failed (HTTP 404 Not Found)."
        );
        server.join().unwrap();
    }
}

#[test]
fn successful_responses_keep_json_and_empty_bodies_intact() {
    let (base, server) = mock_response("200 OK", "[]");
    assert!(list_queues(&base).unwrap().is_empty());
    server.join().unwrap();
    let (base, server) = mock_response("204 No Content", "");
    resume_queue(&base, 1).unwrap();
    server.join().unwrap();
}

#[test]
fn queue_delete_conflict_preserves_confirmation_and_confirmed_errors() {
    let body = r#"{"result":"error","message":"queue still contains downloads"}"#;
    let (base, server) = mock_response("409 Conflict", body);
    assert_eq!(
        delete_queue(&base, 1, false).unwrap(),
        DeleteQueueOutcome::NeedsConfirmation
    );
    server.join().unwrap();
    let (base, server) = mock_response("409 Conflict", body);
    assert_eq!(
        delete_queue(&base, 1, true).unwrap_err().to_string(),
        "queue still contains downloads"
    );
    server.join().unwrap();
}

#[test]
fn transport_errors_retain_their_original_type() {
    assert!(
        resume_queue("not a valid URL", 1)
            .unwrap_err()
            .downcast_ref::<reqwest::Error>()
            .is_some()
    );
}

#[test]
fn health_without_global_options_remains_compatible() {
    let response: HealthResponse = serde_json::from_str(
        r#"{"server":"ok","aria2_reachable":true,"tui_managed":false,"download_speed":0}"#,
    )
    .unwrap();

    assert!(response.aria2_global_options.is_none());
    assert_eq!(response.active_downloads, 0);
}
