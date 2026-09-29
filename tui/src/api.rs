use common::{
    api::ApiResponse,
    download::{DownloadFilter, DownloadLiveStatus},
    finetune::{Aria2GlobalOptions, FineTune},
    lifecycle::ShutdownIfIdleResponse,
    queue::Queue,
};
use serde::Deserialize;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeleteQueueOutcome {
    Deleted,
    NeedsConfirmation,
}

#[derive(Deserialize)]
pub struct HealthResponse {
    #[allow(dead_code)]
    pub server: String,
    pub aria2_reachable: bool,
    #[serde(default)]
    pub tui_managed: bool,
    #[serde(default)]
    pub download_speed: u64,
    #[serde(default)]
    pub active_downloads: u64,
    #[serde(default)]
    pub aria2_global_options: Option<Aria2GlobalOptions>,
}

fn client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("failed to build http client")
}

/// Preserve successful response bodies and expose structured server failures.
fn ensure_success(
    response: reqwest::blocking::Response,
) -> anyhow::Result<reqwest::blocking::Response> {
    let status = response.status();
    if !status.is_client_error() && !status.is_server_error() {
        return Ok(response);
    }
    if let Ok(ApiResponse::<()>::Error { message }) = response.json::<ApiResponse<()>>() {
        if !message.trim().is_empty() {
            anyhow::bail!(message);
        }
    }
    anyhow::bail!("Request failed (HTTP {status}).")
}

pub fn list_downloads(
    base: &str,
    filter: &DownloadFilter,
) -> anyhow::Result<Vec<common::download::DownloadLiveStatus>> {
    let resp = ensure_success(
        client()
            .get(format!("{base}/downloads"))
            .query(filter) // serde_urlencoded serializes DownloadFilter's fields as
            .send()?,
    )?;
    Ok(resp.json()?)
}

pub fn add_downloads(
    base: &str,
    request: &common::download::AddDownloadsRequest,
) -> anyhow::Result<Vec<common::download::DownloadLiveStatus>> {
    let resp = ensure_success(
        client()
            .post(format!("{base}/downloads"))
            .json(request)
            .send()?,
    )?;
    Ok(resp.json()?)
}

pub fn add_torrent(
    base: &str,
    filename: &str,
    data: Vec<u8>,
    metadata: &common::download::TorrentUploadMetadata,
) -> anyhow::Result<common::download::DownloadLiveStatus> {
    let metadata = serde_json::to_string(metadata)?;
    let file = reqwest::blocking::multipart::Part::bytes(data)
        .file_name(filename.to_string())
        .mime_str("application/x-bittorrent")?;
    let form = reqwest::blocking::multipart::Form::new()
        .text("metadata", metadata)
        .part("file", file);
    let resp = ensure_success(
        client()
            .post(format!("{base}/downloads/torrent"))
            .multipart(form)
            .send()?,
    )?;
    Ok(resp.json()?)
}

pub fn list_queues(base: &str) -> anyhow::Result<Vec<Queue>> {
    let resp = ensure_success(client().get(format!("{base}/queues")).send()?)?;
    Ok(resp.json()?)
}

pub fn pause_queue(base: &str, queue_id: i64) -> anyhow::Result<()> {
    ensure_success(
        client()
            .post(format!("{base}/queues/{queue_id}/pause"))
            .send()?,
    )?;
    Ok(())
}

pub fn resume_queue(base: &str, queue_id: i64) -> anyhow::Result<()> {
    ensure_success(
        client()
            .post(format!("{base}/queues/{queue_id}/resume"))
            .send()?,
    )?;
    Ok(())
}

pub fn delete_queue(
    base: &str,
    queue_id: i64,
    delete_downloads: bool,
) -> anyhow::Result<DeleteQueueOutcome> {
    let response = client()
        .delete(format!("{base}/queues/{queue_id}"))
        .query(&[("delete_downloads", delete_downloads)])
        .send()?;

    if response.status() == reqwest::StatusCode::CONFLICT && !delete_downloads {
        return Ok(DeleteQueueOutcome::NeedsConfirmation);
    }

    ensure_success(response)?;
    Ok(DeleteQueueOutcome::Deleted)
}

