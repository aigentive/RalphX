use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rusqlite::{types::Type, Connection};
use tokio::sync::Mutex;

use super::DbConnection;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{DelegatedSession, DelegatedSessionId, ProjectId};
use crate::domain::repositories::DelegatedSessionRepository;
use crate::error::{AppError, AppResult};

fn parse_datetime(value: &str) -> DateTime<Utc> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
        return dt.with_timezone(&Utc);
    }
    if let Ok(ndt) = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S") {
        return Utc.from_utc_datetime(&ndt);
    }
    Utc::now()
}

fn parse_harness(value: &str) -> rusqlite::Result<AgentHarnessKind> {
    value.parse::<AgentHarnessKind>().map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            Type::Text,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
        )
    })
}

fn row_to_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<DelegatedSession> {
    let created_at: String = row.get("created_at")?;
    let updated_at: String = row.get("updated_at")?;
    let completed_at: Option<String> = row.get("completed_at")?;
    let harness: String = row.get("harness")?;
    Ok(DelegatedSession {
        id: DelegatedSessionId::from_string(row.get::<_, String>("id")?),
        project_id: ProjectId::from_string(row.get::<_, String>("project_id")?),
        parent_context_type: row.get("parent_context_type")?,
        parent_context_id: row.get("parent_context_id")?,
        parent_turn_id: row.get("parent_turn_id")?,
        parent_message_id: row.get("parent_message_id")?,
        delegate_context_authorized: row.get("delegate_context_authorized")?,
        caller_conversation_id: row.get("caller_conversation_id")?,
        job_id: row.get("job_id")?,
        parent_agent_run_id: row.get("parent_agent_run_id")?,
        agent_name: row.get("agent_name")?,
        title: row.get("title")?,
        harness: parse_harness(&harness)?,
        status: row.get("status")?,
        provider_session_id: row.get("provider_session_id")?,
        error: row.get("error")?,
        created_at: parse_datetime(&created_at),
        updated_at: parse_datetime(&updated_at),
        completed_at: completed_at.as_deref().map(parse_datetime),
    })
}

pub struct SqliteDelegatedSessionRepository {
    db: DbConnection,
}

