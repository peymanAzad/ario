use serde::{Deserialize, Serialize};

use crate::enums::Recurrence;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Scheduler {
    pub enabled: bool,
    pub recurrence: Recurrence,
    /// Compatibility setting: startup catch-up only runs in an open window; closed windows are skipped.
    pub run_missed_on_startup: bool,
}