pub fn health(base: &str) -> anyhow::Result<HealthResponse> {
    let resp = ensure_success(client().get(format!("{base}/health")).send()?)?;
    Ok(resp.json()?)
}

pub fn shutdown_if_idle(base: &str) -> anyhow::Result<ShutdownIfIdleResponse> {
    let resp = ensure_success(client().post(format!("{base}/shutdown-if-idle")).send()?)?;
    Ok(resp.json()?)
}

pub fn shutdown(base: &str) -> anyhow::Result<ShutdownIfIdleResponse> {
    let resp = ensure_success(client().post(format!("{base}/shutdown")).send()?)?;
    Ok(resp.json()?)
}

pub fn pause_download(base: &str, id: i64) -> anyhow::Result<DownloadLiveStatus> {
    let response = ensure_success(
        client()
            .post(format!("{base}/downloads/{id}/pause"))
            .send()?,
    )?;
    Ok(response.json()?)
}

pub fn resume_download(base: &str, id: i64) -> anyhow::Result<()> {
    ensure_success(
        client()
            .post(format!("{base}/downloads/{id}/resume"))
            .send()?,
    )?;
    Ok(())
}

pub fn delete_download(base: &str, id: i64) -> anyhow::Result<()> {
    ensure_success(client().delete(format!("{base}/downloads/{id}")).send()?)?;
    Ok(())
}

pub fn delete_download_files(
    base: &str,
    id: i64,
) -> anyhow::Result<common::download::DeleteDownloadFilesResult> {
    let response = ensure_success(
        client()
            .delete(format!("{base}/downloads/{id}"))
            .query(&[("delete_files", true)])
            .send()?,
    )?;
    Ok(response.json()?)
}

pub fn delete_completed_downloads(base: &str, queue_id: Option<i64>) -> anyhow::Result<()> {
    let mut request = client().delete(format!("{base}/downloads/completed"));
    if let Some(queue_id) = queue_id {
        request = request.query(&[("queue_id", queue_id)]);
    }
    ensure_success(request.send()?)?;
    Ok(())
}

pub fn create_queue(
    base: &str,
    request: &common::queue::CreateQueueRequest,
) -> anyhow::Result<common::queue::Queue> {
    let resp = ensure_success(
        client()
            .post(format!("{base}/queues"))
            .json(request)
            .send()?,
    )?;
    Ok(resp.json()?)
}

pub fn update_queue(
    base: &str,
    id: i64,
    request: &common::queue::UpdateQueueRequest,
) -> anyhow::Result<common::queue::Queue> {
    let resp = ensure_success(
        client()
            .put(format!("{base}/queues/{id}"))
            .json(request)
            .send()?,
    )?;
    Ok(resp.json()?)
}

pub fn reorder_queue(base: &str, queue_id: i64, ordered_ids: &[i64]) -> anyhow::Result<()> {
    #[derive(serde::Serialize)]
    struct ReorderRequest<'a> {
        ordered_ids: &'a [i64],
    }

    ensure_success(
        client()
            .put(format!("{base}/queues/{queue_id}/reorder"))
            .json(&ReorderRequest { ordered_ids })
            .send()?,
    )?;
    Ok(())
}

pub fn update_finetune(
    base: &str,
    id: i64,
    finetune: &FineTune,
) -> anyhow::Result<DownloadLiveStatus> {
    let resp = ensure_success(
        client()
            .put(format!("{base}/downloads/{id}/finetune"))
            .json(finetune)
            .send()?,
    )?;
    Ok(resp.json()?)
}

pub fn move_download_queue(
    base: &str,
    id: i64,
    queue_id: i64,
) -> anyhow::Result<DownloadLiveStatus> {
    #[derive(serde::Serialize)]
    struct UpdateDownloadQueueRequest {
        queue_id: i64,
    }

    let resp = ensure_success(
        client()
            .put(format!("{base}/downloads/{id}/queue"))
            .json(&UpdateDownloadQueueRequest { queue_id })
            .send()?,
    )?;
    Ok(resp.json()?)
}

#[cfg(test)]
#[path = "api_tests.rs"]
mod tests;
