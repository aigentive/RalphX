// Migration v20260727161131: remote host settings

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS remote_host_settings (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
            exposure_mode TEXT NOT NULL DEFAULT 'serve'
                CHECK (exposure_mode IN ('serve', 'tailnet_direct')),
            port INTEGER NOT NULL DEFAULT 3849 CHECK (port BETWEEN 1 AND 65535),
            environment_id TEXT NOT NULL UNIQUE,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        );",
    )
    .map_err(|error| AppError::Database(error.to_string()))?;
    Ok(())
}
