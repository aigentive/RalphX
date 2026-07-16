use std::{io, str::FromStr, sync::Arc};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{params, types::Type, Connection, OptionalExtension, Row};
use tokio::sync::Mutex;

use super::DbConnection;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{
    AgentWorkflowInvocation, AgentWorkflowInvocationId, AgentWorkflowLogEntry, AgentWorkflowPhase,
    AgentWorkflowPhaseId, AgentWorkflowProgress, AgentWorkflowRun, AgentWorkflowRunId,
    AgentWorkflowRunStatus, AgentWorkflowScript, AgentWorkflowScriptId, AgentWorkflowStepStatus,
    ChatConversationId, DelegatedSessionId, ProjectId,
};
use crate::domain::repositories::AgentWorkflowRepository;
use crate::error::{AppError, AppResult};

fn conversion_error(error: impl ToString) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        Type::Text,
        Box::new(io::Error::new(
            io::ErrorKind::InvalidData,
            error.to_string(),
        )),
    )
}

fn datetime(value: String) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(conversion_error)
}

fn optional_datetime(value: Option<String>) -> rusqlite::Result<Option<DateTime<Utc>>> {
    value.map(datetime).transpose()
}

fn row_to_script(row: &Row<'_>) -> rusqlite::Result<AgentWorkflowScript> {
    Ok(AgentWorkflowScript {
        id: AgentWorkflowScriptId::from_string(row.get::<_, String>("id")?),
        conversation_id: ChatConversationId::from_string(row.get::<_, String>("conversation_id")?),
        project_id: ProjectId::from_string(row.get::<_, String>("project_id")?),
        source: row.get("script_source")?,
        script_hash: row.get("script_hash")?,
        protocol_version: row.get("protocol_version")?,
        meta: serde_json::from_str(&row.get::<_, String>("meta_json")?)
            .map_err(conversion_error)?,
        permission_summary_json: row.get("permission_summary_json")?,
        permission_hash: row.get("permission_hash")?,
        estimated_fanout: row.get("estimated_fanout")?,
        approved_script_hash: row.get("approved_script_hash")?,
        approved_permission_hash: row.get("approved_permission_hash")?,
        approved_at: optional_datetime(row.get("approved_at")?)?,
        created_at: datetime(row.get("created_at")?)?,
        updated_at: datetime(row.get("updated_at")?)?,
    })
}

fn row_to_run(row: &Row<'_>) -> rusqlite::Result<AgentWorkflowRun> {
    Ok(AgentWorkflowRun {
        id: AgentWorkflowRunId::from_string(row.get::<_, String>("id")?),
        script_id: AgentWorkflowScriptId::from_string(row.get::<_, String>("script_id")?),
        conversation_id: ChatConversationId::from_string(row.get::<_, String>("conversation_id")?),
        project_id: ProjectId::from_string(row.get::<_, String>("project_id")?),
        harness: AgentHarnessKind::from_str(&row.get::<_, String>("harness")?)
            .map_err(conversion_error)?,
        script_hash: row.get("script_hash")?,
        permission_hash: row.get("permission_hash")?,
        args_json: row.get("args_json")?,
        status: AgentWorkflowRunStatus::from_str(&row.get::<_, String>("status")?)
            .map_err(conversion_error)?,
        attempt: row.get("attempt")?,
        runner_instance_id: row.get("runner_instance_id")?,
        lease_expires_at: optional_datetime(row.get("lease_expires_at")?)?,
        heartbeat_at: optional_datetime(row.get("heartbeat_at")?)?,
        pause_requested: row.get("pause_requested")?,
        cancel_requested: row.get("cancel_requested")?,
        result_json: row.get("result_json")?,
        error: row.get("error")?,
        created_at: datetime(row.get("created_at")?)?,
        updated_at: datetime(row.get("updated_at")?)?,
        completed_at: optional_datetime(row.get("completed_at")?)?,
    })
}

