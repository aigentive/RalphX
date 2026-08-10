use crate::error::AppResult;
use rusqlite::Connection;
pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS remote_plan_edit_requests (id TEXT PRIMARY KEY NOT NULL, artifact_id TEXT NOT NULL, content TEXT NOT NULL, expected_version INTEGER NOT NULL, status TEXT NOT NULL, error_code TEXT, result_json TEXT, claimed_at TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL); CREATE INDEX IF NOT EXISTS idx_remote_plan_edit_requests_dispatch ON remote_plan_edit_requests(status, created_at); CREATE INDEX IF NOT EXISTS idx_remote_plan_edit_requests_artifact ON remote_plan_edit_requests(artifact_id, status);")?;
    Ok(())
}
