// Migration v20260714184430: workspace review auto merge guard

use rusqlite::Connection;

use crate::{
    error::AppResult,
    infrastructure::sqlite::migrations::helpers::{
        add_column_if_not_exists, create_index_if_not_exists,
    },
};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    add_column_if_not_exists(
        conn,
        "agent_workspace_review_monitors",
        "auto_merge_guard_status",
        "TEXT NULL",
    )?;
    add_column_if_not_exists(
        conn,
        "agent_workspace_review_monitors",
        "auto_merge_guard_pr_number",
        "INTEGER NULL",
    )?;
    add_column_if_not_exists(
        conn,
        "agent_workspace_review_monitors",
        "auto_merge_guard_method",
        "TEXT NULL",
    )?;
    add_column_if_not_exists(
        conn,
        "agent_workspace_review_monitors",
        "auto_merge_guard_target_scope",
        "TEXT NULL",
    )?;
    add_column_if_not_exists(
        conn,
        "agent_workspace_review_monitors",
        "auto_merge_guard_diff_fingerprint",
        "TEXT NULL",
    )?;
    add_column_if_not_exists(
        conn,
        "agent_workspace_review_monitors",
        "auto_merge_guard_head_sha",
        "TEXT NULL",
    )?;
    add_column_if_not_exists(
        conn,
        "agent_workspace_review_monitors",
        "auto_merge_guard_last_error",
        "TEXT NULL",
    )?;
    create_index_if_not_exists(
        conn,
        "idx_agent_workspace_review_monitors_auto_merge_guard",
        "agent_workspace_review_monitors",
        "auto_merge_guard_status, updated_at",
    )?;
    Ok(())
}