fn row_to_invocation(row: &Row<'_>) -> rusqlite::Result<AgentWorkflowInvocation> {
    Ok(AgentWorkflowInvocation {
        id: AgentWorkflowInvocationId::from_string(row.get::<_, String>("id")?),
        run_id: AgentWorkflowRunId::from_string(row.get::<_, String>("run_id")?),
        phase_id: row
            .get::<_, Option<String>>("phase_id")?
            .map(AgentWorkflowPhaseId::from_string),
        logical_key: row.get("logical_key")?,
        agent_name: row.get("agent_name")?,
        prompt_hash: row.get("prompt_hash")?,
        schema_hash: row.get("schema_hash")?,
        status: AgentWorkflowStepStatus::from_str(&row.get::<_, String>("status")?)
            .map_err(conversion_error)?,
        delegated_session_id: row
            .get::<_, Option<String>>("delegated_session_id")?
            .map(DelegatedSessionId::from_string),
        child_conversation_id: row
            .get::<_, Option<String>>("child_conversation_id")?
            .map(ChatConversationId::from_string),
        result_json: row.get("result_json")?,
        error: row.get("error")?,
        created_at: datetime(row.get("created_at")?)?,
        updated_at: datetime(row.get("updated_at")?)?,
        completed_at: optional_datetime(row.get("completed_at")?)?,
    })
}

pub struct SqliteAgentWorkflowRepository {
    db: DbConnection,
}

impl SqliteAgentWorkflowRepository {
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
impl AgentWorkflowRepository for SqliteAgentWorkflowRepository {
    async fn save_script(&self, script: AgentWorkflowScript) -> AppResult<AgentWorkflowScript> {
        self.db.run(move |conn| {
            script.meta.validate().map_err(AppError::Validation)?;
            let meta_json = serde_json::to_string(&script.meta)
                .map_err(|error| AppError::Validation(error.to_string()))?;
            conn.execute(
                "INSERT INTO agent_workflow_scripts (
                    id, conversation_id, project_id, name, description, script_source,
                    script_hash, protocol_version, meta_json, permission_summary_json,
                    permission_hash, estimated_fanout, approved_script_hash,
                    approved_permission_hash, approved_at, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
                 ON CONFLICT(id) DO UPDATE SET
                    name=excluded.name, description=excluded.description,
                    script_source=excluded.script_source, script_hash=excluded.script_hash,
                    meta_json=excluded.meta_json,
                    permission_summary_json=excluded.permission_summary_json,
                    permission_hash=excluded.permission_hash,
                    estimated_fanout=excluded.estimated_fanout, updated_at=excluded.updated_at",
                params![script.id.as_str(), script.conversation_id.as_str(), script.project_id.as_str(),
                    script.meta.name, script.meta.description, script.source, script.script_hash,
                    script.protocol_version, meta_json, script.permission_summary_json,
                    script.permission_hash, script.estimated_fanout, script.approved_script_hash,
                    script.approved_permission_hash, script.approved_at.map(|v| v.to_rfc3339()),
                    script.created_at.to_rfc3339(), script.updated_at.to_rfc3339()],
            )?;
            Ok(script)
        }).await
    }

    async fn get_script(
        &self,
        id: &AgentWorkflowScriptId,
    ) -> AppResult<Option<AgentWorkflowScript>> {
        let id = id.to_string();
        self.db
            .run(move |conn| {
                conn.query_row(
                    "SELECT * FROM agent_workflow_scripts WHERE id=?1",
                    [id],
                    row_to_script,
                )
                .optional()
                .map_err(Into::into)
            })
            .await
    }

    async fn approve_script(
        &self,
        id: &AgentWorkflowScriptId,
        script_hash: &str,
        permission_hash: &str,
    ) -> AppResult<bool> {
        let (id, script_hash, permission_hash) = (
            id.to_string(),
            script_hash.to_string(),
            permission_hash.to_string(),
        );
        self.db
            .run(move |conn| {
                Ok(conn.execute(
                    "UPDATE agent_workflow_scripts SET approved_script_hash=script_hash,
             approved_permission_hash=permission_hash, approved_at=?1, updated_at=?1
             WHERE id=?2 AND script_hash=?3 AND permission_hash=?4",
                    params![Utc::now().to_rfc3339(), id, script_hash, permission_hash],
                )? == 1)
            })
            .await
    }

