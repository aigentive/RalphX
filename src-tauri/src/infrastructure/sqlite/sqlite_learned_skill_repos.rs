use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rusqlite::{Connection, OptionalExtension};
use tokio::sync::Mutex;

use super::project_skill_version_rows::{insert_skill_snapshot, version_from_row, VERSION_COLUMNS};
use super::DbConnection;
use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{
    prepare_new_project_skill, project_skill_content_matches, refresh_project_skill_metadata,
    ProjectSkill, ProjectSkillCreatedBy, ProjectSkillId, ProjectSkillLifecycleStatus,
    ProjectSkillVersion, SkillUsageEvent, SkillUsageEventId, TaskOutcomeId,
};
use crate::domain::repositories::{
    ProjectSkillListOptions, ProjectSkillRepository, SkillUsageEventRepository,
    SkillUsageListOptions,
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

pub(super) fn db_parse_error(error: AppError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
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
        version: row.get("version")?,
        content_hash: row.get("content_hash")?,
        evidence_hash: row.get("evidence_hash")?,
        created_by,
        pipeline_role: row.get("pipeline_role")?,
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

fn select_skill_columns() -> &'static str {
    "id, project_id, title, bucket, stage, status, pinned, archived, scope_paths_json,
     compact_guidance, body_markdown, predicted_effect, provenance_json, companion_of_skill_id,
     version, content_hash, evidence_hash, created_by, pipeline_role, created_at, updated_at"
}

fn select_usage_columns() -> &'static str {
    "id, project_id, project_skill_id, conversation_id, agent_run_id, provider_harness,
     stage, bucket, injection_kind, outcome_id, metadata_json, created_at"
}

#[async_trait]
impl ProjectSkillRepository for SqliteProjectSkillRepository {
    async fn create(&self, skill: ProjectSkill) -> AppResult<ProjectSkill> {
        let skill = prepare_new_project_skill(skill);
        let scope_paths_json = serde_json::to_string(&skill.scope_paths)
            .map_err(|error| AppError::Database(error.to_string()))?;
        let provenance_json = serde_json::to_string(&skill.provenance_json)
            .map_err(|error| AppError::Database(error.to_string()))?;
        let saved = skill.clone();
        self.db
            .run_transaction(move |conn| {
                conn.execute(
                    "INSERT INTO project_skills (
                        id, project_id, title, bucket, stage, status, pinned, archived,
                        scope_paths_json, compact_guidance, body_markdown, predicted_effect,
                        provenance_json, companion_of_skill_id, version, content_hash,
                        evidence_hash, created_by, pipeline_role, created_at, updated_at
                    ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
                        ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21
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
                        saved.version,
                        saved.content_hash,
                        saved.evidence_hash,
                        saved.created_by.to_string(),
                        saved.pipeline_role,
                        saved.created_at.to_rfc3339(),
                        saved.updated_at.to_rfc3339(),
                    ],
                )?;
                insert_skill_snapshot(conn, &skill)?;
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
                if current.version != requested.version {
                    return Err(AppError::Conflict(format!(
                        "project skill expected version {} but current version is {}",
                        requested.version, current.version
                    )));
                }
                if project_skill_content_matches(&current, &requested) {
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
                current.version += 1;
                current.updated_at = Utc::now();
                refresh_project_skill_metadata(&mut current);
                let scope_paths_json = serde_json::to_string(&current.scope_paths)
                    .map_err(|error| AppError::Database(error.to_string()))?;
                let provenance_json = serde_json::to_string(&current.provenance_json)
                    .map_err(|error| AppError::Database(error.to_string()))?;
                let changed = conn.execute(
                    "UPDATE project_skills
                     SET title = ?2,
                         bucket = ?3,
                         stage = ?4,
                         scope_paths_json = ?5,
                         compact_guidance = ?6,
                         body_markdown = ?7,
                         predicted_effect = ?8, provenance_json = ?9, version = ?10,
                         content_hash = ?11, evidence_hash = ?12, created_by = ?13,
                         pipeline_role = ?14, updated_at = ?15
                     WHERE id = ?1 AND version = ?16",
                    rusqlite::params![
                        current.id.as_str(),
                        current.title,
                        current.bucket,
                        current.stage,
                        scope_paths_json,
                        current.compact_guidance,
                        current.body_markdown,
                        current.predicted_effect,
                        provenance_json,
                        current.version,
                        current.content_hash,
                        current.evidence_hash,
                        current.created_by.to_string(),
                        current.pipeline_role,
                        current.updated_at.to_rfc3339(),
                        requested.version,
                    ],
                )?;
                if changed != 1 {
                    return Err(AppError::Conflict(
                        "project skill changed concurrently".to_string(),
                    ));
                }
                insert_skill_snapshot(conn, &current)?;
                Ok(Some(current))
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
