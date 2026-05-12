// Migration v20260512093000: startup local cleanup markers

use rusqlite::Connection;

use super::helpers;
use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(conn, "plan_branches", "local_cleanup_status", "TEXT NULL")?;
    helpers::add_column_if_not_exists(
        conn,
        "plan_branches",
        "local_cleanup_checked_at",
        "TEXT NULL",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "local_cleanup_status",
        "TEXT NULL",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "local_cleanup_checked_at",
        "TEXT NULL",
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_plan_branches_project_cleanup
         ON plan_branches(project_id, local_cleanup_status)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_workspaces_project_cleanup
         ON agent_conversation_workspaces(project_id, local_cleanup_status)",
        [],
    )?;

    Ok(())
}
