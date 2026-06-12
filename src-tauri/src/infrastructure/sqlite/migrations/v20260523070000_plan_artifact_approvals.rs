// Migration v20260523070000: Add Plan-mode artifact approval records.

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS plan_artifact_approvals (
            session_id TEXT PRIMARY KEY REFERENCES ideation_sessions(id) ON DELETE CASCADE,
            artifact_id TEXT NOT NULL REFERENCES artifacts(id) ON DELETE CASCADE,
            artifact_version INTEGER NOT NULL,
            status TEXT NOT NULL DEFAULT 'approved' CHECK(status IN ('approved')),
            approved_at TEXT NOT NULL,
            approved_by TEXT NOT NULL DEFAULT 'user'
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_plan_artifact_approvals_artifact
         ON plan_artifact_approvals(artifact_id)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    tracing::info!("v20260523070000: added plan_artifact_approvals table");

    Ok(())
}
