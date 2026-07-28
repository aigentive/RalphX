// Migration v20260728155615: agent conversation mutes

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS agent_conversation_mutes (
            conversation_id TEXT PRIMARY KEY REFERENCES chat_conversations(id) ON DELETE CASCADE,
            muted_at TEXT NOT NULL,
            state_fingerprint TEXT NOT NULL
        );",
    )?;
    Ok(())
}
