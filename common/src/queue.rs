use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    enums::{QueueStatus, Recurrence},
    finetune::FineTune,
    scheduler::Scheduler,
};

pub const DEFAULT_RETRY_WAIT_SECONDS: u32 = 5;

fn default_retry_wait_seconds() -> u32 {
    DEFAULT_RETRY_WAIT_SECONDS
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct QueueSettings {
    /// aria2 `-j` / `--max-concurrent-downloads`
    pub max_concurrent_downloads: u32,
    pub max_retries: u32,
    #[serde(default = "default_retry_wait_seconds")]
    pub retry_wait_seconds: u32,
    pub default_finetune: FineTune,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Queue {
    /// Scheduled boundary for the current manual or automatic run.
    #[serde(default)]
    pub scheduled_stop_at: Option<chrono::DateTime<chrono::Utc>>,
    pub id: i64,
    pub name: String,
    pub position: i32,
    pub settings: QueueSettings,
    pub scheduler: Scheduler,
    pub created_at: DateTime<Utc>,
    pub status: QueueStatus,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CreateQueueRequest {
    pub name: String,
    pub position: i32,
    pub max_concurrent_downloads: u32,
    pub max_retries: u32,
    #[serde(default = "default_retry_wait_seconds")]
    pub retry_wait_seconds: u32,
    pub default_finetune: FineTune,
    pub scheduler_enabled: bool,
    pub recurrence: Recurrence,
    pub run_missed_on_startup: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UpdateQueueRequest {
    pub name: String,
    pub position: i32,
    pub max_concurrent_downloads: u32,
    pub max_retries: u32,
    #[serde(default = "default_retry_wait_seconds")]
    pub retry_wait_seconds: u32,
    pub default_finetune: FineTune,
    pub scheduler_enabled: bool,
    pub recurrence: Recurrence,
    pub run_missed_on_startup: bool,
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_RETRY_WAIT_SECONDS, QueueSettings};

    #[test]
    fn old_queue_response_without_stop_deadline_is_compatible() {
        let queue: super::Queue = serde_json::from_str(r#"{
            "id":1,"name":"Main","position":0,
            "settings":{"max_concurrent_downloads":1,"max_retries":3,"default_finetune":{}},
            "scheduler":{"enabled":false,"recurrence":{"Weekly":{"days":[],"start_time":"00:00:00","end_time":"00:00:00"}},"run_missed_on_startup":false},
            "created_at":"2026-09-26T00:00:00Z","status":"Paused"
        }"#).unwrap();
        assert!(queue.scheduled_stop_at.is_none());
    }

    #[test]
    fn missing_retry_wait_uses_compatible_default() {
        let settings: QueueSettings = serde_json::from_str(
            r#"{"max_concurrent_downloads":1,"max_retries":3,"default_finetune":{}}"#,
        )
        .unwrap();

        assert_eq!(settings.retry_wait_seconds, DEFAULT_RETRY_WAIT_SECONDS);
    }
}
