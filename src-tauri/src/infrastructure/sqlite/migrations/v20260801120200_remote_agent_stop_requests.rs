// Migration v20260801120200: remote agent stop requests
//
// Forward-only, numbered AFTER v20260801120000 (remote conversation start requests).

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS remote_agent_stop_requests (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            status TEXT NOT NULL,
            error_code TEXT,
            requested_by_device_id TEXT NOT NULL,
            claimed_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_remote_agent_stop_requests_status
            ON remote_agent_stop_requests(status);
        CREATE INDEX IF NOT EXISTS idx_remote_agent_stop_requests_conversation_status
            ON remote_agent_stop_requests(conversation_id, status);",
    )?;
    Ok(())
}
