use crate::aria2::Aria2Client;
use crate::config::ServerConfig;
use crate::db::Database;
use crate::live_status::LiveStatusMap;
use common::finetune::Aria2GlobalOptions;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{Notify, RwLock, RwLockReadGuard};

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Database>,
    pub aria2: Arc<Aria2Client>,
    pub config: Arc<ServerConfig>,
    pub live_status: LiveStatusMap,
    pub aria2_global_options: Option<Aria2GlobalOptions>,
    pub tui_managed: bool,
    pub shutdown_notify: Arc<Notify>,
    activity_gate: Arc<RwLock<()>>,
    stopping: Arc<AtomicBool>,
}

impl AppState {
    pub fn new(db: Database, aria2: Aria2Client, config: ServerConfig, tui_managed: bool) -> Self {
        Self {
            db: Arc::new(db),
            aria2: Arc::new(aria2),
            config: Arc::new(config),
            live_status: crate::live_status::new_map(),
            aria2_global_options: None,
            tui_managed,
            shutdown_notify: Arc::new(Notify::new()),
            activity_gate: Arc::new(RwLock::new(())),
            stopping: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn with_aria2_global_options(mut self, options: Option<Aria2GlobalOptions>) -> Self {
        self.aria2_global_options = options;
        self
    }

    pub async fn activity_guard(&self) -> Result<RwLockReadGuard<'_, ()>, crate::error::AppError> {
        let guard = self.activity_gate.read().await;
        if self.stopping.load(Ordering::SeqCst) {
            return Err(crate::error::AppError::Unavailable(
                "daemon is shutting down".into(),
            ));
        }
        Ok(guard)
    }

    pub async fn request_idle_shutdown(
        &self,
    ) -> Result<common::lifecycle::ShutdownIfIdleResponse, crate::error::AppError> {
        use common::lifecycle::{ShutdownIfIdleResponse, ShutdownOutcome};

        let _guard = self.activity_gate.write().await;
        let (active_downloads, scheduled_queues) = self.db.lifecycle_blocker_counts()?;
        let outcome = if !self.tui_managed {
            ShutdownOutcome::NotManaged
        } else if active_downloads > 0 || scheduled_queues > 0 {
            ShutdownOutcome::KeptRunning
        } else {
            self.stopping.store(true, Ordering::SeqCst);
            self.shutdown_notify.notify_one();
            ShutdownOutcome::ShuttingDown
        };

        Ok(ShutdownIfIdleResponse {
            outcome,
            active_downloads,
            scheduled_queues,
        })
    }

    pub async fn request_shutdown(
        &self,
    ) -> Result<common::lifecycle::ShutdownIfIdleResponse, crate::error::AppError> {
        use common::lifecycle::{ShutdownIfIdleResponse, ShutdownOutcome};

        let _guard = self.activity_gate.write().await;
        let (active_downloads, scheduled_queues) = self.db.lifecycle_blocker_counts()?;
        let outcome = if !self.tui_managed {
            ShutdownOutcome::NotManaged
        } else {
            self.stopping.store(true, Ordering::SeqCst);
            self.shutdown_notify.notify_one();
            ShutdownOutcome::ShuttingDown
        };

        Ok(ShutdownIfIdleResponse {
            outcome,
            active_downloads,
            scheduled_queues,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::lifecycle::ShutdownOutcome;

    fn state(tui_managed: bool) -> AppState {
        AppState::new(
            Database::open(":memory:").unwrap(),
            Aria2Client::new("http://127.0.0.1:1/jsonrpc", None),
            ServerConfig::default(),
            tui_managed,
        )
    }

    #[tokio::test]
    async fn idle_managed_daemon_commits_shutdown_and_rejects_mutation() {
        let state = state(true);
        let response = state.request_idle_shutdown().await.unwrap();
        assert_eq!(response.outcome, ShutdownOutcome::ShuttingDown);
        assert!(state.activity_guard().await.is_err());
    }

    #[tokio::test]
    async fn manual_daemon_is_not_stopped() {
        let state = state(false);
        let response = state.request_idle_shutdown().await.unwrap();
        assert_eq!(response.outcome, ShutdownOutcome::NotManaged);
        assert!(state.activity_guard().await.is_ok());
    }

    #[tokio::test]
    async fn enabled_schedule_keeps_managed_daemon_running() {
        let state = state(true);
        let mut queue = state.db.get_queue(1).unwrap().unwrap();
        queue.scheduler.enabled = true;
        state.db.update_queue(&queue).unwrap();

        let response = state.request_idle_shutdown().await.unwrap();
        assert_eq!(response.outcome, ShutdownOutcome::KeptRunning);
        assert_eq!(response.scheduled_queues, 1);
        assert!(state.activity_guard().await.is_ok());
    }

    fn insert_active_download(state: &AppState) {
        use chrono::Utc;
        use common::download::Download;
        use common::enums::{DownloadStatus, FileCategory, SourceType};
        use common::finetune::FineTune;

        let download = Download {
            id: 0,
            aria2_gid: None,
            url: "https://example.test/file".into(),
            filename: None,
            destination_path: "/tmp".into(),
            source_type: SourceType::Http,
            category: FileCategory::Other,
            status: DownloadStatus::Active,
            paused_by_scheduler: false,
            manually_started: false,
            size: None,
            completed_length: None,
            queue_id: 1,
            position_in_queue: 0,
            finetune: FineTune::default(),
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
        };
        state.db.insert_download(&download).unwrap();
    }

    #[tokio::test]
    async fn force_shutdown_stops_managed_daemon_with_active_work() {
        let state = state(true);
        insert_active_download(&state);
        let mut queue = state.db.get_queue(1).unwrap().unwrap();
        queue.scheduler.enabled = true;
        state.db.update_queue(&queue).unwrap();

        let response = state.request_shutdown().await.unwrap();
        assert_eq!(response.outcome, ShutdownOutcome::ShuttingDown);
        assert_eq!(response.active_downloads, 1);
        assert_eq!(response.scheduled_queues, 1);
        assert!(state.activity_guard().await.is_err());
    }

    #[tokio::test]
    async fn force_shutdown_does_not_stop_unmanaged_daemon() {
        let state = state(false);
        insert_active_download(&state);

        let response = state.request_shutdown().await.unwrap();
        assert_eq!(response.outcome, ShutdownOutcome::NotManaged);
        assert_eq!(response.active_downloads, 1);
        assert!(state.activity_guard().await.is_ok());
    }
}
