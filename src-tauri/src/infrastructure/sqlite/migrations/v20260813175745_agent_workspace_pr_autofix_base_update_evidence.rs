// Migration v20260813175745: agent workspace pr autofix base update evidence

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "agent_workspace_repair_attempts",
        "pr_autofix_issue_kind",
        "TEXT",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_workspace_repair_attempts",
        "base_update_head_commit",
        "TEXT",
    )
}
