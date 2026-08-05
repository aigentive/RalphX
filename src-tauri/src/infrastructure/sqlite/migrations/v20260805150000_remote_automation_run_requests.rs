use crate::error::AppResult;
use rusqlite::Connection;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS remote_automation_run_requests (
            id TEXT PRIMARY KEY NOT NULL,
            automation_id TEXT NOT NULL,
            kind TEXT NOT NULL CHECK(kind IN ('runNow','retryJudge')),
            expected_run_id TEXT,
            status TEXT NOT NULL CHECK(status IN ('pending','starting','completed','failed','failedStale')),
            error_code TEXT,
            result_json TEXT,
            claimed_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_remote_automation_run_requests_pending ON remote_automation_run_requests(status, created_at);
        CREATE INDEX IF NOT EXISTS idx_remote_automation_run_requests_automation_kind ON remote_automation_run_requests(automation_id, kind, status);",
    )?;
    Ok(())
}
