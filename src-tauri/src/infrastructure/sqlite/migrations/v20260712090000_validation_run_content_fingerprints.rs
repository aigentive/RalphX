use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers::add_column_if_not_exists;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    add_column_if_not_exists(conn, "validation_runs", "start_content_fingerprint", "TEXT")?;
    add_column_if_not_exists(
        conn,
        "validation_runs",
        "validated_content_fingerprint",
        "TEXT",
    )?;
    add_column_if_not_exists(conn, "validation_runs", "promoted_commit_sha", "TEXT")?;
    Ok(())
}
