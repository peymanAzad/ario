PRAGMA foreign_keys = ON;

-- =========================================================================
-- queues
-- =========================================================================

CREATE TABLE IF NOT EXISTS queues (
    id                          INTEGER PRIMARY KEY AUTOINCREMENT,
    name                        TEXT NOT NULL,
    position                    INTEGER NOT NULL DEFAULT 0,

    -- QueueSettings
    max_concurrent_downloads    INTEGER NOT NULL DEFAULT 1,
    max_retries                 INTEGER NOT NULL DEFAULT 3,
    retry_wait_seconds          INTEGER NOT NULL DEFAULT 5,
    default_finetune            TEXT NOT NULL DEFAULT '{}', -- JSON: FineTune

    -- Scheduler
    scheduler_enabled           INTEGER NOT NULL DEFAULT 0, -- 0/1 boolean
    scheduler_recurrence        TEXT,                       -- JSON: Recurrence, NULL if scheduler disabled
    scheduler_run_missed        INTEGER NOT NULL DEFAULT 0, -- 0/1 boolean
    scheduler_suppressed_occurrence TEXT,                   -- current occurrence key skipped by a manual pause
    scheduler_active_occurrence TEXT,                       -- occurrence that most recently started this queue

    status                      TEXT NOT NULL DEFAULT 'Paused', -- 'Paused'|'Active'
    created_at                  TEXT NOT NULL               -- RFC3339 UTC
);

-- Seed the default queue every fresh install needs.
INSERT INTO queues (id, name, position, max_concurrent_downloads, max_retries,
                     default_finetune, scheduler_enabled, scheduler_recurrence,
                     scheduler_run_missed, created_at)
SELECT 1, 'Main Queue', 0, 3, 3, '{}', 0, NULL, 0, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE NOT EXISTS (SELECT 1 FROM queues WHERE id = 1);

CREATE INDEX IF NOT EXISTS idx_queues_position ON queues(position);

-- =========================================================================
-- downloads
-- =========================================================================

CREATE TABLE IF NOT EXISTS downloads (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    aria2_gid           TEXT,                      -- NULL until aria2 has registered it

    url                 TEXT NOT NULL,
    filename            TEXT,
    destination_path    TEXT NOT NULL,

    source_type         TEXT NOT NULL,             -- 'Http' | 'Torrent' | 'Magnet'
    category            TEXT NOT NULL,             -- 'Video' | 'Music' | 'Document' | 'Archive' | 'Other'

    status              TEXT NOT NULL DEFAULT 'Pending', -- 'Pending'|'Active'|'Paused'|'Completed'|'Error'|'Removed'
    status_error        TEXT,                      -- populated only when status = 'Error'
    paused_by_scheduler INTEGER NOT NULL DEFAULT 0, -- 0/1 boolean
    manually_started    INTEGER NOT NULL DEFAULT 0, -- user-started item bypasses queue pause/schedule until terminal

    size                INTEGER,                   -- bytes; NULL until aria2 reports it
    completed_length    INTEGER,                   -- last-known bytes completed; NULL until aria2 reports it

    queue_id            INTEGER NOT NULL REFERENCES queues(id) ON DELETE CASCADE,
    position_in_queue   INTEGER NOT NULL DEFAULT 0,

    finetune            TEXT NOT NULL DEFAULT '{}', -- JSON: FineTune, copied from queue at creation

    created_at          TEXT NOT NULL,              -- RFC3339 UTC
    started_at          TEXT,
    completed_at        TEXT
);

CREATE INDEX IF NOT EXISTS idx_downloads_queue_id      ON downloads(queue_id);
CREATE INDEX IF NOT EXISTS idx_downloads_status         ON downloads(status);
CREATE INDEX IF NOT EXISTS idx_downloads_category       ON downloads(category);
CREATE INDEX IF NOT EXISTS idx_downloads_created_at     ON downloads(created_at);
CREATE INDEX IF NOT EXISTS idx_downloads_queue_position ON downloads(queue_id, position_in_queue);

-- Paths reported by aria2 are retained so completed and multi-file downloads
-- can still be removed after aria2 forgets the result.
CREATE TABLE IF NOT EXISTS download_artifacts (
    download_id INTEGER NOT NULL REFERENCES downloads(id) ON DELETE CASCADE,
    path        TEXT NOT NULL,
    kind        TEXT NOT NULL, -- 'Payload' | 'Control'
    PRIMARY KEY (download_id, path, kind)
);

CREATE INDEX IF NOT EXISTS idx_download_artifacts_download_id
    ON download_artifacts(download_id);

-- Original metainfo is retained so queued torrents can be started after a
-- restart and failed/completed torrents can be retried or restarted.
CREATE TABLE IF NOT EXISTS download_torrent_data (
    download_id INTEGER PRIMARY KEY REFERENCES downloads(id) ON DELETE CASCADE,
    data        BLOB NOT NULL CHECK(length(data) > 0)
);
