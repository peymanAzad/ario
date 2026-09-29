use std::{
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc::Sender,
    thread,
};

use anyhow::{Context, ensure};
use common::{
    download::{AddDownloadsRequest, DownloadFilter, TorrentUploadMetadata},
    finetune::FineTune,
    queue::{CreateQueueRequest, UpdateQueueRequest},
};

use crate::{
    api,
    event::Event,
    modal::MAX_TORRENT_BYTES,
    msg::{ApiResult, Msg},
};

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Api(ApiRequest),
    OpenPath(PathBuf),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ApiRequest {
    Refresh {
        filter: DownloadFilter,
        manages_server: bool,
        lifecycle_revision: u64,
    },
    ListQueueDownloads {
        queue_id: i64,
    },
    AddDownloads(AddDownloadsRequest),
    AddTorrent {
        path: PathBuf,
        metadata: TorrentUploadMetadata,
    },
    PauseDownload(i64),
    ResumeDownload(i64),
    DeleteDownload(i64),
    DeleteDownloadFiles(i64),
    DeleteCompleted(Option<i64>),
    SaveDownloadEdit {
        id: i64,
        queue_id: i64,
        finetune: FineTune,
    },
    CreateQueue(CreateQueueRequest),
    UpdateQueue {
        id: i64,
        request: UpdateQueueRequest,
        ordered_ids: Vec<i64>,
    },
    PauseQueue(i64),
    ResumeQueue(i64),
    DeleteQueue {
        id: i64,
        name: String,
        delete_downloads: bool,
    },
}

pub fn run(effect: Effect, api_base: &str, tx: Sender<Event>) {
    match effect {
        Effect::Api(request) => {
            let base = api_base.to_owned();
            thread::spawn(move || {
                let result = execute(&base, request);
                let _ = tx.send(Event::Msg(Msg::Api(result)));
            });
        }
        Effect::OpenPath(path) => {
            thread::spawn(move || open_path(path));
        }
    }
}

fn execute(base: &str, request: ApiRequest) -> ApiResult {
    match request {
        ApiRequest::Refresh {
            filter,
            manages_server,
            lifecycle_revision,
        } => execute_refresh(base, filter, manages_server, lifecycle_revision),
        ApiRequest::ListQueueDownloads { queue_id } => {
            let filter = DownloadFilter {
                queue_id: Some(queue_id),
                category: None,
                status: None,
                sort_by: Some(common::enums::SortField::QueuePosition),
                sort_desc: false,
            };
            ApiResult::QueueDownloadsLoaded {
                queue_id,
                result: api::list_downloads(base, &filter),
            }
        }
        ApiRequest::AddDownloads(request) => match api::add_downloads(base, &request) {
            Ok(_) => ApiResult::Done,
            Err(error) => ApiResult::Failed {
                context: "add downloads".to_string(),
                error: error.to_string(),
            },
        },
        ApiRequest::AddTorrent { path, metadata } => {
            let result = (|| -> anyhow::Result<_> {
                let data = std::fs::read(&path)
                    .with_context(|| format!("failed to read {}", path.display()))?;
                ensure!(!data.is_empty(), "torrent file is empty");
                ensure!(
                    data.len() as u64 <= MAX_TORRENT_BYTES,
                    "torrent file exceeds the 16 MiB limit"
                );
                let filename = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .context("torrent filename is not valid UTF-8")?;
                api::add_torrent(base, filename, data, &metadata)
            })();
            ApiResult::TorrentAdded(result)
        }
        ApiRequest::PauseDownload(id) => ApiResult::DownloadPaused {
            download_id: id,
            result: api::pause_download(base, id),
        },
        ApiRequest::ResumeDownload(id) => {
            ApiResult::from_unit("resume download", api::resume_download(base, id))
        }
        ApiRequest::DeleteDownload(id) => {
            ApiResult::from_unit("delete download", api::delete_download(base, id))
        }
        ApiRequest::DeleteDownloadFiles(id) => {
            ApiResult::DownloadFilesDeleted(api::delete_download_files(base, id))
        }
        ApiRequest::DeleteCompleted(queue_id) => ApiResult::from_unit(
            "delete completed",
            api::delete_completed_downloads(base, queue_id),
        ),
        ApiRequest::SaveDownloadEdit {
            id,
            queue_id,
            finetune,
        } => ApiResult::DownloadEditSaved((|| {
            api::move_download_queue(base, id, queue_id).context("move download")?;
            api::update_finetune(base, id, &finetune).context("update fine tuning")?;
            Ok(())
        })()),
        ApiRequest::CreateQueue(request) => {
            ApiResult::QueueSaved(api::create_queue(base, &request).map(|_| ()))
        }
        ApiRequest::UpdateQueue {
            id,
            request,
            ordered_ids,
        } => {
            let result = api::update_queue(base, id, &request).and_then(|_| {
                if ordered_ids.is_empty() {
                    Ok(())
                } else {
                    api::reorder_queue(base, id, &ordered_ids)
                }
            });
            ApiResult::QueueSaved(result)
        }
        ApiRequest::PauseQueue(id) => {
            ApiResult::from_unit("pause queue", api::pause_queue(base, id))
        }
        ApiRequest::ResumeQueue(id) => match api::resume_queue(base, id) {
            Ok(()) => {
                let stop = api::list_queues(base)
                    .ok()
                    .and_then(|queues| queues.into_iter().find(|queue| queue.id == id))
                    .and_then(|queue| queue.scheduled_stop_at);
                ApiResult::QueueResumed {
                    queue_id: id,
                    result: Ok(stop),
                }
            }
            Err(error) => ApiResult::Failed {
                context: "resume queue".to_string(),
                error: error.to_string(),
            },
        },
        ApiRequest::DeleteQueue {
            id,
            name,
            delete_downloads,
        } => ApiResult::QueueDeleteResolved {
            queue_id: id,
            queue_name: name,
            result: api::delete_queue(base, id, delete_downloads),
        },
    }
}

