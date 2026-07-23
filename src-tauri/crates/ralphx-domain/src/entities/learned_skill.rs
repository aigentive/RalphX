use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::entities::types::ProjectId;
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskOutcomeId(pub String);

impl TaskOutcomeId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn from_string(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for TaskOutcomeId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProjectSkillId(pub String);

impl ProjectSkillId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn from_string(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ProjectSkillId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SkillUsageEventId(pub String);

impl SkillUsageEventId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn from_string(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for SkillUsageEventId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskOutcomeStatus {
    Unknown,
    Eligible,
    Ineligible,
    Succeeded,
    Failed,
}

impl fmt::Display for TaskOutcomeStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Unknown => "unknown",
            Self::Eligible => "eligible",
            Self::Ineligible => "ineligible",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        };
        write!(f, "{value}")
    }
}

impl FromStr for TaskOutcomeStatus {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "unknown" => Ok(Self::Unknown),
            "eligible" => Ok(Self::Eligible),
            "ineligible" => Ok(Self::Ineligible),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            _ => Err(AppError::Validation(format!(
                "invalid task outcome status: {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectSkillLifecycleStatus {
    Staged,
    Approved,
    Rejected,
    Stale,
    Archived,
    Retired,
}

impl fmt::Display for ProjectSkillLifecycleStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Staged => "staged",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Stale => "stale",
            Self::Archived => "archived",
            Self::Retired => "retired",
        };
        write!(f, "{value}")
    }
}

impl FromStr for ProjectSkillLifecycleStatus {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "staged" => Ok(Self::Staged),
            "approved" => Ok(Self::Approved),
            "rejected" => Ok(Self::Rejected),
            "stale" => Ok(Self::Stale),
            "archived" => Ok(Self::Archived),
            "retired" => Ok(Self::Retired),
            _ => Err(AppError::Validation(format!(
                "invalid project skill lifecycle status: {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskOutcome {
    pub id: TaskOutcomeId,
    pub project_id: ProjectId,
    pub source: String,
    pub source_ref_kind: String,
    pub source_ref_id: String,
    pub task_id: Option<String>,
    pub conversation_id: Option<String>,
    pub agent_run_id: Option<String>,
    pub pull_request_id: Option<String>,
    pub proposal_id: Option<String>,
    pub verification_id: Option<String>,
    pub review_id: Option<String>,
    pub outcome_class: Option<String>,
    pub status: TaskOutcomeStatus,
    pub evidence_json: Value,
    pub provider_harness: Option<String>,
    pub provider_session_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSkill {
    pub id: ProjectSkillId,
    pub project_id: ProjectId,
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
    pub created_by: super::project_skill_versioning::ProjectSkillCreatedBy,
    pub pipeline_role: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillUsageEvent {
    pub id: SkillUsageEventId,
    pub project_id: ProjectId,
    pub project_skill_id: ProjectSkillId,
    pub conversation_id: Option<String>,
    pub agent_run_id: Option<String>,
    pub provider_harness: Option<String>,
    pub stage: Option<String>,
    pub bucket: Option<String>,
    pub injection_kind: String,
    pub outcome_id: Option<TaskOutcomeId>,
    pub metadata_json: Value,
    pub created_at: DateTime<Utc>,
}
