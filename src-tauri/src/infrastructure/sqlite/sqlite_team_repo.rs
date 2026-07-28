//! SQLite implementation of the managed Team session and roster repository.

use std::sync::Arc;

use async_trait::async_trait;
use rusqlite::Connection;
use tokio::sync::Mutex;

use super::sqlite_team_support::{
    enum_from_db, enum_to_db, parse_opt_team_timestamp, parse_team_timestamp,
};
use super::DbConnection;
use crate::domain::agents::{AgentHarnessKind, LogicalEffort};
use crate::domain::entities::{
    AgentRunId, AgentTaskAssignmentId, ChatConversationId, DelegatedSessionId, ProjectId,
    TeamMember, TeamMemberId, TeamSession, TeamSessionId,
};
use crate::domain::repositories::TeamRepository;
use crate::error::{AppError, AppResult};

pub struct SqliteTeamRepository {
    db: DbConnection,
}

impl SqliteTeamRepository {
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

fn parse_enum_str<T: std::str::FromStr<Err = String>>(value: String, label: &str) -> AppResult<T> {
    T::from_str(&value).map_err(|error| AppError::Database(format!("invalid {label}: {error}")))
}

pub(crate) fn session_from_row(row: &rusqlite::Row<'_>) -> AppResult<TeamSession> {
    let strategy: Option<String> = row.get("strategy")?;
    let budget: Option<String> = row.get("budget_policy_json")?;
    Ok(TeamSession {
        id: TeamSessionId::from_string(row.get::<_, String>("id")?),
        project_id: ProjectId::from_string(row.get::<_, String>("project_id")?),
        coordinator_conversation_id: ChatConversationId::from_string(
            row.get::<_, String>("coordinator_conversation_id")?,
        ),
        status: enum_from_db(row.get("status")?, "team session status")?,
        strategy: strategy
            .map(|value| enum_from_db(value, "team strategy"))
            .transpose()?,
        configured_concurrency: row.get::<_, i64>("configured_concurrency")? as u32,
        effective_concurrency: row.get::<_, i64>("effective_concurrency")? as u32,
        automatic_wake_limit: row.get::<_, i64>("automatic_wake_limit")? as u32,
        budget_policy: budget
            .map(|value| {
                serde_json::from_str(&value)
                    .map_err(|error| AppError::Database(format!("invalid budget policy: {error}")))
            })
            .transpose()?,
        pending_coordination_mode: row.get("pending_coordination_mode")?,
        pending_exit_action: row.get("pending_exit_action")?,
        version: row.get("version")?,
        last_error: row.get("last_error")?,
        created_at: parse_team_timestamp(&row.get::<_, String>("created_at")?, "team session")?,
        updated_at: parse_team_timestamp(&row.get::<_, String>("updated_at")?, "team session")?,
        closed_at: parse_opt_team_timestamp(row.get("closed_at")?, "team session")?,
    })
}

fn member_from_row(row: &rusqlite::Row<'_>) -> AppResult<TeamMember> {
    Ok(TeamMember {
        id: TeamMemberId::from_string(row.get::<_, String>("id")?),
        team_id: TeamSessionId::from_string(row.get::<_, String>("team_id")?),
        normalized_name: row.get("normalized_name")?,
        name: row.get("name")?,
        canonical_agent_name: row.get("canonical_agent_name")?,
        role_summary: row.get("role_summary")?,
        harness: row
            .get::<_, Option<String>>("harness")?
            .map(|value| parse_enum_str::<AgentHarnessKind>(value, "member harness"))
            .transpose()?,
        logical_model: row.get("logical_model")?,
        logical_effort: row
            .get::<_, Option<String>>("logical_effort")?
            .map(|value| parse_enum_str::<LogicalEffort>(value, "member effort"))
            .transpose()?,
        delegated_session_id: row
            .get::<_, Option<String>>("delegated_session_id")?
            .map(DelegatedSessionId::from_string),
        generation: row.get("generation")?,
        current_run_id: row
            .get::<_, Option<String>>("current_run_id")?
            .map(AgentRunId::from_string),
        current_assignment_id: row
            .get::<_, Option<String>>("current_assignment_id")?
            .map(AgentTaskAssignmentId::from_string),
        status: enum_from_db(row.get("status")?, "team member status")?,
        last_activity_at: parse_opt_team_timestamp(row.get("last_activity_at")?, "team member")?,
        last_error: row.get("last_error")?,
        created_at: parse_team_timestamp(&row.get::<_, String>("created_at")?, "team member")?,
        updated_at: parse_team_timestamp(&row.get::<_, String>("updated_at")?, "team member")?,
        stopped_at: parse_opt_team_timestamp(row.get("stopped_at")?, "team member")?,
    })
}

pub(crate) fn insert_session(conn: &Connection, value: &TeamSession) -> AppResult<()> {
    conn.execute(
        "INSERT INTO managed_team_sessions (
            id, project_id, coordinator_conversation_id, status, strategy,
            configured_concurrency, effective_concurrency, automatic_wake_limit,
            budget_policy_json, pending_coordination_mode, pending_exit_action,
            version, last_error, created_at, updated_at, closed_at
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
        rusqlite::params![
            value.id.as_str(),
            value.project_id.as_str(),
            value.coordinator_conversation_id.as_str(),
            enum_to_db(&value.status, "team session status")?,
            value
                .strategy
                .as_ref()
                .map(|strategy| enum_to_db(strategy, "team strategy"))
                .transpose()?,
            value.configured_concurrency as i64,
            value.effective_concurrency as i64,
            value.automatic_wake_limit as i64,
            value
                .budget_policy
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(|error| AppError::Database(error.to_string()))?,
            value.pending_coordination_mode,
            value.pending_exit_action,
            value.version,
            value.last_error,
            value.created_at.to_rfc3339(),
            value.updated_at.to_rfc3339(),
            value.closed_at.map(|closed| closed.to_rfc3339()),
        ],
    )?;
    Ok(())
}

pub(crate) fn get_open_session(
    conn: &Connection,
    conversation_id: &str,
) -> AppResult<Option<TeamSession>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM managed_team_sessions
         WHERE coordinator_conversation_id = ?1 AND status != 'closed'",
    )?;
    let mut rows = stmt.query([conversation_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(session_from_row(row)?)),
        None => Ok(None),
    }
}

