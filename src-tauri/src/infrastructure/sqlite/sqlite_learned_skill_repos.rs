use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rusqlite::Connection;
use tokio::sync::Mutex;

use super::DbConnection;
use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{
    ProjectSkill, ProjectSkillId, ProjectSkillLifecycleStatus, SkillUsageEvent, SkillUsageEventId,
    TaskOutcome, TaskOutcomeId, TaskOutcomeStatus,
};
use crate::domain::repositories::{
    ProjectSkillListOptions, ProjectSkillRepository, SkillUsageEventRepository,
    SkillUsageListOptions, TaskOutcomeListOptions, TaskOutcomeRepository, UpsertTaskOutcomeInput,
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

pub struct SqliteProjectSkillRepository {
    db: DbConnection,
}

impl SqliteProjectSkillRepository {
    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }
}

pub struct SqliteSkillUsageEventRepository {
    db: DbConnection,
}

impl SqliteSkillUsageEventRepository {
    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }
}

fn parse_datetime(value: &str) -> DateTime<Utc> {
    if let Ok(value) = DateTime::parse_from_rfc3339(value) {
        return value.with_timezone(&Utc);
    }
    if let Ok(value) = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S") {
        return Utc.from_utc_datetime(&value);
    }
    Utc::now()
}

fn db_parse_error(error: AppError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
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
    Ok(TaskOutcome {
        id: TaskOutcomeId::from_string(row.get::<_, String>("id")?),
        project_id: ProjectId::from_string(row.get::<_, String>("project_id")?),
        source: row.get("source")?,
        source_ref_kind: row.get("source_ref_kind")?,
        source_ref_id: row.get("source_ref_id")?,
        task_id: row.get("task_id")?,
        conversation_id: row.get("conversation_id")?,
        agent_run_id: row.get("agent_run_id")?,
        pull_request_id: row.get("pull_request_id")?,
        proposal_id: row.get("proposal_id")?,
        verification_id: row.get("verification_id")?,
        review_id: row.get("review_id")?,
        outcome_class: row.get("outcome_class")?,
        status,
        evidence_json: evidence,
        provider_harness: row.get("provider_harness")?,
        provider_session_id: row.get("provider_session_id")?,
        created_at: parse_datetime(&row.get::<_, String>("created_at")?),
        updated_at: parse_datetime(&row.get::<_, String>("updated_at")?),
    })
}

fn skill_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectSkill> {
    let status = row
        .get::<_, String>("status")?
        .parse::<ProjectSkillLifecycleStatus>()
        .map_err(db_parse_error)?;
    let scope_paths =
        serde_json::from_str(&row.get::<_, String>("scope_paths_json")?).map_err(|error| {
            db_parse_error(AppError::Database(format!(
                "invalid project_skills scope_paths_json: {error}"
            )))
        })?;
    let provenance =
        serde_json::from_str(&row.get::<_, String>("provenance_json")?).map_err(|error| {
            db_parse_error(AppError::Database(format!(
                "invalid project_skills provenance_json: {error}"
            )))
        })?;
    Ok(ProjectSkill {
        id: ProjectSkillId::from_string(row.get::<_, String>("id")?),
        project_id: ProjectId::from_string(row.get::<_, String>("project_id")?),
        title: row.get("title")?,
        bucket: row.get("bucket")?,
        stage: row.get("stage")?,
        status,
        pinned: row.get::<_, i64>("pinned")? != 0,
        archived: row.get::<_, i64>("archived")? != 0,
        scope_paths,
        compact_guidance: row.get("compact_guidance")?,
        body_markdown: row.get("body_markdown")?,
        predicted_effect: row.get("predicted_effect")?,
        provenance_json: provenance,
        companion_of_skill_id: row
            .get::<_, Option<String>>("companion_of_skill_id")?
            .map(ProjectSkillId::from_string),
        created_at: parse_datetime(&row.get::<_, String>("created_at")?),
        updated_at: parse_datetime(&row.get::<_, String>("updated_at")?),
    })
}

