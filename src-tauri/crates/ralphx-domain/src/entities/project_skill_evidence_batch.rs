use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{ProjectSkillId, TaskOutcomeId};
use crate::error::{AppError, AppResult};

pub const PROJECT_SKILL_EVIDENCE_BATCH_MAX_ITEMS: usize = 8;
pub const PROJECT_SKILL_EVIDENCE_DIGEST_MAX_CHARS: usize = 1_200;
const PROJECT_SKILL_BUCKETS: &[&str] =
    &["planning", "verification", "review", "execution", "merge"];

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProjectSkillEvidenceBatchId(pub String);

impl ProjectSkillEvidenceBatchId {
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

impl Default for ProjectSkillEvidenceBatchId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectSkillEvidenceBatchStatus {
    Pending,
    Consumed,
    Archived,
}

impl fmt::Display for ProjectSkillEvidenceBatchStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Pending => "pending",
            Self::Consumed => "consumed",
            Self::Archived => "archived",
        })
    }
}

impl FromStr for ProjectSkillEvidenceBatchStatus {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "consumed" => Ok(Self::Consumed),
            "archived" => Ok(Self::Archived),
            _ => Err(AppError::Validation(format!(
                "invalid project skill evidence batch status: {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectSkillEvidenceBatchItem {
    pub outcome_id: TaskOutcomeId,
    pub ordinal: usize,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSkillEvidenceBatch {
    pub id: ProjectSkillEvidenceBatchId,
    pub project_id: ProjectId,
    pub fingerprint: String,
    pub bucket: String,
    pub status: ProjectSkillEvidenceBatchStatus,
    pub claim_token: Option<String>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub completed_project_skill_id: Option<ProjectSkillId>,
    pub resolution_action: Option<String>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub items: Vec<ProjectSkillEvidenceBatchItem>,
}

impl ProjectSkillEvidenceBatch {
    pub fn validate_for_insert(&self) -> AppResult<()> {
        if self.id.as_str().trim().is_empty() || self.project_id.as_str().trim().is_empty() {
            return Err(AppError::Validation(
                "project skill evidence batch identity is required".to_string(),
            ));
        }
        if self.fingerprint.len() != 64
            || !self
                .fingerprint
                .bytes()
                .all(|value| value.is_ascii_hexdigit() && !value.is_ascii_uppercase())
        {
            return Err(AppError::Validation(
                "project skill evidence batch fingerprint must be lowercase SHA-256".to_string(),
            ));
        }
        if !PROJECT_SKILL_BUCKETS.contains(&self.bucket.as_str()) {
            return Err(AppError::Validation(
                "project skill evidence batch bucket is invalid".to_string(),
            ));
        }
        if self.status != ProjectSkillEvidenceBatchStatus::Pending
            || self.claim_token.is_some()
            || self.claimed_at.is_some()
            || self.completed_project_skill_id.is_some()
            || self.resolution_action.is_some()
            || self.completed_at.is_some()
        {
            return Err(AppError::Validation(
                "new project skill evidence batches must be unclaimed pending rows".to_string(),
            ));
        }
        if self.items.is_empty() || self.items.len() > PROJECT_SKILL_EVIDENCE_BATCH_MAX_ITEMS {
            return Err(AppError::Validation(format!(
                "project skill evidence batches require 1 to {PROJECT_SKILL_EVIDENCE_BATCH_MAX_ITEMS} items"
            )));
        }

        let mut outcome_ids = HashSet::with_capacity(self.items.len());
        for (expected_ordinal, item) in self.items.iter().enumerate() {
            if item.ordinal != expected_ordinal {
                return Err(AppError::Validation(
                    "project skill evidence batch ordinals must be contiguous".to_string(),
                ));
            }
            if item.outcome_id.as_str().trim().is_empty()
                || item.digest.trim().is_empty()
                || item.digest.chars().count() > PROJECT_SKILL_EVIDENCE_DIGEST_MAX_CHARS
                || !outcome_ids.insert(item.outcome_id.as_str())
            {
                return Err(AppError::Validation(
                    "project skill evidence batch item is invalid".to_string(),
                ));
            }
        }
        Ok(())
    }
}
