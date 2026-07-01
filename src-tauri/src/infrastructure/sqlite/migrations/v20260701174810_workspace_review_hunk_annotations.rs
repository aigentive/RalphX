// Migration v20260701174810: workspace review hunk annotations

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS agent_workspace_review_hunk_annotations (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            project_id TEXT NOT NULL,
            artifact_id TEXT NOT NULL,
            artifact_version INTEGER NOT NULL,
            target_scope TEXT NOT NULL,
            head_sha TEXT,
            diff_fingerprint TEXT NOT NULL,
            path TEXT NOT NULL,
            diff_source TEXT NOT NULL,
            hunk_header TEXT NOT NULL,
            old_start INTEGER NOT NULL,
            old_lines INTEGER NOT NULL,
            new_start INTEGER NOT NULL,
            new_lines INTEGER NOT NULL,
            title TEXT,
            message TEXT NOT NULL,
            level TEXT NOT NULL,
            created_by_run_id TEXT,
            created_at TEXT NOT NULL,
            FOREIGN KEY(conversation_id) REFERENCES agent_conversation_workspaces(conversation_id) ON DELETE CASCADE,
            FOREIGN KEY(artifact_id) REFERENCES artifacts(id) ON DELETE CASCADE
        )",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_workspace_review_hunk_annotations_artifact
         ON agent_workspace_review_hunk_annotations(conversation_id, artifact_id)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_workspace_review_hunk_annotations_current
         ON agent_workspace_review_hunk_annotations(conversation_id, diff_fingerprint, target_scope)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_workspace_review_hunk_annotations_path
         ON agent_workspace_review_hunk_annotations(conversation_id, path)",
        [],
    )?;
    Ok(())
}
