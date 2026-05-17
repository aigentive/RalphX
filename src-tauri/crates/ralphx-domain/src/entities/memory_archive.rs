// Memory archive entities - job queue for snapshot generation
//
// This module defines the entities for the memory archive system,
// which generates deterministic markdown snapshots from canonical DB state.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use crate::entities::types::ProjectId;
use crate::error::{AppError, AppResult};

/// Unique identifier for memory archive jobs
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MemoryArchiveJobId(pub String);

impl fmt::Display for MemoryArchiveJobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for MemoryArchiveJobId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for MemoryArchiveJobId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Type of archive snapshot to generate
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveJobType {
    /// Per-memory snapshot in RalphX-owned app-data archive storage.
    MemorySnapshot,
    /// Per-rule reconstruction in RalphX-owned app-data archive storage.
    RuleSnapshot,
    /// Full project rebuild in RalphX-owned app-data archive storage.
    FullRebuild,
}

impl fmt::Display for ArchiveJobType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MemorySnapshot => write!(f, "memory_snapshot"),
            Self::RuleSnapshot => write!(f, "rule_snapshot"),
            Self::FullRebuild => write!(f, "full_rebuild"),
        }
    }
}

impl FromStr for ArchiveJobType {
    type Err = AppError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "memory_snapshot" => Ok(Self::MemorySnapshot),
            "rule_snapshot" => Ok(Self::RuleSnapshot),
            "full_rebuild" => Ok(Self::FullRebuild),
            _ => Err(AppError::Validation(format!(
                "Invalid archive job type: {}",
                s
            ))),
        }
    }
}

/// Status of an archive job
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveJobStatus {
    /// Job is waiting to be processed
    Pending,
    /// Job is currently being processed
    Running,
    /// Job completed successfully
    Done,
    /// Job failed with error
    Failed,
}

impl fmt::Display for ArchiveJobStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Running => write!(f, "running"),
            Self::Done => write!(f, "done"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

impl FromStr for ArchiveJobStatus {
    type Err = AppError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "done" => Ok(Self::Done),
            "failed" => Ok(Self::Failed),
            _ => Err(AppError::Validation(format!(
                "Invalid archive job status: {}",
                s
            ))),
        }
    }
}

/// Payload for memory snapshot jobs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySnapshotPayload {
    pub memory_id: String,
}

/// Payload for rule snapshot jobs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleSnapshotPayload {
    pub scope_key: String,
}

/// Payload for full rebuild jobs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullRebuildPayload {
    pub include_rule_snapshots: bool,
}

/// Archive job payload variants
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ArchiveJobPayload {
    MemorySnapshot(MemorySnapshotPayload),
    RuleSnapshot(RuleSnapshotPayload),
    FullRebuild(FullRebuildPayload),
}

impl ArchiveJobPayload {
    /// Create a memory snapshot payload
    pub fn memory_snapshot(memory_id: impl Into<String>) -> Self {
        Self::MemorySnapshot(MemorySnapshotPayload {
            memory_id: memory_id.into(),
        })
    }

    /// Create a rule snapshot payload
    pub fn rule_snapshot(scope_key: impl Into<String>) -> Self {
        Self::RuleSnapshot(RuleSnapshotPayload {
            scope_key: scope_key.into(),
        })
    }

    /// Create a full rebuild payload
    pub fn full_rebuild(include_rule_snapshots: bool) -> Self {
        Self::FullRebuild(FullRebuildPayload {
            include_rule_snapshots,
        })
    }

    /// Serialize to JSON string for database storage
    pub fn to_json(&self) -> AppResult<String> {
        serde_json::to_string(self)
            .map_err(|e| AppError::Infrastructure(format!("JSON serialization error: {}", e)))
    }

    /// Deserialize from JSON string
    pub fn from_json(json: &str) -> AppResult<Self> {
        serde_json::from_str(json)
            .map_err(|e| AppError::Infrastructure(format!("JSON deserialization error: {}", e)))
    }
}

/// Memory archive job entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryArchiveJob {
    pub id: MemoryArchiveJobId,
    pub project_id: ProjectId,
    pub job_type: ArchiveJobType,
    pub payload: ArchiveJobPayload,
    pub status: ArchiveJobStatus,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

impl MemoryArchiveJob {
    /// Create a new pending archive job
    pub fn new(
        project_id: ProjectId,
        job_type: ArchiveJobType,
        payload: ArchiveJobPayload,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: MemoryArchiveJobId(uuid::Uuid::new_v4().to_string()),
            project_id,
            job_type,
            payload,
            status: ArchiveJobStatus::Pending,
            error_message: None,
            created_at: now,
            updated_at: now,
            started_at: None,
            completed_at: None,
        }
    }

    /// Mark job as running
    pub fn start(&mut self) {
        self.status = ArchiveJobStatus::Running;
        self.started_at = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    /// Mark job as completed successfully
    pub fn complete(&mut self) {
        self.status = ArchiveJobStatus::Done;
        self.completed_at = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    /// Mark job as failed with error message
    pub fn fail(&mut self, error: impl Into<String>) {
        self.status = ArchiveJobStatus::Failed;
        self.error_message = Some(error.into());
        self.completed_at = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    /// Check if job can be claimed (is pending or failed)
    pub fn can_claim(&self) -> bool {
        matches!(
            self.status,
            ArchiveJobStatus::Pending | ArchiveJobStatus::Failed
        )
    }
}

#[cfg(test)]
#[path = "memory_archive_tests.rs"]
mod tests;
