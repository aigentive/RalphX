// Migration v20260617122100: Linear integration settings

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS linear_integration_settings (
            id TEXT PRIMARY KEY CHECK (id = 'default'),
            enabled INTEGER NOT NULL DEFAULT 0,
            token_secret_ref TEXT,
            validation_status TEXT NOT NULL DEFAULT 'not_configured',
            issue_search_available INTEGER NOT NULL DEFAULT 0,
            last_validated_at TEXT,
            last_error TEXT,
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        );

        INSERT OR IGNORE INTO linear_integration_settings (
            id, enabled, token_secret_ref, validation_status, issue_search_available
        ) VALUES (
            'default', 0, NULL, 'not_configured', 0
        );
        "#,
    )?;
    Ok(())
}
