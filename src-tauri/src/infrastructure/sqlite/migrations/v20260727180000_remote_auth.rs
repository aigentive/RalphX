// Migration v20260727180000: remote access auth tables (§4.3)
//
// Five tables backing the :3849 device-auth surface. Deliberately disjoint from `api_keys`:
// those are :3848 bot credentials with a permission bitmask, project scoping, and a
// zero-keys bootstrap bypass, none of which may apply to human device sessions (§4.1, §4.5).

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS remote_devices (
            id            TEXT PRIMARY KEY,
            name          TEXT NOT NULL,
            token_hash    TEXT NOT NULL UNIQUE,
            token_prefix  TEXT NOT NULL,
            scopes        TEXT NOT NULL,
            created_at    TEXT NOT NULL,
            last_seen_at  TEXT,
            revoked_at    TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_remote_devices_token_hash
            ON remote_devices(token_hash);
        CREATE INDEX IF NOT EXISTS idx_remote_devices_revoked
            ON remote_devices(revoked_at);

        CREATE TABLE IF NOT EXISTS remote_pairing_codes (
            id          TEXT PRIMARY KEY,
            code_hash   TEXT NOT NULL UNIQUE,
            scopes      TEXT NOT NULL,
            created_at  TEXT NOT NULL,
            expires_at  TEXT NOT NULL,
            consumed_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_remote_pairing_codes_hash
            ON remote_pairing_codes(code_hash);
        CREATE INDEX IF NOT EXISTS idx_remote_pairing_codes_outstanding
            ON remote_pairing_codes(consumed_at, expires_at);

        CREATE TABLE IF NOT EXISTS remote_sessions (
            id             TEXT PRIMARY KEY,
            device_id      TEXT NOT NULL REFERENCES remote_devices(id),
            connected_at   TEXT NOT NULL,
            last_active_at TEXT NOT NULL,
            remote_addr    TEXT NOT NULL,
            closed_at      TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_remote_sessions_device
            ON remote_sessions(device_id, closed_at);

        CREATE TABLE IF NOT EXISTS remote_ws_tickets (
            ticket_hash TEXT PRIMARY KEY,
            device_id   TEXT NOT NULL,
            expires_at  TEXT NOT NULL,
            consumed_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_remote_ws_tickets_device
            ON remote_ws_tickets(device_id, consumed_at);

        CREATE TABLE IF NOT EXISTS remote_audit_log (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            device_id  TEXT,
            action     TEXT NOT NULL,
            detail     TEXT,
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_remote_audit_log_device
            ON remote_audit_log(device_id, id);
        CREATE INDEX IF NOT EXISTS idx_remote_audit_log_created
            ON remote_audit_log(created_at);",
    )
    .map_err(|error| AppError::Database(error.to_string()))?;
    tracing::info!("v20260727180000: created remote access auth tables");
    Ok(())
}
