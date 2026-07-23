// Migration v20260722022339: usage capture provenance and raw snapshots

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers::add_column_if_not_exists;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    for table in ["agent_runs", "chat_messages"] {
        for (column, definition) in [
            ("usage_provenance", "TEXT"),
            ("raw_usage_input_tokens", "INTEGER"),
            ("raw_usage_output_tokens", "INTEGER"),
            ("raw_usage_cache_creation_tokens", "INTEGER"),
            ("raw_usage_cache_read_tokens", "INTEGER"),
            ("raw_usage_estimated_usd", "REAL"),
        ] {
            add_column_if_not_exists(conn, table, column, definition)?;
        }
    }

    Ok(())
}
