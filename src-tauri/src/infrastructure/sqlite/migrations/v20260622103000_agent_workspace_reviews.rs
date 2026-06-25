// Migration v20260622103000: general agent workspace review monitor

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS agent_workspace_review_monitors (
            conversation_id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'idle',
            current_target_scope TEXT NULL,
            reviewed_target_scope TEXT NULL,
            review_artifact_id TEXT NULL,
            review_artifact_version INTEGER NULL,
            review_artifact_updated_at TEXT NULL,
            reviewed_head_sha TEXT NULL,
            reviewed_diff_fingerprint TEXT NULL,
            selected_source_base_ref TEXT NULL,
            selected_source_base_sha TEXT NULL,
            selected_source_head_ref TEXT NULL,
            selected_source_head_sha TEXT NULL,
            selected_source_pull_request_number INTEGER NULL,
            workspace_base_ref TEXT NULL,
            workspace_base_sha TEXT NULL,
            workspace_head_ref TEXT NULL,
            workspace_head_sha TEXT NULL,
            current_diff_fingerprint TEXT NULL,
            previous_version_id TEXT NULL,
            last_run_id TEXT NULL,
            last_error TEXT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY (conversation_id)
                REFERENCES agent_conversation_workspaces(conversation_id)
                ON DELETE CASCADE
        )",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_workspace_review_monitors_status
         ON agent_workspace_review_monitors(status, updated_at)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_workspace_review_monitors_artifact
         ON agent_workspace_review_monitors(review_artifact_id)",
        [],
    )?;

    Ok(())
}
