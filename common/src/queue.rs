use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{finetune::FineTune, scheduler::Scheduler};

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
    pub settings: QueueSettings,
    pub scheduler: Scheduler,
    pub created_at: DateTime<Utc>,
}
