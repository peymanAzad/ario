use common::{
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

pub fn list_downloads(
    base: &str,
    filter: &DownloadFilter,
) -> anyhow::Result<Vec<common::download::DownloadLiveStatus>> {
    let resp = client()
        .get(format!("{base}/downloads"))
        .query(filter) // serde_urlencoded serializes DownloadFilter's fields as
        .send()?
        .error_for_status()?;
    Ok(resp.json()?)
}

pub fn add_downloads(
    base: &str,
    request: &common::download::AddDownloadsRequest,
) -> anyhow::Result<Vec<common::download::DownloadLiveStatus>> {
    let resp = client()
        .post(format!("{base}/downloads"))
        .json(request)
        .send()?
        .error_for_status()?;
    Ok(resp.json()?)
}

pub fn list_queues(base: &str) -> anyhow::Result<Vec<Queue>> {
    let resp = client()
        .get(format!("{base}/queues"))
        .send()?
        .error_for_status()?;
    Ok(resp.json()?)
}

pub fn pause_queue(base: &str, queue_id: i64) -> anyhow::Result<()> {
    client()
        .post(format!("{base}/queues/{queue_id}/pause"))
        .send()?
        .error_for_status()?;
    Ok(())
}

pub fn resume_queue(base: &str, queue_id: i64) -> anyhow::Result<()> {
    client()
        .post(format!("{base}/queues/{queue_id}/resume"))
        .send()?
        .error_for_status()?;
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

    response.error_for_status()?;
    Ok(DeleteQueueOutcome::Deleted)
}

pub fn health(base: &str) -> anyhow::Result<HealthResponse> {
    let resp = client()
        .get(format!("{base}/health"))
        .send()?
        .error_for_status()?;
    Ok(resp.json()?)
}

pub fn shutdown_if_idle(base: &str) -> anyhow::Result<ShutdownIfIdleResponse> {
    let resp = client()
        .post(format!("{base}/shutdown-if-idle"))
        .send()?
        .error_for_status()?;
    Ok(resp.json()?)
}

pub fn shutdown(base: &str) -> anyhow::Result<ShutdownIfIdleResponse> {
    let resp = client()
        .post(format!("{base}/shutdown"))
        .send()?
        .error_for_status()?;
    Ok(resp.json()?)
}

pub fn pause_download(base: &str, id: i64) -> anyhow::Result<()> {
    client()
        .post(format!("{base}/downloads/{id}/pause"))
        .send()?
        .error_for_status()?;
    Ok(())
}

pub fn resume_download(base: &str, id: i64) -> anyhow::Result<()> {
    client()
        .post(format!("{base}/downloads/{id}/resume"))
        .send()?
        .error_for_status()?;
    Ok(())
}

pub fn delete_download(base: &str, id: i64) -> anyhow::Result<()> {
    client()
        .delete(format!("{base}/downloads/{id}"))
        .send()?
        .error_for_status()?;
    Ok(())
}

pub fn delete_download_files(
    base: &str,
    id: i64,
) -> anyhow::Result<common::download::DeleteDownloadFilesResult> {
    let response = client()
        .delete(format!("{base}/downloads/{id}"))
        .query(&[("delete_files", true)])
        .send()?
        .error_for_status()?;
    Ok(response.json()?)
}

pub fn delete_completed_downloads(base: &str, queue_id: Option<i64>) -> anyhow::Result<()> {
    let mut request = client().delete(format!("{base}/downloads/completed"));
    if let Some(queue_id) = queue_id {
        request = request.query(&[("queue_id", queue_id)]);
    }
    request.send()?.error_for_status()?;
    Ok(())
}

pub fn create_queue(
    base: &str,
    request: &common::queue::CreateQueueRequest,
) -> anyhow::Result<common::queue::Queue> {
    let resp = client()
        .post(format!("{base}/queues"))
        .json(request)
        .send()?
        .error_for_status()?;
    Ok(resp.json()?)
}

pub fn update_queue(
    base: &str,
    id: i64,
    request: &common::queue::UpdateQueueRequest,
) -> anyhow::Result<common::queue::Queue> {
    let resp = client()
        .put(format!("{base}/queues/{id}"))
        .json(request)
        .send()?
        .error_for_status()?;
    Ok(resp.json()?)
}

pub fn reorder_queue(base: &str, queue_id: i64, ordered_ids: &[i64]) -> anyhow::Result<()> {
    #[derive(serde::Serialize)]
    struct ReorderRequest<'a> {
        ordered_ids: &'a [i64],
    }

    client()
        .put(format!("{base}/queues/{queue_id}/reorder"))
        .json(&ReorderRequest { ordered_ids })
        .send()?
        .error_for_status()?;
    Ok(())
}

pub fn update_finetune(
    base: &str,
    id: i64,
    finetune: &FineTune,
) -> anyhow::Result<DownloadLiveStatus> {
    let resp = client()
        .put(format!("{base}/downloads/{id}/finetune"))
        .json(finetune)
        .send()?
        .error_for_status()?;
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

    let resp = client()
        .put(format!("{base}/downloads/{id}/queue"))
        .json(&UpdateDownloadQueueRequest { queue_id })
        .send()?
        .error_for_status()?;
    Ok(resp.json()?)
}

#[cfg(test)]
mod tests {
    use super::HealthResponse;

    #[test]
    fn health_without_global_options_remains_compatible() {
        let response: HealthResponse = serde_json::from_str(
            r#"{"server":"ok","aria2_reachable":true,"tui_managed":false,"download_speed":0}"#,
        )
        .unwrap();

        assert!(response.aria2_global_options.is_none());
        assert_eq!(response.active_downloads, 0);
    }
}