    async fn create_run(&self, run: AgentWorkflowRun) -> AppResult<AgentWorkflowRun> {
        self.db.run(move |conn| {
            let tx = conn.unchecked_transaction()?;
            if let Some(existing) = tx
                .query_row(
                    "SELECT * FROM agent_workflow_runs WHERE id=?1",
                    [run.id.as_str()],
                    row_to_run,
                )
                .optional()?
            {
                if existing.script_id != run.script_id
                    || existing.script_hash != run.script_hash
                    || existing.permission_hash != run.permission_hash
                    || existing.args_json != run.args_json
                {
                    return Err(AppError::Conflict(
                        "Workflow launch id was reused with different inputs".into(),
                    ));
                }
                tx.execute(
                    "UPDATE agent_workflow_scripts SET approved_script_hash=NULL,
                     approved_permission_hash=NULL, approved_at=NULL, updated_at=?1 WHERE id=?2",
                    params![Utc::now().to_rfc3339(), run.script_id.as_str()],
                )?;
                tx.commit()?;
                return Ok(existing);
            }
            let approved: bool = tx.query_row(
                "SELECT approved_at IS NOT NULL AND approved_script_hash=?2 AND approved_permission_hash=?3
                 AND script_hash=?2 AND permission_hash=?3 FROM agent_workflow_scripts WHERE id=?1",
                params![run.script_id.as_str(), run.script_hash, run.permission_hash], |row| row.get(0),
            ).optional()?.unwrap_or(false);
            if !approved { return Err(AppError::Validation("Workflow launch requires approval for the current script and permission hashes".into())); }
            tx.execute(
                "INSERT INTO agent_workflow_runs (id, script_id, conversation_id, project_id, harness,
                 script_hash, permission_hash, args_json, status, attempt, runner_instance_id,
                 lease_expires_at, heartbeat_at, pause_requested, cancel_requested, result_json,
                 error, created_at, updated_at, completed_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)",
                params![run.id.as_str(), run.script_id.as_str(), run.conversation_id.as_str(), run.project_id.as_str(),
                    run.harness.to_string(), run.script_hash, run.permission_hash, run.args_json, run.status.to_string(), run.attempt,
                    run.runner_instance_id, run.lease_expires_at.map(|v| v.to_rfc3339()), run.heartbeat_at.map(|v| v.to_rfc3339()),
                    run.pause_requested, run.cancel_requested, run.result_json, run.error, run.created_at.to_rfc3339(),
                    run.updated_at.to_rfc3339(), run.completed_at.map(|v| v.to_rfc3339())],
            )?;
            tx.execute(
                "UPDATE agent_workflow_scripts SET approved_script_hash=NULL,
                 approved_permission_hash=NULL, approved_at=NULL, updated_at=?1 WHERE id=?2",
                params![Utc::now().to_rfc3339(), run.script_id.as_str()],
            )?;
            tx.commit()?;
            Ok(run)
        }).await
    }

    async fn get_run(&self, id: &AgentWorkflowRunId) -> AppResult<Option<AgentWorkflowRun>> {
        let id = id.to_string();
        self.db
            .run(move |conn| {
                conn.query_row(
                    "SELECT * FROM agent_workflow_runs WHERE id=?1",
                    [id],
                    row_to_run,
                )
                .optional()
                .map_err(Into::into)
            })
            .await
    }

    async fn get_latest_run_for_script(
        &self,
        script_id: &AgentWorkflowScriptId,
    ) -> AppResult<Option<AgentWorkflowRun>> {
        let script_id = script_id.to_string();
        self.db
            .run(move |conn| {
                conn.query_row(
                    "SELECT * FROM agent_workflow_runs WHERE script_id=?1
                     ORDER BY created_at DESC, id DESC LIMIT 1",
                    [script_id],
                    row_to_run,
                )
                .optional()
                .map_err(Into::into)
            })
            .await
    }

