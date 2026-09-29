use chrono::{
    DateTime, Duration as ChronoDuration, Local, NaiveDate, NaiveTime, TimeZone, Timelike, Utc,
    Weekday,
};
use common::{
    download::{Download, DownloadLiveStatus},
    enums::Recurrence,
    finetune::FineTune,
    queue::{CreateQueueRequest, DEFAULT_RETRY_WAIT_SECONDS, UpdateQueueRequest},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Frame, layout::Rect};

use crate::app::{App, ToastLevel, adjust_finetune_field};
use crate::effects::{ApiRequest, Effect};
use crate::modal::{Component, Ctx, Modal, ModalOutcome};
use crate::msg::Action;

mod view;

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
#[derive(Debug)]
pub struct QueueModal {
    pub mode: QueueModalMode,
    pub tab: QueueModalTab,

    // ---- Common tab ----
    pub name: String,
    pub max_concurrent_downloads: u32,
    pub max_retries: u32,
    pub retry_wait_seconds: u32,
    pub finetune: FineTune,
    /// 0 = name, 1 = max_concurrent_downloads, 2 = max_retries,
    /// 3 = retry_wait_seconds, 4-7 = the remaining finetune fields
    /// (same 4-field order as the clipboard modal's
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

impl QueueModal {
    pub fn set_items(&mut self, items: Vec<Download>) {
        self.items = items;
        if !self.items.is_empty() {
            self.item_cursor = self.item_cursor.min(self.items.len() - 1);
        } else {
            self.item_cursor = 0;
        }
    }

    fn next_tab(&mut self) {
        self.tab = self.tab.next(self.mode);
    }

    fn prev_tab(&mut self) {
        self.tab = self.tab.prev(self.mode);
    }

    fn start_text_edit(&mut self) {
        let initial = match (
            self.tab,
            self.common_cursor,
            self.scheduler_cursor,
            self.recurrence_kind,
        ) {
            (QueueModalTab::Common, 0, _, _) => Some(self.name.clone()),
            _ => None,
        };
        if let Some(text) = initial {
            self.text_buffer = text;
            self.editing_text = true;
        }
    }

    fn text_input(&mut self, c: char) {
        if self.editing_text {
            self.text_buffer.push(c);
        }
    }

    fn text_backspace(&mut self) {
        if self.editing_text {
            self.text_buffer.pop();
        }
    }

    fn confirm_text_edit(&mut self) {
        if !self.editing_text {
            return;
        }
        match (
            self.tab,
            self.common_cursor,
            self.scheduler_cursor,
            self.recurrence_kind,
        ) {
            (QueueModalTab::Common, 0, _, _) => self.name = self.text_buffer.clone(),
            _ => {}
        }
        self.editing_text = false;
    }

    fn cancel_text_edit(&mut self) {
        self.editing_text = false;
    }

    fn move_down(&mut self) {
        match self.tab {
            QueueModalTab::Common => self.common_cursor = (self.common_cursor + 1).min(7),
            QueueModalTab::Scheduler => {
                let max = match self.recurrence_kind {
                    RecurrenceKind::Weekly => 5,
                    RecurrenceKind::Once => 6,
                };
                self.scheduler_cursor = (self.scheduler_cursor + 1).min(max);
            }
            QueueModalTab::DownloadItems => {
                if !self.items.is_empty() {
                    self.item_cursor = (self.item_cursor + 1).min(self.items.len() - 1);
                }
            }
        }
    }

    fn move_up(&mut self) {
        match self.tab {
            QueueModalTab::Common => self.common_cursor = self.common_cursor.saturating_sub(1),
            QueueModalTab::Scheduler => {
                self.scheduler_cursor = self.scheduler_cursor.saturating_sub(1)
            }
            QueueModalTab::DownloadItems => self.item_cursor = self.item_cursor.saturating_sub(1),
        }
    }

