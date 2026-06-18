// Migration v20260330000002: Add ideation_model_settings table
//
// Creates a new table for storing per-project and global ideation model settings.
// project_id IS NULL = global row; project_id = 'proj-xyz' = per-project override.
// Valid model values: 'inherit', 'sonnet', 'opus', 'haiku'

use super::helpers;
use crate::error::AppResult;
use rusqlite::Connection;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    if !helpers::table_exists(conn, "ideation_model_settings") {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS ideation_model_settings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id TEXT DEFAULT NULL,
                primary_model TEXT NOT NULL DEFAULT 'inherit',
                verifier_model TEXT NOT NULL DEFAULT 'inherit',
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_ideation_model_settings_project
                ON ideation_model_settings(project_id);",
        )?;
        tracing::info!("v20260330000002: created ideation_model_settings table");
    }
    Ok(())
}
