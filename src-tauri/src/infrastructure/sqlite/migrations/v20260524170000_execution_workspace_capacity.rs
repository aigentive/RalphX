use rusqlite::Connection;

use super::helpers;
use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "global_execution_settings",
        "workspace_max_concurrent",
        "INTEGER NOT NULL DEFAULT 10",
    )?;
    Ok(())
}
