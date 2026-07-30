//! SQLite implementation of the managed Team run-binding repository.

use std::sync::Arc;

use async_trait::async_trait;
use rusqlite::Connection;
use tokio::sync::Mutex;

use super::sqlite_team_support::{
    enum_from_db, enum_to_db, parse_opt_team_timestamp, parse_team_timestamp,
};
use super::DbConnection;
use crate::domain::entities::{
    AgentRunId, AgentTaskAssignmentId, ChatConversationId, DelegatedSessionId, TeamMemberId,
    TeamRunBinding, TeamRunBindingId, TeamSessionId,
};
use crate::domain::repositories::TeamRunBindingRepository;
use crate::error::{AppError, AppResult};

pub struct SqliteTeamRunBindingRepository {
    db: DbConnection,
}

impl SqliteTeamRunBindingRepository {
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

fn binding_from_row(row: &rusqlite::Row<'_>) -> AppResult<TeamRunBinding> {
    Ok(TeamRunBinding {
        id: TeamRunBindingId::from_string(row.get::<_, String>("id")?),
        team_id: TeamSessionId::from_string(row.get::<_, String>("team_id")?),
        team_member_id: row
            .get::<_, Option<String>>("team_member_id")?
            .map(TeamMemberId::from_string),
        team_member_generation: row.get("team_member_generation")?,
        agent_run_id: AgentRunId::from_string(row.get::<_, String>("agent_run_id")?),
        conversation_id: ChatConversationId::from_string(row.get::<_, String>("conversation_id")?),
        delegated_session_id: row
            .get::<_, Option<String>>("delegated_session_id")?
            .map(DelegatedSessionId::from_string),
        trigger_kind: enum_from_db(row.get("trigger_kind")?, "run binding trigger kind")?,
        work_classification: enum_from_db(
            row.get("work_classification")?,
            "run binding work classification",
        )?,
        assignment_id: row
            .get::<_, Option<String>>("assignment_id")?
            .map(AgentTaskAssignmentId::from_string),
        first_message_sequence: row.get("first_message_sequence")?,
        last_message_sequence: row.get("last_message_sequence")?,
        status: enum_from_db(row.get("status")?, "run binding status")?,
        version: row.get("version")?,
        last_error: row.get("last_error")?,
        created_at: parse_team_timestamp(&row.get::<_, String>("created_at")?, "run binding")?,
        launched_at: parse_opt_team_timestamp(row.get("launched_at")?, "run binding")?,
        terminal_at: parse_opt_team_timestamp(row.get("terminal_at")?, "run binding")?,
    })
}

fn query_one_binding(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> AppResult<Option<TeamRunBinding>> {
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query(params)?;
    match rows.next()? {
        Some(row) => Ok(Some(binding_from_row(row)?)),
        None => Ok(None),
    }
}

#[async_trait]
impl TeamRunBindingRepository for SqliteTeamRunBindingRepository {
    async fn create(&self, binding: TeamRunBinding) -> AppResult<TeamRunBinding> {
        binding.validate().map_err(AppError::Validation)?;
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO managed_team_run_bindings (
                        id, team_id, team_member_id, team_member_generation, agent_run_id,
                        conversation_id, delegated_session_id, trigger_kind,
                        work_classification, assignment_id, first_message_sequence,
                        last_message_sequence, status, version, last_error, created_at,
                        launched_at, terminal_at
                    ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
                    rusqlite::params![
                        binding.id.as_str(),
                        binding.team_id.as_str(),
                        binding
                            .team_member_id
                            .as_ref()
                            .map(|id| id.as_str().to_string()),
                        binding.team_member_generation,
                        binding.agent_run_id.as_str(),
                        binding.conversation_id.as_str(),
                        binding
                            .delegated_session_id
                            .as_ref()
                            .map(|id| id.as_str().to_string()),
                        enum_to_db(&binding.trigger_kind, "run binding trigger kind")?,
                        enum_to_db(
                            &binding.work_classification,
                            "run binding work classification"
                        )?,
                        binding
                            .assignment_id
                            .as_ref()
                            .map(|id| id.as_str().to_string()),
                        binding.first_message_sequence,
                        binding.last_message_sequence,
                        enum_to_db(&binding.status, "run binding status")?,
                        binding.version,
                        binding.last_error,
                        binding.created_at.to_rfc3339(),
                        binding.launched_at.map(|at| at.to_rfc3339()),
                        binding.terminal_at.map(|at| at.to_rfc3339()),
                    ],
                )?;
                Ok(binding)
            })
            .await
    }

