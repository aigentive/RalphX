use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{params, types::Type, Connection, OptionalExtension};
use tokio::sync::Mutex;

use super::DbConnection;
use crate::domain::entities::{
    AgentRunId, ChatConversationId, DelegationPark, DelegationParkId, DelegationParkJob,
    DelegationParkState,
};
use crate::domain::repositories::DelegationParkRepository;
use crate::error::{AppError, AppResult};

const PARK_COLUMNS: &str = "id, parent_conversation_id, parent_agent_run_id, generation, \
    wake_policy, wake_on_failure, state, deadline_at, wake_attempts, last_error, created_at, updated_at";

pub struct SqliteDelegationParkRepo {
    db: DbConnection,
}

impl SqliteDelegationParkRepo {
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

fn parse_enum<T>(value: String, column: &str) -> rusqlite::Result<T>
where
    T: std::str::FromStr<Err = String>,
{
    value.parse().map_err(|error: String| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid {column}: {error}"),
            )),
        )
    })
}

fn parse_timestamp(value: String, column: &str) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|error| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(error)))
        .inspect_err(|_error| {
            tracing::debug!(%column, "failed to parse delegation park timestamp");
        })
}

fn park_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DelegationPark> {
    Ok(DelegationPark {
        id: DelegationParkId::from_string(row.get::<_, String>("id")?),
        parent_conversation_id: ChatConversationId::from_string(
            row.get::<_, String>("parent_conversation_id")?,
        ),
        parent_agent_run_id: AgentRunId::from_string(row.get::<_, String>("parent_agent_run_id")?),
        generation: row.get("generation")?,
        wake_policy: parse_enum(row.get("wake_policy")?, "wake policy")?,
        wake_on_failure: row.get::<_, i64>("wake_on_failure")? != 0,
        state: parse_enum(row.get("state")?, "state")?,
        deadline_at: parse_timestamp(row.get("deadline_at")?, "deadline_at")?,
        wake_attempts: row.get("wake_attempts")?,
        last_error: row.get("last_error")?,
        created_at: parse_timestamp(row.get("created_at")?, "created_at")?,
        updated_at: parse_timestamp(row.get("updated_at")?, "updated_at")?,
        jobs: Vec::new(),
    })
}

