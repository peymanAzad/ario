//! Durable run boundaries and at-most-once command attempts.
use super::*;
use crate::queue_events::EventAttempt;
use common::queue::{QueueEventResult, QueueEventResults, QueueEventStatus};

pub(super) fn migrate(conn: &Connection) -> SqlResult<()> {
    for (name, declaration) in [
        ("on_success_command", "TEXT"),
        ("on_stop_command", "TEXT"),
        ("event_run_id", "INTEGER NOT NULL DEFAULT 0"),
        ("event_run_active", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('queues') WHERE name=?1)",
            [name],
            |row| row.get(0),
        )?;
        if !exists {
            conn.execute(
                &format!("ALTER TABLE queues ADD COLUMN {name} {declaration}"),
                [],
            )?;
        }
    }
    conn.execute_batch("CREATE TABLE IF NOT EXISTS queue_event_attempts (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        queue_id INTEGER NOT NULL REFERENCES queues(id) ON DELETE CASCADE,
        run_id INTEGER NOT NULL, event TEXT NOT NULL, command TEXT NOT NULL,
        queue_name TEXT NOT NULL, stop_reason TEXT NOT NULL, result TEXT NOT NULL,
        UNIQUE(queue_id, run_id, event)
    ); CREATE INDEX IF NOT EXISTS idx_queue_event_attempts_queue ON queue_event_attempts(queue_id, id);")?;
    Ok(())
}

fn decode<T: serde::de::DeserializeOwned>(value: String) -> SqlResult<T> {
    serde_json::from_str(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
    })
}

pub(super) fn latest_results(conn: &Connection, id: i64) -> SqlResult<QueueEventResults> {
    let latest = |event| -> SqlResult<Option<QueueEventResult>> {
        conn.query_row("SELECT result FROM queue_event_attempts WHERE queue_id=?1 AND event=?2 ORDER BY id DESC LIMIT 1",
            params![id, event], |row| decode(row.get(0)?)).optional()
    };
    Ok(QueueEventResults {
        on_success: latest("success")?,
        on_stop: latest("stop")?,
    })
}

pub(super) fn interrupt_unfinished(conn: &Connection) -> SqlResult<()> {
    conn.execute("UPDATE queue_event_attempts SET result=json_set(result,
        '$.status','interrupted', '$.finished_at',?1, '$.message','Daemon stopped before execution finished; command will not be replayed')
        WHERE json_extract(result,'$.status') IN ('queued','running')", [Utc::now().to_rfc3339()])?;
    Ok(())
}

impl Database {
    /// Caller holds the queue control mutex. Repeated starts join the existing run.
    pub fn begin_event_run(&self, queue_id: i64) -> SqlResult<()> {
        self.conn.lock().unwrap().execute(
            "UPDATE queues SET event_run_id=event_run_id+1, event_run_active=1 WHERE id=?1 AND event_run_active=0", [queue_id],
        )?;
        Ok(())
    }

    pub fn cancel_event_run(&self, queue_id: i64) -> SqlResult<()> {
        self.conn.lock().unwrap().execute(
            "UPDATE queues SET event_run_active=0 WHERE id=?1",
            [queue_id],
        )?;
        Ok(())
    }

