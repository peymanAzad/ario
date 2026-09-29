use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

pub const MAX_SPEED_HISTORY_SAMPLES: usize = 150;
pub(crate) const SPEED_SAMPLE_INTERVAL: Duration = Duration::from_secs(2);

pub(crate) fn speed_scale_target(history: impl IntoIterator<Item = u64>) -> u64 {
    let peak = history.into_iter().max().unwrap_or(0);
    let headroom = peak / 5 + u64::from(peak % 5 != 0);
    peak.saturating_add(headroom).max(1)
}

#[derive(Debug)]
pub struct SpeedTracker {
    pub history: VecDeque<u64>,
    pub(crate) smoothed: Option<u64>,
    chart_max: u64,
    pub(crate) last_sample_at: Option<Instant>,
}

impl SpeedTracker {
    pub fn new() -> Self {
        Self {
            history: VecDeque::new(),
            smoothed: None,
            chart_max: 1,
            last_sample_at: None,
        }
    }

    pub fn push_sample(&mut self, speed: u64) {
        if self.history.len() == MAX_SPEED_HISTORY_SAMPLES {
            self.history.pop_front();
        }
        self.history.push_back(speed);

        let target = speed_scale_target(self.history.iter().copied());
        self.chart_max = if target >= self.chart_max {
            target
        } else {
            let decay = self.chart_max / 10 + u64::from(self.chart_max % 10 != 0);
            self.chart_max.saturating_sub(decay).max(target)
        };
    }

    pub fn displayed(&self, fallback: u64) -> u64 {
        self.smoothed.unwrap_or(fallback)
    }

    pub fn chart_max(&self) -> u64 {
        self.chart_max
            .max(speed_scale_target(self.history.iter().copied()))
    }

    pub fn clear(&mut self) {
        self.history.clear();
        self.last_sample_at = None;
        self.chart_max = 1;
    }

    pub fn should_record(&self) -> bool {
        self.last_sample_at
            .is_none_or(|at| at.elapsed() >= SPEED_SAMPLE_INTERVAL)
    }

    pub fn record_sample_now(&mut self, speed: u64) {
        self.push_sample(speed);
        self.last_sample_at = Some(Instant::now());
    }

    pub fn update_smoothed(&mut self, download_speed: u64, active_downloads: u64) {
        self.smoothed = Some(if active_downloads == 0 {
            0
        } else {
            self.smoothed.map_or(download_speed, |previous| {
                ((u128::from(previous) * 3 + u128::from(download_speed)) / 4) as u64
            })
        });
    }

    pub fn clear_smoothed(&mut self) {
        self.smoothed = None;
    }

    pub fn set_smoothed(&mut self, value: Option<u64>) {
        self.smoothed = value;
    }
}

impl Default for SpeedTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "speed_tests.rs"]
mod tests;