    async fn get_progress(&self, id: &AgentWorkflowRunId) -> AppResult<AgentWorkflowProgress> {
        let id = id.to_string();
        self.db
            .run(move |conn| {
                let run = conn
                    .query_row(
                        "SELECT * FROM agent_workflow_runs WHERE id=?1",
                        [&id],
                        row_to_run,
                    )
                    .optional()?
                    .ok_or_else(|| AppError::NotFound(format!("Workflow run {id}")))?;
                let mut phase_stmt = conn.prepare(
                    "SELECT * FROM agent_workflow_phases WHERE run_id=?1 ORDER BY ordinal",
                )?;
                let phases = phase_stmt
                    .query_map([&id], |row| {
                        Ok(AgentWorkflowPhase {
                            id: AgentWorkflowPhaseId::from_string(row.get::<_, String>("id")?),
                            run_id: AgentWorkflowRunId::from_string(
                                row.get::<_, String>("run_id")?,
                            ),
                            key: row.get("phase_key")?,
                            name: row.get("name")?,
                            ordinal: row.get("ordinal")?,
                            status: AgentWorkflowStepStatus::from_str(
                                &row.get::<_, String>("status")?,
                            )
                            .map_err(conversion_error)?,
                            started_at: optional_datetime(row.get("started_at")?)?,
                            completed_at: optional_datetime(row.get("completed_at")?)?,
                            error: row.get("error")?,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                let mut invocation_stmt = conn.prepare(
                    "SELECT * FROM agent_workflow_invocations WHERE run_id=?1 ORDER BY created_at",
                )?;
                let invocations = invocation_stmt
                    .query_map([&id], row_to_invocation)?
                    .collect::<Result<Vec<_>, _>>()?;
                let mut log_stmt = conn.prepare(
                    "SELECT * FROM agent_workflow_logs WHERE run_id=?1 ORDER BY sequence",
                )?;
                let logs = log_stmt
                    .query_map([&id], |row| {
                        Ok(AgentWorkflowLogEntry {
                            run_id: AgentWorkflowRunId::from_string(
                                row.get::<_, String>("run_id")?,
                            ),
                            sequence: row.get("sequence")?,
                            level: row.get("level")?,
                            message: row.get("message")?,
                            created_at: datetime(row.get("created_at")?)?,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(AgentWorkflowProgress {
                    run,
                    phases,
                    invocations,
                    logs,
                })
            })
            .await
    }

    async fn claim_run(
        &self,
        id: &AgentWorkflowRunId,
        expected_attempt: u32,
        runner_instance_id: &str,
        lease_expires_at: DateTime<Utc>,
    ) -> AppResult<bool> {
        let (id, runner) = (id.to_string(), runner_instance_id.to_string());
        self.db
            .run(move |conn| {
                Ok(conn.execute(
                    "UPDATE agent_workflow_runs SET status='running', attempt=attempt+1,
             runner_instance_id=?1, lease_expires_at=?2, heartbeat_at=?3, updated_at=?3
             WHERE id=?4 AND attempt=?5 AND status IN ('queued','recovering','paused')",
                    params![
                        runner,
                        lease_expires_at.to_rfc3339(),
                        Utc::now().to_rfc3339(),
                        id,
                        expected_attempt
                    ],
                )? == 1)
            })
            .await
    }

    async fn heartbeat(
        &self,
        id: &AgentWorkflowRunId,
        attempt: u32,
        runner_instance_id: &str,
        lease_expires_at: DateTime<Utc>,
    ) -> AppResult<bool> {
        let (id, runner) = (id.to_string(), runner_instance_id.to_string());
        self.db
            .run(move |conn| {
                Ok(conn.execute(
            "UPDATE agent_workflow_runs SET heartbeat_at=?1, lease_expires_at=?2, updated_at=?1
             WHERE id=?3 AND attempt=?4 AND runner_instance_id=?5
               AND status IN ('running','pause_requested')",
            params![Utc::now().to_rfc3339(), lease_expires_at.to_rfc3339(), id, attempt, runner],
        )? == 1)
            })
            .await
    }

    async fn transition_run(
        &self,
        id: &AgentWorkflowRunId,
        attempt: u32,
        runner_instance_id: &str,
        from: AgentWorkflowRunStatus,
        to: AgentWorkflowRunStatus,
        result_json: Option<String>,
        error: Option<String>,
    ) -> AppResult<bool> {
        let (id, runner) = (id.to_string(), runner_instance_id.to_string());
        let terminal_at = to.is_terminal().then(|| Utc::now().to_rfc3339());
        self.db
            .run(move |conn| {
                Ok(conn.execute(
                    "UPDATE agent_workflow_runs SET status=?1, result_json=?2, error=?3,
             completed_at=?4, updated_at=?5 WHERE id=?6 AND attempt=?7
             AND runner_instance_id=?8 AND status=?9",
                    params![
                        to.to_string(),
                        result_json,
                        error,
                        terminal_at,
                        Utc::now().to_rfc3339(),
                        id,
                        attempt,
                        runner,
                        from.to_string()
                    ],
                )? == 1)
            })
            .await
    }

    async fn request_pause(&self, id: &AgentWorkflowRunId) -> AppResult<bool> {
        let id = id.to_string();
        self.db.run(move |conn| Ok(conn.execute("UPDATE agent_workflow_runs SET pause_requested=1, status=CASE WHEN status IN ('queued','recovering') THEN 'paused' ELSE 'pause_requested' END, updated_at=?1 WHERE id=?2 AND status IN ('queued','recovering','running')", params![Utc::now().to_rfc3339(), id])? == 1)).await
    }

    async fn resume_run(&self, id: &AgentWorkflowRunId) -> AppResult<bool> {
        let id = id.to_string();
        self.db.run(move |conn| Ok(conn.execute(
            "UPDATE agent_workflow_runs SET pause_requested=0, cancel_requested=0, status='queued', runner_instance_id=NULL, lease_expires_at=NULL, heartbeat_at=NULL, updated_at=?1 WHERE id=?2 AND status='paused'",
            params![Utc::now().to_rfc3339(), id],
        )? == 1)).await
    }

    async fn request_cancel(&self, id: &AgentWorkflowRunId) -> AppResult<bool> {
        let id = id.to_string();
        self.db.run(move |conn| {
            let now = Utc::now().to_rfc3339();
            Ok(conn.execute(
                "UPDATE agent_workflow_runs SET cancel_requested=1, status=CASE WHEN status IN ('queued','recovering','paused') THEN 'cancelled' ELSE status END, completed_at=CASE WHEN status IN ('queued','recovering','paused') THEN ?1 ELSE completed_at END, updated_at=?1 WHERE id=?2 AND status NOT IN ('completed','failed','cancelled')",
                params![now, id],
            )? == 1)
        }).await
    }

    async fn prepare_recovery(
        &self,
        id: &AgentWorkflowRunId,
        expected_attempt: u32,
        now: DateTime<Utc>,
    ) -> AppResult<bool> {
        let id = id.to_string();
        self.db.run(move |conn| Ok(conn.execute(
            "UPDATE agent_workflow_runs SET status=CASE WHEN pause_requested=1 THEN 'paused' ELSE 'recovering' END, runner_instance_id=NULL, lease_expires_at=NULL, heartbeat_at=NULL, updated_at=?1 WHERE id=?2 AND attempt=?3 AND status IN ('running','pause_requested') AND (lease_expires_at IS NULL OR lease_expires_at < ?1)",
            params![now.to_rfc3339(), id, expected_attempt],
        )? == 1)).await
    }

    async fn fail_unclaimed_run(
        &self,
        id: &AgentWorkflowRunId,
        expected_status: AgentWorkflowRunStatus,
        error: &str,
    ) -> AppResult<bool> {
        if !matches!(
            expected_status,
            AgentWorkflowRunStatus::Queued | AgentWorkflowRunStatus::Recovering
        ) {
            return Ok(false);
        }
        let id = id.to_string();
        let error = error.to_string();
        self.db
            .run(move |conn| {
                let now = Utc::now().to_rfc3339();
                Ok(conn.execute(
                    "UPDATE agent_workflow_runs SET status='failed', error=?1, completed_at=?2, updated_at=?2 WHERE id=?3 AND status=?4 AND runner_instance_id IS NULL",
                    params![error, now, id, expected_status.to_string()],
                )? == 1)
            })
            .await
    }

    async fn begin_invocation(
        &self,
        invocation: AgentWorkflowInvocation,
    ) -> AppResult<AgentWorkflowInvocation> {
        self.db.run(move |conn| {
            conn.execute("INSERT INTO agent_workflow_invocations (id,run_id,phase_id,logical_key,agent_name,prompt_hash,schema_hash,status,delegated_session_id,child_conversation_id,result_json,error,created_at,updated_at,completed_at)
                VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15) ON CONFLICT(run_id, logical_key) DO NOTHING",
                params![invocation.id.as_str(), invocation.run_id.as_str(), invocation.phase_id.as_ref().map(|v| v.as_str()), invocation.logical_key,
                invocation.agent_name, invocation.prompt_hash, invocation.schema_hash, invocation.status.to_string(), invocation.delegated_session_id.as_ref().map(|v| v.as_str()),
                invocation.child_conversation_id.as_ref().map(|v| v.as_str()), invocation.result_json, invocation.error, invocation.created_at.to_rfc3339(), invocation.updated_at.to_rfc3339(), invocation.completed_at.map(|v| v.to_rfc3339())])?;
            conn.query_row("SELECT * FROM agent_workflow_invocations WHERE run_id=?1 AND logical_key=?2", params![invocation.run_id.as_str(), invocation.logical_key], row_to_invocation).map_err(Into::into)
        }).await
    }

    async fn upsert_phase(
        &self,
        phase: AgentWorkflowPhase,
        attempt: u32,
        runner_instance_id: &str,
    ) -> AppResult<bool> {
        let runner_instance_id = runner_instance_id.to_string();
        self.db.run(move |conn| {
            let owned: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM agent_workflow_runs WHERE id=?1 AND attempt=?2 AND runner_instance_id=?3 AND status IN ('running','pause_requested'))",
                params![phase.run_id.as_str(), attempt, runner_instance_id], |row| row.get(0),
            )?;
            if !owned { return Ok(false); }
            conn.execute(
                "INSERT INTO agent_workflow_phases (id,run_id,phase_key,name,ordinal,status,started_at,completed_at,error)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
                 ON CONFLICT(run_id,phase_key) DO UPDATE SET name=excluded.name, ordinal=excluded.ordinal,
                 status=excluded.status, started_at=excluded.started_at, completed_at=excluded.completed_at, error=excluded.error",
                params![phase.id.as_str(), phase.run_id.as_str(), phase.key, phase.name, phase.ordinal,
                    phase.status.to_string(), phase.started_at.map(|v| v.to_rfc3339()),
                    phase.completed_at.map(|v| v.to_rfc3339()), phase.error],
            )?;
            Ok(true)
        }).await
    }

    async fn settle_invocation(
        &self,
        invocation_id: &str,
        attempt: u32,
        runner_instance_id: &str,
        status: AgentWorkflowStepStatus,
        delegated_session_id: Option<String>,
        child_conversation_id: Option<String>,
        result_json: Option<String>,
        error: Option<String>,
    ) -> AppResult<bool> {
        let invocation_id = invocation_id.to_string();
        let runner_instance_id = runner_instance_id.to_string();
        let completed_at = matches!(
            status,
            AgentWorkflowStepStatus::Completed
                | AgentWorkflowStepStatus::Failed
                | AgentWorkflowStepStatus::Cancelled
        )
        .then(|| Utc::now().to_rfc3339());
        self.db
            .run(move |conn| {
                Ok(conn.execute(
                    "UPDATE agent_workflow_invocations SET status=?1, delegated_session_id=?2,
             child_conversation_id=?3, result_json=?4, error=?5, completed_at=?6, updated_at=?7
             WHERE id=?8 AND EXISTS(SELECT 1 FROM agent_workflow_runs r
               WHERE r.id=agent_workflow_invocations.run_id AND r.attempt=?9
               AND r.runner_instance_id=?10 AND r.status IN ('running','pause_requested'))",
                    params![
                        status.to_string(),
                        delegated_session_id,
                        child_conversation_id,
                        result_json,
                        error,
                        completed_at,
                        Utc::now().to_rfc3339(),
                        invocation_id,
                        attempt,
                        runner_instance_id
                    ],
                )? == 1)
            })
            .await
    }

    async fn append_log(
        &self,
        run_id: &AgentWorkflowRunId,
        attempt: u32,
        runner_instance_id: &str,
        level: &str,
        message: &str,
    ) -> AppResult<Option<AgentWorkflowLogEntry>> {
        if !matches!(level, "debug" | "info" | "warn" | "error") {
            return Err(AppError::Validation("Invalid workflow log level".into()));
        }
        let (run_id, runner_instance_id, level, message) = (
            run_id.to_string(),
            runner_instance_id.to_string(),
            level.to_string(),
            message.to_string(),
        );
        self.db.run(move |conn| {
            let tx = conn.unchecked_transaction()?;
            let owned: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM agent_workflow_runs WHERE id=?1 AND attempt=?2 AND runner_instance_id=?3 AND status IN ('running','pause_requested'))",
                params![run_id, attempt, runner_instance_id], |row| row.get(0),
            )?;
            if !owned { return Ok(None); }
            let sequence: u64 = tx.query_row("SELECT COALESCE(MAX(sequence), -1) + 1 FROM agent_workflow_logs WHERE run_id=?1", [&run_id], |row| row.get(0))?;
            let created_at = Utc::now();
            tx.execute("INSERT INTO agent_workflow_logs (run_id,sequence,level,message,created_at) VALUES (?1,?2,?3,?4,?5)",
                params![run_id, sequence, level, message, created_at.to_rfc3339()])?;
            tx.commit()?;
            Ok(Some(AgentWorkflowLogEntry { run_id: AgentWorkflowRunId::from_string(run_id), sequence, level, message, created_at }))
        }).await
    }

    async fn list_recoverable(&self, now: DateTime<Utc>) -> AppResult<Vec<AgentWorkflowRun>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_workflow_runs
                     WHERE status IN ('queued','recovering','paused')
                        OR (status IN ('running','pause_requested')
                            AND (lease_expires_at IS NULL OR lease_expires_at < ?1))
                     ORDER BY created_at",
                )?;
                let runs = stmt
                    .query_map([now.to_rfc3339()], row_to_run)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(runs)
            })
            .await
    }
}
