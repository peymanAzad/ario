//! Workflow integration tests: real HTTP, database, scheduler and poller; fake aria2 only.
use crate::{aria2::Aria2Client, config::ServerConfig, db::Database, state::AppState};
use axum::{
    Json, Router,
    extract::State,
    response::{IntoResponse, Response},
    routing::post,
};
use chrono::{DateTime, Duration, Local, NaiveTime, TimeZone, Utc};
use common::{
    download::{AddDownloadInput, AddDownloadsRequest, DownloadLiveStatus},
    enums::{DownloadStatus, Recurrence},
    finetune::FineTune,
    queue::{CreateQueueRequest, Queue, UpdateQueueRequest},
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::{sync::Notify, task::JoinHandle};

const SECRET: &str = "integration-secret";
pub(super) const LIMIT: std::time::Duration = std::time::Duration::from_secs(5);
static NEXT: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
struct Transfer {
    status: String,
    path: PathBuf,
    completed: u64,
    speed: u64,
    error: Option<String>,
    followed_by: Vec<String>,
}
impl Transfer {
    pub(super) fn json(&self, gid: &str) -> Value {
        json!({"followedBy":self.followed_by,"gid":gid,"status":self.status,"totalLength":"100","completedLength":self.completed.to_string(),"downloadSpeed":self.speed.to_string(),"errorMessage":self.error,"dir":self.path.parent().unwrap(),"files":[{"path":self.path,"length":"100","selected":"true"}]})
    }
}
#[derive(Clone)]
pub(super) struct Barrier {
    entered: Arc<Notify>,
    pub(super) release: Arc<Notify>,
}
impl Barrier {
    pub(super) fn new() -> Self {
        Self {
            entered: Arc::new(Notify::new()),
            release: Arc::new(Notify::new()),
        }
    }
    pub(super) async fn entered(&self) {
        tokio::time::timeout(LIMIT, self.entered.notified())
            .await
            .expect("RPC barrier not reached");
    }
}
pub(super) enum Fault {
    Rpc,
    Transport,
    LostPause,
    Delay(Barrier),
}
#[derive(Default)]
pub(super) struct Mock {
    transfers: HashMap<String, Transfer>,
    pub(super) calls: Vec<Value>,
    faults: HashMap<String, VecDeque<Fault>>,
    next_gid: u64,
    pub(super) max_active: usize,
}
type MockState = Arc<Mutex<Mock>>;

async fn rpc(State(mock): State<MockState>, Json(request): Json<Value>) -> Response {
    assert_eq!(request["jsonrpc"], "2.0");
    assert_eq!(request["id"], "ario");
    let method = request["method"].as_str().unwrap().to_owned();
    let params = request["params"].as_array().unwrap();
    assert_eq!(params[0], format!("token:{SECRET}"));
    let params = &params[1..];
    let fault = {
        let mut m = mock.lock().unwrap();
        m.calls.push(request.clone());
        m.faults.get_mut(&method).and_then(VecDeque::pop_front)
    };
    match &fault {
        Some(Fault::Delay(barrier)) => {
            barrier.entered.notify_one();
            tokio::time::timeout(LIMIT, barrier.release.notified()).await.expect("RPC barrier was not released");
        }
        Some(Fault::Rpc) => return Json(json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":1,"message":"scripted RPC failure"}})).into_response(),
        Some(Fault::Transport) => return (axum::http::StatusCode::BAD_GATEWAY, "scripted transport failure").into_response(),
        _ => {}
    }
    let result = {
        let mut m = mock.lock().unwrap();
        let result = match method.as_str() {
            "aria2.addUri" => {
                assert_eq!(params.len(), 2);
                let urls = params[0].as_array().unwrap();
                assert_eq!(urls.len(), 1);
                let url = reqwest::Url::parse(urls[0].as_str().unwrap()).unwrap();
                let options = params[1].as_object().unwrap();
                let dir = PathBuf::from(options["dir"].as_str().unwrap());
                assert!(dir.starts_with(std::env::temp_dir()));
                let path = dir.join(
                    url.path_segments()
                        .and_then(|mut parts| parts.next_back())
                        .unwrap_or("metadata"),
                );
                std::fs::write(&path, b"synthetic payload").unwrap();
                std::fs::write(format!("{}.aria2", path.display()), b"synthetic control").unwrap();
                m.next_gid += 1;
                let gid = format!("{:016x}", m.next_gid);
                m.transfers.insert(
                    gid.clone(),
                    Transfer {
                        status: "active".into(),
                        path,
                        completed: 0,
                        speed: 10,
                        error: None,
                        followed_by: Vec::new(),
                    },
                );
                json!(gid)
            }
            "aria2.addTorrent" => {
                use base64::Engine;
                assert_eq!(params.len(), 3);
                assert_eq!(params[1], json!([]));
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(params[0].as_str().unwrap())
                    .unwrap();
                let files = crate::routing::candidates_from_torrent(&bytes).unwrap();
                let dir = PathBuf::from(params[2]["dir"].as_str().unwrap());
                let path = dir.join(&files[0].filename);
                std::fs::write(&path, b"synthetic torrent payload").unwrap();
                m.next_gid += 1;
                let gid = format!("{:016x}", m.next_gid);
                m.transfers.insert(
                    gid.clone(),
                    Transfer {
                        status: "active".into(),
                        path,
                        completed: 0,
                        speed: 10,
                        error: None,
                        followed_by: Vec::new(),
                    },
                );
                json!(gid)
            }
            "aria2.changeOption" => {
                assert_eq!(params.len(), 2);
                let t = m.transfers.get_mut(params[0].as_str().unwrap()).unwrap();
                let dir = PathBuf::from(params[1]["dir"].as_str().unwrap());
                let path = dir.join(t.path.file_name().unwrap());
                std::fs::rename(&t.path, &path).unwrap();
                t.path = path;
                json!("OK")
            }
            "aria2.tellStatus" => {
                assert_eq!(params.len(), 2);
                assert_eq!(
                    params[1],
                    json!([
                        "status",
                        "totalLength",
                        "completedLength",
                        "downloadSpeed",
                        "errorMessage",
                        "files",
                        "dir",
                        "bittorrent",
                        "followedBy"
                    ])
                );
                let gid = params[0].as_str().unwrap();
                match m.transfers.get(gid) {
                    Some(t) => t.json(gid),
                    None => return Json(json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":1,"message":"GID not found"}})).into_response(),
                }
            }
            "aria2.pause" | "aria2.unpause" | "aria2.remove" => {
                assert_eq!(params.len(), 1);
                let gid = params[0].as_str().unwrap();
                let t = m.transfers.get_mut(gid).expect("unknown transfer");
                t.status = match method.as_str() {
                    "aria2.pause" => "paused",
                    "aria2.unpause" => "active",
                    _ => "removed",
                }
                .into();
                t.speed = if t.status == "active" { 10 } else { 0 };
                json!(gid)
            }
            "aria2.removeDownloadResult" => {
                assert_eq!(params.len(), 1);
                m.transfers.remove(params[0].as_str().unwrap());
                json!("OK")
            }
            "aria2.getVersion" => {
                assert!(params.is_empty());
                json!({"version":"mock","enabledFeatures":[]})
            }
            "aria2.getGlobalOption" => {
                assert!(params.is_empty());
                json!({})
            }
            _ => panic!("unexpected aria2 method {method}"),
        };
        let active = m
            .transfers
            .values()
            .filter(|t| t.status == "active")
            .count();
        m.max_active = m.max_active.max(active);
        result
    };
    if matches!(fault, Some(Fault::LostPause)) {
        assert_eq!(method, "aria2.pause");
        return (
            axum::http::StatusCode::BAD_GATEWAY,
            "pause applied, response lost",
        )
            .into_response();
    }
    Json(json!({"jsonrpc":"2.0","id":request["id"],"result":result})).into_response()
}

struct ServerTask {
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: JoinHandle<()>,
}
impl ServerTask {
    async fn stop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        (&mut self.task).await.expect("fixture server failed");
    }
}
impl Drop for ServerTask {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct FixtureDirectory(PathBuf);
impl Drop for FixtureDirectory {
    fn drop(&mut self) {
        let result = std::fs::remove_dir_all(&self.0);
        if !std::thread::panicking() {
            result.expect("failed to clean integration fixture");
        }
    }
}

pub(super) struct Fixture {
    pub(super) root: PathBuf,
    pub(super) state: AppState,
    clock: Arc<Mutex<DateTime<Utc>>>,
    pub(super) mock: MockState,
    rpc_url: String,
    pub(super) base: String,
    api_task: ServerTask,
    rpc_task: ServerTask,
    pub(super) http: reqwest::Client,
    // Fields drop in declaration order: close database/client handles before files.
    _directory: FixtureDirectory,
}
impl Fixture {
    pub(super) async fn new() -> Self {
        Self::with_managed(false).await
    }
    pub(super) async fn with_managed(managed: bool) -> Self {
        let root = std::env::temp_dir().join(format!(
            "ario-integration-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let mock = Arc::new(Mutex::new(Mock::default()));
        let (rpc_url, rpc_task) = serve(
            Router::new()
                .route("/jsonrpc", post(rpc))
                .with_state(mock.clone()),
        )
        .await;
        let rpc_url = format!("{rpc_url}/jsonrpc");
        let clock = Arc::new(Mutex::new(
            Utc.with_ymd_and_hms(2030, 1, 7, 12, 0, 0).unwrap(),
        ));
        let mut state = Self::open(&root, &rpc_url, clock.clone());
        state.tui_managed = managed;
        state.aria2_global_options = crate::load_aria2_global_options(&state.aria2).await;
        assert!(state.aria2_global_options.is_some());
        let (base, api_task) = serve(crate::router(state.clone())).await;
        let directory = FixtureDirectory(root.clone());
        Self {
            root,
            state,
            clock,
            mock,
            rpc_url,
            base,
            api_task,
            rpc_task,
            http: reqwest::Client::builder().timeout(LIMIT).build().unwrap(),
            _directory: directory,
        }
    }
    fn open(root: &std::path::Path, rpc_url: &str, clock: Arc<Mutex<DateTime<Utc>>>) -> AppState {
        let mut config = ServerConfig::default();
        config.settings.default_download_location = root.join("downloads").to_string_lossy().into();
        config.settings.category_locations.clear();
        AppState::new(
            Database::open(root.join("ario.db").to_str().unwrap()).unwrap(),
            Aria2Client::new(rpc_url, Some(SECRET.into())),
            config,
            false,
        )
        .with_clock(move || *clock.lock().unwrap())
    }
    pub(super) async fn reconfigure(&mut self, config: ServerConfig) {
        self.api_task.stop().await;
        self.state.config = Arc::new(config);
        (self.base, self.api_task) = serve(crate::router(self.state.clone())).await;
    }
    pub(super) async fn resolve_magnet(&self, id: i64) -> String {
        let parent = self.get(id).await.download.aria2_gid.unwrap();
        let mut m = self.mock.lock().unwrap();
        let path = self.root.join("downloads/resolved.mp4");
        std::fs::write(&path, b"synthetic magnet payload").unwrap();
        m.next_gid += 1;
        let child = format!("{:016x}", m.next_gid);
        m.transfers.insert(
            child.clone(),
            Transfer {
                status: "paused".into(),
                path,
                completed: 0,
                speed: 0,
                error: None,
                followed_by: Vec::new(),
            },
        );
        let t = m.transfers.get_mut(&parent).unwrap();
        t.status = "complete".into();
        t.followed_by = vec![child.clone()];
        child
    }
    pub(super) async fn restart(&mut self) {
        self.api_task.stop().await;
        // Release every old database/live-state owner before reopening the file.
        let placeholder = AppState::new(
            Database::open(":memory:").unwrap(),
            Aria2Client::new(&self.rpc_url, Some(SECRET.into())),
            ServerConfig::default(),
            false,
        );
        drop(std::mem::replace(&mut self.state, placeholder));
        self.state = Self::open(&self.root, &self.rpc_url, self.clock.clone());
        (self.base, self.api_task) = serve(crate::router(self.state.clone())).await;
        assert!(self.state.live_status.read().await.is_empty());
    }
    pub(super) async fn close(mut self) {
        self.api_task.stop().await;
        self.rpc_task.stop().await;
        drop(self);
    }
    pub(super) fn now(&self) -> DateTime<Utc> {
        *self.clock.lock().unwrap()
    }
    pub(super) fn set_time(&self, t: DateTime<Utc>) {
        *self.clock.lock().unwrap() = t;
    }
    pub(super) async fn tick(&self) {
        let _activity = self.state.activity_guard().await.unwrap();
        tokio::time::timeout(LIMIT, crate::poller::poll_once(&self.state))
            .await
            .expect("poller iteration timed out");
    }
    pub(super) async fn response(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> reqwest::Response {
        let mut req = self.http.request(method, format!("{}{path}", self.base));
        if let Some(body) = body {
            req = req.json(&body);
        }
        req.send().await.unwrap()
    }
    pub(super) async fn json(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Value {
        let r = self.response(method, path, body).await;
        let status = r.status();
        let bytes = r.bytes().await.unwrap();
        assert!(
            status.is_success(),
            "{path}: {status} {}",
            String::from_utf8_lossy(&bytes)
        );
        if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        }
    }
    pub(super) async fn post(&self, path: &str) {
        self.json(reqwest::Method::POST, path, None).await;
    }
    pub(super) async fn queue(
        &self,
        enabled: bool,
        recurrence: Recurrence,
        catchup: bool,
    ) -> Queue {
        let req = CreateQueueRequest {
            name: "integration".into(),
            position: 1,
            max_concurrent_downloads: 1,
            max_retries: 0,
            retry_wait_seconds: 1,
            default_finetune: FineTune::default(),
            scheduler_enabled: enabled,
            recurrence,
            run_missed_on_startup: catchup,
        };
        serde_json::from_value(
            self.json(reqwest::Method::POST, "/queues", Some(json!(req)))
                .await,
        )
        .unwrap()
    }
    pub(super) async fn once(&self) -> Queue {
        self.queue(
            true,
            Recurrence::Once {
                start: self.now() + Duration::minutes(1),
                end: self.now() + Duration::minutes(2),
            },
            false,
        )
        .await
    }
    pub(super) async fn edit(&self, q: &Queue) -> Queue {
        let req = UpdateQueueRequest {
            name: q.name.clone(),
            position: q.position,
            max_concurrent_downloads: q.settings.max_concurrent_downloads,
            max_retries: q.settings.max_retries,
            retry_wait_seconds: q.settings.retry_wait_seconds,
            default_finetune: q.settings.default_finetune.clone(),
            scheduler_enabled: q.scheduler.enabled,
            recurrence: q.scheduler.recurrence.clone(),
            run_missed_on_startup: q.scheduler.run_missed_on_startup,
        };
        serde_json::from_value(
            self.json(
                reqwest::Method::PUT,
                &format!("/queues/{}", q.id),
                Some(json!(req)),
            )
            .await,
        )
        .unwrap()
    }
    pub(super) async fn add(&self, q: i64, n: usize, immediate: bool) -> Vec<DownloadLiveStatus> {
        let offset = self
            .state
            .db
            .list_downloads(&Default::default())
            .unwrap()
            .len();
        let req = AddDownloadsRequest {
            inputs: (offset..offset + n)
                .map(|i| AddDownloadInput::Url(format!("http://fixture.invalid/file-{i}.bin")))
                .collect(),
            queue_id: q,
            finetune_override: None,
            start_immediately: immediate,
        };
        serde_json::from_value(
            self.json(reqwest::Method::POST, "/downloads", Some(json!(req)))
                .await,
        )
        .unwrap()
    }
    pub(super) async fn get(&self, id: i64) -> DownloadLiveStatus {
        serde_json::from_value(
            self.json(reqwest::Method::GET, &format!("/downloads/{id}"), None)
                .await,
        )
        .unwrap()
    }
    pub(super) async fn get_queue(&self, id: i64) -> Queue {
        serde_json::from_value(
            self.json(reqwest::Method::GET, &format!("/queues/{id}"), None)
                .await,
        )
        .unwrap()
    }
    pub(super) fn calls(&self, method: &str) -> usize {
        self.mock
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|c| c["method"] == method)
            .count()
    }
    pub(super) fn fault(&self, method: &str, fault: Fault) {
        self.mock
            .lock()
            .unwrap()
            .faults
            .entry(method.into())
            .or_default()
            .push_back(fault);
    }
    pub(super) async fn transfer(&self, id: i64, status: &str, bytes: u64) {
        let gid = self.get(id).await.download.aria2_gid.unwrap();
        let mut m = self.mock.lock().unwrap();
        let t = m.transfers.get_mut(&gid).unwrap();
        t.status = status.into();
        t.completed = bytes;
        t.speed = if status == "active" { 10 } else { 0 };
        t.error = (status == "error").then(|| "fixture transfer failure".into());
    }
    pub(super) async fn status(&self, id: i64, status: DownloadStatus) {
        assert_eq!(self.get(id).await.download.status, status);
    }
    pub(super) fn capacity(&self, n: usize) {
        assert!(
            self.mock.lock().unwrap().max_active <= n,
            "mock observed excess concurrent transfers"
        );
    }
}
async fn serve(app: Router) -> (String, ServerTask) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (shutdown, stopped) = tokio::sync::oneshot::channel();
    (
        base,
        ServerTask {
            shutdown: Some(shutdown),
            task: tokio::spawn(async move {
                axum::serve(listener, app)
                    .with_graceful_shutdown(async move {
                        let _ = stopped.await;
                    })
                    .await
                    .unwrap();
            }),
        },
    )
}

pub(super) fn local(day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
    Local
        .with_ymd_and_hms(2030, 1, day, hour, minute, 0)
        .single()
        .expect("fixture date must be unambiguous")
        .with_timezone(&Utc)
}
pub(super) fn weekly(start: u32, end: u32) -> Recurrence {
    Recurrence::Weekly {
        days: vec![chrono::Weekday::Mon],
        start_time: NaiveTime::from_hms_opt(start, 0, 0).unwrap(),
        end_time: NaiveTime::from_hms_opt(end, 0, 0).unwrap(),
    }
}
