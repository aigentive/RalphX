// Migration v20260712153932: agent workspace pr review auto approve

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers::add_column_if_not_exists;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    add_column_if_not_exists(
        conn,
        "agent_workspace_pr_review_monitors",
        "auto_approve_enabled",
        "INTEGER NOT NULL DEFAULT 1",
    )
    .map(|_| ())?;
    add_column_if_not_exists(
        conn,
        "agent_workspace_pr_review_monitors",
        "first_action_resolved",
        "INTEGER NOT NULL DEFAULT 0",
    )
    .map(|_| ())?;
    conn.execute(
        "UPDATE agent_workspace_pr_review_monitors
         SET first_action_resolved = 1
         WHERE EXISTS (
             SELECT 1
             FROM agent_workspace_pr_review_actions
             WHERE agent_workspace_pr_review_actions.conversation_id = agent_workspace_pr_review_monitors.conversation_id
               AND agent_workspace_pr_review_actions.status IN ('submitted', 'skipped')
         )",
        [],
    )
    .map(|_| ())?;
    Ok(())
}
