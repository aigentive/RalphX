// Migration v20260629101000: first-class local workspace Review gate state

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "agent_workspace_review_monitors",
        "review_outcome",
        "TEXT NOT NULL DEFAULT 'none'",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_workspace_review_monitors",
        "review_gate_status",
        "TEXT NOT NULL DEFAULT 'not_required'",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_workspace_review_monitors",
        "review_blocking_summary",
        "TEXT NULL",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_workspace_review_monitors",
        "review_blocking_fingerprint",
        "TEXT NULL",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_workspace_review_monitors",
        "review_fixer_run_id",
        "TEXT NULL",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_workspace_review_monitors",
        "review_fixer_conversation_id",
        "TEXT NULL",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_workspace_review_monitors",
        "review_fixer_status",
        "TEXT NULL",
    )?;
    helpers::create_index_if_not_exists(
        conn,
        "idx_agent_workspace_review_monitors_gate",
        "agent_workspace_review_monitors",
        "review_gate_status, updated_at",
    )?;
    helpers::create_index_if_not_exists(
        conn,
        "idx_agent_workspace_review_monitors_blocking_fingerprint",
        "agent_workspace_review_monitors",
        "review_blocking_fingerprint",
    )?;
    Ok(())
}
