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

/// aria2 often reports `completedLength=0` for a few seconds after session
/// restore, before control files are read. Keep last-known progress in that
/// window instead of treating the zero as a real reset.
pub fn coalesce_completed_length(reported: u64, persisted: Option<u64>) -> u64 {
    match persisted {
        Some(known) if reported == 0 && known > 0 => known,
        _ => reported,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aria2_zero_keeps_last_known_progress() {
        assert_eq!(coalesce_completed_length(0, Some(42)), 42);
        assert_eq!(coalesce_completed_length(0, Some(0)), 0);
        assert_eq!(coalesce_completed_length(0, None), 0);
        assert_eq!(coalesce_completed_length(80, Some(42)), 80);
        assert_eq!(coalesce_completed_length(80, None), 80);
    }
}
