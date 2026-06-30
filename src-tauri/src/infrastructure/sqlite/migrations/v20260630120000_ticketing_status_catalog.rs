// Migration v20260630120000: ticketing status catalog

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS ticketing_status_catalog (
            id TEXT PRIMARY KEY,
            provider TEXT NOT NULL,
            scope_kind TEXT NOT NULL,
            scope_id TEXT NOT NULL,
            provider_status_id TEXT NOT NULL,
            provider_status_name TEXT NOT NULL,
            provider_category TEXT NOT NULL,
            provider_color TEXT,
            provider_order INTEGER,
            display_order INTEGER NOT NULL,
            color_override TEXT,
            is_visible INTEGER NOT NULL DEFAULT 1 CHECK (is_visible IN (0, 1)),
            is_terminal INTEGER NOT NULL DEFAULT 0 CHECK (is_terminal IN (0, 1)),
            last_seen_at TEXT,
            stale_since TEXT,
            metadata_json TEXT,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            UNIQUE (provider, scope_kind, scope_id, provider_status_id)
        );

        CREATE INDEX IF NOT EXISTS idx_ticketing_status_catalog_scope_order
            ON ticketing_status_catalog (provider, scope_kind, scope_id, display_order);

        CREATE INDEX IF NOT EXISTS idx_ticketing_status_catalog_scope_stale
            ON ticketing_status_catalog (provider, scope_kind, scope_id, stale_since);
        ",
    )?;
    Ok(())
}