    async fn get_by_id(&self, id: &TeamRunBindingId) -> AppResult<Option<TeamRunBinding>> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                query_one_binding(
                    conn,
                    "SELECT * FROM managed_team_run_bindings WHERE id = ?1",
                    [id],
                )
            })
            .await
    }

    async fn get_by_agent_run_id(
        &self,
        agent_run_id: &AgentRunId,
    ) -> AppResult<Option<TeamRunBinding>> {
        let agent_run_id = agent_run_id.as_str();
        self.db
            .run(move |conn| {
                query_one_binding(
                    conn,
                    "SELECT * FROM managed_team_run_bindings WHERE agent_run_id = ?1",
                    [agent_run_id],
                )
            })
            .await
    }

    async fn list_for_team(&self, team_id: &TeamSessionId) -> AppResult<Vec<TeamRunBinding>> {
        let team_id = team_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM managed_team_run_bindings WHERE team_id = ?1
                     ORDER BY created_at, id",
                )?;
                let mut rows = stmt.query([team_id])?;
                let mut bindings = Vec::new();
                while let Some(row) = rows.next()? {
                    bindings.push(binding_from_row(row)?);
                }
                Ok(bindings)
            })
            .await
    }

    async fn get_current_member_binding(
        &self,
        member_id: &TeamMemberId,
        generation: i64,
    ) -> AppResult<Option<TeamRunBinding>> {
        let member_id = member_id.as_str().to_string();
        self.db
            .run(move |conn| {
                query_one_binding(
                    conn,
                    "SELECT * FROM managed_team_run_bindings
                     WHERE team_member_id = ?1 AND team_member_generation = ?2
                     ORDER BY created_at DESC, id DESC LIMIT 1",
                    rusqlite::params![member_id, generation],
                )
            })
            .await
    }

    async fn transition(
        &self,
        id: &TeamRunBindingId,
        expected_version: i64,
        binding: TeamRunBinding,
    ) -> AppResult<bool> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let count = conn.execute(
                    "UPDATE managed_team_run_bindings SET
                        team_member_id = ?1, team_member_generation = ?2,
                        delegated_session_id = ?3, first_message_sequence = ?4,
                        last_message_sequence = ?5, status = ?6, version = ?7,
                        last_error = ?8, launched_at = ?9, terminal_at = ?10
                     WHERE id = ?11 AND version = ?12",
                    rusqlite::params![
                        binding
                            .team_member_id
                            .as_ref()
                            .map(|member| member.as_str().to_string()),
                        binding.team_member_generation,
                        binding
                            .delegated_session_id
                            .as_ref()
                            .map(|session| session.as_str().to_string()),
                        binding.first_message_sequence,
                        binding.last_message_sequence,
                        enum_to_db(&binding.status, "run binding status")?,
                        binding.version,
                        binding.last_error,
                        binding.launched_at.map(|at| at.to_rfc3339()),
                        binding.terminal_at.map(|at| at.to_rfc3339()),
                        id,
                        expected_version,
                    ],
                )?;
                Ok(count == 1)
            })
            .await
    }

    async fn count_active_dispatches(&self, team_id: &TeamSessionId) -> AppResult<u32> {
        let team_id = team_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let count: u32 = conn.query_row(
                    "SELECT COUNT(*) FROM managed_team_run_bindings
                     WHERE team_id = ?1
                       AND team_member_id IS NOT NULL
                       AND status IN ('launching', 'running')",
                    [team_id],
                    |row| row.get(0),
                )?;
                Ok(count)
            })
            .await
    }
}
