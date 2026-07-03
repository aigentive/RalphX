use rusqlite::Connection;

use crate::error::{AppError, AppResult};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS queued_messages (
            id TEXT PRIMARY KEY,
            context_type TEXT NOT NULL,
            context_id TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at TEXT NOT NULL,
            is_editing INTEGER NOT NULL DEFAULT 0,
            sequence INTEGER NOT NULL,
            payload_json TEXT NOT NULL,
            inserted_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        );
        CREATE INDEX IF NOT EXISTS idx_queued_messages_context_order
            ON queued_messages(context_type, context_id, sequence, created_at, id);
        CREATE INDEX IF NOT EXISTS idx_queued_messages_context
            ON queued_messages(context_type, context_id);
        CREATE INDEX IF NOT EXISTS idx_queued_messages_updated_at
            ON queued_messages(updated_at);",
    )
    .map_err(|error| AppError::Database(error.to_string()))
}
