// Migration v20260619093000: attach versioned review artifacts to PR review monitors

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute(
        "ALTER TABLE agent_workspace_pr_review_monitors
         ADD COLUMN review_artifact_id TEXT NULL",
        [],
    )
    .or_else(ignore_duplicate_column)?;

    conn.execute(
        "ALTER TABLE agent_workspace_pr_review_monitors
         ADD COLUMN review_artifact_version INTEGER NULL",
        [],
    )
    .or_else(ignore_duplicate_column)?;

    conn.execute(
        "ALTER TABLE agent_workspace_pr_review_monitors
         ADD COLUMN review_artifact_head_sha TEXT NULL",
        [],
    )
    .or_else(ignore_duplicate_column)?;

    conn.execute(
        "ALTER TABLE agent_workspace_pr_review_monitors
         ADD COLUMN review_artifact_updated_at TEXT NULL",
        [],
    )
    .or_else(ignore_duplicate_column)?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_workspace_pr_review_monitors_artifact
         ON agent_workspace_pr_review_monitors(review_artifact_id)",
        [],
    )?;

    Ok(())
}

fn ignore_duplicate_column(error: rusqlite::Error) -> Result<usize, rusqlite::Error> {
    match &error {
        rusqlite::Error::SqliteFailure(_, Some(message))
            if message.contains("duplicate column name") =>
        {
            Ok(0)
        }
        _ => Err(error),
    }
}
