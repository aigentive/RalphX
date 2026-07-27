use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{ProjectId, ProjectSkill, ProjectSkillId, ProjectSkillLifecycleStatus};
use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectSkillCreatedBy {
    User,
    Agent,
    Imported,
}

impl fmt::Display for ProjectSkillCreatedBy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::User => "user",
            Self::Agent => "agent",
            Self::Imported => "imported",
        })
    }
}

impl FromStr for ProjectSkillCreatedBy {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "user" => Ok(Self::User),
            "agent" => Ok(Self::Agent),
            "imported" => Ok(Self::Imported),
            _ => Err(AppError::Validation(format!(
                "invalid project skill authorship: {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectSkillVersion {
    pub project_skill_id: ProjectSkillId,
    pub project_id: ProjectId,
    pub version: i64,
    pub title: String,
    pub bucket: String,
    pub stage: String,
    pub status: ProjectSkillLifecycleStatus,
    pub pinned: bool,
    pub archived: bool,
    pub scope_paths: Vec<String>,
    pub compact_guidance: String,
    pub body_markdown: String,
    pub predicted_effect: Option<String>,
    pub provenance_json: Value,
    pub companion_of_skill_id: Option<ProjectSkillId>,
    pub content_hash: String,
    pub evidence_hash: String,
    pub created_by: ProjectSkillCreatedBy,
    pub pipeline_role: Option<String>,
    pub skill_created_at: DateTime<Utc>,
    pub skill_updated_at: DateTime<Utc>,
    pub snapshot_created_at: DateTime<Utc>,
}

impl ProjectSkillVersion {
    pub fn from_skill(
        skill: &ProjectSkill,
        version: i64,
        snapshot_created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            project_skill_id: skill.id.clone(),
            project_id: skill.project_id.clone(),
            version,
            title: skill.title.clone(),
            bucket: skill.bucket.clone(),
            stage: skill.stage.clone(),
            status: skill.status,
            pinned: skill.pinned,
            archived: skill.archived,
            scope_paths: skill.scope_paths.clone(),
            compact_guidance: skill.compact_guidance.clone(),
            body_markdown: skill.body_markdown.clone(),
            predicted_effect: skill.predicted_effect.clone(),
            provenance_json: skill.provenance_json.clone(),
            companion_of_skill_id: skill.companion_of_skill_id.clone(),
            content_hash: skill.content_hash.clone(),
            evidence_hash: skill.evidence_hash.clone(),
            created_by: skill.created_by,
            pipeline_role: skill.pipeline_role.clone(),
            skill_created_at: skill.created_at,
            skill_updated_at: skill.updated_at,
            snapshot_created_at,
        }
    }

    pub fn validate(&self) -> Result<(), AppError> {
        if self.version <= 0 {
            return Err(AppError::Validation(
                "project skill version must be positive".to_string(),
            ));
        }
        validate_project_skill_hash("content_hash", &self.content_hash)?;
        validate_project_skill_hash("evidence_hash", &self.evidence_hash)?;
        validate_project_skill_pipeline_role(self.pipeline_role.as_deref())
    }
}

pub fn validate_project_skill_hash(field: &str, value: &str) -> Result<(), AppError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(AppError::Validation(format!(
            "project skill {field} must be 64 lowercase hexadecimal characters"
        )));
    }
    Ok(())
}

pub fn validate_project_skill_pipeline_role(value: Option<&str>) -> Result<(), AppError> {
    if value.is_some_and(|role| role.trim().is_empty() || role != role.trim()) {
        return Err(AppError::Validation(
            "project skill pipeline_role must be trimmed and non-empty".to_string(),
        ));
    }
    Ok(())
}

fn normalized_identity(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn normalized_body(value: &str) -> String {
    value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_string()
}

fn hash_parts(domain: &str, parts: &[&str]) -> String {
    let mut digest = Sha256::new();
    digest.update(domain.as_bytes());
    for part in parts {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part.as_bytes());
    }
    format!("{:x}", digest.finalize())
}

pub fn project_skill_content_hash(title: &str, bucket: &str, stage: &str, body: &str) -> String {
    let title = normalized_identity(title);
    let bucket = normalized_identity(bucket);
    let stage = normalized_identity(stage);
    let body = normalized_body(body);
    hash_parts(
        "ralphx.project-skill.content.v1",
        &[&title, &bucket, &stage, &body],
    )
}

fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), canonical_json(value)))
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.iter().map(canonical_json).collect()),
        _ => value.clone(),
    }
}

pub fn project_skill_evidence_hash(value: &Value) -> String {
    let canonical = serde_json::to_string(&canonical_json(value)).unwrap_or_else(|_| "null".into());
    hash_parts("ralphx.project-skill.evidence.v1", &[&canonical])
}

pub fn project_skill_evidence_hash_from_raw(raw: &str) -> Result<String, AppError> {
    let value = serde_json::from_str::<Value>(raw).map_err(|error| {
        AppError::Database(format!(
            "invalid project_skills provenance_json during evidence hash backfill: {error}"
        ))
    })?;
    Ok(project_skill_evidence_hash(&value))
}

pub fn project_skill_authorship_from_provenance(value: &Value) -> ProjectSkillCreatedBy {
    let source = value
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if source == "github_pr_history"
        && value.get("source_ref_kind").and_then(Value::as_str) == Some("pull_request")
    {
        ProjectSkillCreatedBy::User
    } else if matches!(
        source,
        "project_skill_import"
            | "project_snapshot"
            | "import"
            | "import_manifest"
            | "external_manifest"
            | "target_project_skill_folder"
    ) {
        ProjectSkillCreatedBy::Imported
    } else if matches!(
        source,
        "task_outcome"
            | "github"
            | "github_pr"
            | "github_pr_history"
            | "gh_pr_list"
            | "gh_pr_view"
            | "git_log"
            | "distiller"
            | "agent"
    ) {
        ProjectSkillCreatedBy::Agent
    } else if matches!(
        source,
        "memory_to_project_skill_promotion" | "github_pr_manual_stage" | "manual"
    ) {
        ProjectSkillCreatedBy::User
    } else {
        ProjectSkillCreatedBy::Agent
    }
}

pub fn project_skill_pipeline_role_from_provenance(value: &Value) -> Option<String> {
    value
        .get("additional")
        .and_then(|additional| additional.get("pipeline_role"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|role| !role.is_empty())
        .map(str::to_string)
}

pub fn prepare_new_project_skill(mut skill: ProjectSkill) -> ProjectSkill {
    refresh_project_skill_metadata(&mut skill);
    skill
}

pub fn refresh_project_skill_metadata(skill: &mut ProjectSkill) {
    skill.content_hash = project_skill_content_hash(
        &skill.title,
        &skill.bucket,
        &skill.stage,
        &skill.body_markdown,
    );
    skill.evidence_hash = project_skill_evidence_hash(&skill.provenance_json);
    skill.pipeline_role = project_skill_pipeline_role_from_provenance(&skill.provenance_json);
}

pub fn project_skill_content_matches(left: &ProjectSkill, right: &ProjectSkill) -> bool {
    left.title == right.title
        && left.bucket == right.bucket
        && left.stage == right.stage
        && left.scope_paths == right.scope_paths
        && left.compact_guidance == right.compact_guidance
        && left.body_markdown == right.body_markdown
        && left.predicted_effect == right.predicted_effect
        && left.provenance_json == right.provenance_json
}
