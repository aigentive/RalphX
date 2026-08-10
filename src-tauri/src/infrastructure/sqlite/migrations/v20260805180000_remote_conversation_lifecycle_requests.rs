use crate::error::AppResult;
use rusqlite::Connection;
pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS remote_conversation_lifecycle_requests (id TEXT PRIMARY KEY NOT NULL,kind TEXT NOT NULL,conversation_id TEXT NOT NULL,close_pull_request INTEGER NOT NULL DEFAULT 0,allocated_conversation_id TEXT UNIQUE,status TEXT NOT NULL,error_code TEXT,result_json TEXT,claimed_at TEXT,created_at TEXT NOT NULL,updated_at TEXT NOT NULL);CREATE INDEX IF NOT EXISTS idx_remote_conversation_lifecycle_pending ON remote_conversation_lifecycle_requests(status,created_at);CREATE INDEX IF NOT EXISTS idx_remote_conversation_lifecycle_conversation ON remote_conversation_lifecycle_requests(conversation_id,status);")?;
    Ok(())
}
