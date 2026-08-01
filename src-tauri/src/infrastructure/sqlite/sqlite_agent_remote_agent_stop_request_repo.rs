use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use tokio::sync::Mutex;

use crate::domain::entities::{ChatConversationId, RemoteAgentStopRequest, RemoteAgentStopStatus};
use crate::domain::repositories::RemoteAgentStopRequestRepository;
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::DbConnection;

#[cfg(test)]
#[path = "sqlite_agent_remote_agent_stop_request_repo_tests.rs"]
mod tests;

const SELECT_COLUMNS: &str = "id, conversation_id, status, error_code, requested_by_device_id, \
     claimed_at, created_at, updated_at";

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

fn row_to_request(row: &rusqlite::Row<'_>) -> rusqlite::Result<RemoteAgentStopRequest> {
    let status_text: String = row.get("status")?;
    let status = RemoteAgentStopStatus::from_str(&status_text).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            2,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
        )
    })?;

    let claimed_at: Option<String> = row.get("claimed_at")?;
    let claimed_at = match claimed_at {
        Some(value) => Some(parse_rfc3339(&value, 5)?),
        None => None,
    };

    let created_at: String = row.get("created_at")?;
    let updated_at: String = row.get("updated_at")?;

    Ok(RemoteAgentStopRequest {
        id: row.get("id")?,
        conversation_id: ChatConversationId::from_string(row.get::<_, String>("conversation_id")?),
        status,
        error_code: row.get("error_code")?,
        requested_by_device_id: row.get("requested_by_device_id")?,
        claimed_at,
        created_at: parse_rfc3339(&created_at, 6)?,
        updated_at: parse_rfc3339(&updated_at, 7)?,
    })
}

pub struct SqliteRemoteAgentStopRequestRepository {
    db: DbConnection,
}

impl SqliteRemoteAgentStopRequestRepository {
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
impl RemoteAgentStopRequestRepository for SqliteRemoteAgentStopRequestRepository {
    async fn create_stop_request(
        &self,
        request: RemoteAgentStopRequest,
    ) -> AppResult<RemoteAgentStopRequest> {
        let stored = request.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO remote_agent_stop_requests (
                        id, conversation_id, status, error_code, requested_by_device_id,
                        claimed_at, created_at, updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        request.id,
                        request.conversation_id.as_str(),
                        request.status.as_db_str(),
                        request.error_code,
                        request.requested_by_device_id,
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

    async fn get_stop_request(&self, id: &str) -> AppResult<Option<RemoteAgentStopRequest>> {
        let id = id.to_string();
        self.db
            .run(move |conn| {
                conn.query_row(
                    &format!(
                        "SELECT {SELECT_COLUMNS} FROM remote_agent_stop_requests WHERE id = ?1"
                    ),
                    params![id],
                    row_to_request,
                )
                .optional()
                .map_err(AppError::from)
            })
            .await
    }

    async fn find_unsettled_stop_request_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<RemoteAgentStopRequest>> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.query_row(
                    &format!(
                        "SELECT {SELECT_COLUMNS} FROM remote_agent_stop_requests \
                         WHERE conversation_id = ?1 AND status IN ('pending', 'stopping') \
                         ORDER BY created_at ASC, id ASC LIMIT 1"
                    ),
                    params![conversation_id],
                    row_to_request,
                )
                .optional()
                .map_err(AppError::from)
            })
            .await
    }

    async fn claim_pending_stop_request(
        &self,
        claimed_at: DateTime<Utc>,
    ) -> AppResult<Option<RemoteAgentStopRequest>> {
        let claimed_at = claimed_at.to_rfc3339();
        self.db
            .run_transaction(move |conn| {
                let candidate_id: Option<String> = conn
                    .query_row(
                        "SELECT id FROM remote_agent_stop_requests WHERE status = 'pending' \
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
                    "UPDATE remote_agent_stop_requests \
                     SET status = 'stopping', claimed_at = ?1, updated_at = ?1 \
                     WHERE id = ?2 AND status = 'pending'",
                    params![claimed_at, candidate_id],
                )?;
                if changed == 0 {
                    return Ok(None);
                }

                let claimed = conn
                    .query_row(
                        &format!(
                            "SELECT {SELECT_COLUMNS} FROM remote_agent_stop_requests \
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

    async fn complete_stop_request(&self, id: &str, updated_at: DateTime<Utc>) -> AppResult<()> {
        let id = id.to_string();
        let updated_at = updated_at.to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE remote_agent_stop_requests SET status = 'stopped', updated_at = ?1 \
                     WHERE id = ?2 AND status = 'stopping'",
                    params![updated_at, id],
                )?;
                Ok(())
            })
            .await
    }

    async fn resolve_stop_request_no_live_run(
        &self,
        id: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<()> {
        let id = id.to_string();
        let updated_at = updated_at.to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE remote_agent_stop_requests SET status = 'noLiveRun', updated_at = ?1 \
                     WHERE id = ?2 AND status = 'stopping'",
                    params![updated_at, id],
                )?;
                Ok(())
            })
            .await
    }

    async fn fail_stop_request(
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
                    "UPDATE remote_agent_stop_requests \
                     SET status = 'failed', error_code = ?1, updated_at = ?2 \
                     WHERE id = ?3 AND status = 'stopping'",
                    params![error_code, updated_at, id],
                )?;
                Ok(())
            })
            .await
    }

    async fn cancel_pending_stop_requests_for_device(
        &self,
        device_id: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<u64> {
        let device_id = device_id.to_string();
        let updated_at = updated_at.to_rfc3339();
        self.db
            .run(move |conn| {
                let changed = conn.execute(
                    "UPDATE remote_agent_stop_requests \
                     SET status = 'cancelled', updated_at = ?1 \
                     WHERE requested_by_device_id = ?2 AND status = 'pending'",
                    params![updated_at, device_id],
                )?;
                Ok(changed as u64)
            })
            .await
    }

    async fn fail_stale_stopping_stop_requests(
        &self,
        claimed_before: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> AppResult<u64> {
        let claimed_before = claimed_before.to_rfc3339();
        let updated_at = updated_at.to_rfc3339();
        self.db
            .run(move |conn| {
                let changed = conn.execute(
                    "UPDATE remote_agent_stop_requests \
                     SET status = 'failedStale', updated_at = ?1 \
                     WHERE status = 'stopping' AND claimed_at IS NOT NULL AND claimed_at < ?2",
                    params![updated_at, claimed_before],
                )?;
                Ok(changed as u64)
            })
            .await
    }
}
