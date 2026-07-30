use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{enums::Recurrence, finetune::FineTune, scheduler::Scheduler};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct QueueSettings {
    /// aria2 `-j` / `--max-concurrent-downloads`
    pub max_concurrent_downloads: u32,
    pub max_retries: u32,
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
}

#[derive(serde::Deserialize)]
pub struct CreateQueueRequest {
    pub name: String,
    pub position: i32,
    pub max_concurrent_downloads: u32,
    pub max_retries: u32,
    pub default_finetune: FineTune,
    pub scheduler_enabled: bool,
    pub recurrence: Recurrence,
    pub run_missed_on_startup: bool,
}
