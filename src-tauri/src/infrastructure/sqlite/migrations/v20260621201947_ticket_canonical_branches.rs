// Migration v20260621201947: ticket canonical branches

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS ticket_canonical_branches (
            project_id TEXT NOT NULL,
            provider TEXT NOT NULL,
            issue_key TEXT NOT NULL,
            branch_name TEXT NOT NULL,
            base_branch TEXT NOT NULL,
            base_commit TEXT,
            origin_pushed INTEGER NOT NULL DEFAULT 0,
            terminal INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (project_id, provider, issue_key)
        );",
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    Ok(())
}