    pub fn finish_event_run(
        &self,
        queue: &Queue,
        success: bool,
        reason: &str,
        now: DateTime<Utc>,
    ) -> SqlResult<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let active: bool = tx.query_row(
            "SELECT event_run_active FROM queues WHERE id=?1",
            [queue.id],
            |r| r.get(0),
        )?;
        if !active {
            return Ok(());
        }
        let run_id: i64 = tx.query_row(
            "SELECT event_run_id FROM queues WHERE id=?1",
            [queue.id],
            |r| r.get(0),
        )?;
        tx.execute(
            "UPDATE queues SET event_run_active=0 WHERE id=?1",
            [queue.id],
        )?;
        for (event, command) in [
            (
                "success",
                success
                    .then_some(queue.settings.on_success_command.as_deref())
                    .flatten(),
            ),
            ("stop", queue.settings.on_stop_command.as_deref()),
        ] {
            let Some(command) = command.filter(|value| !value.trim().is_empty()) else {
                continue;
            };
            let mut result = QueueEventResult {
                id: 0,
                run_id,
                status: QueueEventStatus::Queued,
                queued_at: now,
                started_at: None,
                finished_at: None,
                exit_code: None,
                message: None,
            };
            tx.execute("INSERT INTO queue_event_attempts(queue_id,run_id,event,command,queue_name,stop_reason,result)
                VALUES(?1,?2,?3,?4,?5,?6,'{}')", params![queue.id,run_id,event,command,queue.name,reason])?;
            result.id = tx.last_insert_rowid();
            tx.execute(
                "UPDATE queue_event_attempts SET result=?1 WHERE id=?2",
                params![serde_json::to_string(&result).unwrap(), result.id],
            )?;
        }
        tx.commit()
    }

    /// Claims are committed before spawning a process. A restart never replays them.
    pub fn claim_next_event(&self, queue_id: i64) -> SqlResult<Option<EventAttempt>> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let attempt = tx
            .query_row(
                "SELECT id,event,command,queue_name,stop_reason,result FROM queue_event_attempts
            WHERE queue_id=?1 AND json_extract(result,'$.status')='queued' ORDER BY id LIMIT 1",
                [queue_id],
                |row| {
                    Ok(EventAttempt {
                        queue_id,
                        event: row.get(1)?,
                        command: row.get(2)?,
                        queue_name: row.get(3)?,
                        stop_reason: row.get(4)?,
                        result: decode(row.get(5)?)?,
                    })
                },
            )
            .optional()?;
        if let Some(mut attempt) = attempt {
            attempt.result.status = QueueEventStatus::Running;
            attempt.result.started_at = Some(Utc::now());
            tx.execute(
                "UPDATE queue_event_attempts SET result=?1 WHERE id=?2",
                params![
                    serde_json::to_string(&attempt.result).unwrap(),
                    attempt.result.id
                ],
            )?;
            tx.commit()?;
            Ok(Some(attempt))
        } else {
            Ok(None)
        }
    }

    pub fn complete_event(&self, result: &QueueEventResult) -> SqlResult<()> {
        self.conn.lock().unwrap().execute(
            "UPDATE queue_event_attempts SET result=?1
            WHERE id=?2 AND json_extract(result,'$.status')='running'",
            params![serde_json::to_string(result).unwrap(), result.id],
        )?;
        Ok(())
    }

    pub fn event_command_count(&self) -> SqlResult<u64> {
        self.conn.lock().unwrap().query_row("SELECT count(*) FROM queue_event_attempts WHERE json_extract(result,'$.status') IN ('queued','running')", [], |r| r.get::<_, i64>(0).map(|count| count as u64))
    }

    pub fn interrupt_events(&self) -> SqlResult<()> {
        interrupt_unfinished(&self.conn.lock().unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_migration_is_idempotent_and_preserves_legacy_queues() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE queues(id INTEGER PRIMARY KEY, name TEXT NOT NULL); INSERT INTO queues VALUES(1,'Legacy');").unwrap();
        migrate(&conn).unwrap();
        migrate(&conn).unwrap();
        let defaults: (Option<String>,Option<String>,i64,bool) = conn.query_row(
            "SELECT on_success_command,on_stop_command,event_run_id,event_run_active FROM queues WHERE id=1",[],|row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).unwrap();
        assert_eq!(defaults, (None, None, 0, false));
        assert_eq!(
            latest_results(&conn, 1).unwrap(),
            QueueEventResults::default()
        );
        conn.execute("UPDATE queues SET on_stop_command='exit 7', event_run_id=5, event_run_active=1 WHERE id=1",[]).unwrap();
        migrate(&conn).unwrap();
        let preserved: (String, i64, bool) = conn
            .query_row(
                "SELECT on_stop_command,event_run_id,event_run_active FROM queues WHERE id=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(preserved, ("exit 7".into(), 5, true));
    }
}
