use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use tokio::sync::Mutex;

use crate::domain::entities::{
    AgentConversationLinearIssueLink, AgentConversationLinearRefreshStatus, ChatConversationId,
    ChatMessageId, ProjectId,
};
use crate::domain::repositories::AgentConversationLinearIssueRepository;
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::DbConnection;

#[cfg(test)]
#[path = "sqlite_agent_conversation_linear_issue_repo_tests.rs"]
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

fn row_to_link(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentConversationLinearIssueLink> {
    let refresh_status: String = row.get("refresh_status")?;
    let assigned_at: String = row.get("assigned_at")?;
    let created_at: String = row.get("created_at")?;
    let updated_at: String = row.get("updated_at")?;
    let last_refreshed_at = row
        .get::<_, Option<String>>("last_refreshed_at")?
        .map(|value| parse_datetime(&value));

    Ok(AgentConversationLinearIssueLink {
        conversation_id: ChatConversationId::from_string(row.get::<_, String>("conversation_id")?),
        project_id: ProjectId::from_string(row.get::<_, String>("project_id")?),
        provider: row.get("provider")?,
        issue_id: row.get("issue_id")?,
        issue_key: row.get("issue_key")?,
        issue_url: row.get("issue_url")?,
        title: row.get("title")?,
        status: row.get("status")?,
        assignee: row.get("assignee")?,
        reporter: row.get("reporter")?,
        updated_at_remote: row.get("updated_at_remote")?,
        description_markdown: row.get("description_markdown")?,
        description_text: row.get("description_text")?,
        comments_json: row.get("comments_json")?,
        attachments_json: row.get("attachments_json")?,
        last_refreshed_at,
        refresh_status: AgentConversationLinearRefreshStatus::from_str(&refresh_status)
            .unwrap_or(AgentConversationLinearRefreshStatus::NotLoaded),
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

pub struct SqliteAgentConversationLinearIssueRepository {
    db: DbConnection,
}

impl SqliteAgentConversationLinearIssueRepository {
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
    link: &AgentConversationLinearIssueLink,
    updated_at: &str,
) -> rusqlite::Result<usize> {
    statement.execute(params![
        link.conversation_id.as_str(),
        link.project_id.as_str(),
        &link.provider,
        &link.issue_id,
        link.issue_key.as_deref(),
        link.issue_url.as_deref(),
        link.title.as_deref(),
        link.status.as_deref(),
        link.assignee.as_deref(),
        link.reporter.as_deref(),
        link.updated_at_remote.as_deref(),
        link.description_markdown.as_deref(),
        link.description_text.as_deref(),
        &link.comments_json,
        &link.attachments_json,
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
impl AgentConversationLinearIssueRepository for SqliteAgentConversationLinearIssueRepository {
    async fn get_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentConversationLinearIssueLink>> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.query_row(
                    "SELECT * FROM agent_conversation_linear_issue_links
                     WHERE conversation_id = ?1",
                    params![conversation_id],
                    row_to_link,
                )
                .optional()
                .map_err(AppError::from)
            })
            .await
    }

    async fn upsert(
        &self,
        link: AgentConversationLinearIssueLink,
    ) -> AppResult<AgentConversationLinearIssueLink> {
        let fetch_id = link.conversation_id.clone();
        self.db
            .run(move |conn| {
                let updated_at = Utc::now().to_rfc3339();
                let mut statement = conn.prepare(
                    "INSERT INTO agent_conversation_linear_issue_links (
                        conversation_id, project_id, provider, issue_id, issue_key, issue_url,
                        title, status, assignee, reporter, updated_at_remote,
                        description_markdown, description_text,
                        comments_json, attachments_json, last_refreshed_at, refresh_status,
                        refresh_error, assigned_at, assigned_from_message_id, manually_assigned,
                        created_at, updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)
                    ON CONFLICT(conversation_id) DO UPDATE SET
                        project_id=excluded.project_id,
                        provider=excluded.provider,
                        issue_id=excluded.issue_id,
                        issue_key=excluded.issue_key,
                        issue_url=excluded.issue_url,
                        title=excluded.title,
                        status=excluded.status,
                        assignee=excluded.assignee,
                        reporter=excluded.reporter,
                        updated_at_remote=excluded.updated_at_remote,
                        description_markdown=excluded.description_markdown,
                        description_text=excluded.description_text,
                        comments_json=excluded.comments_json,
                        attachments_json=excluded.attachments_json,
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
                    "SELECT * FROM agent_conversation_linear_issue_links WHERE conversation_id = ?1",
                    params![fetch_id.as_str()],
                    row_to_link,
                )
                .map_err(AppError::from)
            })
            .await
    }

    async fn insert_if_absent(
        &self,
        link: AgentConversationLinearIssueLink,
    ) -> AppResult<AgentConversationLinearIssueLink> {
        let fetch_id = link.conversation_id.clone();
        self.db
            .run(move |conn| {
                let updated_at = link.updated_at.to_rfc3339();
                let mut statement = conn.prepare(
                    "INSERT OR IGNORE INTO agent_conversation_linear_issue_links (
                        conversation_id, project_id, provider, issue_id, issue_key, issue_url,
                        title, status, assignee, reporter, updated_at_remote,
                        description_markdown, description_text,
                        comments_json, attachments_json, last_refreshed_at, refresh_status,
                        refresh_error, assigned_at, assigned_from_message_id, manually_assigned,
                        created_at, updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)",
                )?;
                bind_link(&mut statement, &link, &updated_at)?;
                conn.query_row(
                    "SELECT * FROM agent_conversation_linear_issue_links WHERE conversation_id = ?1",
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
                    "DELETE FROM agent_conversation_linear_issue_links WHERE conversation_id = ?1",
                    params![conversation_id],
                )?;
                Ok(())
            })
            .await
    }
}
