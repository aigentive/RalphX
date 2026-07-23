use std::str::FromStr;

use rusqlite::{Connection, OptionalExtension};

use super::sqlite_learned_skill_repos::{db_parse_error, parse_datetime_strict, parse_sqlite_bool};
use crate::domain::entities::{
    validate_project_skill_hash, validate_project_skill_pipeline_role, ProjectId,
    ProjectSkillCreatedBy, ProjectSkillId, ProjectSkillLifecycleStatus, ProjectSkillVersion,
};
use crate::error::{AppError, AppResult};

pub(super) fn insert_skill_version(
    conn: &Connection,
    version: &ProjectSkillVersion,
) -> AppResult<()> {
    version.validate()?;
    let owning_project = conn
        .query_row(
            "SELECT project_id FROM project_skills WHERE id = ?1",
            [version.project_skill_id.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(owning_project) = owning_project else {
        return Err(AppError::NotFound(format!(
            "project skill {} was not found",
            version.project_skill_id.as_str()
        )));
    };
    if owning_project != version.project_id.as_str() {
        return Err(AppError::Validation(
            "project skill version project does not match its skill".to_string(),
        ));
    }
    let scope_paths_json = serde_json::to_string(&version.scope_paths)
        .map_err(|error| AppError::Database(error.to_string()))?;
    let provenance_json = serde_json::to_string(&version.provenance_json)
        .map_err(|error| AppError::Database(error.to_string()))?;
    conn.execute(
        "INSERT INTO project_skill_versions (
            project_skill_id, project_id, version, title, bucket, stage, status,
            pinned, archived, scope_paths_json, compact_guidance, body_markdown,
            predicted_effect, provenance_json, companion_of_skill_id, content_hash,
            evidence_hash, created_by, pipeline_role, skill_created_at,
            skill_updated_at, snapshot_created_at
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
            ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22
         )",
        rusqlite::params![
            version.project_skill_id.as_str(),
            version.project_id.as_str(),
            version.version,
            version.title,
            version.bucket,
            version.stage,
            version.status.to_string(),
            i64::from(version.pinned),
            i64::from(version.archived),
            scope_paths_json,
            version.compact_guidance,
            version.body_markdown,
            version.predicted_effect,
            provenance_json,
            version.companion_of_skill_id.as_ref().map(|id| id.as_str()),
            version.content_hash,
            version.evidence_hash,
            version.created_by.to_string(),
            version.pipeline_role,
            version.skill_created_at.to_rfc3339(),
            version.skill_updated_at.to_rfc3339(),
            version.snapshot_created_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

pub(super) fn version_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectSkillVersion> {
    let status = ProjectSkillLifecycleStatus::from_str(&row.get::<_, String>("status")?)
        .map_err(db_parse_error)?;
    let created_by = ProjectSkillCreatedBy::from_str(&row.get::<_, String>("created_by")?)
        .map_err(db_parse_error)?;
    let scope_paths = serde_json::from_str(&row.get::<_, String>("scope_paths_json")?)
        .map_err(|error| db_parse_error(AppError::Database(error.to_string())))?;
    let provenance_json = serde_json::from_str(&row.get::<_, String>("provenance_json")?)
        .map_err(|error| db_parse_error(AppError::Database(error.to_string())))?;
    let content_hash = row.get::<_, String>("content_hash")?;
    validate_project_skill_hash("version content_hash", &content_hash).map_err(db_parse_error)?;
    let evidence_hash = row.get::<_, String>("evidence_hash")?;
    validate_project_skill_hash("version evidence_hash", &evidence_hash).map_err(db_parse_error)?;
    let pipeline_role = row.get::<_, Option<String>>("pipeline_role")?;
    validate_project_skill_pipeline_role(pipeline_role.as_deref()).map_err(db_parse_error)?;
    Ok(ProjectSkillVersion {
        project_skill_id: ProjectSkillId::from_string(row.get::<_, String>("project_skill_id")?),
        project_id: ProjectId::from_string(row.get::<_, String>("project_id")?),
        version: row.get("version")?,
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
        provenance_json,
        companion_of_skill_id: row
            .get::<_, Option<String>>("companion_of_skill_id")?
            .map(ProjectSkillId::from_string),
        content_hash,
        evidence_hash,
        created_by,
        pipeline_role,
        skill_created_at: parse_datetime_strict(
            &row.get::<_, String>("skill_created_at")?,
            "skill_created_at",
        )?,
        skill_updated_at: parse_datetime_strict(
            &row.get::<_, String>("skill_updated_at")?,
            "skill_updated_at",
        )?,
        snapshot_created_at: parse_datetime_strict(
            &row.get::<_, String>("snapshot_created_at")?,
            "snapshot_created_at",
        )?,
    })
}

pub(super) const VERSION_COLUMNS: &str =
    "project_skill_id, project_id, version, title, bucket, stage, status, pinned, archived,
     scope_paths_json, compact_guidance, body_markdown, predicted_effect, provenance_json,
     companion_of_skill_id, content_hash, evidence_hash, created_by, pipeline_role,
     skill_created_at, skill_updated_at, snapshot_created_at";