fn execute_refresh(
    base: &str,
    filter: DownloadFilter,
    manages_server: bool,
    lifecycle_revision: u64,
) -> ApiResult {
    let health = api::health(base);
    let (server_reachable, aria2_reachable, download_speed, active_downloads, aria2_global_options) =
        match health {
            Ok(h) => (
                true,
                h.aria2_reachable,
                h.download_speed,
                h.active_downloads,
                h.aria2_global_options,
            ),
            Err(_) => (false, false, 0, 0, None),
        };

    let downloads = api::list_downloads(base, &filter);
    let queues = api::list_queues(base);
    let (server_reachable, aria2_reachable, download_speed, active_downloads, aria2_global_options) =
        if !server_reachable && manages_server {
            match api::health(base) {
                Ok(h) => (
                    true,
                    h.aria2_reachable,
                    h.download_speed,
                    h.active_downloads,
                    h.aria2_global_options,
                ),
                Err(_) => (false, false, 0, 0, None),
            }
        } else {
            (
                server_reachable,
                aria2_reachable,
                download_speed,
                active_downloads,
                aria2_global_options,
            )
        };

    ApiResult::Refreshed {
        downloads,
        queues,
        server_reachable,
        aria2_reachable,
        download_speed,
        active_downloads,
        aria2_global_options,
        lifecycle_revision,
    }
}

fn silence_command_stdio(command: &mut Command) -> &mut Command {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
}

