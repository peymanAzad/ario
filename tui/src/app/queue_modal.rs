use super::*;

use chrono::{DateTime, Duration as ChronoDuration, NaiveTime, Utc, Weekday};

use crate::app::App;
use common::{
    download::Download,
    enums::Recurrence,
    queue::{CreateQueueRequest, UpdateQueueRequest},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueModalTab {
    Common,
    Scheduler,
    DownloadItems,
}

impl QueueModalTab {
    fn next(self, mode: QueueModalMode) -> Self {
        match (self, mode) {
            (QueueModalTab::Common, _) => QueueModalTab::Scheduler,
            (QueueModalTab::Scheduler, QueueModalMode::Edit { .. }) => QueueModalTab::DownloadItems,
            (QueueModalTab::Scheduler, QueueModalMode::Create) => QueueModalTab::Common,
            (QueueModalTab::DownloadItems, _) => QueueModalTab::Common,
        }
    }

    fn prev(self, mode: QueueModalMode) -> Self {
        match (self, mode) {
            (QueueModalTab::Common, QueueModalMode::Edit { .. }) => QueueModalTab::DownloadItems,
            (QueueModalTab::Common, QueueModalMode::Create) => QueueModalTab::Scheduler,
            (QueueModalTab::Scheduler, _) => QueueModalTab::Common,
            (QueueModalTab::DownloadItems, _) => QueueModalTab::Scheduler,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueModalMode {
    Create,
    Edit { queue_id: i64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecurrenceKind {
    Once,
    Weekly,
}

/// Monday-first ordering used for both the day-picker's index math and its
/// display labels — single source of truth for "day index 0 == Monday".
const WEEKDAY_ORDER: [Weekday; 7] = [
    Weekday::Mon,
    Weekday::Tue,
    Weekday::Wed,
    Weekday::Thu,
    Weekday::Fri,
    Weekday::Sat,
    Weekday::Sun,
];

/// State for the create/edit queue modal. Everything here is staged
/// in-memory and only sent to the server on Save — Cancel just drops this
/// struct with zero side effects, including any reordering done on the
/// Download Items tab (that's why reordering happens on a local `Vec`
/// snapshot here, not by calling the reorder endpoint on every keystroke).
pub struct QueueModal {
    pub mode: QueueModalMode,
    pub tab: QueueModalTab,

    // ---- Common tab ----
    pub name: String,
    pub max_concurrent_downloads: u32,
    pub max_retries: u32,
    pub finetune: FineTune,
    /// 0 = name, 1 = max_concurrent_downloads, 2 = max_retries,
    /// 3-6 = finetune fields (same 4-field order as the clipboard modal's
    /// Fine Tuning tab, reusing `adjust_finetune_field`).
    pub common_cursor: usize,

    // ---- Scheduler tab ----
    pub scheduler_enabled: bool,
    pub recurrence_kind: RecurrenceKind,
    /// Indexed via `WEEKDAY_ORDER` (Monday-first).
    pub weekly_days: [bool; 7],
    pub weekly_start: NaiveTime,
    pub weekly_end: NaiveTime,
    /// Free-text RFC3339 datetime strings, parsed on Save — see
    /// `queue_modal_confirm_text_edit`'s doc comment for why `Once` uses
    /// text entry rather than a dedicated date/time widget.
    pub once_start: String,
    pub once_end: String,
    pub run_missed_on_startup: bool,
    /// Which day is highlighted for toggling, within the days row (cursor
    /// position 2 when `recurrence_kind == Weekly`) — a sub-cursor scoped
    /// to just that one row, moved with left/right instead of up/down.
    pub day_cursor: usize,
    /// 0 = enabled toggle, 1 = recurrence kind toggle, then depends on
    /// `recurrence_kind`: Weekly -> [2: days, 3: start time, 4: end time,
    /// 5: run_missed_on_startup]; Once -> [2: start text, 3: end text,
    /// 4: run_missed_on_startup].
    pub scheduler_cursor: usize,

    // ---- Shared text-editing state (name field, Once start/end) ----
    pub editing_text: bool,
    pub text_buffer: String,

    // ---- Download Items tab (Edit mode only) ----
    /// Local snapshot fetched when the modal opened — reordered in-memory,
    /// persisted via a single reorder call on Save, not per-keystroke.
    pub items: Vec<Download>,
    pub item_cursor: usize,

    /// Set when Save fails validation (e.g. an unparseable Once date) —
    /// shown in the modal rather than silently closing or crashing.
    pub error: Option<String>,
}

impl App {
    pub fn open_create_queue_modal(&mut self) {
        if self.modal.is_some() || self.queue_modal.is_some() {
            return;
        }

        self.queue_modal = Some(QueueModal {
            mode: QueueModalMode::Create,
            tab: QueueModalTab::Common,
            name: String::new(),
            max_concurrent_downloads: 1,
            max_retries: 3,
            finetune: FineTune::default(),
            common_cursor: 0,
            scheduler_enabled: false,
            recurrence_kind: RecurrenceKind::Weekly,
            weekly_days: [false; 7],
            weekly_start: NaiveTime::from_hms_opt(2, 0, 0).unwrap(),
            weekly_end: NaiveTime::from_hms_opt(6, 0, 0).unwrap(),
            once_start: String::new(),
            once_end: String::new(),
            run_missed_on_startup: false,
            day_cursor: 0,
            scheduler_cursor: 0,
            editing_text: false,
            text_buffer: String::new(),
            items: Vec::new(),
            item_cursor: 0,
            error: None,
        });
    }

    /// Opens the edit modal for the currently-selected queue in the sidebar
    /// (a no-op if "All" — index 0 — is selected, since that's not a real
    /// queue). Populates every field from the existing queue immediately;
    /// the Download Items tab starts empty and is filled in shortly after
    /// by a background fetch (see `apply_queue_downloads_loaded`) so opening
    /// the modal never blocks on the network.
    pub fn open_edit_queue_modal(&mut self) {
        if self.modal.is_some() || self.queue_modal.is_some() {
            return;
        }
        if self.selected_queue == 0 {
            return;
        }
        let Some(queue) = self.queues.get(self.selected_queue - 1).cloned() else {
            return;
        };

        let (recurrence_kind, weekly_days, weekly_start, weekly_end, once_start, once_end) =
            match &queue.scheduler.recurrence {
                Recurrence::Weekly {
                    days,
                    start_time,
                    end_time,
                } => {
                    let mut wd = [false; 7];
                    for d in days {
                        if let Some(idx) = WEEKDAY_ORDER.iter().position(|w| w == d) {
                            wd[idx] = true;
                        }
                    }
                    (
                        RecurrenceKind::Weekly,
                        wd,
                        *start_time,
                        *end_time,
                        String::new(),
                        String::new(),
                    )
                }
                Recurrence::Once { start, end } => (
                    RecurrenceKind::Once,
                    [false; 7],
                    NaiveTime::from_hms_opt(2, 0, 0).unwrap(),
                    NaiveTime::from_hms_opt(6, 0, 0).unwrap(),
                    start.to_rfc3339(),
                    end.to_rfc3339(),
                ),
            };

        self.queue_modal = Some(QueueModal {
            mode: QueueModalMode::Edit { queue_id: queue.id },
            tab: QueueModalTab::Common,
            name: queue.name,
            max_concurrent_downloads: queue.settings.max_concurrent_downloads,
            max_retries: queue.settings.max_retries,
            finetune: queue.settings.default_finetune,
            common_cursor: 0,
            scheduler_enabled: queue.scheduler.enabled,
            recurrence_kind,
            weekly_days,
            weekly_start,
            weekly_end,
            once_start,
            once_end,
            run_missed_on_startup: queue.scheduler.run_missed_on_startup,
            day_cursor: 0,
            scheduler_cursor: 0,
            editing_text: false,
            text_buffer: String::new(),
            items: Vec::new(),
            item_cursor: 0,
            error: None,
        });

        // Non-blocking fetch of this queue's downloads for the Download
        // Items tab — same background-thread-plus-event pattern as
        // everything else that talks to the server.
        let api_base = self.api_base.clone();
        let sender = self.event_sender.clone();
        let filter = DownloadFilter {
            queue_id: Some(queue.id),
            category: None,
            status: None,
            sort_by: None,
            sort_desc: false,
        };
        thread::spawn(move || {
            let result = api::list_downloads(&api_base, &filter);
            let _ = sender.send(Event::App(AppEvent::QueueDownloadsLoaded(result)));
        });
    }

    /// Applies the background fetch triggered by `open_edit_queue_modal`.
    /// Only takes effect if the queue modal is still open — if the user
    /// cancelled before this arrived, there's nothing to populate.
    pub fn apply_queue_downloads_loaded(
        &mut self,
        result: anyhow::Result<Vec<DownloadLiveStatus>>,
    ) {
        if let (Some(modal), Ok(list)) = (&mut self.queue_modal, result) {
            modal.items = list.into_iter().map(|d| d.download).collect();
        }
    }

    pub fn cancel_queue_modal(&mut self) {
        self.queue_modal = None;
    }

    pub fn queue_modal_next_tab(&mut self) {
        if let Some(m) = &mut self.queue_modal {
            m.tab = m.tab.next(m.mode);
        }
    }

    pub fn queue_modal_prev_tab(&mut self) {
        if let Some(m) = &mut self.queue_modal {
            m.tab = m.tab.prev(m.mode);
        }
    }

    /// Enters text-edit mode for whichever field the cursor is currently on
    /// — the name field (Common tab, cursor 0) or the Once start/end fields
    /// (Scheduler tab, cursors 2/3 when `recurrence_kind == Once`). A
    /// dedicated date/time picker widget would be nicer, but free-text
    /// RFC3339 entry reuses the exact same edit mechanism as the name
    /// field, which is a meaningfully smaller amount of new code for a v1 —
    /// worth revisiting if hand-typing timestamps proves too fiddly in
    /// practice.
    pub fn queue_modal_start_text_edit(&mut self) {
        if let Some(m) = &mut self.queue_modal {
            let initial = match (
                m.tab,
                m.common_cursor,
                m.scheduler_cursor,
                m.recurrence_kind,
            ) {
                (QueueModalTab::Common, 0, _, _) => Some(m.name.clone()),
                (QueueModalTab::Scheduler, _, 2, RecurrenceKind::Once) => {
                    Some(m.once_start.clone())
                }
                (QueueModalTab::Scheduler, _, 3, RecurrenceKind::Once) => Some(m.once_end.clone()),
                _ => None,
            };
            if let Some(text) = initial {
                m.text_buffer = text;
                m.editing_text = true;
            }
        }
    }

    pub fn queue_modal_text_input(&mut self, c: char) {
        if let Some(m) = &mut self.queue_modal {
            if m.editing_text {
                m.text_buffer.push(c);
            }
        }
    }

    pub fn queue_modal_text_backspace(&mut self) {
        if let Some(m) = &mut self.queue_modal {
            if m.editing_text {
                m.text_buffer.pop();
            }
        }
    }

    pub fn queue_modal_confirm_text_edit(&mut self) {
        if let Some(m) = &mut self.queue_modal {
            if !m.editing_text {
                return;
            }
            match (
                m.tab,
                m.common_cursor,
                m.scheduler_cursor,
                m.recurrence_kind,
            ) {
                (QueueModalTab::Common, 0, _, _) => m.name = m.text_buffer.clone(),
                (QueueModalTab::Scheduler, _, 2, RecurrenceKind::Once) => {
                    m.once_start = m.text_buffer.clone()
                }
                (QueueModalTab::Scheduler, _, 3, RecurrenceKind::Once) => {
                    m.once_end = m.text_buffer.clone()
                }
                _ => {}
            }
            m.editing_text = false;
        }
    }

    pub fn queue_modal_cancel_text_edit(&mut self) {
        if let Some(m) = &mut self.queue_modal {
            m.editing_text = false;
        }
    }

    pub fn queue_modal_move_down(&mut self) {
        if let Some(m) = &mut self.queue_modal {
            match m.tab {
                QueueModalTab::Common => m.common_cursor = (m.common_cursor + 1).min(6),
                QueueModalTab::Scheduler => {
                    let max = match m.recurrence_kind {
                        RecurrenceKind::Weekly => 5,
                        RecurrenceKind::Once => 4,
                    };
                    m.scheduler_cursor = (m.scheduler_cursor + 1).min(max);
                }
                QueueModalTab::DownloadItems => {
                    if !m.items.is_empty() {
                        m.item_cursor = (m.item_cursor + 1).min(m.items.len() - 1);
                    }
                }
            }
        }
    }

    pub fn queue_modal_move_up(&mut self) {
        if let Some(m) = &mut self.queue_modal {
            match m.tab {
                QueueModalTab::Common => m.common_cursor = m.common_cursor.saturating_sub(1),
                QueueModalTab::Scheduler => {
                    m.scheduler_cursor = m.scheduler_cursor.saturating_sub(1)
                }
                QueueModalTab::DownloadItems => m.item_cursor = m.item_cursor.saturating_sub(1),
            }
        }
    }

    /// Left/right: field-value adjustment (Common/Scheduler tabs) or moving
    /// the day-picker's sub-cursor (Scheduler tab, days row). Not used on
    /// Download Items — that tab uses dedicated move-up/down keys instead
    /// (`J`/`K`), since left/right has no natural meaning for reordering a
    /// vertical list.
    fn queue_modal_adjust(&mut self, forward: bool) {
        if let Some(m) = &mut self.queue_modal {
            match m.tab {
                QueueModalTab::Common => match m.common_cursor {
                    1 => {
                        m.max_concurrent_downloads =
                            adjust_u32_bounded(m.max_concurrent_downloads, forward, 1, 20)
                    }
                    2 => m.max_retries = adjust_u32_bounded(m.max_retries, forward, 0, 20),
                    3..=6 => adjust_finetune_field(&mut m.finetune, m.common_cursor - 3, forward),
                    _ => {} // cursor 0 (name) — handled via text-edit instead
                },
                QueueModalTab::Scheduler => match (m.scheduler_cursor, m.recurrence_kind) {
                    (0, _) => m.scheduler_enabled = !m.scheduler_enabled,
                    (1, _) => {
                        m.recurrence_kind = match m.recurrence_kind {
                            RecurrenceKind::Once => RecurrenceKind::Weekly,
                            RecurrenceKind::Weekly => RecurrenceKind::Once,
                        }
                    }
                    (2, RecurrenceKind::Weekly) => {
                        m.day_cursor = if forward {
                            (m.day_cursor + 1).min(6)
                        } else {
                            m.day_cursor.saturating_sub(1)
                        }
                    }
                    (3, RecurrenceKind::Weekly) => {
                        m.weekly_start = adjust_time(m.weekly_start, forward)
                    }
                    (4, RecurrenceKind::Weekly) => {
                        m.weekly_end = adjust_time(m.weekly_end, forward)
                    }
                    (5, RecurrenceKind::Weekly) => {
                        m.run_missed_on_startup = !m.run_missed_on_startup
                    }
                    (4, RecurrenceKind::Once) => m.run_missed_on_startup = !m.run_missed_on_startup,
                    _ => {} // Once's start/end (cursors 2/3) — handled via text-edit
                },
                QueueModalTab::DownloadItems => {}
            }
        }
    }

    pub fn queue_modal_adjust_left(&mut self) {
        self.queue_modal_adjust(false);
    }

    pub fn queue_modal_adjust_right(&mut self) {
        self.queue_modal_adjust(true);
    }

    /// Toggles the currently-highlighted day in the Weekly days row
    /// (Scheduler tab, cursor 2, sub-cursor `day_cursor`).
    pub fn queue_modal_toggle_day(&mut self) {
        if let Some(m) = &mut self.queue_modal {
            if m.tab == QueueModalTab::Scheduler
                && m.scheduler_cursor == 2
                && m.recurrence_kind == RecurrenceKind::Weekly
            {
                m.weekly_days[m.day_cursor] = !m.weekly_days[m.day_cursor];
            }
        }
    }

    /// Moves the selected download one position later in the queue's order
    /// (Download Items tab only) — a local `Vec::swap`, not an API call;
    /// persisted all at once on Save.
    pub fn queue_modal_move_item_down(&mut self) {
        if let Some(m) = &mut self.queue_modal {
            if m.tab == QueueModalTab::DownloadItems && m.item_cursor + 1 < m.items.len() {
                m.items.swap(m.item_cursor, m.item_cursor + 1);
                m.item_cursor += 1;
            }
        }
    }

    pub fn queue_modal_move_item_up(&mut self) {
        if let Some(m) = &mut self.queue_modal {
            if m.tab == QueueModalTab::DownloadItems && m.item_cursor > 0 {
                m.items.swap(m.item_cursor, m.item_cursor - 1);
                m.item_cursor -= 1;
            }
        }
    }

    fn build_recurrence(m: &QueueModal) -> Result<Recurrence, String> {
        match m.recurrence_kind {
            RecurrenceKind::Weekly => {
                let days: Vec<Weekday> = WEEKDAY_ORDER
                    .iter()
                    .zip(m.weekly_days.iter())
                    .filter(|&(_, &selected)| selected)
                    .map(|(day, _)| *day)
                    .collect();
                Ok(Recurrence::Weekly {
                    days,
                    start_time: m.weekly_start,
                    end_time: m.weekly_end,
                })
            }
            RecurrenceKind::Once => {
                let start = DateTime::parse_from_rfc3339(&m.once_start)
                    .map(|dt| dt.with_timezone(&Utc))
                    .map_err(|_| {
                        format!(
                            "start date {:?} isn't valid RFC3339 (e.g. \"2026-08-10T02:00:00Z\")",
                            m.once_start
                        )
                    })?;
                let end = DateTime::parse_from_rfc3339(&m.once_end)
                    .map(|dt| dt.with_timezone(&Utc))
                    .map_err(|_| {
                        format!(
                            "end date {:?} isn't valid RFC3339 (e.g. \"2026-08-10T06:00:00Z\")",
                            m.once_end
                        )
                    })?;
                Ok(Recurrence::Once { start, end })
            }
        }
    }

    /// Save: validates, then fires the create/update request (and a
    /// reorder call if the Download Items tab's order changed) in a
    /// background thread. Closes the modal immediately on a successful
    /// build of the request — errors from the request itself (as opposed
    /// to local validation) currently just surface on the next refresh,
    /// matching how other fire-and-forget actions in this app behave.
    pub fn save_queue_modal(&mut self) {
        let Some(modal) = &mut self.queue_modal else {
            return;
        };

        if modal.name.trim().is_empty() {
            modal.error = Some("name can't be empty".to_string());
            return;
        }

        let recurrence = match Self::build_recurrence(modal) {
            Ok(r) => r,
            Err(e) => {
                modal.error = Some(e);
                return;
            }
        };

        let modal = self.queue_modal.take().unwrap(); // known Some, just validated above
        let api_base = self.api_base.clone();

        match modal.mode {
            QueueModalMode::Create => {
                let request = CreateQueueRequest {
                    name: modal.name,
                    position: 0,
                    max_concurrent_downloads: modal.max_concurrent_downloads,
                    max_retries: modal.max_retries,
                    default_finetune: modal.finetune,
                    scheduler_enabled: modal.scheduler_enabled,
                    recurrence,
                    run_missed_on_startup: modal.run_missed_on_startup,
                };
                thread::spawn(move || {
                    let _ = api::create_queue(&api_base, &request);
                });
            }
            QueueModalMode::Edit { queue_id } => {
                let request = UpdateQueueRequest {
                    name: modal.name,
                    position: 0,
                    max_concurrent_downloads: modal.max_concurrent_downloads,
                    max_retries: modal.max_retries,
                    default_finetune: modal.finetune,
                    scheduler_enabled: modal.scheduler_enabled,
                    recurrence,
                    run_missed_on_startup: modal.run_missed_on_startup,
                };
                let ordered_ids: Vec<i64> = modal.items.iter().map(|d| d.id).collect();
                thread::spawn(move || {
                    let _ = api::update_queue(&api_base, queue_id, &request);
                    if !ordered_ids.is_empty() {
                        let _ = api::reorder_queue(&api_base, queue_id, &ordered_ids);
                    }
                });
            }
        }

        self.refresh();
    }
}

fn adjust_u32_bounded(current: u32, forward: bool, min: u32, max: u32) -> u32 {
    if forward {
        (current + 1).min(max)
    } else {
        current.saturating_sub(1).max(min)
    }
}

fn adjust_time(time: NaiveTime, forward: bool) -> NaiveTime {
    let delta = if forward {
        ChronoDuration::minutes(30)
    } else {
        ChronoDuration::minutes(-30)
    };
    time + delta
}
