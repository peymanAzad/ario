//! Headless smoke: real effect worker, HTTP client, app update and terminal renderer.
use crate::{
    app::{App, update},
    effects::{ApiRequest, Effect},
    event::Event,
    icons::{GlyphMode, IconSet},
    theme::Theme,
};
use common::download::{AddDownloadInput, AddDownloadsRequest};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc,
    thread,
    time::Duration,
};

fn exchange(app: &mut App, request: ApiRequest, responses: Vec<(&'static str, u16, Value)>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        for (path, status, body) in responses {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(std::time::Instant::now() < deadline, "expected {path}");
                        thread::yield_now();
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            // Accepted sockets can inherit the listener's nonblocking mode.
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut chunk = [0; 4096];
            loop {
                let n = stream.read(&mut chunk).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&chunk[..n]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]);
                    let length = headers
                        .lines()
                        .filter_map(|l| l.split_once(':'))
                        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                        .map(|(_, v)| v.trim().parse::<usize>().unwrap())
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        assert!(headers.lines().next().unwrap().contains(path), "{headers}");
                        break;
                    }
                }
            }
            let body = if status == 204 {
                String::new()
            } else {
                body.to_string()
            };
            let response = format!(
                "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
        }
    });
    let (tx, rx) = mpsc::channel();
    crate::effects::run(Effect::Api(request), &base, tx);
    let Event::Msg(msg) = rx.recv_timeout(Duration::from_secs(5)).unwrap() else {
        panic!("expected API message")
    };
    update::update(app, msg);
    server.join().unwrap();
}
fn snapshot(status: &str, completed: u64) -> Value {
    json!({"download":{"id":1,"aria2_gid":"gid","url":"http://fixture.invalid/smoke.bin","filename":"smoke.bin","destination_path":"/tmp","source_type":"Http","category":"Other","status":status,"paused_by_scheduler":false,"manually_started":true,"size":100,"completed_length":completed,"queue_id":1,"position_in_queue":0,"finetune":{},"created_at":"2030-01-07T12:00:00Z","started_at":null,"completed_at":null},"completed_length":completed,"download_speed":if status=="Active"{10}else{0},"eta_seconds":null})
}
fn refresh(app: &mut App, downloads: Value) {
    exchange(
        app,
        ApiRequest::Refresh {
            filter_revision: app.filter_revision,
            filter: Default::default(),
            manages_server: false,
            lifecycle_revision: app.lifecycle_revision,
        },
        vec![
            (
                "/health",
                200,
                json!({"server":"ok","aria2_reachable":true}),
            ),
            ("/downloads", 200, downloads),
            ("/queues", 200, json!([])),
        ],
    );
}
fn render(app: &mut App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal
        .draw(|frame| crate::ui::render(app, frame))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect()
}
#[test]
fn add_progress_pause_resume_delete_and_error_render_without_terminal() {
    let mut app = App::new(Theme::default_dark(), IconSet::new(GlyphMode::Ascii), false);
    exchange(
        &mut app,
        ApiRequest::AddDownloads(AddDownloadsRequest {
            inputs: vec![AddDownloadInput::Url(
                "http://fixture.invalid/smoke.bin".into(),
            )],
            queue_id: 1,
            finetune_override: None,
            start_immediately: true,
        }),
        vec![("/downloads", 200, json!([snapshot("Active", 0)]))],
    );
    refresh(&mut app, json!([snapshot("Active", 40)]));
    assert_eq!(app.downloads[0].completed_length, 40);
    assert!(render(&mut app).contains("smoke.bin"));
    exchange(
        &mut app,
        ApiRequest::PauseDownload(1),
        vec![("/downloads/1/pause", 200, snapshot("Paused", 40))],
    );
    refresh(&mut app, json!([snapshot("Paused", 40)]));
    assert_eq!(app.downloads[0].download_speed, 0);
    assert!(render(&mut app).contains("smoke.bin"));
    exchange(
        &mut app,
        ApiRequest::ResumeDownload(1),
        vec![("/downloads/1/resume", 204, Value::Null)],
    );
    refresh(&mut app, json!([snapshot("Active", 70)]));
    assert_eq!(app.downloads[0].completed_length, 70);
    exchange(
        &mut app,
        ApiRequest::DeleteDownload(1),
        vec![("/downloads/1", 204, Value::Null)],
    );
    refresh(&mut app, json!([]));
    assert!(app.downloads.is_empty());
    assert!(!render(&mut app).contains("smoke.bin"));
    exchange(
        &mut app,
        ApiRequest::ResumeDownload(1),
        vec![(
            "/downloads/1/resume",
            404,
            json!({"result":"error","message":"Smoke download missing"}),
        )],
    );
    assert!(render(&mut app).contains("Smoke download missing"));
    assert!(!app.should_quit);
}
