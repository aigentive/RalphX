// Migration v20260811015146: data retention settings
//
// Single-row policy table for chat payload retention. The repository seeds row 1
// lazily from config defaults; this migration deliberately inserts nothing so the
// seeding contract lives in one place.
//
// `payload_size_budget_bytes` and `size_budget_confirmed_at` are nullable and ship
// NULL: size-based pruning deletes payloads that are still inside the time window,
// so it stays opt-in and requires a recorded user confirmation.

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS data_retention_settings (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            payload_retention_enabled INTEGER NOT NULL,
            payload_retention_days INTEGER NOT NULL,
            payload_retention_archived_days INTEGER NOT NULL,
            payload_size_budget_bytes INTEGER,
            size_budget_confirmed_at TEXT,
            payload_retention_batch_rows INTEGER NOT NULL,
            seeded_pristine INTEGER NOT NULL DEFAULT 1,
            size_budget_advised INTEGER NOT NULL DEFAULT 0,
            last_run_at TEXT,
            last_run_pruned_rows INTEGER,
            last_run_payload_bytes INTEGER,
            last_run_payload_rows INTEGER,
            updated_at TEXT NOT NULL
        );
        "#,
    )
    .map_err(|error| AppError::Database(error.to_string()))
}
