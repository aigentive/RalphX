use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rusqlite::{Connection, OptionalExtension};
use tokio::sync::Mutex;

use super::project_skill_version_rows::{insert_skill_version, version_from_row, VERSION_COLUMNS};
use super::DbConnection;
use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{
    validate_project_skill_hash, validate_project_skill_pipeline_role, ProjectSkill,
    ProjectSkillCreatedBy, ProjectSkillId, ProjectSkillLifecycleStatus, ProjectSkillVersion,
    SkillUsageEvent, SkillUsageEventId, SkillUsageInjectionKind, TaskOutcomeId,
};
use crate::domain::repositories::{
    ProjectSkillListOptions, ProjectSkillRepository, ProjectSkillResolutionCommand,
    ProjectSkillResolutionOutcome, ProjectSkillResolutionResult, SkillUsageEventRepository,
    SkillUsageListOptions,
};
use crate::domain::services::project_skill_resolution::{
    enforce_project_skill_staging_policy, evaluate_project_skill_resolution,
};
use crate::error::{AppError, AppResult};

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

pub(super) fn parse_datetime(value: &str) -> DateTime<Utc> {
    if let Ok(value) = DateTime::parse_from_rfc3339(value) {
        return value.with_timezone(&Utc);
    }
    if let Ok(value) = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S") {
        return Utc.from_utc_datetime(&value);
    }
    Utc::now()
}

pub(super) fn parse_datetime_strict(
    value: &str,
    field: &'static str,
) -> rusqlite::Result<DateTime<Utc>> {
    if let Ok(value) = DateTime::parse_from_rfc3339(value) {
        return Ok(value.with_timezone(&Utc));
    }
    if let Ok(value) = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S") {
        return Ok(Utc.from_utc_datetime(&value));
    }
    Err(db_parse_error(AppError::Database(format!(
        "invalid project skill datetime {field}: {value}"
    ))))
}

pub(super) fn db_parse_error(error: AppError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

pub(super) fn parse_sqlite_bool(
    row: &rusqlite::Row<'_>,
    column: &'static str,
) -> rusqlite::Result<bool> {
    match row.get::<_, i64>(column)? {
        0 => Ok(false),
        1 => Ok(true),
        value => Err(db_parse_error(AppError::Database(format!(
            "invalid project skill boolean {column}: {value}"
        )))),
    }
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
    let created_by = row
        .get::<_, String>("created_by")?
        .parse::<ProjectSkillCreatedBy>()
        .map_err(db_parse_error)?;
    let content_hash = row.get::<_, String>("content_hash")?;
    validate_project_skill_hash("content_hash", &content_hash).map_err(db_parse_error)?;
    let evidence_hash = row.get::<_, String>("evidence_hash")?;
    validate_project_skill_hash("evidence_hash", &evidence_hash).map_err(db_parse_error)?;
    let pipeline_role = row.get::<_, Option<String>>("pipeline_role")?;
    validate_project_skill_pipeline_role(pipeline_role.as_deref()).map_err(db_parse_error)?;
    Ok(ProjectSkill {
        id: ProjectSkillId::from_string(row.get::<_, String>("id")?),
        project_id: ProjectId::from_string(row.get::<_, String>("project_id")?),
        title: row.get("title")?,
        bucket: row.get("bucket")?,
        stage: row.get("stage")?,
        status,
        pinned: parse_sqlite_bool(row, "pinned")?,
        archived: parse_sqlite_bool(row, "archived")?,
        scope_paths,
        compact_guidance: row.get("compact_guidance")?,
        body_markdown: row.get("body_markdown")?,
        predicted_effect: row.get("predicted_effect")?,
        provenance_json: provenance,
        companion_of_skill_id: row
            .get::<_, Option<String>>("companion_of_skill_id")?
            .map(ProjectSkillId::from_string),
        content_hash,
        evidence_hash,
        created_by,
        pipeline_role,
        created_at: parse_datetime_strict(&row.get::<_, String>("created_at")?, "created_at")?,
        updated_at: parse_datetime_strict(&row.get::<_, String>("updated_at")?, "updated_at")?,
    })
}

fn usage_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SkillUsageEvent> {
    let metadata =
        serde_json::from_str(&row.get::<_, String>("metadata_json")?).map_err(|error| {
            db_parse_error(AppError::Database(format!(
                "invalid skill_usage_events metadata_json: {error}"
            )))
        })?;
    let injection_kind = row
        .get::<_, String>("injection_kind")?
        .parse::<SkillUsageInjectionKind>()
        .map_err(db_parse_error)?;
    Ok(SkillUsageEvent {
        id: SkillUsageEventId::from_string(row.get::<_, String>("id")?),
        project_id: ProjectId::from_string(row.get::<_, String>("project_id")?),
        project_skill_id: ProjectSkillId::from_string(row.get::<_, String>("project_skill_id")?),
        conversation_id: row.get("conversation_id")?,
        agent_run_id: row.get("agent_run_id")?,
        provider_harness: row.get("provider_harness")?,
        stage: row.get("stage")?,
        bucket: row.get("bucket")?,
        injection_kind,
        outcome_id: row
            .get::<_, Option<String>>("outcome_id")?
            .map(TaskOutcomeId::from_string),
        metadata_json: metadata,
        created_at: parse_datetime(&row.get::<_, String>("created_at")?),
    })
}

fn select_skill_columns() -> &'static str {
    "id, project_id, title, bucket, stage, status, pinned, archived, scope_paths_json,
     compact_guidance, body_markdown, predicted_effect, provenance_json, companion_of_skill_id,
     content_hash, evidence_hash, created_by, pipeline_role, created_at, updated_at"
}

