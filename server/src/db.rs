use chrono::{DateTime, Utc};
use common::{
    download::{Download, DownloadFilter},
    enums::{DownloadStatus, FileCategory, Recurrence, SortField, SourceType},
    finetune::FineTune,
    queue::{Queue, QueueSettings},
    scheduler::Scheduler,
};
use rusqlite::{Connection, OptionalExtension, Result as SqlResult, Row, params};
use std::sync::Mutex;

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
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn insert_queue(&self, q: &Queue) -> SqlResult<i64> {
        let conn = self.conn.lock().unwrap();
        let finetune_json = serde_json::to_string(&q.settings.default_finetune).unwrap();
        let recurrence_json = serde_json::to_string(&q.scheduler.recurrence).unwrap();

        conn.execute(
            "INSERT INTO queues (name, position, max_concurrent_downloads, max_retries,
                                  default_finetune, scheduler_enabled, scheduler_recurrence,
                                  scheduler_run_missed, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                q.name,
                q.position,
                q.settings.max_concurrent_downloads,
                q.settings.max_retries,
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
                                max_retries = ?4, default_finetune = ?5, scheduler_enabled = ?6,
                                scheduler_recurrence = ?7, scheduler_run_missed = ?8
             WHERE id = ?9",
            params![
                q.name,
                q.position,
                q.settings.max_concurrent_downloads,
                q.settings.max_retries,
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

    pub fn insert_download(&self, d: &Download) -> SqlResult<i64> {
        let conn = self.conn.lock().unwrap();
        let finetune_json = serde_json::to_string(&d.finetune).unwrap();
        let (status_str, status_err) = status_to_str(&d.status);

        conn.execute(
            "INSERT INTO downloads (aria2_gid, url, filename, destination_path, source_type,
                                     category, status, status_error, paused_by_scheduler, size, queue_id,
                                     position_in_queue, finetune, created_at, started_at, completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
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
                d.size.map(|v| v as i64),
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

    pub fn count_active_downloads_in_queue(&self, queue_id: i64) -> SqlResult<i64> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM downloads WHERE queue_id = ?1 AND status = 'Active'",
            params![queue_id],
            |row| row.get(0),
        )
    }

    pub fn list_startable_downloads(&self, queue_id: i64) -> SqlResult<Vec<Download>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT * FROM downloads
             WHERE queue_id = ?1
               AND (status = 'Pending' OR (status = 'Paused' AND paused_by_scheduler = 1))
             ORDER BY position_in_queue ASC",
        )?;
        let rows = stmt.query_map(params![queue_id], row_to_download)?;
        rows.collect()
    }

    pub fn list_active_downloads_in_queue(&self, queue_id: i64) -> SqlResult<Vec<Download>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT * FROM downloads WHERE queue_id = ?1 AND status = 'Active'")?;
        let rows = stmt.query_map(params![queue_id], row_to_download)?;
        rows.collect()
    }

    pub fn set_paused_by_scheduler(&self, id: i64, value: bool) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE downloads SET paused_by_scheduler = ?1 WHERE id = ?2",
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
        size: row.get::<_, Option<i64>>("size")?.map(|v| v as u64),
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

    Ok(Queue {
        id: row.get("id")?,
        name: row.get("name")?,
        position: row.get("position")?,
        settings: QueueSettings {
            max_concurrent_downloads: row.get("max_concurrent_downloads")?,
            max_retries: row.get("max_retries")?,
            default_finetune,
        },
        scheduler: Scheduler {
            enabled: row.get::<_, i64>("scheduler_enabled")? != 0,
            recurrence,
            run_missed_on_startup: row.get::<_, i64>("scheduler_run_missed")? != 0,
        },
        created_at: row.get::<_, DateTime<Utc>>("created_at")?,
    })
}