fn usage_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SkillUsageEvent> {
    let metadata =
        serde_json::from_str(&row.get::<_, String>("metadata_json")?).map_err(|error| {
            db_parse_error(AppError::Database(format!(
                "invalid skill_usage_events metadata_json: {error}"
            )))
        })?;
    Ok(SkillUsageEvent {
        id: SkillUsageEventId::from_string(row.get::<_, String>("id")?),
        project_id: ProjectId::from_string(row.get::<_, String>("project_id")?),
        project_skill_id: ProjectSkillId::from_string(row.get::<_, String>("project_skill_id")?),
        conversation_id: row.get("conversation_id")?,
        agent_run_id: row.get("agent_run_id")?,
        provider_harness: row.get("provider_harness")?,
        stage: row.get("stage")?,
        bucket: row.get("bucket")?,
        injection_kind: row.get("injection_kind")?,
        outcome_id: row
            .get::<_, Option<String>>("outcome_id")?
            .map(TaskOutcomeId::from_string),
        metadata_json: metadata,
        created_at: parse_datetime(&row.get::<_, String>("created_at")?),
    })
}

fn select_outcome_columns() -> &'static str {
    "id, project_id, source, source_ref_kind, source_ref_id, task_id, conversation_id,
     agent_run_id, pull_request_id, proposal_id, verification_id, review_id, outcome_class,
     status, evidence_json, provider_harness, provider_session_id, created_at, updated_at"
}

fn select_skill_columns() -> &'static str {
    "id, project_id, title, bucket, stage, status, pinned, archived, scope_paths_json,
     compact_guidance, body_markdown, predicted_effect, provenance_json, companion_of_skill_id,
     created_at, updated_at"
}

fn select_usage_columns() -> &'static str {
    "id, project_id, project_skill_id, conversation_id, agent_run_id, provider_harness,
     stage, bucket, injection_kind, outcome_id, metadata_json, created_at"
}

