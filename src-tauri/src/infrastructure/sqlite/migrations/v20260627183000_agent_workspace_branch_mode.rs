// Migration v20260627183000: agent workspace branch mode

use rusqlite::Connection;

use super::helpers;
use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "branch_mode",
        "TEXT NOT NULL DEFAULT 'isolated'",
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_conversation_workspaces_project_branch_status
         ON agent_conversation_workspaces(project_id, branch_name, status)",
        [],
    )?;

    Ok(())
}
