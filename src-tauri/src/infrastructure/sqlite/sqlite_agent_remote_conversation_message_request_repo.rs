use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use tokio::sync::Mutex;

use crate::domain::entities::{
    ChatConversationId, ProjectId, RemoteConversationMessageRequest,
    RemoteConversationMessageStatus,
};
use crate::domain::repositories::RemoteConversationMessageRequestRepository;
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::DbConnection;

#[cfg(test)]
#[path = "sqlite_agent_remote_conversation_message_request_repo_tests.rs"]
mod tests;

const SELECT_COLUMNS: &str = "id, conversation_id, project_id, content, provider, model_override, \
     logical_effort, status, error_code, requested_by_device_id, agent_run_id, claimed_at, \
     created_at, updated_at";

fn parse_rfc3339(value: &str, column_index: usize) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                column_index,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })
}

fn row_to_request(row: &rusqlite::Row<'_>) -> rusqlite::Result<RemoteConversationMessageRequest> {
    let status_text: String = row.get("status")?;
    let status = RemoteConversationMessageStatus::from_str(&status_text).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            7,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
        )
    })?;

    let claimed_at: Option<String> = row.get("claimed_at")?;
    let claimed_at = match claimed_at {
        Some(value) => Some(parse_rfc3339(&value, 11)?),
        None => None,
    };

    let created_at: String = row.get("created_at")?;
    let updated_at: String = row.get("updated_at")?;

    Ok(RemoteConversationMessageRequest {
        id: row.get("id")?,
        conversation_id: ChatConversationId::from_string(row.get::<_, String>("conversation_id")?),
        project_id: ProjectId::from_string(row.get::<_, String>("project_id")?),
        content: row.get("content")?,
        provider: row.get("provider")?,
        model_override: row.get("model_override")?,
        logical_effort: row.get("logical_effort")?,
        status,
        error_code: row.get("error_code")?,
        requested_by_device_id: row.get("requested_by_device_id")?,
        agent_run_id: row.get("agent_run_id")?,
        claimed_at,
        created_at: parse_rfc3339(&created_at, 12)?,
        updated_at: parse_rfc3339(&updated_at, 13)?,
    })
}

pub struct SqliteRemoteConversationMessageRequestRepository {
    db: DbConnection,
}

impl SqliteRemoteConversationMessageRequestRepository {
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

#[async_trait]
impl RemoteConversationMessageRequestRepository
    for SqliteRemoteConversationMessageRequestRepository
{
    async fn create_message_request(
        &self,
        request: RemoteConversationMessageRequest,
    ) -> AppResult<RemoteConversationMessageRequest> {
        let stored = request.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO remote_conversation_message_requests (
                        id, conversation_id, project_id, content, provider, model_override,
                        logical_effort, status, error_code, requested_by_device_id, agent_run_id,
                        claimed_at, created_at, updated_at
                    ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14
                    )",
                    params![
                        request.id,
                        request.conversation_id.as_str(),
                        request.project_id.as_str(),
                        request.content,
                        request.provider,
                        request.model_override,
                        request.logical_effort,
                        request.status.as_db_str(),
                        request.error_code,
                        request.requested_by_device_id,
                        request.agent_run_id,
                        request.claimed_at.map(|value| value.to_rfc3339()),
                        request.created_at.to_rfc3339(),
                        request.updated_at.to_rfc3339(),
                    ],
                )?;
                Ok(())
            })
            .await?;
        Ok(stored)
    }

    async fn get_message_request(
        &self,
        id: &str,
    ) -> AppResult<Option<RemoteConversationMessageRequest>> {
        let id = id.to_string();
        self.db
            .run(move |conn| {
                conn.query_row(
                    &format!(
                        "SELECT {SELECT_COLUMNS} FROM remote_conversation_message_requests \
                         WHERE id = ?1"
                    ),
                    params![id],
                    row_to_request,
                )
                .optional()
                .map_err(AppError::from)
            })
            .await
    }

    async fn claim_pending_message_request(
        &self,
        claimed_at: DateTime<Utc>,
    ) -> AppResult<Option<RemoteConversationMessageRequest>> {
        let claimed_at = claimed_at.to_rfc3339();
        self.db
            .run_transaction(move |conn| {
                // Select the oldest pending row's id under the writer lock.
                let candidate_id: Option<String> = conn
                    .query_row(
                        "SELECT id FROM remote_conversation_message_requests \
                         WHERE status = 'pending' \
                         ORDER BY created_at ASC, id ASC LIMIT 1",
                        [],
                        |row| row.get(0),
                    )
                    .optional()?;

                let Some(candidate_id) = candidate_id else {
                    return Ok(None);
                };

                // Guarded flip: only succeeds if the row is still pending.
                let changed = conn.execute(
                    "UPDATE remote_conversation_message_requests \
                     SET status = 'dispatching', claimed_at = ?1, updated_at = ?1 \
                     WHERE id = ?2 AND status = 'pending'",
                    params![claimed_at, candidate_id],
                )?;
                if changed == 0 {
                    return Ok(None);
                }

                let claimed = conn
                    .query_row(
                        &format!(
                            "SELECT {SELECT_COLUMNS} FROM remote_conversation_message_requests \
                             WHERE id = ?1"
                        ),
                        params![candidate_id],
                        row_to_request,
                    )
                    .optional()?;
                Ok(claimed)
            })
            .await
    }

    async fn complete_message_request(
        &self,
        id: &str,
        agent_run_id: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<()> {
        let id = id.to_string();
        let agent_run_id = agent_run_id.to_string();
        let updated_at = updated_at.to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE remote_conversation_message_requests \
                     SET status = 'dispatched', agent_run_id = ?1, updated_at = ?2 \
                     WHERE id = ?3 AND status = 'dispatching'",
                    params![agent_run_id, updated_at, id],
                )?;
                Ok(())
            })
            .await
    }

    async fn fail_message_request(
        &self,
        id: &str,
        error_code: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<()> {
        let id = id.to_string();
        let error_code = error_code.to_string();
        let updated_at = updated_at.to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE remote_conversation_message_requests \
                     SET status = 'failed', error_code = ?1, updated_at = ?2 \
                     WHERE id = ?3 AND status = 'dispatching'",
                    params![error_code, updated_at, id],
                )?;
                Ok(())
            })
            .await
    }

    async fn cancel_pending_message_requests_for_device(
        &self,
        device_id: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<u64> {
        let device_id = device_id.to_string();
        let updated_at = updated_at.to_rfc3339();
        self.db
            .run(move |conn| {
                let changed = conn.execute(
                    "UPDATE remote_conversation_message_requests \
                     SET status = 'cancelled', updated_at = ?1 \
                     WHERE requested_by_device_id = ?2 AND status = 'pending'",
                    params![updated_at, device_id],
                )?;
                Ok(changed as u64)
            })
            .await
    }

    async fn fail_stale_dispatching_message_requests(
        &self,
        claimed_before: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> AppResult<u64> {
        let claimed_before = claimed_before.to_rfc3339();
        let updated_at = updated_at.to_rfc3339();
        self.db
            .run(move |conn| {
                let changed = conn.execute(
                    "UPDATE remote_conversation_message_requests \
                     SET status = 'failedStale', updated_at = ?1 \
                     WHERE status = 'dispatching' AND claimed_at IS NOT NULL \
                       AND claimed_at < ?2",
                    params![updated_at, claimed_before],
                )?;
                Ok(changed as u64)
            })
            .await
    }
}
