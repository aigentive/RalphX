//! SQLite implementation of the managed Team wake-batch repository.

use std::sync::Arc;

use async_trait::async_trait;
use rusqlite::Connection;
use tokio::sync::Mutex;

use super::sqlite_team_support::{
    enum_from_db, enum_to_db, parse_opt_team_timestamp, parse_team_timestamp,
};
use super::DbConnection;
use crate::domain::entities::{
    AgentRunId, TeamMemberId, TeamMessageDeliveryId, TeamSessionId, TeamWakeBatch, TeamWakeBatchId,
    TeamWakeBatchStatus,
};
use crate::domain::repositories::TeamWakeBatchRepository;
use crate::error::{AppError, AppResult};

pub struct SqliteTeamWakeBatchRepository {
    db: DbConnection,
}

impl SqliteTeamWakeBatchRepository {
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

fn delivery_ids_from_json(value: String) -> AppResult<Vec<TeamMessageDeliveryId>> {
    let raw: Vec<String> = serde_json::from_str(&value)
        .map_err(|error| AppError::Database(format!("invalid wake batch delivery ids: {error}")))?;
    Ok(raw
        .into_iter()
        .map(TeamMessageDeliveryId::from_string)
        .collect())
}

fn delivery_ids_to_json(ids: &[TeamMessageDeliveryId]) -> AppResult<String> {
    let raw: Vec<&str> = ids.iter().map(|id| id.0.as_str()).collect();
    serde_json::to_string(&raw)
        .map_err(|error| AppError::Database(format!("failed to encode delivery ids: {error}")))
}

fn batch_from_row(row: &rusqlite::Row<'_>) -> AppResult<TeamWakeBatch> {
    Ok(TeamWakeBatch {
        id: TeamWakeBatchId::from_string(row.get::<_, String>("id")?),
        team_id: TeamSessionId::from_string(row.get::<_, String>("team_id")?),
        recipient_kind: enum_from_db(row.get("recipient_kind")?, "wake recipient kind")?,
        recipient_member_id: row
            .get::<_, Option<String>>("recipient_member_id")?
            .map(TeamMemberId::from_string),
        recipient_generation: row.get("recipient_generation")?,
        first_message_sequence: row.get("first_message_sequence")?,
        last_message_sequence: row.get("last_message_sequence")?,
        delivery_ids: delivery_ids_from_json(row.get("delivery_ids_json")?)?,
        status: enum_from_db(row.get("status")?, "wake batch status")?,
        planned_agent_run_id: row
            .get::<_, Option<String>>("planned_agent_run_id")?
            .map(AgentRunId::from_string),
        bound_agent_run_id: row
            .get::<_, Option<String>>("bound_agent_run_id")?
            .map(AgentRunId::from_string),
        attempt_count: row.get("attempt_count")?,
        budget_count: row.get("budget_count")?,
        lease_token: row.get("lease_token")?,
        version: row.get("version")?,
        last_error: row.get("last_error")?,
        created_at: parse_team_timestamp(&row.get::<_, String>("created_at")?, "wake batch")?,
        updated_at: parse_team_timestamp(&row.get::<_, String>("updated_at")?, "wake batch")?,
        settled_at: parse_opt_team_timestamp(row.get("settled_at")?, "wake batch")?,
    })
}

#[async_trait]
impl TeamWakeBatchRepository for SqliteTeamWakeBatchRepository {
    async fn create_or_extend_active(&self, batch: TeamWakeBatch) -> AppResult<TeamWakeBatch> {
        batch.validate(i64::MAX).map_err(AppError::Validation)?;
        self.db
            .run_transaction(move |conn| {
                let existing = {
                    let mut stmt = conn.prepare(
                        "SELECT * FROM managed_team_wake_batches
                         WHERE team_id = ?1 AND recipient_kind = ?2
                           AND recipient_member_id IS ?3 AND recipient_generation IS ?4
                           AND status IN ('queued', 'launching', 'running')",
                    )?;
                    let mut rows = stmt.query(rusqlite::params![
                        batch.team_id.as_str(),
                        enum_to_db(&batch.recipient_kind, "wake recipient kind")?,
                        batch
                            .recipient_member_id
                            .as_ref()
                            .map(|id| id.as_str().to_string()),
                        batch.recipient_generation,
                    ])?;
                    match rows.next()? {
                        Some(row) => Some(batch_from_row(row)?),
                        None => None,
                    }
                };
                if let Some(mut current) = existing {
                    current.first_message_sequence = current
                        .first_message_sequence
                        .min(batch.first_message_sequence);
                    current.last_message_sequence = current
                        .last_message_sequence
                        .max(batch.last_message_sequence);
                    for id in batch.delivery_ids {
                        if !current.delivery_ids.contains(&id) {
                            current.delivery_ids.push(id);
                        }
                    }
                    current.updated_at = batch.updated_at;
                    conn.execute(
                        "UPDATE managed_team_wake_batches SET
                            first_message_sequence = ?1, last_message_sequence = ?2,
                            delivery_ids_json = ?3, updated_at = ?4
                         WHERE id = ?5",
                        rusqlite::params![
                            current.first_message_sequence,
                            current.last_message_sequence,
                            delivery_ids_to_json(&current.delivery_ids)?,
                            current.updated_at.to_rfc3339(),
                            current.id.0,
                        ],
                    )?;
                    return Ok(current);
                }
                conn.execute(
                    "INSERT INTO managed_team_wake_batches (
                        id, team_id, recipient_kind, recipient_member_id,
                        recipient_generation, first_message_sequence, last_message_sequence,
                        delivery_ids_json, status, planned_agent_run_id, bound_agent_run_id,
                        attempt_count, budget_count, lease_token, version, last_error,
                        created_at, updated_at, settled_at
                    ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
                    rusqlite::params![
                        batch.id.0,
                        batch.team_id.as_str(),
                        enum_to_db(&batch.recipient_kind, "wake recipient kind")?,
                        batch
                            .recipient_member_id
                            .as_ref()
                            .map(|id| id.as_str().to_string()),
                        batch.recipient_generation,
                        batch.first_message_sequence,
                        batch.last_message_sequence,
                        delivery_ids_to_json(&batch.delivery_ids)?,
                        enum_to_db(&batch.status, "wake batch status")?,
                        batch
                            .planned_agent_run_id
                            .as_ref()
                            .map(|id| id.as_str().to_string()),
                        batch
                            .bound_agent_run_id
                            .as_ref()
                            .map(|id| id.as_str().to_string()),
                        batch.attempt_count,
                        batch.budget_count,
                        batch.lease_token,
                        batch.version,
                        batch.last_error,
                        batch.created_at.to_rfc3339(),
                        batch.updated_at.to_rfc3339(),
                        batch.settled_at.map(|at| at.to_rfc3339()),
                    ],
                )?;
                Ok(batch)
            })
            .await
    }

