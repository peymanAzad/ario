use serde::{Deserialize, Serialize};

use crate::enums::Recurrence;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Scheduler {
    pub enabled: bool,
    pub recurrence: Recurrence,
    /// If a scheduled window was missed while the daemon was off, run it on next startup.
    pub run_missed_on_startup: bool,
}
