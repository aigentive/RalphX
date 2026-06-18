// Migration v20260521150003: agent workspace source pull request

use rusqlite::Connection;

use crate::error::AppResult;
use crate::infrastructure::sqlite::migrations::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "source_pr_number",
        "INTEGER NULL",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "source_pr_url",
        "TEXT NULL",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "source_pr_title",
        "TEXT NULL",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "source_pr_head_ref",
        "TEXT NULL",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "source_pr_base_ref",
        "TEXT NULL",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "source_pr_head_sha",
        "TEXT NULL",
    )?;
    Ok(())
}