fn select_usage_columns() -> &'static str {
    "id, project_id, project_skill_id, conversation_id, agent_run_id, provider_harness,
     stage, bucket, injection_kind, outcome_id, metadata_json, created_at"
}

fn load_project_skill_candidates(
    conn: &Connection,
    project_id: &ProjectId,
) -> AppResult<Vec<ProjectSkill>> {
    let mut statement = conn.prepare(&format!(
        "SELECT {} FROM project_skills WHERE project_id = ?1",
        select_skill_columns()
    ))?;
    let candidates = statement
        .query_map([project_id.as_str()], skill_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(candidates)
}

fn insert_project_skill_row(conn: &Connection, skill: &ProjectSkill) -> AppResult<()> {
    let scope_paths_json = serde_json::to_string(&skill.scope_paths)
        .map_err(|error| AppError::Database(error.to_string()))?;
    let provenance_json = serde_json::to_string(&skill.provenance_json)
        .map_err(|error| AppError::Database(error.to_string()))?;
    conn.execute(
        "INSERT INTO project_skills (
            id, project_id, title, bucket, stage, status, pinned, archived,
            scope_paths_json, compact_guidance, body_markdown, predicted_effect,
            provenance_json, companion_of_skill_id, content_hash,
            evidence_hash, created_by, pipeline_role, created_at, updated_at
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
            ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20
         )",
        rusqlite::params![
            skill.id.as_str(),
            skill.project_id.as_str(),
            skill.title,
            skill.bucket,
            skill.stage,
            skill.status.to_string(),
            i64::from(skill.pinned),
            i64::from(skill.archived),
            scope_paths_json,
            skill.compact_guidance,
            skill.body_markdown,
            skill.predicted_effect,
            provenance_json,
            skill.companion_of_skill_id.as_ref().map(|id| id.as_str()),
            skill.content_hash,
            skill.evidence_hash,
            skill.created_by.to_string(),
            skill.pipeline_role,
            skill.created_at.to_rfc3339(),
            skill.updated_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn update_project_skill_row(conn: &Connection, skill: &ProjectSkill) -> AppResult<()> {
    let scope_paths_json = serde_json::to_string(&skill.scope_paths)
        .map_err(|error| AppError::Database(error.to_string()))?;
    let provenance_json = serde_json::to_string(&skill.provenance_json)
        .map_err(|error| AppError::Database(error.to_string()))?;
    let changed = conn.execute(
        "UPDATE project_skills
         SET title = ?2, bucket = ?3, stage = ?4, scope_paths_json = ?5,
             compact_guidance = ?6, body_markdown = ?7, predicted_effect = ?8,
             provenance_json = ?9, companion_of_skill_id = ?10, content_hash = ?11,
             evidence_hash = ?12, created_by = ?13, pipeline_role = ?14, updated_at = ?15
         WHERE id = ?1 AND project_id = ?16",
        rusqlite::params![
            skill.id.as_str(),
            skill.title,
            skill.bucket,
            skill.stage,
            scope_paths_json,
            skill.compact_guidance,
            skill.body_markdown,
            skill.predicted_effect,
            provenance_json,
            skill.companion_of_skill_id.as_ref().map(|id| id.as_str()),
            skill.content_hash,
            skill.evidence_hash,
            skill.created_by.to_string(),
            skill.pipeline_role,
            skill.updated_at.to_rfc3339(),
            skill.project_id.as_str(),
        ],
    )?;
    if changed != 1 {
        return Err(AppError::Conflict(
            "project skill resolution target changed concurrently".to_string(),
        ));
    }
    Ok(())
}

fn validate_sqlite_companion(conn: &Connection, skill: &ProjectSkill) -> AppResult<()> {
    let Some(companion_id) = skill.companion_of_skill_id.as_ref() else {
        return Ok(());
    };
    let parent = conn
        .query_row(
            &format!(
                "SELECT {} FROM project_skills WHERE id = ?1",
                select_skill_columns()
            ),
            [companion_id.as_str()],
            skill_from_row,
        )
        .optional()?
        .ok_or_else(|| {
            AppError::Validation(format!(
                "companion project skill {} was not found",
                companion_id.as_str()
            ))
        })?;
    if parent.project_id != skill.project_id
        || parent.status != ProjectSkillLifecycleStatus::Approved
        || parent.archived
    {
        return Err(AppError::Validation(
            "companion project skill must be an active approved skill in the same project"
                .to_string(),
        ));
    }
    Ok(())
}

fn next_project_skill_version(conn: &Connection, skill_id: &ProjectSkillId) -> AppResult<i64> {
    let current = conn.query_row(
        "SELECT COALESCE(MAX(version), 0)
         FROM project_skill_versions WHERE project_skill_id = ?1",
        [skill_id.as_str()],
        |row| row.get::<_, i64>(0),
    )?;
    current
        .checked_add(1)
        .ok_or_else(|| AppError::Conflict("project skill version overflow".to_string()))
}

#[async_trait]
impl ProjectSkillRepository for SqliteProjectSkillRepository {
    async fn resolve(
        &self,
        command: ProjectSkillResolutionCommand,
    ) -> AppResult<ProjectSkillResolutionResult> {
        self.db
            .run_transaction(move |conn| {
                let candidates =
                    load_project_skill_candidates(conn, &command.candidate.project_id)?;
                let staging_policy = command.staging_policy.clone();
                let plan = evaluate_project_skill_resolution(command, &candidates)?;
                if plan.outcome == ProjectSkillResolutionOutcome::Duplicate {
                    return Ok(ProjectSkillResolutionResult {
                        outcome: plan.outcome,
                        skill: plan.skill,
                        version: None,
                    });
                }
                enforce_project_skill_staging_policy(staging_policy.as_ref(), &candidates, &plan)?;
                validate_sqlite_companion(conn, &plan.skill)?;
                match plan.outcome {
                    ProjectSkillResolutionOutcome::CreateNew => {
                        insert_project_skill_row(conn, &plan.skill)?;
                    }
                    ProjectSkillResolutionOutcome::PatchExisting
                    | ProjectSkillResolutionOutcome::AppendEvidence => {
                        update_project_skill_row(conn, &plan.skill)?;
                    }
                    ProjectSkillResolutionOutcome::Duplicate => {
                        return Err(AppError::Conflict(
                            "duplicate project skill resolution reached the mutation path"
                                .to_string(),
                        ));
                    }
                }
                let next_version = next_project_skill_version(conn, &plan.skill.id)?;
                let version =
                    ProjectSkillVersion::from_skill(&plan.skill, next_version, Utc::now());
                insert_skill_version(conn, &version)?;
                Ok(ProjectSkillResolutionResult {
                    outcome: plan.outcome,
                    skill: plan.skill,
                    version: Some(version),
                })
            })
            .await
    }

    #[cfg(any(test, feature = "test-utils"))]
    async fn seed_for_test(&self, skill: ProjectSkill) -> AppResult<ProjectSkill> {
        let skill = crate::domain::entities::prepare_new_project_skill(skill);
        let saved = skill.clone();
        self.db
            .run(move |conn| {
                validate_sqlite_companion(conn, &saved)?;
                insert_project_skill_row(conn, &saved)?;
                Ok(saved)
            })
            .await
    }

    #[cfg(any(test, feature = "test-utils"))]
    async fn create(&self, skill: ProjectSkill) -> AppResult<ProjectSkill> {
        self.seed_for_test(skill).await
    }

    #[cfg(any(test, feature = "test-utils"))]
    async fn update_content(&self, requested: ProjectSkill) -> AppResult<Option<ProjectSkill>> {
        self.db
            .run_transaction(move |conn| {
                let current = conn
                    .query_row(
                        &format!(
                            "SELECT {} FROM project_skills WHERE id = ?1",
                            select_skill_columns()
                        ),
                        [requested.id.as_str()],
                        skill_from_row,
                    )
                    .optional()?;
                let Some(mut current) = current else {
                    return Ok(None);
                };
                if crate::domain::entities::project_skill_content_matches(&current, &requested) {
                    return Ok(Some(current));
                }
                current.title = requested.title;
                current.bucket = requested.bucket;
                current.stage = requested.stage;
                current.scope_paths = requested.scope_paths;
                current.compact_guidance = requested.compact_guidance;
                current.body_markdown = requested.body_markdown;
                current.predicted_effect = requested.predicted_effect;
                current.provenance_json = requested.provenance_json;
                current.updated_at = Utc::now();
                crate::domain::entities::refresh_project_skill_metadata(&mut current);
                update_project_skill_row(conn, &current)?;
                Ok(Some(current))
            })
            .await
    }

    #[cfg(any(test, feature = "test-utils"))]
    async fn append_version(&self, version: ProjectSkillVersion) -> AppResult<ProjectSkillVersion> {
        version.validate()?;
        self.db
            .run(move |conn| {
                insert_skill_version(conn, &version)?;
                Ok(version)
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

    async fn list_versions(&self, id: &ProjectSkillId) -> AppResult<Vec<ProjectSkillVersion>> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut statement = conn.prepare(&format!(
                    "SELECT {VERSION_COLUMNS} FROM project_skill_versions
                 WHERE project_skill_id = ?1 ORDER BY version ASC"
                ))?;
                let versions = statement
                    .query_map([id], version_from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(versions)
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
        let mut saved = self.record_batch(vec![event]).await?;
        saved.pop().ok_or_else(|| {
            AppError::Database("skill usage batch unexpectedly returned no event".to_string())
        })
    }

    async fn record_batch(&self, events: Vec<SkillUsageEvent>) -> AppResult<Vec<SkillUsageEvent>> {
        let serialized = events
            .iter()
            .map(|event| {
                serde_json::to_string(&event.metadata_json)
                    .map(|metadata_json| (event.clone(), metadata_json))
                    .map_err(|error| AppError::Database(error.to_string()))
            })
            .collect::<AppResult<Vec<_>>>()?;
        let saved = events;
        self.db
            .run_transaction(move |conn| {
                for (event, metadata_json) in serialized {
                    conn.execute(
                        "INSERT INTO skill_usage_events (
                        id, project_id, project_skill_id, conversation_id, agent_run_id,
                        provider_harness, stage, bucket, injection_kind, outcome_id,
                        metadata_json, created_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                    ON CONFLICT(id) DO NOTHING",
                        rusqlite::params![
                            event.id.as_str(),
                            event.project_id.as_str(),
                            event.project_skill_id.as_str(),
                            event.conversation_id,
                            event.agent_run_id,
                            event.provider_harness,
                            event.stage,
                            event.bucket,
                            event.injection_kind.to_string(),
                            event.outcome_id.as_ref().map(|id| id.as_str().to_string()),
                            metadata_json,
                            event.created_at.to_rfc3339(),
                        ],
                    )?;
                }
                Ok(saved)
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
