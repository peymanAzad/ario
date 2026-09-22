use chrono::{DateTime, Utc};
use common::{
    download::{Download, DownloadFilter},
    enums::{DownloadStatus, FileCategory, QueueStatus, Recurrence, SortField, SourceType},
    finetune::FineTune,
    queue::{Queue, QueueSettings},
    scheduler::Scheduler,
};
use rusqlite::{Connection, OptionalExtension, Result as SqlResult, Row, params};
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DownloadArtifactKind {
    Payload,
    Control,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadArtifact {
    pub path: String,
    pub kind: DownloadArtifactKind,
}

const SCHEMA: &str = include_str!("../schema.sql");

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn open(path: &str) -> SqlResult<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA busy_timeout = 5000;",
        )?;
        conn.execute_batch(SCHEMA)?;
        run_migrations(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn insert_queue(&self, q: &Queue) -> SqlResult<i64> {
        let conn = self.conn.lock().unwrap();
        let finetune_json = serde_json::to_string(&q.settings.default_finetune).unwrap();
        let recurrence_json = serde_json::to_string(&q.scheduler.recurrence).unwrap();

        conn.execute(
            "INSERT INTO queues (name, position, max_concurrent_downloads, max_retries, retry_wait_seconds,
                                  default_finetune, scheduler_enabled, scheduler_recurrence,
                                  scheduler_run_missed, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                q.name,
                q.position,
                q.settings.max_concurrent_downloads,
                q.settings.max_retries,
                q.settings.retry_wait_seconds,
                finetune_json,
                q.scheduler.enabled as i64,
                recurrence_json,
                q.scheduler.run_missed_on_startup as i64,
                q.created_at,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_queue(&self, id: i64) -> SqlResult<Option<Queue>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT * FROM queues WHERE id = ?1",
            params![id],
            row_to_queue,
        )
        .optional()
    }

    pub fn list_queues(&self) -> SqlResult<Vec<Queue>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT * FROM queues ORDER BY position ASC")?;
        let rows = stmt.query_map([], row_to_queue)?;
        rows.collect()
    }

    pub fn update_queue(&self, q: &Queue) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        let finetune_json = serde_json::to_string(&q.settings.default_finetune).unwrap();
        let recurrence_json = serde_json::to_string(&q.scheduler.recurrence).unwrap();

        conn.execute(
            "UPDATE queues SET name = ?1, position = ?2, max_concurrent_downloads = ?3,
                                max_retries = ?4, retry_wait_seconds = ?5, default_finetune = ?6,
                                scheduler_enabled = ?7, scheduler_recurrence = ?8,
                                scheduler_run_missed = ?9
             WHERE id = ?10",
            params![
                q.name,
                q.position,
                q.settings.max_concurrent_downloads,
                q.settings.max_retries,
                q.settings.retry_wait_seconds,
                finetune_json,
                q.scheduler.enabled as i64,
                recurrence_json,
                q.scheduler.run_missed_on_startup as i64,
                q.id,
            ],
        )?;
        Ok(())
    }

    pub fn delete_queue(&self, id: i64) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM queues WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn delete_queue_if_empty(&self, id: i64) -> SqlResult<bool> {
        let conn = self.conn.lock().unwrap();
        let deleted = conn.execute(
            "DELETE FROM queues
             WHERE id = ?1
               AND NOT EXISTS (SELECT 1 FROM downloads WHERE queue_id = ?1)",
            params![id],
        )?;
        Ok(deleted == 1)
    }

    pub fn insert_download(&self, d: &Download) -> SqlResult<i64> {
        let conn = self.conn.lock().unwrap();
        let finetune_json = serde_json::to_string(&d.finetune).unwrap();
        let (status_str, status_err) = status_to_str(&d.status);

        conn.execute(
            "INSERT INTO downloads (aria2_gid, url, filename, destination_path, source_type,
                                     category, status, status_error, paused_by_scheduler, manually_started,
                                     size, completed_length, queue_id, position_in_queue, finetune, created_at,
                                     started_at, completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            params![
                d.aria2_gid,
                d.url,
                d.filename,
                d.destination_path,
                source_to_str(&d.source_type),
                category_to_str(&d.category),
                status_str,
                status_err,
                d.paused_by_scheduler as i64,
                d.manually_started as i64,
                d.size.map(|v| v as i64),
                d.completed_length.map(|v| v as i64),
                d.queue_id,
                d.position_in_queue,
                finetune_json,
                d.created_at,
                d.started_at,
                d.completed_at,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_download(&self, id: i64) -> SqlResult<Option<Download>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT * FROM downloads WHERE id = ?1",
            params![id],
            row_to_download,
        )
        .optional()
    }

    pub fn get_download_by_gid(&self, gid: &str) -> SqlResult<Option<Download>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT * FROM downloads WHERE aria2_gid = ?1",
            params![gid],
            row_to_download,
        )
        .optional()
    }

    pub fn list_downloads(&self, filter: &DownloadFilter) -> SqlResult<Vec<Download>> {
        let conn = self.conn.lock().unwrap();

        let mut sql = String::from("SELECT * FROM downloads WHERE 1=1");
        let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(queue_id) = filter.queue_id {
            sql.push_str(" AND queue_id = ?");
            param_values.push(Box::new(queue_id));
        }
        if let Some(status) = &filter.status {
            let (s, _) = status_to_str(status);
            sql.push_str(" AND status = ?");
            param_values.push(Box::new(s));
        }
        if let Some(category) = &filter.category {
            sql.push_str(" AND category = ?");
            param_values.push(Box::new(category_to_str(category)));
        }

        let sort_col = match filter.sort_by {
            Some(SortField::Size) => "size",
            Some(SortField::Name) => "filename",
            Some(SortField::QueuePosition) => "position_in_queue",
            Some(SortField::CreatedAt) | None => "created_at",
        };
        sql.push_str(&format!(
            " ORDER BY {} {}",
            sort_col,
            if filter.sort_desc { "DESC" } else { "ASC" }
        ));

        let mut stmt = conn.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::ToSql> =
            param_values.iter().map(|b| b.as_ref()).collect();
        let rows = stmt.query_map(param_refs.as_slice(), row_to_download)?;
        rows.collect()
    }

    pub fn update_download_status(&self, id: i64, status: &DownloadStatus) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        let (s, err) = status_to_str(status);
        conn.execute(
            "UPDATE downloads SET status = ?1, status_error = ?2 WHERE id = ?3",
            params![s, err, id],
        )?;
        Ok(())
    }

    pub fn update_download_resolved_info(
        &self,
        id: i64,
        filename: &str,
        size: Option<u64>,
        category: &FileCategory,
    ) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE downloads SET filename = ?1, size = ?2, category = ?3 WHERE id = ?4",
            params![
                filename,
                size.map(|v| v as i64),
                category_to_str(category),
                id
            ],
        )?;
        Ok(())
    }

    pub fn update_download_completed_length(
        &self,
        id: i64,
        completed_length: u64,
    ) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE downloads SET completed_length = ?1 WHERE id = ?2",
            params![completed_length as i64, id],
        )?;
        Ok(())
    }

    pub fn update_download_gid(&self, id: i64, gid: &str) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE downloads SET aria2_gid = ?1, started_at = ?2 WHERE id = ?3",
            params![gid, Utc::now(), id],
        )?;
        Ok(())
    }

    pub fn update_download_finetune(&self, id: i64, finetune: &FineTune) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        let json = serde_json::to_string(finetune).unwrap();
        conn.execute(
            "UPDATE downloads SET finetune = ?1 WHERE id = ?2",
            params![json, id],
        )?;
        Ok(())
    }

    pub fn update_download_queue(
        &self,
        id: i64,
        queue_id: i64,
        position_in_queue: i32,
    ) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE downloads SET queue_id = ?1, position_in_queue = ?2 WHERE id = ?3",
            params![queue_id, position_in_queue, id],
        )?;
        Ok(())
    }

    pub fn reorder_queue(&self, queue_id: i64, ordered_ids: &[i64]) -> SqlResult<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        for (position, id) in ordered_ids.iter().enumerate() {
            tx.execute(
                "UPDATE downloads SET position_in_queue = ?1 WHERE id = ?2 AND queue_id = ?3",
                params![position as i32, id, queue_id],
            )?;
        }
        tx.commit()
    }

    pub fn next_position_in_queue(&self, queue_id: i64) -> SqlResult<i32> {
        let conn = self.conn.lock().unwrap();
        let max: Option<i32> = conn.query_row(
            "SELECT MAX(position_in_queue) FROM downloads WHERE queue_id = ?1",
            params![queue_id],
            |row| row.get(0),
        )?;
        Ok(max.map(|m| m + 1).unwrap_or(0))
    }

    pub fn delete_download(&self, id: i64) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM downloads WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn replace_download_artifacts(
        &self,
        download_id: i64,
        artifacts: &[DownloadArtifact],
    ) -> SqlResult<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM download_artifacts WHERE download_id = ?1",
            params![download_id],
        )?;
        for artifact in artifacts {
            let kind = match artifact.kind {
                DownloadArtifactKind::Payload => "Payload",
                DownloadArtifactKind::Control => "Control",
            };
            tx.execute(
                "INSERT OR IGNORE INTO download_artifacts (download_id, path, kind)
                 VALUES (?1, ?2, ?3)",
                params![download_id, artifact.path, kind],
            )?;
        }
        tx.commit()
    }

    pub fn list_download_artifacts(&self, download_id: i64) -> SqlResult<Vec<DownloadArtifact>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT path, kind FROM download_artifacts
             WHERE download_id = ?1 ORDER BY kind, path",
        )?;
        let rows = stmt.query_map(params![download_id], |row| {
            let kind: String = row.get(1)?;
            Ok(DownloadArtifact {
                path: row.get(0)?,
                kind: if kind == "Control" {
                    DownloadArtifactKind::Control
                } else {
                    DownloadArtifactKind::Payload
                },
            })
        })?;
        rows.collect()
    }

    pub fn delete_completed_downloads(&self, queue_id: Option<i64>) -> SqlResult<usize> {
        let conn = self.conn.lock().unwrap();
        match queue_id {
            Some(queue_id) => conn.execute(
                "DELETE FROM downloads WHERE status = 'Completed' AND queue_id = ?1",
                params![queue_id],
            ),
            None => conn.execute("DELETE FROM downloads WHERE status = 'Completed'", []),
        }
    }

    pub fn count_active_downloads_in_queue(&self, queue_id: i64) -> SqlResult<i64> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM downloads WHERE queue_id = ?1 AND status = 'Active'",
            params![queue_id],
            |row| row.get(0),
        )
    }

    pub fn lifecycle_blocker_counts(&self) -> SqlResult<(u64, u64)> {
        let conn = self.conn.lock().unwrap();
        let active: i64 = conn.query_row(
            "SELECT COUNT(*) FROM downloads WHERE status = 'Active'",
            [],
            |row| row.get(0),
        )?;
        let scheduled: i64 = conn.query_row(
            "SELECT COUNT(*) FROM queues WHERE scheduler_enabled = 1",
            [],
            |row| row.get(0),
        )?;
        Ok((active as u64, scheduled as u64))
    }

    pub fn list_startable_downloads(&self, queue_id: i64) -> SqlResult<Vec<Download>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT * FROM downloads
             WHERE queue_id = ?1
               AND (status = 'Pending' OR status = 'Paused')
             ORDER BY position_in_queue ASC",
        )?;
        let rows = stmt.query_map(params![queue_id], row_to_download)?;
        rows.collect()
    }

    /// Active downloads still controlled by their queue. An item explicitly
    /// resumed by the user has priority over queue/scheduler pause decisions.
    pub fn list_queue_controlled_active_downloads(
        &self,
        queue_id: i64,
    ) -> SqlResult<Vec<Download>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT * FROM downloads
             WHERE queue_id = ?1 AND status = 'Active' AND manually_started = 0",
        )?;
        let rows = stmt.query_map(params![queue_id], row_to_download)?;
        rows.collect()
    }

    pub fn update_queue_status(&self, queue_id: i64, new_status: QueueStatus) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE queues SET status = $1 WHERE id = $2",
            params![queue_status_to_str(&new_status), queue_id],
        )?;
        Ok(())
    }

    pub fn get_queue_scheduler_suppression(&self, queue_id: i64) -> SqlResult<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT scheduler_suppressed_occurrence FROM queues WHERE id = ?1",
            params![queue_id],
            |row| row.get(0),
        )
    }

    pub fn set_queue_scheduler_suppression(
        &self,
        queue_id: i64,
        occurrence: Option<&str>,
    ) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE queues SET scheduler_suppressed_occurrence = ?1 WHERE id = ?2",
            params![occurrence, queue_id],
        )?;
        Ok(())
    }

    pub fn set_paused_by_scheduler(&self, id: i64, value: bool) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE downloads SET paused_by_scheduler = ?1 WHERE id = ?2",
            params![value as i64, id],
        )?;
        Ok(())
    }

    pub fn set_manually_started(&self, id: i64, value: bool) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE downloads SET manually_started = ?1 WHERE id = ?2",
            params![value as i64, id],
        )?;
        Ok(())
    }

    pub fn set_completed_at_now(&self, id: i64) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE downloads SET completed_at = ?1 WHERE id = ?2",
            params![Utc::now(), id],
        )?;
        Ok(())
    }

    pub fn clear_completed_at(&self, id: i64) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE downloads SET completed_at = NULL WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }
}

