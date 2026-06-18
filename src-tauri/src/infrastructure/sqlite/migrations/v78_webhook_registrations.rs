use crate::error::AppResult;
use rusqlite::Connection;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS webhook_registrations (
            id TEXT PRIMARY KEY,
            api_key_id TEXT NOT NULL,
            url TEXT NOT NULL,
            event_types TEXT,
            project_ids TEXT NOT NULL,
            secret TEXT NOT NULL,
            active INTEGER NOT NULL DEFAULT 1,
            failure_count INTEGER NOT NULL DEFAULT 0,
            last_failure_at TEXT,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
        );
        CREATE INDEX IF NOT EXISTS idx_webhook_registrations_api_key ON webhook_registrations(api_key_id);
        CREATE INDEX IF NOT EXISTS idx_webhook_registrations_active ON webhook_registrations(active, api_key_id);
    ")?;
    tracing::info!("v78: created webhook_registrations table");
    Ok(())
}
