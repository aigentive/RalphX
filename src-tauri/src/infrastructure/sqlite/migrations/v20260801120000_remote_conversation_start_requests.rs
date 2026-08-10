// Migration v20260801120000: remote conversation start requests

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS remote_conversation_start_requests (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            project_id TEXT NOT NULL,
            content TEXT NOT NULL,
            provider TEXT NOT NULL,
            model TEXT,
            effort TEXT,
            mode TEXT NOT NULL,
            status TEXT NOT NULL,
            error_code TEXT,
            requested_by_device_id TEXT NOT NULL,
            agent_run_id TEXT,
            claimed_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_remote_conversation_start_requests_status
            ON remote_conversation_start_requests(status);",
    )?;
    Ok(())
}
