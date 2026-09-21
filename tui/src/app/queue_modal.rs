use super::*;

use chrono::{
    DateTime, Duration as ChronoDuration, Local, NaiveDate, NaiveTime, TimeZone, Timelike, Utc,
    Weekday,
};

use crate::app::App;
use common::{
    download::Download,
    enums::{Recurrence, SortField},
    queue::{CreateQueueRequest, UpdateQueueRequest},
};

const TIME_STEP_MIN: i64 = 5;

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
    /// One-time schedules use separate date and time controls. They are
    /// interpreted in the same local timezone as weekly schedules, then
    /// converted to UTC only when the request is built.
    pub once_start_date: NaiveDate,
    pub once_start_time: NaiveTime,
    pub once_end_date: NaiveDate,
    pub once_end_time: NaiveTime,
    pub run_missed_on_startup: bool,
    /// Which day is highlighted for toggling, within the days row (cursor
    /// position 2 when `recurrence_kind == Weekly`) — a sub-cursor scoped
    /// to just that one row, moved with left/right instead of up/down.
    pub day_cursor: usize,
    /// 0 = enabled toggle, 1 = recurrence kind toggle, then depends on
    /// `recurrence_kind`: Weekly -> [2: days, 3: start time, 4: end time,
    /// 5: run_missed_on_startup]; Once -> [2: start date, 3: start time,
    /// 4: end date, 5: end time, 6: run_missed_on_startup].
    pub scheduler_cursor: usize,

    // ---- Name text-editing state ----
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
        if self.has_open_modal() {
            return;
        }

        let (once_start_date, once_start_time, once_end_date, once_end_time) =
            default_once_window();
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
            once_start_date,
            once_start_time,
            once_end_date,
            once_end_time,
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

    pub fn open_edit_queue_modal(&mut self) {
        if self.has_open_modal() {
            return;
        }
        if self.selected_queue == 0 {
            return;
        }
        let Some(queue) = self.queues.get(self.selected_queue - 1).cloned() else {
            return;
        };

        let (default_start_date, default_start_time, default_end_date, default_end_time) =
            default_once_window();
        let (
            recurrence_kind,
            weekly_days,
            weekly_start,
            weekly_end,
            once_start_date,
            once_start_time,
            once_end_date,
            once_end_time,
        ) = match &queue.scheduler.recurrence {
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
                    default_start_date,
                    default_start_time,
                    default_end_date,
                    default_end_time,
                )
            }
            Recurrence::Once { start, end } => (
                RecurrenceKind::Once,
                [false; 7],
                NaiveTime::from_hms_opt(2, 0, 0).unwrap(),
                NaiveTime::from_hms_opt(6, 0, 0).unwrap(),
                start.with_timezone(&Local).date_naive(),
                start.with_timezone(&Local).time(),
                end.with_timezone(&Local).date_naive(),
                end.with_timezone(&Local).time(),
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
            once_start_date,
            once_start_time,
            once_end_date,
            once_end_time,
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
            sort_by: Some(SortField::QueuePosition),
            sort_desc: false,
        };
        thread::spawn(move || {
            let result = api::list_downloads(&api_base, &filter);
            let _ = sender.send(Event::App(AppEvent::QueueDownloadsLoaded {
                queue_id: queue.id,
                result,
            }));
        });
    }

    /// Applies the background fetch triggered by `open_edit_queue_modal`.
    /// Only takes effect if the queue modal is still open — if the user
    /// cancelled before this arrived, there's nothing to populate.
    pub fn apply_queue_downloads_loaded(
        &mut self,
        queue_id: i64,
        result: anyhow::Result<Vec<DownloadLiveStatus>>,
    ) {
        let modal_matches_queue = matches!(
            self.queue_modal.as_ref().map(|modal| modal.mode),
            Some(QueueModalMode::Edit { queue_id: open_queue_id }) if open_queue_id == queue_id
        );
        if modal_matches_queue && let (Some(modal), Ok(list)) = (&mut self.queue_modal, result) {
            modal.items = list.into_iter().map(|d| d.download).collect();
        }
    }

    pub fn apply_queue_saved(&mut self, result: anyhow::Result<()>) {
        if let Err(error) = result {
            self.apply_toast(error.to_string(), ToastLevel::Error);
        }
        self.refresh();
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

    /// Enters text-edit mode for the queue name. Scheduler values use
    /// left/right adjustment controls instead of free-form text.
    pub fn queue_modal_start_text_edit(&mut self) {
        if let Some(m) = &mut self.queue_modal {
            let initial = match (
                m.tab,
                m.common_cursor,
                m.scheduler_cursor,
                m.recurrence_kind,
            ) {
                (QueueModalTab::Common, 0, _, _) => Some(m.name.clone()),
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
                        RecurrenceKind::Once => 6,
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
                        };
                        let max = match m.recurrence_kind {
                            RecurrenceKind::Weekly => 5,
                            RecurrenceKind::Once => 6,
                        };
                        m.scheduler_cursor = m.scheduler_cursor.min(max);
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
                    (2, RecurrenceKind::Once) => {
                        m.once_start_date = adjust_date(m.once_start_date, forward)
                    }
                    (3, RecurrenceKind::Once) => {
                        m.once_start_time = adjust_time(m.once_start_time, forward)
                    }
                    (4, RecurrenceKind::Once) => {
                        m.once_end_date = adjust_date(m.once_end_date, forward)
                    }
                    (5, RecurrenceKind::Once) => {
                        m.once_end_time = adjust_time(m.once_end_time, forward)
                    }
                    (6, RecurrenceKind::Once) => m.run_missed_on_startup = !m.run_missed_on_startup,
                    _ => {}
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
                let start = once_datetime(m.once_start_date, m.once_start_time, "start")?;
                let end = once_datetime(m.once_end_date, m.once_end_time, "end")?;
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
                let sender = self.event_sender.clone();
                thread::spawn(move || {
                    let result = api::create_queue(&api_base, &request).map(|_| ());
                    let _ = sender.send(Event::App(AppEvent::QueueSaved(result)));
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
                let sender = self.event_sender.clone();
                thread::spawn(move || {
                    let result = api::update_queue(&api_base, queue_id, &request).and_then(|_| {
                        if ordered_ids.is_empty() {
                            Ok(())
                        } else {
                            api::reorder_queue(&api_base, queue_id, &ordered_ids)
                        }
                    });
                    let _ = sender.send(Event::App(AppEvent::QueueSaved(result)));
                });
            }
        }
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
        ChronoDuration::minutes(TIME_STEP_MIN)
    } else {
        ChronoDuration::minutes(-TIME_STEP_MIN)
    };
    time + delta
}

fn adjust_date(date: NaiveDate, forward: bool) -> NaiveDate {
    let delta = ChronoDuration::days(if forward { 1 } else { -1 });
    date.checked_add_signed(delta).unwrap_or(date)
}

fn once_datetime(
    date: NaiveDate,
    time: NaiveTime,
    field_name: &str,
) -> Result<DateTime<Utc>, String> {
    Local
        .from_local_datetime(&date.and_time(time))
        .single()
        .map(|datetime| datetime.with_timezone(&Utc))
        .ok_or_else(|| format!("{field_name} date and time aren't valid in the local timezone"))
}

fn default_once_window() -> (NaiveDate, NaiveTime, NaiveDate, NaiveTime) {
    let now = Local::now();
    let remainder = now.minute() % TIME_STEP_MIN as u32;
    let minutes_to_next_step = if remainder == 0 && now.second() == 0 {
        0
    } else {
        TIME_STEP_MIN - i64::from(remainder)
    };
    let start = now + ChronoDuration::minutes(minutes_to_next_step);
    let start_time = NaiveTime::from_hms_opt(start.hour(), start.minute(), 0).unwrap();
    let start = start.date_naive().and_time(start_time);
    let end = start + ChronoDuration::hours(4);
    (start.date(), start.time(), end.date(), end.time())
}
