use common::{
    enums::{AllocStrategy, StreamPieceSelector},
    finetune::{Aria2GlobalOptions, FineTune},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Debug)]
pub enum Aria2Error {
    Http(reqwest::Error),
    Rpc { code: i64, message: String },
    UnexpectedResponse(String),
}

impl std::fmt::Display for Aria2Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Aria2Error::Http(e) => write!(f, "aria2 unreachable: {e}"),
            Aria2Error::Rpc { code, message } => write!(f, "aria2 error {code}: {message}"),
            Aria2Error::UnexpectedResponse(s) => write!(f, "unexpected aria2 response: {s}"),
        }
    }
}

impl std::error::Error for Aria2Error {}

impl From<reqwest::Error> for Aria2Error {
    fn from(e: reqwest::Error) -> Self {
        Aria2Error::Http(e)
    }
}

/// Extra aria2 options applied when adding a URI.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Aria2AddMode {
    /// First start / scheduler start — no continue or overwrite flags.
    #[default]
    Fresh,
    /// Retry a failed download; reuse any partial file on disk.
    Retry,
    /// Restart a completed download from scratch, overwriting the existing file.
    Restart,
}

pub struct Aria2Client {
    http: reqwest::Client,
    rpc_url: String,
    secret: Option<String>,
}

impl Aria2Client {
    pub fn new(rpc_url: impl Into<String>, secret: Option<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            rpc_url: rpc_url.into(),
            secret,
        }
    }

    async fn call(&self, method: &str, params: Vec<Value>) -> Result<Value, Aria2Error> {
        let mut full_params = params;
        if let Some(secret) = &self.secret {
            full_params.insert(0, json!(format!("token:{secret}")));
        }

        let body = json!({
            "jsonrpc": "2.0",
            "id": "ario",
            "method": method,
            "params": full_params,
        });

        let response: Value = self
            .http
            .post(&self.rpc_url)
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        if let Some(error) = response.get("error") {
            let code = error.get("code").and_then(Value::as_i64).unwrap_or(0);
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown aria2 error")
                .to_string();
            return Err(Aria2Error::Rpc { code, message });
        }

        response
            .get("result")
            .cloned()
            .ok_or_else(|| Aria2Error::UnexpectedResponse(response.to_string()))
    }

    pub async fn add_uri(
        &self,
        url: &str,
        finetune: &FineTune,
        destination_path: &str,
        mode: Aria2AddMode,
    ) -> Result<String, Aria2Error> {
        let options = finetune_to_options(finetune, destination_path, mode);
        let result = self
            .call("aria2.addUri", vec![json!([url]), options])
            .await?;
        result
            .as_str()
            .map(String::from)
            .ok_or_else(|| Aria2Error::UnexpectedResponse("expected gid string".into()))
    }

    pub async fn add_torrent(
        &self,
        torrent_b64: &str,
        finetune: &FineTune,
        destination_path: &str,
    ) -> Result<String, Aria2Error> {
        let options = finetune_to_options(finetune, destination_path, Aria2AddMode::Fresh);
        let result = self
            .call(
                "aria2.addTorrent",
                vec![json!(torrent_b64), json!([]), options],
            )
            .await?;
        result
            .as_str()
            .map(String::from)
            .ok_or_else(|| Aria2Error::UnexpectedResponse("expected gid string".into()))
    }

    pub async fn pause(&self, gid: &str) -> Result<(), Aria2Error> {
        self.call("aria2.pause", vec![json!(gid)]).await?;
        Ok(())
    }

    pub async fn unpause(&self, gid: &str) -> Result<(), Aria2Error> {
        self.call("aria2.unpause", vec![json!(gid)]).await?;
        Ok(())
    }

    pub async fn remove(&self, gid: &str) -> Result<(), Aria2Error> {
        self.call("aria2.remove", vec![json!(gid)]).await?;
        Ok(())
    }

    pub async fn remove_download_result(&self, gid: &str) -> Result<(), Aria2Error> {
        self.call("aria2.removeDownloadResult", vec![json!(gid)])
            .await?;
        Ok(())
    }

    pub async fn get_version(&self) -> Result<(), Aria2Error> {
        self.call("aria2.getVersion", vec![]).await?;
        Ok(())
    }

    pub async fn get_global_options(&self) -> Result<Aria2GlobalOptions, Aria2Error> {
        let result = self.call("aria2.getGlobalOption", vec![]).await?;
        parse_global_options(&result)
    }

    pub async fn shutdown(&self) -> Result<(), Aria2Error> {
        self.call("aria2.shutdown", vec![]).await?;
        Ok(())
    }

    pub async fn tell_status(&self, gid: &str) -> Result<Aria2Status, Aria2Error> {
        let keys = json!([
            "gid",
            "status",
            "totalLength",
            "completedLength",
            "downloadSpeed",
            "errorMessage",
            "files",
            "dir",
            "bittorrent"
        ]);
        let result = self
            .call("aria2.tellStatus", vec![json!(gid), keys])
            .await?;
        serde_json::from_value(result).map_err(|e| Aria2Error::UnexpectedResponse(e.to_string()))
    }
}