fn open_path(path: PathBuf) {
    #[cfg(target_os = "linux")]
    {
        let mut command = Command::new("xdg-open");
        command.arg(&path);
        let _ = silence_command_stdio(&mut command).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("open");
        command.arg(&path);
        let _ = silence_command_stdio(&mut command).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", ""]).arg(&path);
        let _ = silence_command_stdio(&mut command).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::{ApiRequest, execute, silence_command_stdio};
    use crate::msg::ApiResult;
    use chrono::Utc;
    use common::{
        download::{Download, DownloadLiveStatus},
        enums::{DownloadStatus, FileCategory, SourceType},
        finetune::FineTune,
    };
    use std::{
        io::{Read, Write},
        net::TcpListener,
        process::Command,
        thread,
        time::Duration,
    };

    fn live_status(queue_id: i64) -> String {
        serde_json::to_string(&DownloadLiveStatus {
            download: Download {
                id: 7,
                aria2_gid: None,
                url: "https://example.test/file".into(),
                filename: Some("file".into()),
                destination_path: "/tmp".into(),
                source_type: SourceType::Http,
                category: FileCategory::Other,
                status: DownloadStatus::Pending,
                paused_by_scheduler: false,
                manually_started: false,
                size: None,
                completed_length: None,
                queue_id,
                position_in_queue: 0,
                finetune: FineTune::default(),
                created_at: Utc::now(),
                started_at: None,
                completed_at: None,
            },
            completed_length: 0,
            download_speed: 0,
            eta_seconds: None,
        })
        .unwrap()
    }

    fn mock_server(
        responses: Vec<(&'static str, String)>,
    ) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, body) in responses {
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
                requests.push(String::from_utf8_lossy(&request).into_owned());
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
            }
            requests
        });
        (base, server)
    }

    #[test]
    fn download_edit_moves_before_finetune_for_changed_and_unchanged_queues() {
        for queue_id in [1, 2] {
            let (base, server) = mock_server(vec![
                ("200 OK", live_status(queue_id)),
                ("200 OK", live_status(queue_id)),
            ]);
            let result = execute(
                &base,
                ApiRequest::SaveDownloadEdit {
                    id: 7,
                    queue_id,
                    finetune: FineTune::default(),
                },
            );
            assert!(matches!(result, ApiResult::DownloadEditSaved(Ok(()))));
            let requests = server.join().unwrap();
            assert_eq!(requests.len(), 2);
            assert!(requests[0].starts_with("PUT /downloads/7/queue HTTP/1.1"));
            assert!(requests[0].contains(&format!("\"queue_id\":{queue_id}")));
            assert!(requests[1].starts_with("PUT /downloads/7/finetune HTTP/1.1"));
        }
    }

    #[test]
    fn download_edit_stops_when_move_fails() {
        let (base, server) = mock_server(vec![(
            "400 Bad Request",
            r#"{"result":"error","message":"queue missing"}"#.into(),
        )]);
        let result = execute(
            &base,
            ApiRequest::SaveDownloadEdit {
                id: 7,
                queue_id: 2,
                finetune: FineTune::default(),
            },
        );
        let ApiResult::DownloadEditSaved(Err(error)) = result else {
            panic!("failed move should fail the save");
        };
        assert_eq!(format!("{error:#}"), "move download: queue missing");
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("PUT /downloads/7/queue HTTP/1.1"));
    }

    #[test]
    fn download_edit_reports_finetune_failure_after_move() {
        let (base, server) = mock_server(vec![
            ("200 OK", live_status(2)),
            (
                "400 Bad Request",
                r#"{"result":"error","message":"invalid fine tuning"}"#.into(),
            ),
        ]);
        let result = execute(
            &base,
            ApiRequest::SaveDownloadEdit {
                id: 7,
                queue_id: 2,
                finetune: FineTune::default(),
            },
        );
        let ApiResult::DownloadEditSaved(Err(error)) = result else {
            panic!("failed fine tuning should fail the save");
        };
        assert_eq!(
            format!("{error:#}"),
            "update fine tuning: invalid fine tuning"
        );
        assert_eq!(server.join().unwrap().len(), 2);
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn external_command_stdio_is_silenced() {
        #[cfg(unix)]
        let mut command = {
            let mut command = Command::new("sh");
            command.args(["-c", "printf stdout; printf stderr >&2"]);
            command
        };

        #[cfg(windows)]
        let mut command = {
            let mut command = Command::new("cmd");
            command.args(["/C", "echo stdout & echo stderr 1>&2"]);
            command
        };

        let output = silence_command_stdio(&mut command)
            .output()
            .expect("test command should run");

        assert!(output.status.success());
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }
}
