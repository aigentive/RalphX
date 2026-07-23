use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension};
use tokio::sync::Mutex;

use super::sqlite_learned_skill_repos::{db_parse_error, parse_datetime};
use super::DbConnection;
use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{
    TaskOutcome, TaskOutcomeClass, TaskOutcomeId, TaskOutcomeSource, TaskOutcomeStatus,
};
use crate::domain::repositories::{
    resolve_task_outcome_upsert, TaskOutcomeListOptions, TaskOutcomeRepository,
    UpsertTaskOutcomeInput,
};
use crate::error::{AppError, AppResult};

pub struct SqliteTaskOutcomeRepository {
    db: DbConnection,
}

impl SqliteTaskOutcomeRepository {
    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }
}

fn outcome_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskOutcome> {
    let status = row
        .get::<_, String>("status")?
        .parse::<TaskOutcomeStatus>()
        .map_err(db_parse_error)?;
    let evidence =
        serde_json::from_str(&row.get::<_, String>("evidence_json")?).map_err(|error| {
            db_parse_error(AppError::Database(format!(
                "invalid task_outcomes evidence_json: {error}"
            )))
        })?;
    let source = row
        .get::<_, String>("source")?
        .parse::<TaskOutcomeSource>()
        .map_err(db_parse_error)?;
    let outcome_class = row
        .get::<_, Option<String>>("outcome_class")?
        .map(|value| TaskOutcomeClass::from(value.as_str()));
    Ok(TaskOutcome {
        id: TaskOutcomeId::from_string(row.get::<_, String>("id")?),
        project_id: ProjectId::from_string(row.get::<_, String>("project_id")?),
        source,
        source_ref_kind: row.get("source_ref_kind")?,
        source_ref_id: row.get("source_ref_id")?,
        task_id: row.get("task_id")?,
        conversation_id: row.get("conversation_id")?,
        agent_run_id: row.get("agent_run_id")?,
        pull_request_id: row.get("pull_request_id")?,
        proposal_id: row.get("proposal_id")?,
        verification_id: row.get("verification_id")?,
        review_id: row.get("review_id")?,
        outcome_class,
        status,
        evidence_json: evidence,
        failure_fingerprint: row.get("failure_fingerprint")?,
        provider_harness: row.get("provider_harness")?,
        provider_session_id: row.get("provider_session_id")?,
        created_at: parse_datetime(&row.get::<_, String>("created_at")?),
        updated_at: parse_datetime(&row.get::<_, String>("updated_at")?),
    })
}

fn select_outcome_columns() -> &'static str {
    "id, project_id, source, source_ref_kind, source_ref_id, task_id, conversation_id,
     agent_run_id, pull_request_id, proposal_id, verification_id, review_id, outcome_class,
     status, evidence_json, failure_fingerprint, provider_harness, provider_session_id,
     created_at, updated_at"
}

fn get_by_dedupe_from_connection(
    conn: &Connection,
    project_id: &str,
    source: &str,
    source_ref_kind: &str,
    source_ref_id: &str,
) -> AppResult<Option<TaskOutcome>> {
    conn.query_row(
        &format!(
            "SELECT {} FROM task_outcomes
             WHERE project_id = ?1 AND source = ?2
               AND source_ref_kind = ?3 AND source_ref_id = ?4",
            select_outcome_columns()
        ),
        rusqlite::params![project_id, source, source_ref_kind, source_ref_id],
        outcome_from_row,
    )
    .optional()
    .map_err(AppError::from)
}

