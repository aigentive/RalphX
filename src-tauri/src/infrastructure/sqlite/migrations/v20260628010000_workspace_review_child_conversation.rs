// Migration v20260628010000: link workspace Review monitors to child chat conversations

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "agent_workspace_review_monitors",
        "review_conversation_id",
        "TEXT NULL",
    )?;
    helpers::create_index_if_not_exists(
        conn,
        "idx_agent_workspace_review_monitors_review_conversation",
        "agent_workspace_review_monitors",
        "review_conversation_id",
    )?;
    Ok(())
}
