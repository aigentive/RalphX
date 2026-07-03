// Migration v20260625153000: Agent conversation issues and autonomy policy

use rusqlite::Connection;

use crate::error::AppResult;
use crate::infrastructure::sqlite::migrations::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "review_settings",
        "auto_create_followup_agent_conversation",
        "INTEGER NOT NULL DEFAULT 1",
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS agent_conversation_issues (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            conversation_id TEXT NOT NULL,
            source_task_id TEXT NULL,
            source_context_type TEXT NULL,
            source_context_id TEXT NULL,
            source_agent_name TEXT NULL,
            issue_kind TEXT NOT NULL,
            severity TEXT NOT NULL DEFAULT 'medium',
            status TEXT NOT NULL DEFAULT 'open',
            blocking_scope TEXT NOT NULL DEFAULT 'none',
            title TEXT NOT NULL,
            summary TEXT NOT NULL,
            evidence TEXT NULL,
            recommendation TEXT NULL,
            blocker_fingerprint TEXT NULL,
            followup_title TEXT NULL,
            followup_prompt TEXT NULL,
            auto_followup_eligible INTEGER NOT NULL DEFAULT 0,
            linked_followup_conversation_id TEXT NULL,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            resolved_at TEXT NULL
        )",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_conversation_issues_conversation_status
         ON agent_conversation_issues(conversation_id, status, updated_at)",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_conversation_issues_project_status
         ON agent_conversation_issues(project_id, status, updated_at)",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_conversation_issues_fingerprint
         ON agent_conversation_issues(
            conversation_id,
            source_task_id,
            issue_kind,
            blocker_fingerprint,
            status,
            updated_at
         )",
        [],
    )?;

    Ok(())
}
