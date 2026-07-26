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
}
