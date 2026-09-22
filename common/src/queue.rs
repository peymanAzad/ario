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
    fn missing_retry_wait_uses_compatible_default() {
        let settings: QueueSettings = serde_json::from_str(
            r#"{"max_concurrent_downloads":1,"max_retries":3,"default_finetune":{}}"#,
        )
        .unwrap();

        assert_eq!(settings.retry_wait_seconds, DEFAULT_RETRY_WAIT_SECONDS);
    }
}
