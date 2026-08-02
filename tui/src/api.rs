use common::{download::DownloadLiveStatus, queue::Queue};
use serde::Deserialize;
use std::time::Duration;

#[derive(Deserialize)]
pub struct HealthResponse {
    #[allow(dead_code)]
    pub server: String,
    pub aria2_reachable: bool,
}

fn client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("failed to build http client")
}

pub fn list_downloads(base: &str) -> anyhow::Result<Vec<DownloadLiveStatus>> {
    let resp = client()
        .get(format!("{base}/downloads"))
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

pub fn health(base: &str) -> anyhow::Result<HealthResponse> {
    let resp = client()
        .get(format!("{base}/health"))
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
