// Migration v20260330000001: Add ideation_effort_settings table
//
// Creates a new table for storing per-project and global ideation effort settings.
// project_id IS NULL = global row; project_id = 'proj-xyz' = per-project override.
// Valid effort values: 'low', 'medium', 'high', 'max', 'inherit'

use super::helpers;
use crate::error::AppResult;
use rusqlite::Connection;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    if !helpers::table_exists(conn, "ideation_effort_settings") {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS ideation_effort_settings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id TEXT DEFAULT NULL,
                primary_effort TEXT NOT NULL DEFAULT 'inherit',
                verifier_effort TEXT NOT NULL DEFAULT 'inherit',
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_ideation_effort_project
                ON ideation_effort_settings(project_id);",
        )?;
        tracing::info!("v20260330000001: created ideation_effort_settings table");
    }
    Ok(())
}