fn load_jobs(
    conn: &Connection,
    park_id: &DelegationParkId,
) -> rusqlite::Result<Vec<DelegationParkJob>> {
    let mut statement = conn.prepare(
        "SELECT job_id, delegated_session_id, delegated_agent_run_id, settled_status
         FROM delegation_park_jobs WHERE park_id = ?1 ORDER BY job_id",
    )?;
    let jobs = statement
        .query_map([park_id.as_str()], |row| {
            Ok(DelegationParkJob {
                job_id: row.get("job_id")?,
                delegated_session_id: row.get("delegated_session_id")?,
                delegated_agent_run_id: AgentRunId::from_string(
                    row.get::<_, String>("delegated_agent_run_id")?,
                ),
                settled_status: row.get("settled_status")?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(jobs)
}

fn load_park(conn: &Connection, id: &str) -> AppResult<Option<DelegationPark>> {
    let query = format!("SELECT {PARK_COLUMNS} FROM delegation_parks WHERE id = ?1");
    let Some(mut park) = conn.query_row(&query, [id], park_from_row).optional()? else {
        return Ok(None);
    };
    park.jobs = load_jobs(conn, &park.id)?;
    Ok(Some(park))
}

fn load_parks(
    conn: &Connection,
    query: &str,
    values: &[&dyn rusqlite::ToSql],
) -> AppResult<Vec<DelegationPark>> {
    let mut statement = conn.prepare(query)?;
    let headers = statement
        .query_map(values, park_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    headers
        .into_iter()
        .map(|mut park| {
            park.jobs = load_jobs(conn, &park.id)?;
            Ok(park)
        })
        .collect()
}

#[async_trait]
impl DelegationParkRepository for SqliteDelegationParkRepo {
    async fn arm(&self, park: DelegationPark) -> AppResult<DelegationPark> {
        self.db
            .run_transaction(move |conn| {
                conn.execute(
                    "INSERT INTO delegation_parks (
                        id, parent_conversation_id, parent_agent_run_id, generation, wake_policy,
                        wake_on_failure, state, deadline_at, wake_attempts, last_error, created_at, updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    params![
                        park.id.as_str(),
                        park.parent_conversation_id.as_str(),
                        park.parent_agent_run_id.as_str(),
                        park.generation,
                        park.wake_policy.as_str(),
                        if park.wake_on_failure { 1 } else { 0 },
                        park.state.as_str(),
                        park.deadline_at.to_rfc3339(),
                        park.wake_attempts,
                        park.last_error,
                        park.created_at.to_rfc3339(),
                        park.updated_at.to_rfc3339(),
                    ],
                )?;
                for job in &park.jobs {
                    conn.execute(
                        "INSERT INTO delegation_park_jobs (
                            park_id, job_id, delegated_session_id, delegated_agent_run_id, settled_status
                         ) VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![
                            park.id.as_str(),
                            job.job_id,
                            job.delegated_session_id,
                            job.delegated_agent_run_id.as_str(),
                            job.settled_status,
                        ],
                    )?;
                }
                Ok(park)
            })
            .await
    }

    async fn get(&self, id: &DelegationParkId) -> AppResult<Option<DelegationPark>> {
        let id = id.as_str();
        self.db.run(move |conn| load_park(conn, &id)).await
    }

    async fn get_armed_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<DelegationPark>> {
        let conversation_id = conversation_id.as_str();
        self.db
            .run(move |conn| {
                let query = format!(
                    "SELECT {PARK_COLUMNS} FROM delegation_parks
                     WHERE parent_conversation_id = ?1 AND state = 'armed'
                     ORDER BY created_at LIMIT 1"
                );
                let Some(mut park) = conn
                    .query_row(&query, [conversation_id], park_from_row)
                    .optional()?
                else {
                    return Ok(None);
                };
                park.jobs = load_jobs(conn, &park.id)?;
                Ok(Some(park))
            })
            .await
    }

    async fn get_settlement_blocking_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<DelegationPark>> {
        let conversation_id = conversation_id.as_str();
        self.db
            .run(move |conn| {
                let query = format!(
                    "SELECT {PARK_COLUMNS} FROM delegation_parks
                     WHERE parent_conversation_id = ?1
                       AND state IN ('armed', 'waking', 'woken')
                     ORDER BY updated_at DESC, id DESC LIMIT 1"
                );
                let Some(mut park) = conn
                    .query_row(&query, [conversation_id], park_from_row)
                    .optional()?
                else {
                    return Ok(None);
                };
                park.jobs = load_jobs(conn, &park.id)?;
                Ok(Some(park))
            })
            .await
    }

    async fn list_armed(&self) -> AppResult<Vec<DelegationPark>> {
        self.db
            .run(move |conn| {
                let query = format!(
                    "SELECT {PARK_COLUMNS} FROM delegation_parks WHERE state = 'armed' ORDER BY deadline_at, id"
                );
                load_parks(conn, &query, &[])
            })
            .await
    }

    async fn list_armed_for_delegated_run(
        &self,
        run_id: &AgentRunId,
    ) -> AppResult<Vec<DelegationPark>> {
        let run_id = run_id.as_str();
        self.db
            .run(move |conn| {
                let query = format!(
                    "SELECT DISTINCT {PARK_COLUMNS} FROM delegation_parks p
                     JOIN delegation_park_jobs j ON j.park_id = p.id
                     WHERE p.state = 'armed' AND j.delegated_agent_run_id = ?1
                     ORDER BY p.deadline_at, p.id"
                );
                load_parks(conn, &query, &[&run_id])
            })
            .await
    }

    async fn record_job_settled(
        &self,
        id: &DelegationParkId,
        delegated_run_id: &AgentRunId,
        status: &str,
    ) -> AppResult<()> {
        let id = id.as_str();
        let delegated_run_id = delegated_run_id.as_str();
        let status = status.to_string();
        let updated_at = Utc::now().to_rfc3339();
        self.db
            .run_transaction(move |conn| {
                conn.execute(
                    "UPDATE delegation_park_jobs SET settled_status = ?1
                     WHERE park_id = ?2 AND delegated_agent_run_id = ?3",
                    params![status, id, delegated_run_id],
                )?;
                conn.execute(
                    "UPDATE delegation_parks SET updated_at = ?1 WHERE id = ?2",
                    params![updated_at, id],
                )?;
                Ok(())
            })
            .await
    }

    async fn claim_wake(&self, id: &DelegationParkId, expected_generation: i64) -> AppResult<bool> {
        let id = id.as_str();
        let updated_at = Utc::now().to_rfc3339();
        self.db
            .run_transaction(move |conn| {
                let rows_affected = conn.execute(
                    "UPDATE delegation_parks SET state = 'waking', updated_at = ?1
                     WHERE id = ?2 AND state = 'armed' AND generation = ?3",
                    params![updated_at, id, expected_generation],
                )?;
                Ok(rows_affected == 1)
            })
            .await
    }

    async fn record_wake_failure(&self, id: &DelegationParkId, error: &str) -> AppResult<i32> {
        let id = id.as_str();
        let error = error.to_string();
        let updated_at = Utc::now().to_rfc3339();
        self.db
            .run_transaction(move |conn| {
                conn.query_row(
                    "UPDATE delegation_parks
                     SET wake_attempts = wake_attempts + 1, last_error = ?1, updated_at = ?2
                     WHERE id = ?3
                     RETURNING wake_attempts",
                    params![error, updated_at, id],
                    |row| row.get(0),
                )
                .optional()?
                .ok_or_else(|| AppError::NotFound(format!("delegation park not found: {id}")))
            })
            .await
    }

    async fn list_wake_stalled(&self, older_than: DateTime<Utc>) -> AppResult<Vec<DelegationPark>> {
        let older_than = older_than.to_rfc3339();
        self.db
            .run(move |conn| {
                let query = format!(
                    "SELECT {PARK_COLUMNS} FROM delegation_parks
                     WHERE state = 'waking' AND updated_at <= ?1 ORDER BY updated_at, id"
                );
                load_parks(conn, &query, &[&older_than])
            })
            .await
    }

    async fn reset_wake_claim(&self, id: &DelegationParkId) -> AppResult<bool> {
        let id = id.as_str();
        let updated_at = Utc::now().to_rfc3339();
        self.db
            .run_transaction(move |conn| {
                // `wake_attempts` belongs to the dispatcher that spent it. Reclaiming an abandoned
                // claim starts a new dispatcher, so it gets a full retry budget; `park_max_secs`
                // still bounds total effort across recoveries.
                let rows_affected = conn.execute(
                    "UPDATE delegation_parks SET state = 'armed', wake_attempts = 0, updated_at = ?1
                     WHERE id = ?2 AND state = 'waking'",
                    params![updated_at, id],
                )?;
                Ok(rows_affected == 1)
            })
            .await
    }

    async fn settle(
        &self,
        id: &DelegationParkId,
        state: DelegationParkState,
        error: Option<&str>,
    ) -> AppResult<()> {
        let id = id.as_str();
        let error = error.map(str::to_string);
        let updated_at = Utc::now().to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE delegation_parks SET state = ?1, last_error = ?2, updated_at = ?3 WHERE id = ?4",
                    params![state.as_str(), error, updated_at, id],
                )?;
                Ok(())
            })
            .await
    }

    async fn supersede_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<usize> {
        let conversation_id = conversation_id.as_str();
        let updated_at = Utc::now().to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE delegation_parks SET state = 'superseded', updated_at = ?1
                     WHERE parent_conversation_id = ?2 AND state IN ('armed', 'waking')",
                    params![updated_at, conversation_id],
                )
                .map_err(AppError::from)
            })
            .await
    }

    async fn list_expired(&self, now: DateTime<Utc>) -> AppResult<Vec<DelegationPark>> {
        let now = now.to_rfc3339();
        self.db
            .run(move |conn| {
                let query = format!(
                    "SELECT {PARK_COLUMNS} FROM delegation_parks
                     WHERE state = 'armed' AND deadline_at <= ?1 ORDER BY deadline_at, id"
                );
                load_parks(conn, &query, &[&now])
            })
            .await
    }
}
