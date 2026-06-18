use crate::error::AppResult;
use rusqlite::Connection;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS project_metrics_config (
            project_id         TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
            simple_base_hours  REAL NOT NULL DEFAULT 2.0,
            medium_base_hours  REAL NOT NULL DEFAULT 4.0,
            complex_base_hours REAL NOT NULL DEFAULT 8.0,
            calendar_factor    REAL NOT NULL DEFAULT 1.5,
            updated_at         TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        );
    ",
    )
    .map_err(|e| crate::error::AppError::Database(e.to_string()))
}
