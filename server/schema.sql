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
    default_finetune            TEXT NOT NULL DEFAULT '{}', -- JSON: FineTune

    -- Scheduler
    scheduler_enabled           INTEGER NOT NULL DEFAULT 0, -- 0/1 boolean
    scheduler_recurrence        TEXT,                       -- JSON: Recurrence, NULL if scheduler disabled
    scheduler_run_missed        INTEGER NOT NULL DEFAULT 0, -- 0/1 boolean

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

    size                INTEGER,                   -- bytes; NULL until aria2 reports it

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

