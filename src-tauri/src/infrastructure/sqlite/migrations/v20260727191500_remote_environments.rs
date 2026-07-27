// Migration v20260727191500: remote environments registry (client mode, §6.1)
//
// One row per paired remote host. `environment_id` is the host-reported identity and is
// UNIQUE so pairing the same host via MagicDNS and via its 100.x address merges into one
// environment instead of inserting a second row. `status` drives the partial-failure
// reconciler (`active | pending_add | pending_delete`).

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS remote_environments (
            id                TEXT PRIMARY KEY,
            environment_id    TEXT NOT NULL UNIQUE,
            name              TEXT NOT NULL,
            base_url          TEXT NOT NULL,
            candidate_urls    TEXT NOT NULL,
            token_secret_ref  TEXT NOT NULL,
            scopes            TEXT NOT NULL,
            protocol_version  INTEGER NOT NULL,
            status            TEXT NOT NULL
                CHECK (status IN ('active', 'pending_add', 'pending_delete')),
            created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            last_connected_at TEXT
        );",
    )
    .map_err(|error| AppError::Database(error.to_string()))?;
    Ok(())
}
