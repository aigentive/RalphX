// Migration v20260802194326: agent workspace repair explicit publish consent

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers::add_column_if_not_exists;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    add_column_if_not_exists(
        conn,
        "agent_workspace_repair_attempts",
        "explicit_publish_requested",
        "INTEGER NOT NULL DEFAULT 0",
    )?;

    Ok(())
}
