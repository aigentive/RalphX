// Migration v20260615092455: project skill settings

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS project_skill_settings (
            project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
            export_enabled INTEGER NOT NULL DEFAULT 0 CHECK (export_enabled IN (0, 1)),
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );",
    )
    .map_err(|error| AppError::Database(error.to_string()))?;
    Ok(())
}