fn run_migrations(conn: &Connection) -> SqlResult<()> {
    let has_queue_status: bool = conn.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM pragma_table_info('queues') WHERE name = 'status'
         )",
        [],
        |row| row.get(0),
    )?;

    if !has_queue_status {
        conn.execute(
            "ALTER TABLE queues ADD COLUMN status TEXT NOT NULL DEFAULT 'Paused'",
            [],
        )?;
    }

    let has_retry_wait_seconds: bool = conn.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM pragma_table_info('queues') WHERE name = 'retry_wait_seconds'
         )",
        [],
        |row| row.get(0),
    )?;

    if !has_retry_wait_seconds {
        conn.execute(
            "ALTER TABLE queues ADD COLUMN retry_wait_seconds INTEGER NOT NULL DEFAULT 5",
            [],
        )?;
    }

    let has_scheduler_suppression: bool = conn.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM pragma_table_info('queues')
             WHERE name = 'scheduler_suppressed_occurrence'
         )",
        [],
        |row| row.get(0),
    )?;

    if !has_scheduler_suppression {
        conn.execute(
            "ALTER TABLE queues ADD COLUMN scheduler_suppressed_occurrence TEXT",
            [],
        )?;
    }

    let has_downloads_table: bool = conn.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'downloads'
         )",
        [],
        |row| row.get(0),
    )?;
    let has_manually_started: bool = conn.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM pragma_table_info('downloads') WHERE name = 'manually_started'
         )",
        [],
        |row| row.get(0),
    )?;

    if has_downloads_table && !has_manually_started {
        conn.execute(
            "ALTER TABLE downloads ADD COLUMN manually_started INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }

    let has_completed_length: bool = conn.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM pragma_table_info('downloads') WHERE name = 'completed_length'
         )",
        [],
        |row| row.get(0),
    )?;

    if has_downloads_table && !has_completed_length {
        conn.execute(
            "ALTER TABLE downloads ADD COLUMN completed_length INTEGER",
            [],
        )?;
    }

    Ok(())
}

