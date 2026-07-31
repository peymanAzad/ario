use common::{
    enums::{AllocStrategy, StreamPieceSelector},
    finetune::FineTune,
};
use serde::Deserialize;
use serde_json::{Value, json};

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
    ) -> Result<String, Aria2Error> {
        let options = finetune_to_options(finetune, destination_path);
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
        let options = finetune_to_options(finetune, destination_path);
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
            "files"
        ]);
        let result = self
            .call("aria2.tellStatus", vec![json!(gid), keys])
            .await?;
        serde_json::from_value(result).map_err(|e| Aria2Error::UnexpectedResponse(e.to_string()))
    }
}

fn finetune_to_options(f: &FineTune, destination_path: &str) -> Value {
    let mut opts = serde_json::Map::new();
    opts.insert("dir".into(), json!(destination_path));

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
}
