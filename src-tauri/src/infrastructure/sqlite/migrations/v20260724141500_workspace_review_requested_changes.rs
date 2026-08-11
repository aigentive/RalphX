use rusqlite::Connection;

use crate::{
    error::AppResult, infrastructure::sqlite::migrations::helpers::add_column_if_not_exists,
};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    for (column, definition) in [
        ("review_requested_changes_artifact_id", "TEXT NULL"),
        ("review_requested_changes_artifact_version", "INTEGER NULL"),
        ("review_requested_changes_artifact_updated_at", "TEXT NULL"),
        ("review_requested_changes_previous_version_id", "TEXT NULL"),
    ] {
        add_column_if_not_exists(conn, "agent_workspace_review_monitors", column, definition)?;
    }
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_workspace_review_monitors_requested_changes
         ON agent_workspace_review_monitors(review_requested_changes_artifact_id)",
        [],
    )?;
    Ok(())
}
