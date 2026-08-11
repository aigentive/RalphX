//! SQLite implementation of the durable Team message + delivery outbox repository.

use std::sync::Arc;

use async_trait::async_trait;
use rusqlite::Connection;
use tokio::sync::Mutex;

use super::sqlite_team_support::{
    enum_from_db, enum_to_db, parse_opt_team_timestamp, parse_team_timestamp,
};
use super::DbConnection;
use crate::domain::entities::{
    AgentRunId, AgentTaskAssignmentId, TeamMemberId, TeamMessage, TeamMessageDelivery,
    TeamMessageDeliveryId, TeamMessageDeliveryStatus, TeamMessageId, TeamSessionId,
};
use crate::domain::repositories::TeamMessageRepository;
use crate::error::{AppError, AppResult};

const TEAM_MESSAGE_UNIQUE_CONFLICT: &str = "managed_team_message_unique_conflict";

pub struct SqliteTeamMessageRepository {
    db: DbConnection,
}

impl SqliteTeamMessageRepository {
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

fn message_from_row(row: &rusqlite::Row<'_>) -> AppResult<TeamMessage> {
    Ok(TeamMessage {
        id: TeamMessageId::from_string(row.get::<_, String>("id")?),
        team_id: TeamSessionId::from_string(row.get::<_, String>("team_id")?),
        sequence: row.get("sequence")?,
        sender_kind: enum_from_db(row.get("sender_kind")?, "message sender kind")?,
        sender_member_id: row
            .get::<_, Option<String>>("sender_member_id")?
            .map(TeamMemberId::from_string),
        target_kind: enum_from_db(row.get("target_kind")?, "message target kind")?,
        target_member_id: row
            .get::<_, Option<String>>("target_member_id")?
            .map(TeamMemberId::from_string),
        kind: enum_from_db(row.get("kind")?, "message kind")?,
        content: row.get("content")?,
        source_run_id: row
            .get::<_, Option<String>>("source_run_id")?
            .map(AgentRunId::from_string),
        assignment_id: row
            .get::<_, Option<String>>("assignment_id")?
            .map(AgentTaskAssignmentId::from_string),
        idempotency_key: row.get("idempotency_key")?,
        created_at: parse_team_timestamp(&row.get::<_, String>("created_at")?, "team message")?,
    })
}

fn delivery_from_row(row: &rusqlite::Row<'_>) -> AppResult<TeamMessageDelivery> {
    Ok(TeamMessageDelivery {
        id: TeamMessageDeliveryId::from_string(row.get::<_, String>("id")?),
        message_id: TeamMessageId::from_string(row.get::<_, String>("message_id")?),
        recipient_kind: enum_from_db(row.get("recipient_kind")?, "delivery recipient kind")?,
        recipient_member_id: row
            .get::<_, Option<String>>("recipient_member_id")?
            .map(TeamMemberId::from_string),
        recipient_generation: row.get("recipient_generation")?,
        status: enum_from_db(row.get("status")?, "delivery status")?,
        queued_message_id: row.get("queued_message_id")?,
        attempt_count: row.get("attempt_count")?,
        next_retry_at: parse_opt_team_timestamp(row.get("next_retry_at")?, "delivery")?,
        last_error: row.get("last_error")?,
        created_at: parse_team_timestamp(&row.get::<_, String>("created_at")?, "delivery")?,
        updated_at: parse_team_timestamp(&row.get::<_, String>("updated_at")?, "delivery")?,
    })
}

fn insert_delivery(conn: &Connection, delivery: &TeamMessageDelivery) -> AppResult<()> {
    conn.execute(
        "INSERT INTO managed_team_message_deliveries (
            id, message_id, recipient_kind, recipient_member_id, recipient_generation,
            status, queued_message_id, attempt_count, next_retry_at, last_error,
            created_at, updated_at
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        rusqlite::params![
            delivery.id.0,
            delivery.message_id.as_str(),
            enum_to_db(&delivery.recipient_kind, "delivery recipient kind")?,
            delivery
                .recipient_member_id
                .as_ref()
                .map(|id| id.as_str().to_string()),
            delivery.recipient_generation,
            enum_to_db(&delivery.status, "delivery status")?,
            delivery.queued_message_id,
            delivery.attempt_count,
            delivery.next_retry_at.map(|at| at.to_rfc3339()),
            delivery.last_error,
            delivery.created_at.to_rfc3339(),
            delivery.updated_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn deliveries_for_message(
    conn: &Connection,
    message_id: &TeamMessageId,
) -> AppResult<Vec<TeamMessageDelivery>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM managed_team_message_deliveries
         WHERE message_id = ?1 ORDER BY created_at, id",
    )?;
    let mut rows = stmt.query([message_id.as_str()])?;
    let mut deliveries = Vec::new();
    while let Some(row) = rows.next()? {
        deliveries.push(delivery_from_row(row)?);
    }
    Ok(deliveries)
}

#[async_trait]
impl TeamMessageRepository for SqliteTeamMessageRepository {
    async fn create_envelope_with_deliveries(
        &self,
        message: TeamMessage,
        deliveries: Vec<TeamMessageDelivery>,
    ) -> AppResult<(TeamMessage, Vec<TeamMessageDelivery>)> {
        message.validate().map_err(AppError::Validation)?;
        if deliveries
            .iter()
            .any(|delivery| delivery.message_id != message.id)
        {
            return Err(AppError::Validation(
                "delivery message id mismatch".to_string(),
            ));
        }
        self.db
            .run_transaction(move |conn| {
                if let Err(error) = conn.execute(
                    "INSERT INTO managed_team_messages (
                        id, team_id, sequence, sender_kind, sender_member_id, target_kind,
                        target_member_id, kind, content, source_run_id, assignment_id,
                        idempotency_key, created_at
                    ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                    rusqlite::params![
                        message.id.as_str(),
                        message.team_id.as_str(),
                        message.sequence,
                        enum_to_db(&message.sender_kind, "message sender kind")?,
                        message
                            .sender_member_id
                            .as_ref()
                            .map(|id| id.as_str().to_string()),
                        enum_to_db(&message.target_kind, "message target kind")?,
                        message
                            .target_member_id
                            .as_ref()
                            .map(|id| id.as_str().to_string()),
                        enum_to_db(&message.kind, "message kind")?,
                        message.content,
                        message
                            .source_run_id
                            .as_ref()
                            .map(|id| id.as_str().to_string()),
                        message
                            .assignment_id
                            .as_ref()
                            .map(|id| id.as_str().to_string()),
                        message.idempotency_key,
                        message.created_at.to_rfc3339(),
                    ],
                ) {
                    if matches!(
                        &error,
                        rusqlite::Error::SqliteFailure(failure, _)
                            if failure.code == rusqlite::ErrorCode::ConstraintViolation
                    ) {
                        return Err(AppError::Conflict(TEAM_MESSAGE_UNIQUE_CONFLICT.to_string()));
                    }
                    return Err(error.into());
                }
                for delivery in &deliveries {
                    insert_delivery(conn, delivery)?;
                }
                Ok((message, deliveries))
            })
            .await
    }

    async fn get_message(&self, id: &TeamMessageId) -> AppResult<Option<TeamMessage>> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare("SELECT * FROM managed_team_messages WHERE id = ?1")?;
                let mut rows = stmt.query([id])?;
                match rows.next()? {
                    Some(row) => Ok(Some(message_from_row(row)?)),
                    None => Ok(None),
                }
            })
            .await
    }

    async fn get_envelope_by_idempotency_key(
        &self,
        team_id: &TeamSessionId,
        idempotency_key: &str,
    ) -> AppResult<Option<(TeamMessage, Vec<TeamMessageDelivery>)>> {
        let team_id = team_id.as_str().to_string();
        let idempotency_key = idempotency_key.to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM managed_team_messages
                     WHERE team_id = ?1 AND idempotency_key = ?2",
                )?;
                let mut rows = stmt.query(rusqlite::params![team_id, idempotency_key])?;
                match rows.next()? {
                    Some(row) => {
                        let message = message_from_row(row)?;
                        let deliveries = deliveries_for_message(conn, &message.id)?;
                        Ok(Some((message, deliveries)))
                    }
                    None => Ok(None),
                }
            })
            .await
    }

    async fn next_message_sequence(&self, team_id: &TeamSessionId) -> AppResult<i64> {
        let team_id = team_id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.query_row(
                    "SELECT COALESCE(MAX(sequence), 0) + 1
                     FROM managed_team_messages WHERE team_id = ?1",
                    [team_id],
                    |row| row.get(0),
                )
                .map_err(Into::into)
            })
            .await
    }

    async fn list_messages_after(
        &self,
        team_id: &TeamSessionId,
        sequence: i64,
        limit: u32,
    ) -> AppResult<Vec<TeamMessage>> {
        let team_id = team_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM managed_team_messages
                     WHERE team_id = ?1 AND sequence > ?2
                     ORDER BY sequence LIMIT ?3",
                )?;
                let mut rows = stmt.query(rusqlite::params![team_id, sequence, limit as i64])?;
                let mut messages = Vec::new();
                while let Some(row) = rows.next()? {
                    messages.push(message_from_row(row)?);
                }
                Ok(messages)
            })
            .await
    }

    async fn list_actionable_deliveries(
        &self,
        team_id: &TeamSessionId,
        limit: u32,
    ) -> AppResult<Vec<TeamMessageDelivery>> {
        let team_id = team_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT d.* FROM managed_team_message_deliveries d
                     JOIN managed_team_messages m ON m.id = d.message_id
                     WHERE m.team_id = ?1 AND d.status IN ('pending', 'failed')
                     ORDER BY m.sequence, d.created_at, d.id LIMIT ?2",
                )?;
                let mut rows = stmt.query(rusqlite::params![team_id, limit as i64])?;
                let mut deliveries = Vec::new();
                while let Some(row) = rows.next()? {
                    deliveries.push(delivery_from_row(row)?);
                }
                Ok(deliveries)
            })
            .await
    }

    async fn transition_delivery(
        &self,
        id: &TeamMessageDeliveryId,
        expected: TeamMessageDeliveryStatus,
        next: TeamMessageDeliveryStatus,
        delivery: TeamMessageDelivery,
    ) -> AppResult<bool> {
        if delivery.status != next {
            return Ok(false);
        }
        let id = id.0.clone();
        self.db
            .run(move |conn| {
                let count = conn.execute(
                    "UPDATE managed_team_message_deliveries SET
                        status = ?1, queued_message_id = ?2, attempt_count = ?3,
                        next_retry_at = ?4, last_error = ?5, updated_at = ?6
                     WHERE id = ?7 AND status = ?8",
                    rusqlite::params![
                        enum_to_db(&next, "delivery status")?,
                        delivery.queued_message_id,
                        delivery.attempt_count,
                        delivery.next_retry_at.map(|at| at.to_rfc3339()),
                        delivery.last_error,
                        delivery.updated_at.to_rfc3339(),
                        id,
                        enum_to_db(&expected, "delivery status")?,
                    ],
                )?;
                Ok(count == 1)
            })
            .await
    }
}