    fn adjust(&mut self, forward: bool) {
        match self.tab {
            QueueModalTab::Common => match self.common_cursor {
                1 => {
                    self.max_concurrent_downloads =
                        adjust_u32_bounded(self.max_concurrent_downloads, forward, 1, 20)
                }
                2 => self.max_retries = adjust_u32_bounded(self.max_retries, forward, 0, 20),
                3 => {
                    self.retry_wait_seconds =
                        adjust_u32_bounded(self.retry_wait_seconds, forward, 0, 300)
                }
                4..=7 => adjust_finetune_field(&mut self.finetune, self.common_cursor - 4, forward),
                _ => {}
            },
            QueueModalTab::Scheduler => match (self.scheduler_cursor, self.recurrence_kind) {
                (0, _) => self.scheduler_enabled = !self.scheduler_enabled,
                (1, _) => {
                    self.recurrence_kind = match self.recurrence_kind {
                        RecurrenceKind::Once => RecurrenceKind::Weekly,
                        RecurrenceKind::Weekly => RecurrenceKind::Once,
                    };
                    let max = match self.recurrence_kind {
                        RecurrenceKind::Weekly => 5,
                        RecurrenceKind::Once => 6,
                    };
                    self.scheduler_cursor = self.scheduler_cursor.min(max);
                }
                (2, RecurrenceKind::Weekly) => {
                    self.day_cursor = if forward {
                        (self.day_cursor + 1).min(6)
                    } else {
                        self.day_cursor.saturating_sub(1)
                    }
                }
                (3, RecurrenceKind::Weekly) => {
                    self.weekly_start = adjust_time(self.weekly_start, forward)
                }
                (4, RecurrenceKind::Weekly) => {
                    self.weekly_end = adjust_time(self.weekly_end, forward)
                }
                (5, RecurrenceKind::Weekly) => {
                    self.run_missed_on_startup = !self.run_missed_on_startup
                }
                (2, RecurrenceKind::Once) => {
                    self.once_start_date = adjust_date(self.once_start_date, forward)
                }
                (3, RecurrenceKind::Once) => {
                    self.once_start_time = adjust_time(self.once_start_time, forward)
                }
                (4, RecurrenceKind::Once) => {
                    self.once_end_date = adjust_date(self.once_end_date, forward)
                }
                (5, RecurrenceKind::Once) => {
                    self.once_end_time = adjust_time(self.once_end_time, forward)
                }
                (6, RecurrenceKind::Once) => {
                    self.run_missed_on_startup = !self.run_missed_on_startup
                }
                _ => {}
            },
            QueueModalTab::DownloadItems => {}
        }
    }

    fn toggle_day(&mut self) {
        if self.tab == QueueModalTab::Scheduler
            && self.scheduler_cursor == 2
            && self.recurrence_kind == RecurrenceKind::Weekly
        {
            self.weekly_days[self.day_cursor] = !self.weekly_days[self.day_cursor];
        }
    }

    fn move_item_down(&mut self) {
        if self.tab == QueueModalTab::DownloadItems && self.item_cursor + 1 < self.items.len() {
            self.items.swap(self.item_cursor, self.item_cursor + 1);
            self.item_cursor += 1;
        }
    }

    fn move_item_up(&mut self) {
        if self.tab == QueueModalTab::DownloadItems && self.item_cursor > 0 {
            self.items.swap(self.item_cursor, self.item_cursor - 1);
            self.item_cursor -= 1;
        }
    }

    fn build_recurrence(&self) -> Result<Recurrence, String> {
        match self.recurrence_kind {
            RecurrenceKind::Weekly => {
                let days: Vec<Weekday> = WEEKDAY_ORDER
                    .iter()
                    .zip(self.weekly_days.iter())
                    .filter(|&(_, &selected)| selected)
                    .map(|(day, _)| *day)
                    .collect();
                Ok(Recurrence::Weekly {
                    days,
                    start_time: self.weekly_start,
                    end_time: self.weekly_end,
                })
            }
            RecurrenceKind::Once => {
                let start = once_datetime(self.once_start_date, self.once_start_time, "start")?;
                let end = once_datetime(self.once_end_date, self.once_end_time, "end")?;
                Ok(Recurrence::Once { start, end })
            }
        }
    }

    /// Validates and builds a SaveQueue action. On validation failure sets
    /// `error` and returns Continue; on success returns Emit.
    fn try_save(&mut self) -> ModalOutcome {
        if self.name.trim().is_empty() {
            self.error = Some("name can't be empty".to_string());
            return ModalOutcome::Continue;
        }

        let recurrence = match self.build_recurrence() {
            Ok(r) => r,
            Err(e) => {
                self.error = Some(e);
                return ModalOutcome::Continue;
            }
        };

        match self.mode {
            QueueModalMode::Create => ModalOutcome::Emit(Action::SaveQueue {
                mode: QueueModalMode::Create,
                create: Some(CreateQueueRequest {
                    name: self.name.clone(),
                    position: 0,
                    max_concurrent_downloads: self.max_concurrent_downloads,
                    max_retries: self.max_retries,
                    retry_wait_seconds: self.retry_wait_seconds,
                    default_finetune: self.finetune.clone(),
                    scheduler_enabled: self.scheduler_enabled,
                    recurrence,
                    run_missed_on_startup: self.run_missed_on_startup,
                }),
                update: None,
                ordered_ids: Vec::new(),
            }),
            QueueModalMode::Edit { queue_id } => {
                let ordered_ids: Vec<i64> = self.items.iter().map(|d| d.id).collect();
                ModalOutcome::Emit(Action::SaveQueue {
                    mode: QueueModalMode::Edit { queue_id },
                    create: None,
                    update: Some((
                        queue_id,
                        UpdateQueueRequest {
                            name: self.name.clone(),
                            position: 0,
                            max_concurrent_downloads: self.max_concurrent_downloads,
                            max_retries: self.max_retries,
                            retry_wait_seconds: self.retry_wait_seconds,
                            default_finetune: self.finetune.clone(),
                            scheduler_enabled: self.scheduler_enabled,
                            recurrence,
                            run_missed_on_startup: self.run_missed_on_startup,
                        },
                    )),
                    ordered_ids,
                })
            }
        }
    }
}

impl Component for QueueModal {
    fn handle_key(&mut self, key: KeyEvent, _ctx: &Ctx<'_>) -> ModalOutcome {
        if self.editing_text {
            match key.code {
                KeyCode::Enter => self.confirm_text_edit(),
                KeyCode::Esc => self.cancel_text_edit(),
                KeyCode::Backspace => self.text_backspace(),
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.text_input(c)
                }
                _ => {}
            }
            return ModalOutcome::Continue;
        }