#[async_trait]
impl TaskOutcomeRepository for SqliteTaskOutcomeRepository {
    async fn upsert(&self, input: UpsertTaskOutcomeInput) -> AppResult<TaskOutcome> {
        let mut outcome = input.outcome;
        let evidence_json = serde_json::to_string(&outcome.evidence_json)
            .map_err(|error| AppError::Database(error.to_string()))?;
        let now = Utc::now();
        outcome.updated_at = now;
        let inserted = outcome.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO task_outcomes (
                        id, project_id, source, source_ref_kind, source_ref_id, task_id,
                        conversation_id, agent_run_id, pull_request_id, proposal_id,
                        verification_id, review_id, outcome_class, status, evidence_json,
                        provider_harness, provider_session_id, created_at, updated_at
                    ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                        ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19
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
                        provider_harness = excluded.provider_harness,
                        provider_session_id = excluded.provider_session_id,
                        updated_at = excluded.updated_at",
                    rusqlite::params![
                        inserted.id.as_str(),
                        inserted.project_id.as_str(),
                        inserted.source,
                        inserted.source_ref_kind,
                        inserted.source_ref_id,
                        inserted.task_id,
                        inserted.conversation_id,
                        inserted.agent_run_id,
                        inserted.pull_request_id,
                        inserted.proposal_id,
                        inserted.verification_id,
                        inserted.review_id,
                        inserted.outcome_class,
                        inserted.status.to_string(),
                        evidence_json,
                        inserted.provider_harness,
                        inserted.provider_session_id,
                        inserted.created_at.to_rfc3339(),
                        inserted.updated_at.to_rfc3339(),
                    ],
                )?;
                conn.query_row(
                    &format!(
                        "SELECT {} FROM task_outcomes
                         WHERE project_id = ?1 AND source = ?2
                           AND source_ref_kind = ?3 AND source_ref_id = ?4",
                        select_outcome_columns()
                    ),
                    rusqlite::params![
                        outcome.project_id.as_str(),
                        outcome.source,
                        outcome.source_ref_kind,
                        outcome.source_ref_id,
                    ],
                    outcome_from_row,
                )
                .map_err(AppError::from)
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
        let source = options.source;
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

#[async_trait]
impl ProjectSkillRepository for SqliteProjectSkillRepository {
    async fn create(&self, skill: ProjectSkill) -> AppResult<ProjectSkill> {
        let scope_paths_json = serde_json::to_string(&skill.scope_paths)
            .map_err(|error| AppError::Database(error.to_string()))?;
        let provenance_json = serde_json::to_string(&skill.provenance_json)
            .map_err(|error| AppError::Database(error.to_string()))?;
        let saved = skill.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO project_skills (
                        id, project_id, title, bucket, stage, status, pinned, archived,
                        scope_paths_json, compact_guidance, body_markdown, predicted_effect,
                        provenance_json, companion_of_skill_id, created_at, updated_at
                    ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
                        ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16
                    )",
                    rusqlite::params![
                        saved.id.as_str(),
                        saved.project_id.as_str(),
                        saved.title,
                        saved.bucket,
                        saved.stage,
                        saved.status.to_string(),
                        if saved.pinned { 1 } else { 0 },
                        if saved.archived { 1 } else { 0 },
                        scope_paths_json,
                        saved.compact_guidance,
                        saved.body_markdown,
                        saved.predicted_effect,
                        provenance_json,
                        saved
                            .companion_of_skill_id
                            .as_ref()
                            .map(|id| id.as_str().to_string()),
                        saved.created_at.to_rfc3339(),
                        saved.updated_at.to_rfc3339(),
                    ],
                )?;
                Ok(skill)
            })
            .await
    }

    async fn get_by_id(&self, id: &ProjectSkillId) -> AppResult<Option<ProjectSkill>> {
        let id = id.as_str().to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    &format!(
                        "SELECT {} FROM project_skills WHERE id = ?1",
                        select_skill_columns()
                    ),
                    [id],
                    skill_from_row,
                )
            })
            .await
    }

    async fn list_by_project(
        &self,
        project_id: &ProjectId,
        options: ProjectSkillListOptions,
    ) -> AppResult<Vec<ProjectSkill>> {
        let project_id = project_id.as_str().to_string();
        let status = options.status.map(|status| status.to_string());
        let stage = options.stage;
        let bucket = options.bucket;
        let include_archived = options.include_archived;
        let scope_path = options.scope_path;
        self.db
            .run(move |conn| {
                let mut statement = conn.prepare(&format!(
                    "SELECT {} FROM project_skills
                     WHERE project_id = ?1
                       AND (?2 IS NULL OR status = ?2)
                       AND (?3 IS NULL OR stage = ?3)
                       AND (?4 IS NULL OR bucket = ?4)
                       AND (?5 = 1 OR archived = 0)
                     ORDER BY pinned DESC, updated_at DESC, title ASC",
                    select_skill_columns()
                ))?;
                let rows = statement
                    .query_map(
                        rusqlite::params![
                            project_id,
                            status,
                            stage,
                            bucket,
                            if include_archived { 1 } else { 0 }
                        ],
                        skill_from_row,
                    )?
                    .collect::<Result<Vec<_>, _>>()?;
                if let Some(scope_path) = scope_path {
                    Ok(rows
                        .into_iter()
                        .filter(|row| {
                            row.scope_paths.is_empty()
                                || row
                                    .scope_paths
                                    .iter()
                                    .any(|path| scope_path.starts_with(path))
                        })
                        .collect())
                } else {
                    Ok(rows)
                }
            })
            .await
    }

    async fn update_content(&self, mut skill: ProjectSkill) -> AppResult<Option<ProjectSkill>> {
        let scope_paths_json = serde_json::to_string(&skill.scope_paths)
            .map_err(|error| AppError::Database(error.to_string()))?;
        skill.updated_at = Utc::now();
        let saved = skill;
        self.db
            .query_optional(move |conn| {
                conn.execute(
                    "UPDATE project_skills
                     SET title = ?2,
                         bucket = ?3,
                         stage = ?4,
                         scope_paths_json = ?5,
                         compact_guidance = ?6,
                         body_markdown = ?7,
                         predicted_effect = ?8,
                         updated_at = ?9
                     WHERE id = ?1",
                    rusqlite::params![
                        saved.id.as_str(),
                        saved.title,
                        saved.bucket,
                        saved.stage,
                        scope_paths_json,
                        saved.compact_guidance,
                        saved.body_markdown,
                        saved.predicted_effect,
                        saved.updated_at.to_rfc3339(),
                    ],
                )?;
                conn.query_row(
                    &format!(
                        "SELECT {} FROM project_skills WHERE id = ?1",
                        select_skill_columns()
                    ),
                    [saved.id.as_str()],
                    skill_from_row,
                )
            })
            .await
    }

    async fn update_lifecycle_status(
        &self,
        id: &ProjectSkillId,
        status: ProjectSkillLifecycleStatus,
    ) -> AppResult<Option<ProjectSkill>> {
        let id = id.as_str().to_string();
        let status_text = status.to_string();
        let archived = if matches!(
            status,
            ProjectSkillLifecycleStatus::Archived | ProjectSkillLifecycleStatus::Retired
        ) {
            1
        } else {
            0
        };
        self.db
            .query_optional(move |conn| {
                conn.execute(
                    "UPDATE project_skills
                     SET status = ?2, archived = ?3, updated_at = ?4
                     WHERE id = ?1",
                    rusqlite::params![id, status_text, archived, Utc::now().to_rfc3339()],
                )?;
                conn.query_row(
                    &format!(
                        "SELECT {} FROM project_skills WHERE id = ?1",
                        select_skill_columns()
                    ),
                    [&id],
                    skill_from_row,
                )
            })
            .await
    }

    async fn update_pinned(
        &self,
        id: &ProjectSkillId,
        pinned: bool,
    ) -> AppResult<Option<ProjectSkill>> {
        let id = id.as_str().to_string();
        let pinned_value = if pinned { 1 } else { 0 };
        self.db
            .query_optional(move |conn| {
                conn.execute(
                    "UPDATE project_skills
                     SET pinned = ?2, updated_at = ?3
                     WHERE id = ?1",
                    rusqlite::params![id, pinned_value, Utc::now().to_rfc3339()],
                )?;
                conn.query_row(
                    &format!(
                        "SELECT {} FROM project_skills WHERE id = ?1",
                        select_skill_columns()
                    ),
                    [&id],
                    skill_from_row,
                )
            })
            .await
    }
}