#[async_trait]
impl TaskOutcomeRepository for SqliteTaskOutcomeRepository {
    async fn upsert(&self, input: UpsertTaskOutcomeInput) -> AppResult<TaskOutcome> {
        let mut incoming = input.outcome;
        incoming.updated_at = Utc::now();
        self.db
            .run_transaction(move |conn| {
                let existing = get_by_dedupe_from_connection(
                    conn,
                    incoming.project_id.as_str(),
                    incoming.source.as_str(),
                    &incoming.source_ref_kind,
                    &incoming.source_ref_id,
                )?;
                let resolution = resolve_task_outcome_upsert(existing.as_ref(), incoming);
                if !resolution.should_write {
                    return Ok(resolution.outcome);
                }
                let saved = resolution.outcome;
                let evidence_json = serde_json::to_string(&saved.evidence_json)
                    .map_err(|error| AppError::Database(error.to_string()))?;
                conn.execute(
                    "INSERT INTO task_outcomes (
                        id, project_id, source, source_ref_kind, source_ref_id, task_id,
                        conversation_id, agent_run_id, pull_request_id, proposal_id,
                        verification_id, review_id, outcome_class, status, evidence_json,
                        failure_fingerprint, provider_harness, provider_session_id,
                        created_at, updated_at
                    ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                        ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20
                    )
                    ON CONFLICT(project_id, source, source_ref_kind, source_ref_id)
                    DO UPDATE SET
                        task_id = excluded.task_id,
                        conversation_id = excluded.conversation_id,
                        agent_run_id = excluded.agent_run_id,
                        pull_request_id = excluded.pull_request_id,
                        proposal_id = excluded.proposal_id,
                        verification_id = excluded.verification_id,
                        review_id = excluded.review_id,
                        outcome_class = excluded.outcome_class,
                        status = excluded.status,
                        evidence_json = excluded.evidence_json,
                        failure_fingerprint = excluded.failure_fingerprint,
                        provider_harness = excluded.provider_harness,
                        provider_session_id = excluded.provider_session_id,
                        updated_at = excluded.updated_at",
                    rusqlite::params![
                        saved.id.as_str(),
                        saved.project_id.as_str(),
                        saved.source.to_string(),
                        saved.source_ref_kind,
                        saved.source_ref_id,
                        saved.task_id,
                        saved.conversation_id,
                        saved.agent_run_id,
                        saved.pull_request_id,
                        saved.proposal_id,
                        saved.verification_id,
                        saved.review_id,
                        saved.outcome_class.as_ref().map(ToString::to_string),
                        saved.status.to_string(),
                        evidence_json,
                        saved.failure_fingerprint,
                        saved.provider_harness,
                        saved.provider_session_id,
                        saved.created_at.to_rfc3339(),
                        saved.updated_at.to_rfc3339(),
                    ],
                )?;
                Ok(saved)
            })
            .await
    }

    async fn get_by_dedupe(
        &self,
        project_id: &ProjectId,
        source: TaskOutcomeSource,
        source_ref_kind: &str,
        source_ref_id: &str,
    ) -> AppResult<Option<TaskOutcome>> {
        let project_id = project_id.as_str().to_string();
        let source = source.to_string();
        let source_ref_kind = source_ref_kind.to_string();
        let source_ref_id = source_ref_id.to_string();
        self.db
            .run(move |conn| {
                get_by_dedupe_from_connection(
                    conn,
                    &project_id,
                    &source,
                    &source_ref_kind,
                    &source_ref_id,
                )
            })
            .await
    }

    async fn get_by_id(&self, id: &TaskOutcomeId) -> AppResult<Option<TaskOutcome>> {
        let id = id.as_str().to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    &format!(
                        "SELECT {} FROM task_outcomes WHERE id = ?1",
                        select_outcome_columns()
                    ),
                    [id],
                    outcome_from_row,
                )
            })
            .await
    }

    async fn list_by_project(
        &self,
        project_id: &ProjectId,
        options: TaskOutcomeListOptions,
    ) -> AppResult<Vec<TaskOutcome>> {
        let project_id = project_id.as_str().to_string();
        let source = options.source.map(|source| source.to_string());
        let status = options.status.map(|status| status.to_string());
        self.db
            .run(move |conn| {
                let mut statement = conn.prepare(&format!(
                    "SELECT {} FROM task_outcomes
                     WHERE project_id = ?1
                       AND (?2 IS NULL OR source = ?2)
                       AND (?3 IS NULL OR status = ?3)
                     ORDER BY updated_at DESC",
                    select_outcome_columns()
                ))?;
                let rows = statement
                    .query_map(
                        rusqlite::params![project_id, source, status],
                        outcome_from_row,
                    )?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
    }
}
