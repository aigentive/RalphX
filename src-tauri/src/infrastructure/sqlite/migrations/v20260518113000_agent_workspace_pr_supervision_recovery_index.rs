// Migration v20260518113000: index blocked agent workspace PR supervision recovery candidates

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_workspaces_pr_supervision_recovery
         ON agent_conversation_workspaces(
             status,
             mode,
             publication_push_status,
             pr_supervision_status,
             updated_at DESC
         )",
        [],
    )?;

    Ok(())
}
