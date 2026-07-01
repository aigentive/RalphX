// Migration v20260701143000: add provider-keyed Workspace Review runtime settings.

use rusqlite::Connection;

use super::helpers::table_exists;
use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    if table_exists(conn, "workspace_review_runtime_settings") {
        return Ok(());
    }

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS workspace_review_runtime_settings (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            scope_type TEXT NOT NULL,
            scope_id TEXT,
            provider TEXT NOT NULL,
            model TEXT,
            effort TEXT,
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_workspace_review_runtime_settings_scope_provider
            ON workspace_review_runtime_settings(scope_type, scope_id, provider);
        CREATE INDEX IF NOT EXISTS idx_workspace_review_runtime_settings_scope
            ON workspace_review_runtime_settings(scope_type, scope_id);",
    )?;

    Ok(())
}
