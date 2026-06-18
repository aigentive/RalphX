// Migration v20260618181405: agent conversation linear issue links

use rusqlite::{params, Connection};

use crate::domain::services::primary_linear_issue_from_composer_metadata;
use crate::error::{AppError, AppResult};

use super::helpers::{column_exists, table_exists};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS agent_conversation_linear_issue_links (
            conversation_id TEXT PRIMARY KEY REFERENCES chat_conversations(id) ON DELETE CASCADE,
            project_id TEXT NOT NULL,
            provider TEXT NOT NULL DEFAULT 'linear',
            issue_id TEXT NOT NULL,
            issue_key TEXT,
            issue_url TEXT,
            title TEXT,
            status TEXT,
            assignee TEXT,
            reporter TEXT,
            updated_at_remote TEXT,
            description_markdown TEXT,
            description_text TEXT,
            comments_json TEXT NOT NULL DEFAULT '[]',
            attachments_json TEXT NOT NULL DEFAULT '[]',
            last_refreshed_at TEXT,
            refresh_status TEXT NOT NULL DEFAULT 'not_loaded'
                CHECK(refresh_status IN ('not_loaded', 'loaded', 'error')),
            refresh_error TEXT,
            assigned_at TEXT NOT NULL,
            assigned_from_message_id TEXT REFERENCES chat_messages(id) ON DELETE SET NULL,
            manually_assigned INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_agent_conversation_linear_issue_links_project_id
            ON agent_conversation_linear_issue_links(project_id);
        CREATE INDEX IF NOT EXISTS idx_agent_conversation_linear_issue_links_project_key
            ON agent_conversation_linear_issue_links(project_id, issue_key);
        CREATE INDEX IF NOT EXISTS idx_agent_conversation_linear_issue_links_conversation
            ON agent_conversation_linear_issue_links(conversation_id);",
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    backfill_from_message_metadata(conn)
}

fn backfill_from_message_metadata(conn: &Connection) -> AppResult<()> {
    if !table_exists(conn, "chat_conversations")
        || !table_exists(conn, "chat_messages")
        || !column_exists(conn, "chat_conversations", "agent_mode")
    {
        return Ok(());
    }

    let has_workspace_table = table_exists(conn, "agent_conversation_workspaces");
    let candidate_sql = if has_workspace_table {
        "SELECT c.id,
                COALESCE(
                    w.project_id,
                    CASE WHEN c.context_type = 'project' THEN c.context_id ELSE NULL END
                ) AS project_id
         FROM chat_conversations c
         LEFT JOIN agent_conversation_workspaces w ON w.conversation_id = c.id
         WHERE (c.agent_mode IS NOT NULL OR w.conversation_id IS NOT NULL)
           AND COALESCE(
                w.project_id,
                CASE WHEN c.context_type = 'project' THEN c.context_id ELSE NULL END
           ) IS NOT NULL
           AND NOT EXISTS (
                SELECT 1 FROM agent_conversation_linear_issue_links link
                WHERE link.conversation_id = c.id
           )
         ORDER BY c.created_at ASC, c.rowid ASC"
    } else {
        "SELECT c.id,
                CASE WHEN c.context_type = 'project' THEN c.context_id ELSE NULL END AS project_id
         FROM chat_conversations c
         WHERE c.agent_mode IS NOT NULL
           AND c.context_type = 'project'
           AND NOT EXISTS (
                SELECT 1 FROM agent_conversation_linear_issue_links link
                WHERE link.conversation_id = c.id
           )
         ORDER BY c.created_at ASC, c.rowid ASC"
    };

    let candidates = {
        let mut statement = conn.prepare(candidate_sql)?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };

    for (conversation_id, project_id) in candidates {
        backfill_conversation(conn, &conversation_id, &project_id)?;
    }

    Ok(())
}

fn backfill_conversation(
    conn: &Connection,
    conversation_id: &str,
    project_id: &str,
) -> AppResult<()> {
    let messages = {
        let mut statement = conn.prepare(
            "SELECT id, metadata, created_at
             FROM chat_messages
             WHERE conversation_id = ?1
               AND role = 'user'
               AND metadata IS NOT NULL
             ORDER BY created_at ASC, rowid ASC",
        )?;
        let rows = statement
            .query_map(params![conversation_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };

    for (message_id, metadata, created_at) in messages {
        let Some((issue_id, issue_key, issue_url)) =
            primary_linear_issue_from_composer_metadata(metadata.as_deref())
        else {
            continue;
        };
        let assigned_at = created_at.unwrap_or_else(|| {
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        });
        conn.execute(
            "INSERT OR IGNORE INTO agent_conversation_linear_issue_links (
                conversation_id, project_id, provider, issue_id, issue_key, issue_url,
                comments_json, attachments_json, refresh_status, assigned_at,
                assigned_from_message_id, manually_assigned, created_at, updated_at
             ) VALUES (?1, ?2, 'linear', ?3, ?4, ?5, '[]', '[]', 'not_loaded', ?6, ?7, 0, ?6, ?6)",
            params![
                conversation_id,
                project_id,
                issue_id,
                issue_key,
                issue_url,
                assigned_at,
                message_id,
            ],
        )?;
        break;
    }

    Ok(())
}
