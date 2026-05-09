use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers::table_exists;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    if table_exists(conn, "agent_provider_settings") {
        return Ok(());
    }

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS agent_provider_settings (
            provider TEXT PRIMARY KEY,
            enabled INTEGER NOT NULL DEFAULT 0,
            is_default INTEGER NOT NULL DEFAULT 0,
            model TEXT,
            effort TEXT,
            approval_policy TEXT,
            sandbox_mode TEXT,
            claude_permission_mode TEXT,
            claude_dangerously_skip_permissions INTEGER NOT NULL DEFAULT 1,
            claude_allow_dangerously_skip_permissions INTEGER NOT NULL DEFAULT 0,
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_provider_settings_default
            ON agent_provider_settings(is_default)
            WHERE is_default = 1;",
    )?;

    Ok(())
}