fn parse_global_options(value: &Value) -> Result<Aria2GlobalOptions, Aria2Error> {
    let options = value
        .as_object()
        .ok_or_else(|| Aria2Error::UnexpectedResponse("expected global options object".into()))?;
    let string = |name: &str| options.get(name).and_then(Value::as_str);

    Ok(Aria2GlobalOptions {
        connections_per_download: string("split").and_then(|value| value.parse().ok()),
        max_connections_per_server: string("max-connection-per-server")
            .and_then(|value| value.parse().ok()),
        alloc_strategy: match string("file-allocation") {
            Some("none") => Some(AllocStrategy::None),
            Some("prealloc") => Some(AllocStrategy::Prealloc),
            Some("falloc") => Some(AllocStrategy::Falloc),
            Some("trunc") => Some(AllocStrategy::Trunc),
            _ => None,
        },
        stream_piece_selector: match string("stream-piece-selector") {
            Some("default") => Some(StreamPieceSelector::Default),
            Some("inorder") => Some(StreamPieceSelector::InOrder),
            Some("random") => Some(StreamPieceSelector::Random),
            Some("geom") => Some(StreamPieceSelector::Geom),
            _ => None,
        },
    })
}

fn finetune_to_options(f: &FineTune, destination_path: &str, mode: Aria2AddMode) -> Value {
    let mut opts = serde_json::Map::new();
    opts.insert("dir".into(), json!(destination_path));

    match mode {
        Aria2AddMode::Fresh => {}
        Aria2AddMode::Retry => {
            opts.insert("continue".into(), json!("true"));
        }
        Aria2AddMode::Restart => {
            opts.insert("allow-overwrite".into(), json!("true"));
        }
    }

    if let Some(split) = f.connections_per_download {
        opts.insert("split".into(), json!(split.to_string()));
    }
    if let Some(max_conn) = f.max_connections_per_server {
        opts.insert(
            "max-connection-per-server".into(),
            json!(max_conn.to_string()),
        );
    }
    if let Some(alloc) = &f.alloc_strategy {
        let s = match alloc {
            AllocStrategy::None => "none",
            AllocStrategy::Prealloc => "prealloc",
            AllocStrategy::Falloc => "falloc",
            AllocStrategy::Trunc => "trunc",
        };
        opts.insert("file-allocation".into(), json!(s));
    }
    if let Some(sel) = &f.stream_piece_selector {
        let s = match sel {
            StreamPieceSelector::Default => "default",
            StreamPieceSelector::InOrder => "inorder",
            StreamPieceSelector::Random => "random",
            StreamPieceSelector::Geom => "geom",
        };
        opts.insert("stream-piece-selector".into(), json!(s));
    }
    if let Some(max_retries) = f.max_retries {
        opts.insert(
            "max-tries".into(),
            json!(max_retries.saturating_add(1).to_string()),
        );
    }
    if let Some(retry_wait_seconds) = f.retry_wait_seconds {
        opts.insert("retry-wait".into(), json!(retry_wait_seconds.to_string()));
    }

    Value::Object(opts)
}

#[derive(Debug, Deserialize)]
pub struct Aria2Status {
    pub gid: String,
    /// "active" | "waiting" | "paused" | "error" | "complete" | "removed"
    pub status: String,
    #[serde(rename = "totalLength")]
    pub total_length: String,
    #[serde(rename = "completedLength")]
    pub completed_length: String,
    #[serde(rename = "downloadSpeed")]
    pub download_speed: String,
    #[serde(rename = "errorMessage")]
    pub error_message: Option<String>,
    #[serde(default)]
    pub files: Vec<Aria2File>,
    #[serde(default)]
    pub dir: String,
    pub bittorrent: Option<Aria2BitTorrent>,
}

