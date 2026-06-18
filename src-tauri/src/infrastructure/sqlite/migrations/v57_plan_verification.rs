use rusqlite::Connection;

use super::helpers;
use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "ideation_sessions",
        "verification_status",
        "TEXT NOT NULL DEFAULT 'unverified'",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "ideation_sessions",
        "verification_in_progress",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    helpers::add_column_if_not_exists(conn, "ideation_sessions", "verification_metadata", "TEXT")?;
    Ok(())
}
