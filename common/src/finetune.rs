use serde::{Deserialize, Serialize};

use crate::enums::{AllocStrategy, StreamPieceSelector};

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct FineTune {
    /// aria2 `--split` — number of connections used to fetch a single download.
    pub connections_per_download: Option<u32>,
    /// aria2 `--max-connection-per-server`
    pub max_connections_per_server: Option<u32>,
    /// aria2 `--file-allocation`
    pub alloc_strategy: Option<AllocStrategy>,
    /// aria2 `--stream-piece-selector` — only meaningful for segmented/torrent downloads.
    pub stream_piece_selector: Option<StreamPieceSelector>,
    /// Number of retries after the initial attempt. Converted to aria2
    /// `--max-tries` by adding one.
    pub max_retries: Option<u32>,
    /// aria2 `--retry-wait`, in seconds.
    pub retry_wait_seconds: Option<u32>,
}

/// Effective aria2 global values captured when the daemon starts. Individual
/// fields remain optional so an unexpected or unavailable value can degrade
/// to a text-only fallback in clients.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Aria2GlobalOptions {
    pub connections_per_download: Option<u32>,
    pub max_connections_per_server: Option<u32>,
    pub alloc_strategy: Option<AllocStrategy>,
    pub stream_piece_selector: Option<StreamPieceSelector>,
}

impl FineTune {
    /// Applies only explicitly selected per-download values, leaving all
    /// other fields at their queue-provided defaults.
    pub fn apply_override(&mut self, overrides: FineTune) {
        if overrides.connections_per_download.is_some() {
            self.connections_per_download = overrides.connections_per_download;
        }
        if overrides.max_connections_per_server.is_some() {
            self.max_connections_per_server = overrides.max_connections_per_server;
        }
        if overrides.alloc_strategy.is_some() {
            self.alloc_strategy = overrides.alloc_strategy;
        }
        if overrides.stream_piece_selector.is_some() {
            self.stream_piece_selector = overrides.stream_piece_selector;
        }
        if overrides.max_retries.is_some() {
            self.max_retries = overrides.max_retries;
        }
        if overrides.retry_wait_seconds.is_some() {
            self.retry_wait_seconds = overrides.retry_wait_seconds;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FineTune;

    #[test]
    fn old_json_deserializes_without_retry_fields() {
        let value: FineTune = serde_json::from_str(
            r#"{"connections_per_download":4,"max_connections_per_server":null,"alloc_strategy":null,"stream_piece_selector":null}"#,
        )
        .unwrap();

        assert_eq!(value.connections_per_download, Some(4));
        assert_eq!(value.max_retries, None);
        assert_eq!(value.retry_wait_seconds, None);
    }

    #[test]
    fn overrides_merge_one_field_at_a_time_and_preserve_explicit_zero() {
        let mut value = FineTune {
            connections_per_download: Some(4),
            max_connections_per_server: Some(2),
            max_retries: Some(3),
            retry_wait_seconds: Some(5),
            ..FineTune::default()
        };

        value.apply_override(FineTune {
            max_retries: Some(0),
            ..FineTune::default()
        });

        assert_eq!(value.connections_per_download, Some(4));
        assert_eq!(value.max_connections_per_server, Some(2));
        assert_eq!(value.max_retries, Some(0));
        assert_eq!(value.retry_wait_seconds, Some(5));
    }
}
