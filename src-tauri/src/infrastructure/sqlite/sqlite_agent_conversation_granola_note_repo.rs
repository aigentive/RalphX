use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use tokio::sync::Mutex;

use crate::domain::entities::{
    AgentConversationGranolaNoteLink, AgentConversationGranolaRefreshStatus, ChatConversationId,
    ChatMessageId, ProjectId,
};
use crate::domain::repositories::AgentConversationGranolaNoteRepository;
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::DbConnection;

#[cfg(test)]
#[path = "sqlite_agent_conversation_granola_note_repo_tests.rs"]
mod tests;

fn parse_datetime(value: &str) -> DateTime<Utc> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
        return dt.with_timezone(&Utc);
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S") {
        return Utc.from_utc_datetime(&dt);
    }
    Utc::now()
}

fn row_to_link(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentConversationGranolaNoteLink> {
    let refresh_status: String = row.get("refresh_status")?;
    let assigned_at: String = row.get("assigned_at")?;
    let created_at: String = row.get("created_at")?;
    let updated_at: String = row.get("updated_at")?;
    let last_refreshed_at = row
        .get::<_, Option<String>>("last_refreshed_at")?
        .map(|value| parse_datetime(&value));

    Ok(AgentConversationGranolaNoteLink {
        conversation_id: ChatConversationId::from_string(row.get::<_, String>("conversation_id")?),
        project_id: ProjectId::from_string(row.get::<_, String>("project_id")?),
        provider: row.get("provider")?,
        note_id: row.get("note_id")?,
        note_url: row.get("note_url")?,
        title: row.get("title")?,
        summary_markdown: row.get("summary_markdown")?,
        transcript_json: row.get("transcript_json")?,
        include_transcript: row.get("include_transcript")?,
        last_refreshed_at,
        refresh_status: AgentConversationGranolaRefreshStatus::from_str(&refresh_status)
            .unwrap_or(AgentConversationGranolaRefreshStatus::NotLoaded),
        refresh_error: row.get("refresh_error")?,
        assigned_at: parse_datetime(&assigned_at),
        assigned_from_message_id: row
            .get::<_, Option<String>>("assigned_from_message_id")?
            .map(ChatMessageId::from_string),
        manually_assigned: row.get("manually_assigned")?,
        created_at: parse_datetime(&created_at),
        updated_at: parse_datetime(&updated_at),
    })
}

pub struct SqliteAgentConversationGranolaNoteRepository {
    db: DbConnection,
}

impl SqliteAgentConversationGranolaNoteRepository {
    pub fn new(conn: Connection) -> Self {
        Self {
            db: DbConnection::new(conn),
        }
    }

    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }
}

fn bind_link(
    statement: &mut rusqlite::Statement<'_>,
    link: &AgentConversationGranolaNoteLink,
    updated_at: &str,
) -> rusqlite::Result<usize> {
    statement.execute(params![
        link.conversation_id.as_str(),
        link.project_id.as_str(),
        &link.provider,
        &link.note_id,
        link.note_url.as_deref(),
        link.title.as_deref(),
        link.summary_markdown.as_deref(),
        &link.transcript_json,
        link.include_transcript,
        link.last_refreshed_at.as_ref().map(DateTime::to_rfc3339),
        link.refresh_status.to_string(),
        link.refresh_error.as_deref(),
        link.assigned_at.to_rfc3339(),
        link.assigned_from_message_id.as_ref().map(|id| id.as_str()),
        link.manually_assigned,
        link.created_at.to_rfc3339(),
        updated_at,
    ])
}

#[async_trait]
impl AgentConversationGranolaNoteRepository for SqliteAgentConversationGranolaNoteRepository {
    async fn get_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentConversationGranolaNoteLink>> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.query_row(
                    "SELECT * FROM agent_conversation_granola_note_links
                     WHERE conversation_id = ?1",
                    params![conversation_id],
                    row_to_link,
                )
                .optional()
                .map_err(AppError::from)
            })
            .await
    }

    async fn list_by_project_id(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<AgentConversationGranolaNoteLink>> {
        let project_id = project_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut statement = conn.prepare(
                    "SELECT * FROM agent_conversation_granola_note_links
                     WHERE project_id = ?1
                     ORDER BY updated_at DESC",
                )?;
                let rows = statement
                    .query_map(params![project_id], row_to_link)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
    }

    async fn upsert(
        &self,
        link: AgentConversationGranolaNoteLink,
    ) -> AppResult<AgentConversationGranolaNoteLink> {
        let fetch_id = link.conversation_id.clone();
        self.db
            .run(move |conn| {
                let updated_at = Utc::now().to_rfc3339();
                let mut statement = conn.prepare(
                    "INSERT INTO agent_conversation_granola_note_links (
                        conversation_id, project_id, provider, note_id, note_url,
                        title, summary_markdown, transcript_json, include_transcript,
                        last_refreshed_at, refresh_status, refresh_error, assigned_at,
                        assigned_from_message_id, manually_assigned, created_at, updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
                    ON CONFLICT(conversation_id) DO UPDATE SET
                        project_id=excluded.project_id,
                        provider=excluded.provider,
                        note_id=excluded.note_id,
                        note_url=excluded.note_url,
                        title=excluded.title,
                        summary_markdown=excluded.summary_markdown,
                        transcript_json=excluded.transcript_json,
                        include_transcript=excluded.include_transcript,
                        last_refreshed_at=excluded.last_refreshed_at,
                        refresh_status=excluded.refresh_status,
                        refresh_error=excluded.refresh_error,
                        assigned_at=excluded.assigned_at,
                        assigned_from_message_id=excluded.assigned_from_message_id,
                        manually_assigned=excluded.manually_assigned,
                        updated_at=excluded.updated_at",
                )?;
                bind_link(&mut statement, &link, &updated_at)?;
                conn.query_row(
                    "SELECT * FROM agent_conversation_granola_note_links WHERE conversation_id = ?1",
                    params![fetch_id.as_str()],
                    row_to_link,
                )
                .map_err(AppError::from)
            })
            .await
    }

    async fn insert_if_absent(
        &self,
        link: AgentConversationGranolaNoteLink,
    ) -> AppResult<AgentConversationGranolaNoteLink> {
        let fetch_id = link.conversation_id.clone();
        self.db
            .run(move |conn| {
                let updated_at = link.updated_at.to_rfc3339();
                let mut statement = conn.prepare(
                    "INSERT OR IGNORE INTO agent_conversation_granola_note_links (
                        conversation_id, project_id, provider, note_id, note_url,
                        title, summary_markdown, transcript_json, include_transcript,
                        last_refreshed_at, refresh_status, refresh_error, assigned_at,
                        assigned_from_message_id, manually_assigned, created_at, updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
                )?;
                bind_link(&mut statement, &link, &updated_at)?;
                conn.query_row(
                    "SELECT * FROM agent_conversation_granola_note_links WHERE conversation_id = ?1",
                    params![fetch_id.as_str()],
                    row_to_link,
                )
                .map_err(AppError::from)
            })
            .await
    }

    async fn clear(&self, conversation_id: &ChatConversationId) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM agent_conversation_granola_note_links WHERE conversation_id = ?1",
                    params![conversation_id],
                )?;
                Ok(())
            })
            .await
    }
}
