// SQLite implementation of ExternalEventsRepository
//
// All DB access uses `db.run(|conn| { ... })` via DbConnection (NON-NEGOTIABLE).

use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use rusqlite::Connection;

use super::DbConnection;
use crate::domain::repositories::external_events_repository::{
    ExternalEventRecord, ExternalEventsRepository,
};
use crate::error::{AppError, AppResult};

pub struct SqliteExternalEventsRepository {
    pub(crate) db: DbConnection,
}

impl SqliteExternalEventsRepository {
    pub fn new(conn: Connection) -> Self {
        Self {
            db: DbConnection::new(conn),
        }
    }

    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }
}

#[async_trait]
impl ExternalEventsRepository for SqliteExternalEventsRepository {
    async fn insert_event(
        &self,
        event_type: &str,
        project_id: &str,
        payload: &str,
    ) -> AppResult<i64> {
        let event_type = event_type.to_string();
        let project_id = project_id.to_string();
        let payload = payload.to_string();

        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO external_events (event_type, project_id, payload) VALUES (?1, ?2, ?3)",
                    rusqlite::params![event_type, project_id, payload],
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
                Ok(conn.last_insert_rowid())
            })
            .await
    }

    async fn insert_event_once_for_attempt(
        &self,
        event_type: &str,
        project_id: &str,
        agent_run_id: &str,
        payload: &str,
    ) -> AppResult<bool> {
        let event_type = event_type.to_string();
        let project_id = project_id.to_string();
        let agent_run_marker = format!(
            "\"agent_run_id\":{}",
            serde_json::to_string(agent_run_id)
                .map_err(|error| AppError::Validation(error.to_string()))?
        );
        let payload = payload.to_string();

        self.db
            .run(move |conn| {
                let inserted = conn
                    .execute(
                        "INSERT INTO external_events (event_type, project_id, payload) \
                         SELECT ?1, ?2, ?4 \
                         WHERE NOT EXISTS ( \
                             SELECT 1 FROM external_events \
                             WHERE event_type = ?1 AND project_id = ?2 \
                             AND instr(payload, ?3) > 0 \
                         )",
                        rusqlite::params![event_type, project_id, agent_run_marker, payload],
                    )
                    .map_err(|error| AppError::Database(error.to_string()))?;
                Ok(inserted == 1)
            })
            .await
    }

    async fn get_events_after_cursor(
        &self,
        project_ids: &[String],
        cursor: i64,
        limit: i64,
    ) -> AppResult<Vec<ExternalEventRecord>> {
        if project_ids.is_empty() {
            return Ok(Vec::new());
        }

        let project_ids_owned: Vec<String> = project_ids.to_vec();

        self.db
            .run(move |conn| {
                // Build IN clause placeholders
                let placeholders: Vec<String> = (1..=project_ids_owned.len())
                    .map(|i| format!("?{}", i + 2))
                    .collect();
                let in_clause = placeholders.join(", ");

                let sql = format!(
                    "SELECT id, event_type, project_id, payload, created_at \
                     FROM external_events \
                     WHERE id > ?1 AND project_id IN ({}) \
                     ORDER BY id ASC \
                     LIMIT ?2",
                    in_clause
                );

                let mut stmt = conn
                    .prepare(&sql)
                    .map_err(|e| AppError::Database(e.to_string()))?;

                // Build params: cursor, limit, then each project_id
                let mut rows_result: Vec<ExternalEventRecord> = Vec::new();
                {
                    use rusqlite::types::ToSql;
                    let mut params_box: Vec<Box<dyn ToSql>> = Vec::new();
                    params_box.push(Box::new(cursor));
                    params_box.push(Box::new(limit));
                    for pid in &project_ids_owned {
                        params_box.push(Box::new(pid.clone()));
                    }

                    let params_refs: Vec<&dyn ToSql> =
                        params_box.iter().map(|b| b.as_ref()).collect();

                    let rows = stmt
                        .query(params_refs.as_slice())
                        .map_err(|e| AppError::Database(e.to_string()))?;

                    // Collect rows
                    let mut rows = rows;
                    while let Some(row) =
                        rows.next().map_err(|e| AppError::Database(e.to_string()))?
                    {
                        rows_result.push(ExternalEventRecord {
                            id: row.get(0).map_err(|e| AppError::Database(e.to_string()))?,
                            event_type: row
                                .get(1)
                                .map_err(|e| AppError::Database(e.to_string()))?,
                            project_id: row
                                .get(2)
                                .map_err(|e| AppError::Database(e.to_string()))?,
                            payload: row.get(3).map_err(|e| AppError::Database(e.to_string()))?,
                            created_at: row
                                .get(4)
                                .map_err(|e| AppError::Database(e.to_string()))?,
                        });
                    }
                }

                Ok(rows_result)
            })
            .await
    }

    async fn event_exists(
        &self,
        event_type: &str,
        project_id: &str,
        session_id: &str,
    ) -> AppResult<bool> {
        let event_type = event_type.to_string();
        let project_id = project_id.to_string();
        let session_id = session_id.to_string();

        self.db
            .run(move |conn| {
                let count: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM external_events \
                         WHERE event_type = ?1 AND project_id = ?2 \
                         AND instr(payload, ?3) > 0 \
                         LIMIT 1",
                        rusqlite::params![event_type, project_id, session_id],
                        |row| row.get(0),
                    )
                    .map_err(|e| AppError::Database(e.to_string()))?;
                Ok(count > 0)
            })
            .await
    }

    async fn cleanup_old_events(&self) -> AppResult<u64> {
        self.db
            .run(move |conn| {
                // Delete entries older than 24 hours
                let deleted_old = conn
                    .execute(
                        "DELETE FROM external_events \
                         WHERE created_at < datetime('now', '-24 hours')",
                        [],
                    )
                    .map_err(|e| AppError::Database(e.to_string()))?;

                // Delete entries beyond the 10 000-row high-water mark
                // For each project, keep only the newest 10 000 rows.
                // We identify the cutoff id by looking at the 10 001st row per project.
                let deleted_overflow = conn
                    .execute(
                        "DELETE FROM external_events \
                         WHERE id IN ( \
                             SELECT id FROM external_events e1 \
                             WHERE ( \
                                 SELECT COUNT(*) FROM external_events e2 \
                                 WHERE e2.project_id = e1.project_id AND e2.id >= e1.id \
                             ) > 10000 \
                         )",
                        [],
                    )
                    .map_err(|e| AppError::Database(e.to_string()))?;

                Ok((deleted_old + deleted_overflow) as u64)
            })
            .await
    }
}