    async fn get_by_id(&self, id: &TeamWakeBatchId) -> AppResult<Option<TeamWakeBatch>> {
        let id = id.0.clone();
        self.db
            .run(move |conn| {
                let mut stmt =
                    conn.prepare("SELECT * FROM managed_team_wake_batches WHERE id = ?1")?;
                let mut rows = stmt.query([id])?;
                match rows.next()? {
                    Some(row) => Ok(Some(batch_from_row(row)?)),
                    None => Ok(None),
                }
            })
            .await
    }

    async fn transition(
        &self,
        id: &TeamWakeBatchId,
        expected_version: i64,
        expected: TeamWakeBatchStatus,
        batch: TeamWakeBatch,
    ) -> AppResult<bool> {
        let id = id.0.clone();
        self.db
            .run(move |conn| {
                let count = conn.execute(
                    "UPDATE managed_team_wake_batches SET
                        first_message_sequence = ?1, last_message_sequence = ?2,
                        delivery_ids_json = ?3, status = ?4, planned_agent_run_id = ?5,
                        bound_agent_run_id = ?6, attempt_count = ?7, budget_count = ?8,
                        lease_token = ?9, version = ?10, last_error = ?11, updated_at = ?12,
                        settled_at = ?13
                     WHERE id = ?14 AND version = ?15 AND status = ?16",
                    rusqlite::params![
                        batch.first_message_sequence,
                        batch.last_message_sequence,
                        delivery_ids_to_json(&batch.delivery_ids)?,
                        enum_to_db(&batch.status, "wake batch status")?,
                        batch
                            .planned_agent_run_id
                            .as_ref()
                            .map(|run| run.as_str().to_string()),
                        batch
                            .bound_agent_run_id
                            .as_ref()
                            .map(|run| run.as_str().to_string()),
                        batch.attempt_count,
                        batch.budget_count,
                        batch.lease_token,
                        batch.version,
                        batch.last_error,
                        batch.updated_at.to_rfc3339(),
                        batch.settled_at.map(|at| at.to_rfc3339()),
                        id,
                        expected_version,
                        enum_to_db(&expected, "wake batch status")?,
                    ],
                )?;
                Ok(count == 1)
            })
            .await
    }
}