#[derive(Debug, Deserialize)]
pub struct Aria2File {
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct Aria2BitTorrent {
    pub info: Option<Aria2BitTorrentInfo>,
}

#[derive(Debug, Deserialize)]
pub struct Aria2BitTorrentInfo {
    pub name: String,
}

impl Aria2Status {
    pub fn artifact_paths(&self) -> (Vec<String>, Vec<String>) {
        let payloads: Vec<String> = self
            .files
            .iter()
            .filter(|file| !file.path.is_empty())
            .map(|file| file.path.clone())
            .collect();

        let controls = self
            .bittorrent
            .as_ref()
            .and_then(|torrent| torrent.info.as_ref())
            .filter(|info| !info.name.is_empty() && !self.dir.is_empty())
            .map(|info| {
                vec![
                    PathBuf::from(&self.dir)
                        .join(format!("{}.aria2", info.name))
                        .to_string_lossy()
                        .into_owned(),
                ]
            })
            .unwrap_or_else(|| {
                payloads
                    .iter()
                    .map(|path| format!("{path}.aria2"))
                    .collect()
            });

        (payloads, controls)
    }
}

#[cfg(test)]
mod tests {
    use super::{Aria2AddMode, Aria2Status, finetune_to_options, parse_global_options};
    use common::enums::{AllocStrategy, StreamPieceSelector};
    use common::finetune::FineTune;

    #[test]
    fn add_mode_sets_continue_or_overwrite() {
        let finetune = FineTune::default();

        let fresh = finetune_to_options(&finetune, "/tmp", Aria2AddMode::Fresh);
        assert_eq!(fresh.get("continue"), None);
        assert_eq!(fresh.get("allow-overwrite"), None);

        let retry = finetune_to_options(&finetune, "/tmp", Aria2AddMode::Retry);
        assert_eq!(retry.get("continue").and_then(|v| v.as_str()), Some("true"));
        assert_eq!(retry.get("allow-overwrite"), None);

        let restart = finetune_to_options(&finetune, "/tmp", Aria2AddMode::Restart);
        assert_eq!(
            restart.get("allow-overwrite").and_then(|v| v.as_str()),
            Some("true")
        );
        assert_eq!(restart.get("continue"), None);
    }

    #[test]
    fn retry_options_are_translated_for_aria2() {
        let finetune = FineTune {
            max_retries: Some(3),
            retry_wait_seconds: Some(5),
            ..FineTune::default()
        };
        let options = finetune_to_options(&finetune, "/tmp", Aria2AddMode::Fresh);

        assert_eq!(options.get("max-tries").and_then(|v| v.as_str()), Some("4"));
        assert_eq!(
            options.get("retry-wait").and_then(|v| v.as_str()),
            Some("5")
        );

        let no_retries = FineTune {
            max_retries: Some(0),
            retry_wait_seconds: Some(0),
            ..FineTune::default()
        };
        let options = finetune_to_options(&no_retries, "/tmp", Aria2AddMode::Fresh);
        assert_eq!(options.get("max-tries").and_then(|v| v.as_str()), Some("1"));
        assert_eq!(
            options.get("retry-wait").and_then(|v| v.as_str()),
            Some("0")
        );

        let defaults = finetune_to_options(&FineTune::default(), "/tmp", Aria2AddMode::Fresh);
        assert_eq!(defaults.get("max-tries"), None);
        assert_eq!(defaults.get("retry-wait"), None);
    }

    #[test]
    fn parses_global_options_and_tolerates_bad_individual_values() {
        let parsed = parse_global_options(&serde_json::json!({
            "split": "5",
            "max-connection-per-server": "1",
            "file-allocation": "prealloc",
            "stream-piece-selector": "default"
        }))
        .unwrap();
        assert_eq!(parsed.connections_per_download, Some(5));
        assert_eq!(parsed.max_connections_per_server, Some(1));
        assert_eq!(parsed.alloc_strategy, Some(AllocStrategy::Prealloc));
        assert_eq!(
            parsed.stream_piece_selector,
            Some(StreamPieceSelector::Default)
        );

        let parsed = parse_global_options(&serde_json::json!({
            "split": "invalid",
            "file-allocation": "unknown"
        }))
        .unwrap();
        assert_eq!(parsed.connections_per_download, None);
        assert_eq!(parsed.max_connections_per_server, None);
        assert_eq!(parsed.alloc_strategy, None);
        assert_eq!(parsed.stream_piece_selector, None);
    }

    #[test]
    fn parses_multi_file_artifacts_and_top_level_control_file() {
        let status: Aria2Status = serde_json::from_value(serde_json::json!({
            "gid": "gid",
            "status": "active",
            "totalLength": "2",
            "completedLength": "1",
            "downloadSpeed": "1",
            "files": [{"path": "/downloads/set/a"}, {"path": "/downloads/set/b"}],
            "dir": "/downloads",
            "bittorrent": {"info": {"name": "set"}}
        }))
        .unwrap();

        let (payloads, controls) = status.artifact_paths();
        assert_eq!(payloads, vec!["/downloads/set/a", "/downloads/set/b"]);
        assert_eq!(controls, vec!["/downloads/set.aria2"]);
    }
}
