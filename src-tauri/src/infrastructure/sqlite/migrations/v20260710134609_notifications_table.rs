// Migration v20260710134609: notifications table

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS notifications (
            id TEXT PRIMARY KEY,
            created_at TEXT NOT NULL,
            project_id TEXT,
            category TEXT NOT NULL,
            severity TEXT NOT NULL,
            title TEXT NOT NULL,
            body TEXT,
            target_json TEXT,
            dedupe_key TEXT UNIQUE,
            read_at TEXT
        )",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_notifications_unread
         ON notifications(created_at) WHERE read_at IS NULL",
        [],
    )?;
    Ok(())
}