impl SqliteDelegatedSessionRepository {
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
impl DelegatedSessionRepository for SqliteDelegatedSessionRepository {
    async fn create(&self, session: DelegatedSession) -> AppResult<DelegatedSession> {
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO delegated_sessions (
                        id, project_id, parent_context_type, parent_context_id,
                        parent_turn_id, parent_message_id, delegate_context_authorized,
                        caller_conversation_id, job_id, parent_agent_run_id, agent_name, title,
                        harness, status, provider_session_id, error, created_at, updated_at,
                        completed_at
                    ) VALUES (
                        ?1, ?2, ?3, ?4,
                        ?5, ?6, ?7, ?8,
                        ?9, ?10, ?11, ?12, ?13,
                        ?14, ?15, ?16, ?17, ?18,
                        ?19
                    )",
                    rusqlite::params![
                        session.id.as_str(),
                        session.project_id.as_str(),
                        session.parent_context_type,
                        session.parent_context_id,
                        session.parent_turn_id,
                        session.parent_message_id,
                        session.delegate_context_authorized,
                        session.caller_conversation_id,
                        session.job_id,
                        session.parent_agent_run_id,
                        session.agent_name,
                        session.title,
                        session.harness.to_string(),
                        session.status,
                        session.provider_session_id,
                        session.error,
                        session.created_at.to_rfc3339(),
                        session.updated_at.to_rfc3339(),
                        session.completed_at.map(|dt| dt.to_rfc3339()),
                    ],
                )?;
                Ok(session)
            })
            .await
    }

    async fn get_by_id(&self, id: &DelegatedSessionId) -> AppResult<Option<DelegatedSession>> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let result = conn.query_row(
                    "SELECT id, project_id, parent_context_type, parent_context_id,
                            parent_turn_id, parent_message_id, delegate_context_authorized,
                            caller_conversation_id, job_id, parent_agent_run_id, agent_name,
                            title, harness, status, provider_session_id, error, created_at,
                            updated_at, completed_at
                     FROM delegated_sessions
                     WHERE id = ?1",
                    [id.as_str()],
                    row_to_session,
                );
                match result {
                    Ok(session) => Ok(Some(session)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(error) => Err(AppError::Database(error.to_string())),
                }
            })
            .await
    }

    async fn get_by_parent_context(
        &self,
        parent_context_type: &str,
        parent_context_id: &str,
    ) -> AppResult<Vec<DelegatedSession>> {
        let parent_context_type = parent_context_type.to_string();
        let parent_context_id = parent_context_id.to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, project_id, parent_context_type, parent_context_id,
                            parent_turn_id, parent_message_id, delegate_context_authorized,
                            caller_conversation_id, job_id, parent_agent_run_id, agent_name,
                            title, harness, status, provider_session_id, error, created_at,
                            updated_at, completed_at
                     FROM delegated_sessions
                     WHERE parent_context_type = ?1 AND parent_context_id = ?2
                     ORDER BY created_at DESC",
                )?;
                let sessions = stmt
                    .query_map(
                        [parent_context_type.as_str(), parent_context_id.as_str()],
                        row_to_session,
                    )?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(sessions)
            })
            .await
    }

    async fn list_active_by_caller_conversation(
        &self,
        caller_conversation_id: &str,
    ) -> AppResult<Vec<DelegatedSession>> {
        let caller_conversation_id = caller_conversation_id.to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, project_id, parent_context_type, parent_context_id,
                            parent_turn_id, parent_message_id, delegate_context_authorized,
                            caller_conversation_id, job_id, parent_agent_run_id, agent_name,
                            title, harness, status, provider_session_id, error, created_at,
                            updated_at, completed_at
                     FROM delegated_sessions
                     WHERE caller_conversation_id = ?1
                       AND status NOT IN ('completed', 'failed', 'cancelled')
                     ORDER BY created_at DESC",
                )?;
                let sessions = stmt
                    .query_map([caller_conversation_id.as_str()], row_to_session)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(sessions)
            })
            .await
    }

    async fn update_job_identity(
        &self,
        id: &DelegatedSessionId,
        caller_conversation_id: Option<String>,
        job_id: String,
        parent_agent_run_id: Option<String>,
    ) -> AppResult<()> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE delegated_sessions
                     SET caller_conversation_id = ?1, job_id = ?2, parent_agent_run_id = ?3,
                         updated_at = ?4
                     WHERE id = ?5",
                    rusqlite::params![
                        caller_conversation_id,
                        job_id,
                        parent_agent_run_id,
                        Utc::now().to_rfc3339(),
                        id.as_str()
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn update_provider_session_id(
        &self,
        id: &DelegatedSessionId,
        provider_session_id: Option<String>,
    ) -> AppResult<()> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE delegated_sessions
                     SET provider_session_id = ?1, updated_at = ?2
                     WHERE id = ?3",
                    rusqlite::params![provider_session_id, Utc::now().to_rfc3339(), id.as_str()],
                )?;
                Ok(())
            })
            .await
    }

    async fn update_status(
        &self,
        id: &DelegatedSessionId,
        status: &str,
        error: Option<String>,
        completed_at: Option<DateTime<Utc>>,
    ) -> AppResult<()> {
        let id = id.as_str().to_string();
        let status = status.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE delegated_sessions
                     SET status = ?1, error = ?2, completed_at = ?3, updated_at = ?4
                     WHERE id = ?5",
                    rusqlite::params![
                        status,
                        error,
                        completed_at.map(|dt| dt.to_rfc3339()),
                        Utc::now().to_rfc3339(),
                        id.as_str()
                    ],
                )?;
                Ok(())
            })
            .await
    }
}
