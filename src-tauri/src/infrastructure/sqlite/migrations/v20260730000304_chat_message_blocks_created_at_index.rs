// Migration v20260730000304: index chat_message_blocks.created_at
//
// The chat payload retention prune deletes payload rows in bounded batches
// selected by `block.created_at` with an ORDER BY + LIMIT. Without this index
// every batch full-scans and sorts all blocks while holding the shared DB
// connection; with it SQLite walks the index in order and stops at the limit.

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_chat_message_blocks_created_at
         ON chat_message_blocks(created_at);",
    )
    .map_err(|e| crate::error::AppError::Database(e.to_string()))?;

    Ok(())
}
