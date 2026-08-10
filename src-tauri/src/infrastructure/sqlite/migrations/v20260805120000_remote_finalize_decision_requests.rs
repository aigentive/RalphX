use crate::error::AppResult;
use rusqlite::Connection;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS remote_finalize_decision_requests (
        id TEXT PRIMARY KEY NOT NULL, session_id TEXT NOT NULL, decision TEXT NOT NULL,
        status TEXT NOT NULL, error_code TEXT, result_json TEXT, claimed_at TEXT,
        created_at TEXT NOT NULL, updated_at TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_remote_finalize_decision_requests_pending ON remote_finalize_decision_requests(status, created_at);
    CREATE INDEX IF NOT EXISTS idx_remote_finalize_decision_requests_session ON remote_finalize_decision_requests(session_id, status);")?;
    Ok(())
}
