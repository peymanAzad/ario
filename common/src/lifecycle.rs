use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShutdownOutcome {
    ShuttingDown,
    KeptRunning,
    NotManaged,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ShutdownIfIdleResponse {
    pub outcome: ShutdownOutcome,
    pub active_downloads: u64,
    pub scheduled_queues: u64,
}
