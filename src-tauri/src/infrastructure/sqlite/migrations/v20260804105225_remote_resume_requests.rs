// Migration v20260804105225: remote resume requests

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS remote_resume_requests (
            id TEXT PRIMARY KEY, family TEXT NOT NULL, action TEXT, task_id TEXT,
            project_id TEXT, group_kind TEXT, group_id TEXT,
            force_restart INTEGER NOT NULL DEFAULT 0, note TEXT,
            status TEXT NOT NULL, error_code TEXT, result_json TEXT, claimed_at TEXT,
            created_at TEXT NOT NULL, updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_remote_resume_requests_pending
            ON remote_resume_requests(family, status, created_at);
        CREATE INDEX IF NOT EXISTS idx_remote_resume_requests_task
            ON remote_resume_requests(task_id, status);",
    )?;
    Ok(())
}