#[async_trait]
impl TeamRepository for SqliteTeamRepository {
    async fn ensure_session(&self, session: TeamSession) -> AppResult<TeamSession> {
        self.db
            .run_transaction(move |conn| {
                if let Some(existing) =
                    get_open_session(conn, &session.coordinator_conversation_id.as_str())?
                {
                    return Ok(existing);
                }
                insert_session(conn, &session)?;
                Ok(session)
            })
            .await
    }

    async fn get_session(&self, id: &TeamSessionId) -> AppResult<Option<TeamSession>> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare("SELECT * FROM managed_team_sessions WHERE id = ?1")?;
                let mut rows = stmt.query([id])?;
                match rows.next()? {
                    Some(row) => Ok(Some(session_from_row(row)?)),
                    None => Ok(None),
                }
            })
            .await
    }

    async fn get_open_session_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<TeamSession>> {
        let conversation_id = conversation_id.as_str();
        self.db
            .run(move |conn| get_open_session(conn, &conversation_id))
            .await
    }

    async fn update_session(&self, session: TeamSession, expected_version: i64) -> AppResult<bool> {
        self.db
            .run(move |conn| {
                let count = conn.execute(
                    "UPDATE managed_team_sessions SET
                        status = ?1, strategy = ?2, configured_concurrency = ?3,
                        effective_concurrency = ?4, automatic_wake_limit = ?5,
                        budget_policy_json = ?6, pending_coordination_mode = ?7,
                        pending_exit_action = ?8, version = ?9, last_error = ?10,
                        updated_at = ?11, closed_at = ?12
                     WHERE id = ?13 AND version = ?14",
                    rusqlite::params![
                        enum_to_db(&session.status, "team session status")?,
                        session
                            .strategy
                            .as_ref()
                            .map(|strategy| enum_to_db(strategy, "team strategy"))
                            .transpose()?,
                        session.configured_concurrency as i64,
                        session.effective_concurrency as i64,
                        session.automatic_wake_limit as i64,
                        session
                            .budget_policy
                            .as_ref()
                            .map(serde_json::to_string)
                            .transpose()
                            .map_err(|error| AppError::Database(error.to_string()))?,
                        session.pending_coordination_mode,
                        session.pending_exit_action,
                        session.version,
                        session.last_error,
                        session.updated_at.to_rfc3339(),
                        session.closed_at.map(|closed| closed.to_rfc3339()),
                        session.id.as_str(),
                        expected_version,
                    ],
                )?;
                Ok(count == 1)
            })
            .await
    }

    async fn create_member(&self, member: TeamMember) -> AppResult<TeamMember> {
        member.validate_name().map_err(AppError::Validation)?;
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO managed_team_members (
                        id, team_id, normalized_name, name, canonical_agent_name,
                        role_summary, harness, logical_model, logical_effort,
                        delegated_session_id, generation, current_run_id,
                        current_assignment_id, status, last_activity_at, last_error,
                        created_at, updated_at, stopped_at
                    ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
                    rusqlite::params![
                        member.id.as_str(),
                        member.team_id.as_str(),
                        member.normalized_name,
                        member.name,
                        member.canonical_agent_name,
                        member.role_summary,
                        member.harness.map(|harness| harness.to_string()),
                        member.logical_model,
                        member.logical_effort.map(|effort| effort.to_string()),
                        member
                            .delegated_session_id
                            .as_ref()
                            .map(|id| id.as_str().to_string()),
                        member.generation,
                        member
                            .current_run_id
                            .as_ref()
                            .map(|id| id.as_str().to_string()),
                        member
                            .current_assignment_id
                            .as_ref()
                            .map(|id| id.as_str().to_string()),
                        enum_to_db(&member.status, "team member status")?,
                        member.last_activity_at.map(|at| at.to_rfc3339()),
                        member.last_error,
                        member.created_at.to_rfc3339(),
                        member.updated_at.to_rfc3339(),
                        member.stopped_at.map(|at| at.to_rfc3339()),
                    ],
                )?;
                Ok(member)
            })
            .await
    }

    async fn get_member(&self, id: &TeamMemberId) -> AppResult<Option<TeamMember>> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare("SELECT * FROM managed_team_members WHERE id = ?1")?;
                let mut rows = stmt.query([id])?;
                match rows.next()? {
                    Some(row) => Ok(Some(member_from_row(row)?)),
                    None => Ok(None),
                }
            })
            .await
    }

    async fn list_members(&self, team_id: &TeamSessionId) -> AppResult<Vec<TeamMember>> {
        let team_id = team_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM managed_team_members WHERE team_id = ?1
                     ORDER BY created_at, id",
                )?;
                let mut rows = stmt.query([team_id])?;
                let mut members = Vec::new();
                while let Some(row) = rows.next()? {
                    members.push(member_from_row(row)?);
                }
                Ok(members)
            })
            .await
    }

    async fn update_member(&self, member: TeamMember, expected_generation: i64) -> AppResult<bool> {
        member.validate_name().map_err(AppError::Validation)?;
        self.db
            .run(move |conn| {
                let count = conn.execute(
                    "UPDATE managed_team_members SET
                        normalized_name = ?1, name = ?2, canonical_agent_name = ?3,
                        role_summary = ?4, harness = ?5, logical_model = ?6,
                        logical_effort = ?7, delegated_session_id = ?8, generation = ?9,
                        current_run_id = ?10, current_assignment_id = ?11, status = ?12,
                        last_activity_at = ?13, last_error = ?14, updated_at = ?15,
                        stopped_at = ?16
                     WHERE id = ?17 AND generation = ?18",
                    rusqlite::params![
                        member.normalized_name,
                        member.name,
                        member.canonical_agent_name,
                        member.role_summary,
                        member.harness.map(|harness| harness.to_string()),
                        member.logical_model,
                        member.logical_effort.map(|effort| effort.to_string()),
                        member
                            .delegated_session_id
                            .as_ref()
                            .map(|id| id.as_str().to_string()),
                        member.generation,
                        member
                            .current_run_id
                            .as_ref()
                            .map(|id| id.as_str().to_string()),
                        member
                            .current_assignment_id
                            .as_ref()
                            .map(|id| id.as_str().to_string()),
                        enum_to_db(&member.status, "team member status")?,
                        member.last_activity_at.map(|at| at.to_rfc3339()),
                        member.last_error,
                        member.updated_at.to_rfc3339(),
                        member.stopped_at.map(|at| at.to_rfc3339()),
                        member.id.as_str(),
                        expected_generation,
                    ],
                )?;
                Ok(count == 1)
            })
            .await
    }
}
