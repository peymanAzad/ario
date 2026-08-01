use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone, Copy, Debug, Default)]
pub struct LiveStats {
    pub completed_length: u64,
    pub download_speed: u64,
}

/// Key: Download.id
pub type LiveStatusMap = Arc<RwLock<HashMap<i64, LiveStats>>>;

pub fn new_map() -> LiveStatusMap {
    Arc::new(RwLock::new(HashMap::new()))
}
