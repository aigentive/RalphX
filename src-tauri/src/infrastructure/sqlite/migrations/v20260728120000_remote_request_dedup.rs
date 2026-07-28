// Migration v20260728120000: remote request idempotency + attachment metadata (§4.3, C-16)
//
// `remote_request_dedup` is the DURABLE half of the two-layer idempotency design. The
// in-memory reservation map coalesces concurrent duplicates inside one process; this table is
// what survives a host restart, so a client that retries after the host died gets the cached
// outcome instead of a second side effect.
//
// The primary key is `(device_id, request_id)` and NOT `request_id` alone: request ids are
// minted client-side, so a global key would let one device's ids collide with — or read —
// another's. `args_hash` binds the id to its payload; a matching id with a different hash is
// rejected (`REMOTE_REQUEST_ID_REUSED`) rather than served a wrong cache hit.
//
// `remote_attachments` stores the client-supplied `display_name` as DATA precisely so it never
// becomes a filesystem path component: the on-disk name is the server-minted UUID `id`.
// `size` is INTEGER so the per-device quota is exact integer arithmetic, never float.

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS remote_request_dedup (
            device_id  TEXT NOT NULL,
            request_id TEXT NOT NULL,
            args_hash  TEXT NOT NULL,
            outcome    TEXT NOT NULL,
            response   TEXT NOT NULL,
            created_at TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            PRIMARY KEY (device_id, request_id)
        );
        CREATE INDEX IF NOT EXISTS idx_remote_request_dedup_expires
            ON remote_request_dedup (expires_at);
        CREATE TABLE IF NOT EXISTS remote_attachments (
            id           TEXT PRIMARY KEY,
            device_id    TEXT NOT NULL,
            display_name TEXT,
            mime         TEXT NOT NULL,
            size         INTEGER NOT NULL,
            created_at   TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_remote_attachments_device
            ON remote_attachments (device_id);",
    )
    .map_err(|error| AppError::Database(error.to_string()))?;
    Ok(())
}
