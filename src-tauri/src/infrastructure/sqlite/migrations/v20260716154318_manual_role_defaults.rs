// Migration v20260716154318: manual role defaults

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS manual_role_defaults (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            scope_type TEXT NOT NULL CHECK (scope_type IN ('global', 'project')),
            scope_id TEXT NOT NULL DEFAULT '',
            role TEXT NOT NULL,
            value_json TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT (
                strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')
            ),
            UNIQUE(scope_type, scope_id, role),
            CHECK (
                (scope_type = 'global' AND scope_id = '') OR
                (scope_type = 'project' AND length(scope_id) > 0)
            )
        );
        CREATE INDEX IF NOT EXISTS idx_manual_role_defaults_scope
            ON manual_role_defaults(scope_type, scope_id);",
    )
    .map_err(|error| crate::error::AppError::Database(error.to_string()))?;
    Ok(())
}
