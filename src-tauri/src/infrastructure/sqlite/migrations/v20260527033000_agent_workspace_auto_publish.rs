// Migration v20260527033000: agent workspace auto publish gate

use rusqlite::Connection;

use super::helpers;
use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "auto_publish_enabled",
        "INTEGER NOT NULL DEFAULT 1",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "auto_publish_paused_pr_autofix_enabled",
        "INTEGER NULL",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "auto_publish_paused_pr_auto_merge_desired",
        "INTEGER NULL",
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_workspaces_auto_publish
         ON agent_conversation_workspaces(auto_publish_enabled, publication_pr_number, publication_push_status)",
        [],
    )?;

    Ok(())
}
