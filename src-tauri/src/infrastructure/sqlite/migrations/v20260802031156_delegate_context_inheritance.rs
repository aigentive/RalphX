// Migration v20260802031156: delegate context inheritance

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "delegated_sessions",
        "delegate_context_authorized",
        "INTEGER NOT NULL DEFAULT 1",
    )?;
    helpers::add_column_if_not_exists(conn, "delegated_sessions", "caller_conversation_id", "TEXT")
}
