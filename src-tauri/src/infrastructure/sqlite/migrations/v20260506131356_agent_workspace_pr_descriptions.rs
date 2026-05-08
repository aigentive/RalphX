// Migration v20260506131356: agent workspace pr descriptions

use rusqlite::Connection;

use super::helpers;
use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "publication_pr_title",
        "TEXT NULL",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "publication_pr_body",
        "TEXT NULL",
    )?;
    Ok(())
}