fn status_to_str(s: &DownloadStatus) -> (&'static str, Option<String>) {
    match s {
        DownloadStatus::Pending => ("Pending", None),
        DownloadStatus::Active => ("Active", None),
        DownloadStatus::Paused => ("Paused", None),
        DownloadStatus::Completed => ("Completed", None),
        DownloadStatus::Error(msg) => ("Error", Some(msg.clone())),
        DownloadStatus::Removed => ("Removed", None),
    }
}

fn status_from_str(s: &str, err: Option<String>) -> DownloadStatus {
    match s {
        "Active" => DownloadStatus::Active,
        "Paused" => DownloadStatus::Paused,
        "Completed" => DownloadStatus::Completed,
        "Error" => DownloadStatus::Error(err.unwrap_or_default()),
        "Removed" => DownloadStatus::Removed,
        _ => DownloadStatus::Pending,
    }
}

fn queue_status_to_str(s: &QueueStatus) -> &'static str {
    match s {
        QueueStatus::Paused => "Paused",
        QueueStatus::Active => "Active",
    }
}

fn queue_status_from_str(s: &str) -> QueueStatus {
    match s {
        "Paused" => QueueStatus::Paused,
        "Active" => QueueStatus::Active,
        _ => QueueStatus::Paused,
    }
}

