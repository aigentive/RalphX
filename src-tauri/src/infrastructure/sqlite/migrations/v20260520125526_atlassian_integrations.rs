// Migration v20260520125526: atlassian integrations

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS atlassian_integration_settings (
            id TEXT PRIMARY KEY CHECK (id = 'default'),
            enabled INTEGER NOT NULL DEFAULT 0,
            site_url TEXT,
            email TEXT,
            token_secret_ref TEXT,
            validation_status TEXT NOT NULL DEFAULT 'not_configured'
                CHECK (validation_status IN ('not_configured', 'pending', 'valid', 'invalid')),
            jira_available INTEGER NOT NULL DEFAULT 0,
            confluence_available INTEGER NOT NULL DEFAULT 0,
            last_validated_at TEXT,
            last_error TEXT,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        );

        INSERT OR IGNORE INTO atlassian_integration_settings (
            id,
            enabled,
            validation_status,
            jira_available,
            confluence_available
        ) VALUES (
            'default',
            0,
            'not_configured',
            0,
            0
        );
        "#,
    )?;
    Ok(())
}
