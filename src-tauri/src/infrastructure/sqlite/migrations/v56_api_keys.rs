use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS api_keys (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            key_hash TEXT NOT NULL UNIQUE,
            key_prefix TEXT NOT NULL,
            permissions INTEGER NOT NULL DEFAULT 3,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
            revoked_at TEXT,
            last_used_at TEXT,
            grace_expires_at TEXT,
            metadata TEXT
        );
        CREATE TABLE IF NOT EXISTS api_key_projects (
            api_key_id TEXT NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
            project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            PRIMARY KEY (api_key_id, project_id)
        );
        CREATE TABLE IF NOT EXISTS external_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            event_type TEXT NOT NULL,
            project_id TEXT NOT NULL,
            payload TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
        );
        CREATE TABLE IF NOT EXISTS api_audit_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            api_key_id TEXT NOT NULL,
            tool_name TEXT NOT NULL,
            project_id TEXT,
            success INTEGER NOT NULL DEFAULT 1,
            latency_ms INTEGER,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
        );
        CREATE INDEX IF NOT EXISTS idx_api_keys_hash ON api_keys(key_hash);
        CREATE INDEX IF NOT EXISTS idx_api_key_projects_project ON api_key_projects(project_id);
        CREATE INDEX IF NOT EXISTS idx_external_events_project ON external_events(project_id, id);
        CREATE INDEX IF NOT EXISTS idx_external_events_created ON external_events(created_at);
        CREATE INDEX IF NOT EXISTS idx_audit_log_key ON api_audit_log(api_key_id, created_at);
    ",
    )?;
    Ok(())
}
