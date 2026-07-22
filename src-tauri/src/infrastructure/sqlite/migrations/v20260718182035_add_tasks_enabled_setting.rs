// Migration v20260718182035: add tasks enabled setting

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers::add_column_if_not_exists;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    add_column_if_not_exists(
        conn,
        "ideation_settings",
        "tasks_enabled",
        "INTEGER NOT NULL DEFAULT 0",
    )
}
