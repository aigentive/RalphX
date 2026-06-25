// Migration v20260618123000: agent workspace PR review monitoring

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS agent_workspace_pr_review_monitors (
            conversation_id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            pr_number INTEGER NOT NULL,
            status TEXT NOT NULL DEFAULT 'idle',
            monitor_enabled INTEGER NOT NULL DEFAULT 0,
            first_review_completed INTEGER NOT NULL DEFAULT 0,
            last_seen_head_sha TEXT NULL,
            last_reviewed_head_sha TEXT NULL,
            last_review_run_id TEXT NULL,
            last_review_outcome TEXT NULL,
            last_submitted_review_id TEXT NULL,
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
        "CREATE INDEX IF NOT EXISTS idx_agent_workspace_pr_review_monitors_active
         ON agent_workspace_pr_review_monitors(status, monitor_enabled, updated_at)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_workspace_pr_review_monitors_pr
         ON agent_workspace_pr_review_monitors(conversation_id, pr_number)",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS agent_workspace_pr_review_actions (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            pr_number INTEGER NOT NULL,
            head_sha TEXT NOT NULL,
            proposed_action TEXT NOT NULL,
            summary TEXT NOT NULL,
            review_body TEXT NOT NULL,
            findings_json TEXT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            submitted_review_id TEXT NULL,
            created_by_run_id TEXT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            resolved_at TEXT NULL,
            FOREIGN KEY (conversation_id)
                REFERENCES agent_conversation_workspaces(conversation_id)
                ON DELETE CASCADE
        )",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_workspace_pr_review_actions_workspace
         ON agent_workspace_pr_review_actions(conversation_id, pr_number, head_sha)",
        [],
    )?;
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_workspace_pr_review_actions_one_pending_head
         ON agent_workspace_pr_review_actions(conversation_id, pr_number, head_sha)
         WHERE status = 'pending'",
        [],
    )?;

    Ok(())
}
