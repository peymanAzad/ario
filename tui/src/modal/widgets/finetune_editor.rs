use common::enums::{AllocStrategy, StreamPieceSelector};
use common::finetune::{Aria2GlobalOptions, FineTune};

#[derive(Debug, Clone)]
pub struct FineTuneEditor {
    pub finetune: FineTune,
    /// 0..=5 for the six finetune fields.
    pub cursor: usize,
}

impl FineTuneEditor {
    pub fn new(finetune: FineTune) -> Self {
        Self {
            finetune,
            cursor: 0,
        }
    }

    pub fn move_down(&mut self) {
        self.cursor = (self.cursor + 1).min(5);
    }

    pub fn move_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn adjust(&mut self, forward: bool) {
        adjust_finetune_field(&mut self.finetune, self.cursor, forward);
    }

    /// Label + display value for each finetune field.
    ///
    /// When `globals` is `Some`, unset fields show the aria2 global fallback;
    /// when `None`, unset fields show `(queue default)`.
    pub fn rows(&self, globals: Option<&Aria2GlobalOptions>) -> Vec<(String, String)> {
        let queue_default = || "(queue default)".to_string();
        let num = |value: Option<u32>, global: Option<u32>| match (value, globals) {
            (Some(v), _) => v.to_string(),
            (None, Some(_)) => aria2_global_label(global.map(|v| v.to_string())),
            (None, None) => queue_default(),
        };
        let alloc = match &self.finetune.alloc_strategy {
            Some(strategy) => alloc_strategy_label(strategy),
            None => match globals {
                Some(options) => aria2_global_label(
                    options
                        .alloc_strategy
                        .as_ref()
                        .map(alloc_strategy_label),
                ),
                None => queue_default(),
            },
        };
        let selector = match &self.finetune.stream_piece_selector {
            Some(strategy) => stream_piece_selector_label(strategy),
            None => match globals {
                Some(options) => aria2_global_label(
                    options
                        .stream_piece_selector
                        .as_ref()
                        .map(stream_piece_selector_label),
                ),
                None => queue_default(),
            },
        };

        vec![
            (
                "Connections per download".into(),
                num(
                    self.finetune.connections_per_download,
                    globals.and_then(|g| g.connections_per_download),
                ),
            ),
            (
                "Max connections per server".into(),
                num(
                    self.finetune.max_connections_per_server,
                    globals.and_then(|g| g.max_connections_per_server),
                ),
            ),
            ("File allocation".into(), alloc),
            ("Stream piece selector".into(), selector),
            (
                "Max retries".into(),
                num(self.finetune.max_retries, None),
            ),
            (
                "Retry wait (seconds)".into(),
                num(self.finetune.retry_wait_seconds, None),
            ),
        ]
    }
}

fn adjust_finetune_field(f: &mut FineTune, cursor: usize, forward: bool) {
    match cursor {
        0 => f.connections_per_download = adjust_opt_u32(f.connections_per_download, forward, 16),
        1 => {
            f.max_connections_per_server = adjust_opt_u32(f.max_connections_per_server, forward, 16)
        }
        2 => f.alloc_strategy = cycle(&ALLOC_STRATEGY_ORDER, &f.alloc_strategy, forward),
        3 => {
            f.stream_piece_selector =
                cycle(&STREAM_SELECTOR_ORDER, &f.stream_piece_selector, forward)
        }
        4 => f.max_retries = adjust_opt_u32_including_zero(f.max_retries, forward, 20),
        5 => {
            f.retry_wait_seconds = adjust_opt_u32_including_zero(f.retry_wait_seconds, forward, 300)
        }
        _ => {}
    }
}

fn adjust_opt_u32_including_zero(current: Option<u32>, forward: bool, max: u32) -> Option<u32> {
    match (current, forward) {
        (None, true) => Some(0),
        (None, false) => None,
        (Some(0), false) => None,
        (Some(value), true) => Some(value.saturating_add(1).min(max)),
        (Some(value), false) => Some(value - 1),
    }
}

fn adjust_opt_u32(current: Option<u32>, forward: bool, max: u32) -> Option<u32> {
    let val = current.unwrap_or(0);
    let new_val = if forward {
        (val + 1).min(max)
    } else {
        val.saturating_sub(1)
    };
    if new_val == 0 { None } else { Some(new_val) }
}

const ALLOC_STRATEGY_ORDER: [Option<AllocStrategy>; 5] = [
    None,
    Some(AllocStrategy::None),
    Some(AllocStrategy::Prealloc),
    Some(AllocStrategy::Falloc),
    Some(AllocStrategy::Trunc),
];

const STREAM_SELECTOR_ORDER: [Option<StreamPieceSelector>; 5] = [
    None,
    Some(StreamPieceSelector::Default),
    Some(StreamPieceSelector::InOrder),
    Some(StreamPieceSelector::Random),
    Some(StreamPieceSelector::Geom),
];

fn cycle<T: PartialEq + Clone>(
    order: &[Option<T>],
    current: &Option<T>,
    forward: bool,
) -> Option<T> {
    let idx = order.iter().position(|v| v == current).unwrap_or(0);
    let len = order.len();
    let new_idx = if forward {
        (idx + 1) % len
    } else {
        (idx + len - 1) % len
    };
    order[new_idx].clone()
}

pub(crate) fn aria2_global_label(value: Option<String>) -> String {
    value.map_or_else(
        || "(aria2 global)".to_string(),
        |value| format!("{value} (aria2 global)"),
    )
}

pub(crate) fn alloc_strategy_label(value: &AllocStrategy) -> String {
    match value {
        AllocStrategy::None => "none",
        AllocStrategy::Prealloc => "prealloc",
        AllocStrategy::Falloc => "falloc",
        AllocStrategy::Trunc => "trunc",
    }
    .to_string()
}

pub(crate) fn stream_piece_selector_label(value: &StreamPieceSelector) -> String {
    match value {
        StreamPieceSelector::Default => "default",
        StreamPieceSelector::InOrder => "inorder",
        StreamPieceSelector::Random => "random",
        StreamPieceSelector::Geom => "geom",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        adjust_opt_u32_including_zero, alloc_strategy_label, aria2_global_label,
        stream_piece_selector_label,
    };
    use common::enums::{AllocStrategy, StreamPieceSelector};

    #[test]
    fn optional_retry_values_include_default_zero_and_bounded_values() {
        assert_eq!(adjust_opt_u32_including_zero(None, true, 20), Some(0));
        assert_eq!(adjust_opt_u32_including_zero(Some(0), false, 20), None);
        assert_eq!(adjust_opt_u32_including_zero(Some(20), true, 20), Some(20));
        assert_eq!(adjust_opt_u32_including_zero(Some(1), false, 20), Some(0));
    }

    #[test]
    fn global_labels_include_effective_value_or_text_fallback() {
        assert_eq!(aria2_global_label(Some("5".into())), "5 (aria2 global)");
        assert_eq!(aria2_global_label(None), "(aria2 global)");
        assert_eq!(alloc_strategy_label(&AllocStrategy::Prealloc), "prealloc");
        assert_eq!(
            stream_piece_selector_label(&StreamPieceSelector::InOrder),
            "inorder"
        );
    }
}
