// Migration v20260718014631: mcp policy overrides

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS mcp_policy_overrides (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            scope_type TEXT NOT NULL CHECK (scope_type IN ('global', 'project')),
            scope_id TEXT NOT NULL DEFAULT '',
            provider TEXT NOT NULL CHECK (provider IN ('claude', 'codex')),
            server_id TEXT NOT NULL,
            server_state TEXT NOT NULL DEFAULT 'follow'
                CHECK (server_state IN ('follow', 'enabled', 'disabled')),
            tool_states_json TEXT NOT NULL DEFAULT '{}',
            updated_at TEXT NOT NULL DEFAULT (
                strftime('%Y-%m-%dT%H:%M:%f+00:00', 'now')
            ),
            UNIQUE(scope_type, scope_id, provider, server_id),
            CHECK (
                (scope_type = 'global' AND scope_id = '') OR
                (scope_type = 'project' AND length(scope_id) > 0)
            ),
            CHECK (length(server_id) BETWEEN 1 AND 128),
            CHECK (server_id NOT IN ('ralphx', 'ralphx_internal') OR server_state != 'disabled')
        );
        CREATE INDEX IF NOT EXISTS idx_mcp_policy_overrides_scope
            ON mcp_policy_overrides(scope_type, scope_id, provider);",
    )
    .map_err(|error| crate::error::AppError::Database(error.to_string()))?;
    Ok(())
}
