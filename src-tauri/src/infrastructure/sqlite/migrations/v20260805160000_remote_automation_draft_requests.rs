use crate::error::AppResult;
use rusqlite::Connection;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS remote_automation_draft_requests (
            id TEXT PRIMARY KEY NOT NULL,
            project_id TEXT NOT NULL,
            automation_id TEXT NOT NULL UNIQUE,
            name TEXT NOT NULL,
            authoring_mode TEXT NOT NULL,
            base_ref_kind TEXT NOT NULL,
            base_branch_mode TEXT NOT NULL,
            base_branch TEXT,
            status TEXT NOT NULL,
            error_code TEXT,
            result_json TEXT,
            claimed_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_remote_automation_draft_requests_pending ON remote_automation_draft_requests(status, created_at);
        CREATE INDEX IF NOT EXISTS idx_remote_automation_draft_requests_project_name ON remote_automation_draft_requests(project_id, name, status);",
    )?;
    Ok(())
}
