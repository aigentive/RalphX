// Migration v20260517153000: agent workspace PR supervision

use rusqlite::Connection;

use super::helpers;
use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "pr_autofix_enabled",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "pr_auto_merge_desired",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "pr_auto_merge_method",
        "TEXT NOT NULL DEFAULT 'squash'",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "pr_auto_merge_current",
        "INTEGER NULL",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "pr_supervision_status",
        "TEXT NULL",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "pr_supervision_summary",
        "TEXT NULL",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "pr_supervision_updated_at",
        "TEXT NULL",
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_workspaces_pr_supervision
         ON agent_conversation_workspaces(pr_autofix_enabled, pr_auto_merge_desired, publication_pr_number)",
        [],
    )?;

    Ok(())
}
