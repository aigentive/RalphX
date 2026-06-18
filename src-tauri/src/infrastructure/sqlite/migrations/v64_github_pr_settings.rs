use crate::error::AppResult;
use crate::infrastructure::sqlite::migrations::helpers::add_column_if_not_exists;
use rusqlite::Connection;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    // Add github_pr_enabled to projects table
    add_column_if_not_exists(
        conn,
        "projects",
        "github_pr_enabled",
        "BOOLEAN NOT NULL DEFAULT 1",
    )?;

    // Add PR columns to plan_branches table
    add_column_if_not_exists(conn, "plan_branches", "pr_number", "INTEGER")?;
    add_column_if_not_exists(conn, "plan_branches", "pr_url", "TEXT")?;
    add_column_if_not_exists(conn, "plan_branches", "pr_status", "TEXT")?;
    add_column_if_not_exists(
        conn,
        "plan_branches",
        "pr_polling_active",
        "BOOLEAN NOT NULL DEFAULT 0",
    )?;
    add_column_if_not_exists(
        conn,
        "plan_branches",
        "pr_eligible",
        "BOOLEAN NOT NULL DEFAULT 0",
    )?; // AD16: existing plans stay push-to-main
    add_column_if_not_exists(conn, "plan_branches", "last_polled_at", "TEXT")?;
    add_column_if_not_exists(
        conn,
        "plan_branches",
        "pr_push_status",
        "TEXT NOT NULL DEFAULT 'pending'",
    )?;
    add_column_if_not_exists(conn, "plan_branches", "merge_commit_sha", "TEXT")?;
    add_column_if_not_exists(conn, "plan_branches", "pr_draft", "BOOLEAN")?;

    Ok(())
}
