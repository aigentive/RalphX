use std::str::FromStr;

use rusqlite::Connection;

use super::sqlite_learned_skill_repos::{db_parse_error, parse_datetime};
use crate::domain::entities::{
    ProjectId, ProjectSkill, ProjectSkillCreatedBy, ProjectSkillId, ProjectSkillLifecycleStatus,
    ProjectSkillVersion,
};
use crate::error::{AppError, AppResult};

pub(super) fn insert_skill_snapshot(conn: &Connection, skill: &ProjectSkill) -> AppResult<()> {
    let scope_paths_json = serde_json::to_string(&skill.scope_paths)
        .map_err(|error| AppError::Database(error.to_string()))?;
    let provenance_json = serde_json::to_string(&skill.provenance_json)
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
            skill.id.as_str(),
            skill.project_id.as_str(),
            skill.version,
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
            skill.updated_at.to_rfc3339(),
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
    Ok(ProjectSkillVersion {
        project_skill_id: ProjectSkillId::from_string(row.get::<_, String>("project_skill_id")?),
        project_id: ProjectId::from_string(row.get::<_, String>("project_id")?),
        version: row.get("version")?,
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
        provenance_json,
        companion_of_skill_id: row
            .get::<_, Option<String>>("companion_of_skill_id")?
            .map(ProjectSkillId::from_string),
        content_hash: row.get("content_hash")?,
        evidence_hash: row.get("evidence_hash")?,
        created_by,
        pipeline_role: row.get("pipeline_role")?,
        skill_created_at: parse_datetime(&row.get::<_, String>("skill_created_at")?),
        skill_updated_at: parse_datetime(&row.get::<_, String>("skill_updated_at")?),
        snapshot_created_at: parse_datetime(&row.get::<_, String>("snapshot_created_at")?),
    })
}

pub(super) const VERSION_COLUMNS: &str =
    "project_skill_id, project_id, version, title, bucket, stage, status, pinned, archived,
     scope_paths_json, compact_guidance, body_markdown, predicted_effect, provenance_json,
     companion_of_skill_id, content_hash, evidence_hash, created_by, pipeline_role,
     skill_created_at, skill_updated_at, snapshot_created_at";
