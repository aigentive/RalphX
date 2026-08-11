// Migration v20260720200633: auto verify draft plans

use rusqlite::Connection;

use super::helpers::add_column_if_not_exists;
use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    add_column_if_not_exists(
        conn,
        "ideation_settings",
        "auto_verify_draft_plans",
        "INTEGER NOT NULL DEFAULT 1",
    )?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS deferred_plan_approval_notifications (
            session_id TEXT PRIMARY KEY NOT NULL,
            artifact_id TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
         );
         CREATE INDEX IF NOT EXISTS idx_deferred_plan_approval_artifact
           ON deferred_plan_approval_notifications(artifact_id);",
    )?;
    Ok(())
}
