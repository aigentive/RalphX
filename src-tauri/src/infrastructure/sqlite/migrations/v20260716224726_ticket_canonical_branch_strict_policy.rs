// Migration v20260716224726: ticket canonical branch strict policy

use rusqlite::Connection;

use crate::error::AppResult;
use crate::infrastructure::sqlite::migrations::helpers::add_column_if_not_exists;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    add_column_if_not_exists(
        conn,
        "ticket_canonical_branches",
        "policy_kind",
        "TEXT NOT NULL DEFAULT 'legacy_canonical_base'",
    )?;
    add_column_if_not_exists(
        conn,
        "ticket_canonical_branches",
        "policy_version",
        "INTEGER",
    )?;
    add_column_if_not_exists(
        conn,
        "ticket_canonical_branches",
        "task_title_snapshot",
        "TEXT",
    )?;
    add_column_if_not_exists(
        conn,
        "ticket_canonical_branches",
        "clickup_username_snapshot",
        "TEXT",
    )?;
    add_column_if_not_exists(
        conn,
        "ticket_canonical_branches",
        "commit_subject_rule",
        "TEXT",
    )?;
    add_column_if_not_exists(
        conn,
        "ticket_canonical_branches",
        "pr_title_snapshot",
        "TEXT",
    )?;
    add_column_if_not_exists(
        conn,
        "ticket_canonical_branches",
        "cycle_generation",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column_if_not_exists(
        conn,
        "ticket_canonical_branches",
        "cycle_state",
        "TEXT NOT NULL DEFAULT 'legacy'",
    )?;
    add_column_if_not_exists(
        conn,
        "ticket_canonical_branches",
        "cycle_base_commit",
        "TEXT",
    )?;
    add_column_if_not_exists(
        conn,
        "ticket_canonical_branches",
        "cycle_effective_merge_base",
        "TEXT",
    )?;
    add_column_if_not_exists(
        conn,
        "ticket_canonical_branches",
        "cycle_started_at",
        "TEXT",
    )?;
    add_column_if_not_exists(
        conn,
        "ticket_canonical_branches",
        "cycle_terminal_at",
        "TEXT",
    )?;
    conn.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_ticket_canonical_branches_project_branch
             ON ticket_canonical_branches(project_id, branch_name);",
    )?;
    Ok(())
}