fn category_to_str(c: &FileCategory) -> &'static str {
    match c {
        FileCategory::Video => "Video",
        FileCategory::Music => "Music",
        FileCategory::Document => "Document",
        FileCategory::Archive => "Archive",
        FileCategory::Program => "Program",
        FileCategory::Other => "Other",
    }
}

fn category_from_str(s: &str) -> FileCategory {
    match s {
        "Video" => FileCategory::Video,
        "Music" => FileCategory::Music,
        "Document" => FileCategory::Document,
        "Archive" => FileCategory::Archive,
        "Program" => FileCategory::Program,
        _ => FileCategory::Other,
    }
}

fn source_to_str(s: &SourceType) -> &'static str {
    match s {
        SourceType::Http => "Http",
        SourceType::Torrent => "Torrent",
        SourceType::Magnet => "Magnet",
    }
}

fn source_from_str(s: &str) -> SourceType {
    match s {
        "Torrent" => SourceType::Torrent,
        "Magnet" => SourceType::Magnet,
        _ => SourceType::Http,
    }
}

fn row_to_download(row: &Row) -> SqlResult<Download> {
    let finetune_json: String = row.get("finetune")?;
    let finetune: FineTune = serde_json::from_str(&finetune_json).unwrap_or_default();

    let status_str: String = row.get("status")?;
    let status_err: Option<String> = row.get("status_error")?;

    Ok(Download {
        id: row.get("id")?,
        aria2_gid: row.get("aria2_gid")?,
        url: row.get("url")?,
        filename: row.get("filename")?,
        destination_path: row.get("destination_path")?,
        source_type: source_from_str(&row.get::<_, String>("source_type")?),
        category: category_from_str(&row.get::<_, String>("category")?),
        status: status_from_str(&status_str, status_err),
        paused_by_scheduler: row.get::<_, i64>("paused_by_scheduler")? != 0,
        manually_started: row.get::<_, i64>("manually_started")? != 0,
        size: row.get::<_, Option<i64>>("size")?.map(|v| v as u64),
        completed_length: row
            .get::<_, Option<i64>>("completed_length")?
            .map(|v| v as u64),
        queue_id: row.get("queue_id")?,
        position_in_queue: row.get("position_in_queue")?,
        finetune,
        created_at: row.get::<_, DateTime<Utc>>("created_at")?,
        started_at: row.get::<_, Option<DateTime<Utc>>>("started_at")?,
        completed_at: row.get::<_, Option<DateTime<Utc>>>("completed_at")?,
    })
}

