use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS agent_model_registry (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            provider TEXT NOT NULL,
            model_id TEXT NOT NULL,
            label TEXT NOT NULL,
            menu_label TEXT NOT NULL,
            description TEXT,
            default_effort TEXT NOT NULL,
            supported_efforts TEXT NOT NULL,
            source TEXT NOT NULL DEFAULT 'custom',
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            UNIQUE(provider, model_id)
        );
        CREATE INDEX IF NOT EXISTS idx_agent_model_registry_provider
            ON agent_model_registry(provider);
        CREATE INDEX IF NOT EXISTS idx_agent_model_registry_enabled
            ON agent_model_registry(enabled);",
    )?;

    Ok(())
}