        let on_items_tab = self.tab == QueueModalTab::DownloadItems;

        match key.code {
            KeyCode::Esc | KeyCode::Char('c') => ModalOutcome::Close,
            KeyCode::Tab => {
                self.next_tab();
                ModalOutcome::Continue
            }
            KeyCode::BackTab => {
                self.prev_tab();
                ModalOutcome::Continue
            }
            KeyCode::Enter => {
                self.start_text_edit();
                ModalOutcome::Continue
            }
            KeyCode::Char('s') => self.try_save(),
            KeyCode::Char('J') if on_items_tab => {
                self.move_item_down();
                ModalOutcome::Continue
            }
            KeyCode::Char('K') if on_items_tab => {
                self.move_item_up();
                ModalOutcome::Continue
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_down();
                ModalOutcome::Continue
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_up();
                ModalOutcome::Continue
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.adjust(false);
                ModalOutcome::Continue
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.adjust(true);
                ModalOutcome::Continue
            }
            KeyCode::Char(' ') => {
                self.toggle_day();
                ModalOutcome::Continue
            }
            _ => ModalOutcome::Continue,
        }
    }

    fn handle_paste(&mut self, text: &str) {
        if self.editing_text {
            for character in text.chars().filter(|character| !character.is_control()) {
                self.text_input(character);
            }
        }
    }

    fn render(&mut self, f: &mut Frame, _area: Rect, ctx: &Ctx<'_>) {
        view::draw_queue_modal(f, self, ctx);
    }
}

impl App {
    pub fn open_create_queue_modal(&mut self) {
        if self.has_open_modal() {
            return;
        }

        let (once_start_date, once_start_time, once_end_date, once_end_time) =
            default_once_window();
        self.modal = Some(Modal::Queue(QueueModal {
            mode: QueueModalMode::Create,
            tab: QueueModalTab::Common,
            name: String::new(),
            max_concurrent_downloads: 1,
            max_retries: 3,
            retry_wait_seconds: DEFAULT_RETRY_WAIT_SECONDS,
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
        }));
    }

    pub fn open_edit_queue_modal(&mut self) -> Vec<Effect> {
        if self.has_open_modal() {
            return vec![];
        }
        if self.selected_queue == 0 {
            return vec![];
        }
        let Some(queue) = self.queues.get(self.selected_queue - 1).cloned() else {
            return vec![];
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

        self.modal = Some(Modal::Queue(QueueModal {
            mode: QueueModalMode::Edit { queue_id: queue.id },
            tab: QueueModalTab::Common,
            name: queue.name,
            max_concurrent_downloads: queue.settings.max_concurrent_downloads,
            max_retries: queue.settings.max_retries,
            retry_wait_seconds: queue.settings.retry_wait_seconds,
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
        }));

        vec![Effect::Api(ApiRequest::ListQueueDownloads {
            queue_id: queue.id,
        })]
    }

    /// Applies the background fetch triggered by `open_edit_queue_modal`.
    /// Only takes effect if the queue modal is still open — if the user
    /// cancelled before this arrived, there's nothing to populate.
    pub fn apply_queue_downloads_loaded(
        &mut self,
        queue_id: i64,
        result: anyhow::Result<Vec<DownloadLiveStatus>>,
    ) {
        let Some(Modal::Queue(modal)) = &mut self.modal else {
            return;
        };
        let matches = matches!(
            modal.mode,
            QueueModalMode::Edit { queue_id: open_id } if open_id == queue_id
        );
        if matches && let Ok(list) = result {
            modal.set_items(list.into_iter().map(|d| d.download).collect());
        }
    }

    pub fn apply_queue_saved(&mut self, result: anyhow::Result<()>) -> Vec<Effect> {
        if let Err(error) = result {
            self.apply_toast(error.to_string(), ToastLevel::Error);
        }
        self.refresh()
    }

    #[allow(dead_code)]
    pub fn cancel_queue_modal(&mut self) {
        if matches!(self.modal, Some(Modal::Queue(_))) {
            self.modal = None;
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
