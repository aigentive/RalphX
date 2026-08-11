// Migration v20260716202015: workspace review bypass and bound agent

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers::add_column_if_not_exists;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    add_column_if_not_exists(
        conn,
        "agent_workspace_review_monitors",
        "review_gate_bypassed_at",
        "TEXT NULL",
    )?;
    add_column_if_not_exists(
        conn,
        "agent_workspace_review_monitors",
        "review_gate_bypassed_target_scope",
        "TEXT NULL",
    )?;
    add_column_if_not_exists(
        conn,
        "agent_workspace_review_monitors",
        "review_gate_bypassed_diff_fingerprint",
        "TEXT NULL",
    )?;
    add_column_if_not_exists(
        conn,
        "agent_workspace_review_monitors",
        "review_gate_bypassed_artifact_id",
        "TEXT NULL",
    )?;
    add_column_if_not_exists(
        conn,
        "agent_workspace_review_monitors",
        "review_gate_bypassed_artifact_version",
        "INTEGER NULL",
    )?;
    add_column_if_not_exists(conn, "chat_conversations", "bound_agent_name", "TEXT NULL")?;
    Ok(())
}
