// Migration v20260706113000: Agent conversation issue identity and occurrences

use rusqlite::Connection;

use crate::error::AppResult;
use crate::infrastructure::sqlite::migrations::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_issues",
        "canonical_fingerprint",
        "TEXT NULL",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_issues",
        "canonical_scope_kind",
        "TEXT NULL",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_issues",
        "canonical_scope_subject",
        "TEXT NULL",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_issues",
        "canonical_family",
        "TEXT NULL",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_issues",
        "superseded_by_issue_id",
        "TEXT NULL",
    )?;

    conn.execute(
        "UPDATE agent_conversation_issues
         SET canonical_fingerprint = blocker_fingerprint
         WHERE canonical_fingerprint IS NULL
           AND blocker_fingerprint IS NOT NULL",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS agent_conversation_issue_occurrences (
            id TEXT PRIMARY KEY,
            issue_id TEXT NOT NULL,
            project_id TEXT NOT NULL,
            conversation_id TEXT NOT NULL,
            source_task_id TEXT NULL,
            source_context_type TEXT NULL,
            source_context_id TEXT NULL,
            source_agent_name TEXT NULL,
            issue_kind TEXT NOT NULL,
            severity TEXT NOT NULL,
            blocking_scope TEXT NOT NULL,
            title TEXT NOT NULL,
            summary TEXT NOT NULL,
            evidence TEXT NULL,
            recommendation TEXT NULL,
            raw_blocker_fingerprint TEXT NULL,
            canonical_fingerprint TEXT NULL,
            dedupe_decision TEXT NULL,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        )",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_conversation_issues_canonical
         ON agent_conversation_issues(conversation_id, canonical_fingerprint, status, updated_at)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_conversation_issues_identity_candidates
         ON agent_conversation_issues(
            conversation_id,
            canonical_scope_kind,
            canonical_scope_subject,
            canonical_family,
            status,
            updated_at
         )",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_conversation_issue_occurrences_issue
         ON agent_conversation_issue_occurrences(issue_id, created_at)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_conversation_issue_occurrences_conversation
         ON agent_conversation_issue_occurrences(conversation_id, created_at)",
        [],
    )?;

    Ok(())
}
