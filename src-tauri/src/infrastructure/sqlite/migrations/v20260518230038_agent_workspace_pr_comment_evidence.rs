// Migration v20260518230038: agent workspace pr comment evidence

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS agent_workspace_pr_comment_evidence (
            conversation_id TEXT NOT NULL,
            pr_number INTEGER NOT NULL,
            comment_id TEXT NOT NULL,
            author TEXT NULL,
            body TEXT NOT NULL,
            body_excerpt TEXT NOT NULL,
            body_sha256 TEXT NOT NULL,
            url TEXT NULL,
            github_created_at TEXT NULL,
            github_updated_at TEXT NULL,
            is_codecov INTEGER NOT NULL DEFAULT 0,
            is_bot INTEGER NOT NULL DEFAULT 0,
            first_seen_at TEXT NOT NULL,
            last_seen_at TEXT NOT NULL,
            last_included_at TEXT NULL,
            last_read_at TEXT NULL,
            edit_count INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (conversation_id, pr_number, comment_id),
            FOREIGN KEY (conversation_id)
                REFERENCES agent_conversation_workspaces(conversation_id)
                ON DELETE CASCADE
        )",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_workspace_pr_comment_evidence_workspace_pr
         ON agent_workspace_pr_comment_evidence(conversation_id, pr_number)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_workspace_pr_comment_evidence_seen
         ON agent_workspace_pr_comment_evidence(conversation_id, pr_number, last_seen_at DESC)",
        [],
    )?;

    Ok(())
}
