// Migration v20260325131500: execution ideation allocation settings

use rusqlite::Connection;

use super::helpers;
use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "execution_settings",
        "project_ideation_max",
        "INTEGER NOT NULL DEFAULT 5",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "global_execution_settings",
        "global_ideation_max",
        "INTEGER NOT NULL DEFAULT 10",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "global_execution_settings",
        "allow_ideation_borrow_idle_execution",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    Ok(())
}
