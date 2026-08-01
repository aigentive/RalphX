// Migration v20260801130000: remote conversation message (continuation) requests
//
// Forward-only, numbered AFTER v20260801120000 (the start-intent table). The two tables are
// deliberately separate: a start seeds a NEW conversation and mints a fresh run, a message
// CONTINUES an existing one through its provider-session resume seam. One table with a `kind`
// column would make the dispatcher unable to prove which terminal call a claimed row authorizes.

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS remote_conversation_message_requests (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            project_id TEXT NOT NULL,
            content TEXT NOT NULL,
            provider TEXT NOT NULL,
            model_override TEXT,
            logical_effort TEXT,
            status TEXT NOT NULL,
            error_code TEXT,
            requested_by_device_id TEXT NOT NULL,
            agent_run_id TEXT,
            claimed_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_remote_conversation_message_requests_status
            ON remote_conversation_message_requests(status);
        CREATE INDEX IF NOT EXISTS idx_remote_conversation_message_requests_conversation
            ON remote_conversation_message_requests(conversation_id);",
    )?;
    Ok(())
}
