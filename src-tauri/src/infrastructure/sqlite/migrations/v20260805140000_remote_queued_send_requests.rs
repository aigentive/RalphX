use crate::error::AppResult;
use rusqlite::Connection;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS remote_queued_send_requests (
            id TEXT PRIMARY KEY NOT NULL,
            conversation_id TEXT NOT NULL,
            queued_message_id TEXT NOT NULL,
            expected_active_run_id TEXT,
            status TEXT NOT NULL,
            error_code TEXT,
            result_json TEXT,
            claimed_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_remote_queued_send_requests_status
            ON remote_queued_send_requests(status);
        CREATE INDEX IF NOT EXISTS idx_remote_queued_send_requests_entry
            ON remote_queued_send_requests(conversation_id, queued_message_id);",
    )?;
    Ok(())
}
