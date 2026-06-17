// Migration v20260617122430: agent workspace initial auto publish

use rusqlite::Connection;

use super::helpers;
use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "auto_publish_initial_pr_enabled",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    Ok(())
}