#[async_trait]
impl SkillUsageEventRepository for SqliteSkillUsageEventRepository {
    async fn record(&self, event: SkillUsageEvent) -> AppResult<SkillUsageEvent> {
        let metadata_json = serde_json::to_string(&event.metadata_json)
            .map_err(|error| AppError::Database(error.to_string()))?;
        let saved = event.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO skill_usage_events (
                        id, project_id, project_skill_id, conversation_id, agent_run_id,
                        provider_harness, stage, bucket, injection_kind, outcome_id,
                        metadata_json, created_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    rusqlite::params![
                        saved.id.as_str(),
                        saved.project_id.as_str(),
                        saved.project_skill_id.as_str(),
                        saved.conversation_id,
                        saved.agent_run_id,
                        saved.provider_harness,
                        saved.stage,
                        saved.bucket,
                        saved.injection_kind,
                        saved.outcome_id.as_ref().map(|id| id.as_str().to_string()),
                        metadata_json,
                        saved.created_at.to_rfc3339(),
                    ],
                )?;
                Ok(event)
            })
            .await
    }

    async fn list_by_project(
        &self,
        project_id: &ProjectId,
        options: SkillUsageListOptions,
    ) -> AppResult<Vec<SkillUsageEvent>> {
        let project_id = project_id.as_str().to_string();
        let project_skill_id = options.project_skill_id.map(|id| id.as_str().to_string());
        let agent_run_id = options.agent_run_id;
        self.db
            .run(move |conn| {
                let mut statement = conn.prepare(&format!(
                    "SELECT {} FROM skill_usage_events
                     WHERE project_id = ?1
                       AND (?2 IS NULL OR project_skill_id = ?2)
                       AND (?3 IS NULL OR agent_run_id = ?3)
                     ORDER BY created_at DESC",
                    select_usage_columns()
                ))?;
                let rows = statement
                    .query_map(
                        rusqlite::params![project_id, project_skill_id, agent_run_id],
                        usage_from_row,
                    )?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::Utc;
    use rusqlite::Connection;
    use serde_json::json;
    use tokio::sync::Mutex;

    use super::{
        SqliteProjectSkillRepository, SqliteSkillUsageEventRepository, SqliteTaskOutcomeRepository,
    };
    use crate::domain::entities::types::ProjectId;
    use crate::domain::entities::{
        ProjectSkill, ProjectSkillId, ProjectSkillLifecycleStatus, SkillUsageEvent,
        SkillUsageEventId, TaskOutcome, TaskOutcomeId, TaskOutcomeStatus,
    };
    use crate::domain::repositories::{
        ProjectSkillListOptions, ProjectSkillRepository, SkillUsageEventRepository,
        SkillUsageListOptions, TaskOutcomeListOptions, TaskOutcomeRepository,
        UpsertTaskOutcomeInput,
    };
    use crate::infrastructure::sqlite::run_migrations;

    fn shared_test_connection() -> Arc<Mutex<Connection>> {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON").unwrap();
        run_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, working_directory)
             VALUES ('project-1', 'Project 1', '/tmp/project-1')",
            [],
        )
        .unwrap();
        Arc::new(Mutex::new(conn))
    }

    fn task_outcome(status: TaskOutcomeStatus, outcome_class: Option<&str>) -> TaskOutcome {
        let now = Utc::now();
        TaskOutcome {
            id: TaskOutcomeId::new(),
            project_id: ProjectId::from_string("project-1".to_string()),
            source: "task_pipeline".to_string(),
            source_ref_kind: "task".to_string(),
            source_ref_id: "task-1".to_string(),
            task_id: Some("task-1".to_string()),
            conversation_id: None,
            agent_run_id: None,
            pull_request_id: None,
            proposal_id: None,
            verification_id: None,
            review_id: None,
            outcome_class: outcome_class.map(str::to_string),
            status,
            evidence_json: json!({ "summary": "evidence" }),
            provider_harness: Some("codex".to_string()),
            provider_session_id: Some("session-1".to_string()),
            created_at: now,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn task_outcome_upsert_upgrades_class_without_duplicate() {
        let repo = SqliteTaskOutcomeRepository::from_shared(shared_test_connection());

        repo.upsert(UpsertTaskOutcomeInput {
            outcome: task_outcome(TaskOutcomeStatus::Unknown, None),
        })
        .await
        .unwrap();
        let updated = repo
            .upsert(UpsertTaskOutcomeInput {
                outcome: task_outcome(TaskOutcomeStatus::Eligible, Some("merge_passed")),
            })
            .await
            .unwrap();

        assert_eq!(updated.status, TaskOutcomeStatus::Eligible);
        assert_eq!(updated.outcome_class.as_deref(), Some("merge_passed"));

        let rows = repo
            .list_by_project(
                &ProjectId::from_string("project-1".to_string()),
                TaskOutcomeListOptions::default(),
            )
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
    }

    #[tokio::test]
    async fn project_skill_lifecycle_and_usage_round_trip() {
        let conn = shared_test_connection();
        let skill_repo = SqliteProjectSkillRepository::from_shared(Arc::clone(&conn));
        let usage_repo = SqliteSkillUsageEventRepository::from_shared(conn);
        let now = Utc::now();
        let skill = ProjectSkill {
            id: ProjectSkillId::new(),
            project_id: ProjectId::from_string("project-1".to_string()),
            title: "Prefer repository-backed learned skills".to_string(),
            bucket: "execution".to_string(),
            stage: "execution".to_string(),
            status: ProjectSkillLifecycleStatus::Staged,
            pinned: false,
            archived: false,
            scope_paths: vec!["src-tauri/".to_string()],
            compact_guidance: "Use repository-backed skill records.".to_string(),
            body_markdown: "Detailed guidance".to_string(),
            predicted_effect: Some("Prevents adapter-only learned skill injection.".to_string()),
            provenance_json: json!({ "source": "test" }),
            companion_of_skill_id: None,
            created_at: now,
            updated_at: now,
        };
        let skill_id = skill.id.clone();

        skill_repo.create(skill).await.unwrap();
        let approved = skill_repo
            .update_lifecycle_status(&skill_id, ProjectSkillLifecycleStatus::Approved)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(approved.status, ProjectSkillLifecycleStatus::Approved);

        let listed = skill_repo
            .list_by_project(
                &ProjectId::from_string("project-1".to_string()),
                ProjectSkillListOptions {
                    status: Some(ProjectSkillLifecycleStatus::Approved),
                    scope_path: Some("src-tauri/src/lib.rs".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(listed.len(), 1);

        let event = SkillUsageEvent {
            id: SkillUsageEventId::new(),
            project_id: ProjectId::from_string("project-1".to_string()),
            project_skill_id: skill_id.clone(),
            conversation_id: Some("conversation-1".to_string()),
            agent_run_id: Some("run-1".to_string()),
            provider_harness: Some("claude".to_string()),
            stage: Some("execution".to_string()),
            bucket: Some("execution".to_string()),
            injection_kind: "compact_index".to_string(),
            outcome_id: None,
            metadata_json: json!({ "selected": true }),
            created_at: Utc::now(),
        };
        usage_repo.record(event).await.unwrap();
        let usage = usage_repo
            .list_by_project(
                &ProjectId::from_string("project-1".to_string()),
                SkillUsageListOptions {
                    project_skill_id: Some(skill_id),
                    agent_run_id: Some("run-1".to_string()),
                },
            )
            .await
            .unwrap();
        assert_eq!(usage.len(), 1);
    }
}
