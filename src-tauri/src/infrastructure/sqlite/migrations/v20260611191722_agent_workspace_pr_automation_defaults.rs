// Migration v20260611191722: agent workspace pr automation defaults

use rusqlite::Connection;

use super::helpers;
use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "execution_settings",
        "agent_workspace_pr_autofix_default",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "execution_settings",
        "agent_workspace_pr_auto_merge_default",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    Ok(())
}
