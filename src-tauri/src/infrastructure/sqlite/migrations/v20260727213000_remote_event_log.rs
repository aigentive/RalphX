// Migration v20260727213000: durable remote event log + seq high-water (§3.4 DDL)
//
// `seq` is `INTEGER PRIMARY KEY` and deliberately **not** AUTOINCREMENT: the sequencer actor
// is the only assigner of `seq`, so letting SQLite pick one would create a second numbering
// authority. `epoch` records the in-memory `streamEpoch` that wrote the row — the epoch is
// never persisted as host state (§3.2 rule A), so rows from a prior epoch are unreplayable by
// construction and prune-eligible immediately.
//
// The high-water lives on `remote_host_settings` rather than being derived from
// `MAX(seq)`: pruning deletes rows, so `MAX(seq)` would let numbering move backwards after a
// prune of the tail. It is written in the same `run_transaction` as the batch commit, so the
// counter can never disagree with the log.

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS remote_event_log (
            seq        INTEGER PRIMARY KEY,
            epoch      TEXT NOT NULL,
            name       TEXT NOT NULL,
            payload    TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
        );
        CREATE INDEX IF NOT EXISTS idx_remote_event_log_epoch_seq
            ON remote_event_log (epoch, seq);
        CREATE INDEX IF NOT EXISTS idx_remote_event_log_created_at
            ON remote_event_log (created_at);",
    )
    .map_err(|error| AppError::Database(error.to_string()))?;

    // Monotonic and never reused across boots; 0 means "nothing sequenced yet".
    helpers::add_column_if_not_exists(
        conn,
        "remote_host_settings",
        "event_seq_high_water",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    Ok(())
}
