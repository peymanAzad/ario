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
    #[serde(default)]
    pub on_success_command: Option<String>,
    #[serde(default)]
    pub on_stop_command: Option<String>,
    /// aria2 `-j` / `--max-concurrent-downloads`
    pub max_concurrent_downloads: u32,
    pub max_retries: u32,
    #[serde(default = "default_retry_wait_seconds")]
    pub retry_wait_seconds: u32,
    pub default_finetune: FineTune,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Queue {
    #[serde(default)]
    pub event_results: QueueEventResults,
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CreateQueueRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_success_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_stop_command: Option<String>,
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct UpdateQueueRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_success_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_stop_command: Option<String>,
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
        assert_eq!(queue.event_results, super::QueueEventResults::default());
        assert!(queue.settings.on_success_command.is_none());
        assert!(queue.settings.on_stop_command.is_none());
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

/// Latest command attempt for each event; execution history remains in the daemon database.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct QueueEventResults {
    pub on_success: Option<QueueEventResult>,
    pub on_stop: Option<QueueEventResult>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueueEventStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Interrupted,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct QueueEventResult {
    pub id: i64,
    pub run_id: i64,
    pub status: QueueEventStatus,
    pub queued_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub exit_code: Option<i32>,
    pub message: Option<String>,
}
