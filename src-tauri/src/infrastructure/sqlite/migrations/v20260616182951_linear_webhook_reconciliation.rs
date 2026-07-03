// Migration v20260616182951: linear webhook reconciliation

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    super::v20260616182441_external_issue_links::migrate(conn)?;

    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS linear_webhook_config (
            id TEXT PRIMARY KEY CHECK (id = 'default'),
            enabled INTEGER NOT NULL DEFAULT 0,
            signing_secret_ref TEXT,
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        );

        CREATE TABLE IF NOT EXISTS linear_webhook_deliveries (
            delivery_id TEXT PRIMARY KEY,
            webhook_id TEXT,
            event_type TEXT NOT NULL,
            received_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS external_issue_sync_events (
            id TEXT PRIMARY KEY,
            provider TEXT NOT NULL,
            external_id TEXT NOT NULL,
            delivery_id TEXT NOT NULL,
            event_type TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            UNIQUE(provider, delivery_id, external_id, event_type)
        );

        INSERT OR IGNORE INTO linear_webhook_config (id, enabled, signing_secret_ref)
        VALUES ('default', 0, NULL);
        "#,
    )?;
    Ok(())
}