fn row_to_queue(row: &Row) -> SqlResult<Queue> {
    let finetune_json: String = row.get("default_finetune")?;
    let default_finetune: FineTune = serde_json::from_str(&finetune_json).unwrap_or_default();

    let recurrence_json: Option<String> = row.get("scheduler_recurrence")?;
    let fallback_recurrence = || Recurrence::Weekly {
        days: vec![],
        start_time: Default::default(),
        end_time: Default::default(),
    };
    let recurrence: Recurrence = recurrence_json
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_else(fallback_recurrence);
    let status: String = row.get("status")?;

    Ok(Queue {
        id: row.get("id")?,
        name: row.get("name")?,
        position: row.get("position")?,
        settings: QueueSettings {
            max_concurrent_downloads: row.get("max_concurrent_downloads")?,
            max_retries: row.get("max_retries")?,
            retry_wait_seconds: row.get("retry_wait_seconds")?,
            default_finetune,
        },
        scheduler: Scheduler {
            enabled: row.get::<_, i64>("scheduler_enabled")? != 0,
            recurrence,
            run_missed_on_startup: row.get::<_, i64>("scheduler_run_missed")? != 0,
        },
        status: queue_status_from_str(&status),
        created_at: row.get::<_, DateTime<Utc>>("created_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn insert_download_with_status(db: &Database, queue_id: i64, status: &str) -> i64 {
        let conn = db.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO downloads
             (url, destination_path, source_type, category, status, queue_id,
              position_in_queue, finetune, created_at)
             VALUES (?1, '/tmp', 'Http', 'Other', ?2, ?3, 0, '{}',
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![
                format!("https://example.test/{queue_id}/{status}"),
                status,
                queue_id
            ],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn opening_a_legacy_database_adds_retry_wait_before_queue_reads() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ario-retry-wait-migration-{}-{nonce}.sqlite",
            std::process::id()
        ));
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE queues (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     name TEXT NOT NULL,
                     position INTEGER NOT NULL DEFAULT 0,
                     max_concurrent_downloads INTEGER NOT NULL DEFAULT 1,
                     max_retries INTEGER NOT NULL DEFAULT 3,
                     default_finetune TEXT NOT NULL DEFAULT '{}',
                     scheduler_enabled INTEGER NOT NULL DEFAULT 0,
                     scheduler_recurrence TEXT,
                     scheduler_run_missed INTEGER NOT NULL DEFAULT 0,
                     scheduler_suppressed_occurrence TEXT,
                     scheduler_active_occurrence TEXT,
                     status TEXT NOT NULL DEFAULT 'Paused',
                     created_at TEXT NOT NULL
                 );
                 INSERT INTO queues (id, name, created_at)
                 VALUES (1, 'Legacy Queue', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));",
            )
            .unwrap();
        }

        let db = Database::open(path.to_str().unwrap()).unwrap();
        let queue = db.get_queue(1).unwrap().unwrap();
        assert_eq!(queue.settings.retry_wait_seconds, 5);
        drop(db);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn queue_position_sort_reflects_reordered_downloads() {
        let db = Database::open(":memory:").unwrap();
        let first = insert_download_with_status(&db, 1, "Pending");
        let second = insert_download_with_status(&db, 1, "Paused");

        db.reorder_queue(1, &[second, first]).unwrap();

        let downloads = db
            .list_downloads(&DownloadFilter {
                queue_id: Some(1),
                sort_by: Some(SortField::QueuePosition),
                ..DownloadFilter::default()
            })
            .unwrap();
        let ids: Vec<i64> = downloads.into_iter().map(|download| download.id).collect();
        assert_eq!(ids, vec![second, first]);
    }

    #[test]
    fn queue_migrations_are_idempotent_and_backfill_paused_status() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE queues (
                 id INTEGER PRIMARY KEY,
                 name TEXT NOT NULL
             );
             INSERT INTO queues (id, name) VALUES (1, 'Existing Queue');",
        )
        .unwrap();

        run_migrations(&conn).unwrap();
        run_migrations(&conn).unwrap();

        let status: String = conn
            .query_row("SELECT status FROM queues WHERE id = 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(status, "Paused");

        let retry_wait_seconds: i64 = conn
            .query_row(
                "SELECT retry_wait_seconds FROM queues WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(retry_wait_seconds, 5);

        let suppression: Option<String> = conn
            .query_row(
                "SELECT scheduler_suppressed_occurrence FROM queues WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(suppression, None);
    }

    #[test]
    fn lifecycle_counts_only_active_downloads_and_enabled_schedules() {
        let db = Database::open(":memory:").unwrap();
        assert_eq!(db.lifecycle_blocker_counts().unwrap(), (0, 0));

        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO downloads
                 (url, destination_path, source_type, category, status, queue_id,
                  position_in_queue, finetune, created_at)
                 VALUES ('https://example.test/file', '/tmp', 'Http', 'Other',
                         'Pending', 1, 0, '{}', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
                [],
            )
            .unwrap();
        }
        assert_eq!(db.lifecycle_blocker_counts().unwrap(), (0, 0));

        db.update_download_status(1, &DownloadStatus::Active)
            .unwrap();
        assert_eq!(db.lifecycle_blocker_counts().unwrap(), (1, 0));

        db.update_download_status(1, &DownloadStatus::Completed)
            .unwrap();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute("UPDATE queues SET scheduler_enabled = 1 WHERE id = 1", [])
                .unwrap();
        }
        assert_eq!(db.lifecycle_blocker_counts().unwrap(), (0, 1));
    }

    #[test]
    fn deletes_completed_downloads_across_all_queues_only() {
        let db = Database::open(":memory:").unwrap();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO queues (id, name, created_at)
                 VALUES (2, 'Second Queue', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
                [],
            )
            .unwrap();
        }

        let completed_in_main = insert_download_with_status(&db, 1, "Completed");
        let completed_in_second = insert_download_with_status(&db, 2, "Completed");
        let pending = insert_download_with_status(&db, 1, "Pending");
        let active = insert_download_with_status(&db, 1, "Active");
        let paused = insert_download_with_status(&db, 2, "Paused");
        let error = insert_download_with_status(&db, 2, "Error");

        assert_eq!(db.delete_completed_downloads(None).unwrap(), 2);
        assert!(db.get_download(completed_in_main).unwrap().is_none());
        assert!(db.get_download(completed_in_second).unwrap().is_none());
        for retained in [pending, active, paused, error] {
            assert!(db.get_download(retained).unwrap().is_some());
        }
        assert_eq!(db.delete_completed_downloads(None).unwrap(), 0);
    }

    #[test]
    fn deletes_completed_downloads_from_selected_queue_only() {
        let db = Database::open(":memory:").unwrap();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO queues (id, name, created_at)
                 VALUES (2, 'Second Queue', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
                [],
            )
            .unwrap();
        }

        let completed_in_main = insert_download_with_status(&db, 1, "Completed");
        let completed_in_second = insert_download_with_status(&db, 2, "Completed");
        let pending_in_main = insert_download_with_status(&db, 1, "Pending");

        assert_eq!(db.delete_completed_downloads(Some(1)).unwrap(), 1);
        assert!(db.get_download(completed_in_main).unwrap().is_none());
        assert!(db.get_download(completed_in_second).unwrap().is_some());
        assert!(db.get_download(pending_in_main).unwrap().is_some());
    }

    #[test]
    fn clear_completed_at_nulls_the_timestamp() {
        let db = Database::open(":memory:").unwrap();
        let id = insert_download_with_status(&db, 1, "Completed");
        db.set_completed_at_now(id).unwrap();
        assert!(db.get_download(id).unwrap().unwrap().completed_at.is_some());

        db.clear_completed_at(id).unwrap();
        assert!(db.get_download(id).unwrap().unwrap().completed_at.is_none());
    }

    #[test]
    fn artifact_snapshots_deduplicate_replace_and_cascade() {
        let db = Database::open(":memory:").unwrap();
        let id = insert_download_with_status(&db, 1, "Active");
        let payload = DownloadArtifact {
            path: "/tmp/file.bin".into(),
            kind: DownloadArtifactKind::Payload,
        };
        db.replace_download_artifacts(id, &[payload.clone(), payload.clone()])
            .unwrap();
        assert_eq!(db.list_download_artifacts(id).unwrap(), vec![payload]);

        let control = DownloadArtifact {
            path: "/tmp/file.bin.aria2".into(),
            kind: DownloadArtifactKind::Control,
        };
        db.replace_download_artifacts(id, std::slice::from_ref(&control))
            .unwrap();
        assert_eq!(db.list_download_artifacts(id).unwrap(), vec![control]);

        db.delete_download(id).unwrap();
        assert!(db.list_download_artifacts(id).unwrap().is_empty());
    }

    #[test]
    fn completed_length_round_trips() {
        let db = Database::open(":memory:").unwrap();
        let id = insert_download_with_status(&db, 1, "Active");
        assert_eq!(db.get_download(id).unwrap().unwrap().completed_length, None);

        db.update_download_completed_length(id, 42).unwrap();
        assert_eq!(
            db.get_download(id).unwrap().unwrap().completed_length,
            Some(42)
        );

        db.update_download_completed_length(id, 42).unwrap();
        assert_eq!(
            db.get_download(id).unwrap().unwrap().completed_length,
            Some(42)
        );

        db.update_download_completed_length(id, 80).unwrap();
        assert_eq!(
            db.get_download(id).unwrap().unwrap().completed_length,
            Some(80)
        );
    }

    #[test]
    fn completed_length_migration_is_idempotent_and_nullable() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE queues (
                 id INTEGER PRIMARY KEY,
                 name TEXT NOT NULL
             );
             CREATE TABLE downloads (
                 id INTEGER PRIMARY KEY,
                 url TEXT NOT NULL
             );
             INSERT INTO downloads (id, url) VALUES (1, 'https://example.test/file');",
        )
        .unwrap();

        run_migrations(&conn).unwrap();
        run_migrations(&conn).unwrap();

        let completed_length: Option<i64> = conn
            .query_row(
                "SELECT completed_length FROM downloads WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(completed_length, None);
    }
}
