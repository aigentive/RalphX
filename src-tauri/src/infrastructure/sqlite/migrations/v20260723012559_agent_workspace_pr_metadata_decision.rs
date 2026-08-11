// Migration v20260723012559: agent workspace pr metadata decision

use rusqlite::Connection;

use super::helpers;
use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "publication_pr_metadata_decision",
        "TEXT NULL",
    )
}
