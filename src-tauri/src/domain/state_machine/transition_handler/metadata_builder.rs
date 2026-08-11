// Pure metadata builder functions for atomic metadata writes
//
// This module provides builder functions that construct metadata JSON
// from state data without any I/O operations. All functions are pure
// and testable in isolation.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::domain::entities::status::InternalStatus;
use crate::domain::state_machine::types::FailedData;

// ============================================================================
// ResumeCategory - Categorizes states for smart resume handling
// ============================================================================

/// Categories for how a stopped task should be resumed.
///
/// This enum determines the resume behavior based on the state the task
/// was in when stopped:
///
/// | Category | States | Resume Behavior |
/// |----------|--------|-----------------|
/// | **Direct** | `Executing`, `ReExecuting`, `Reviewing`, `QaRefining`, `QaTesting` | Transition directly, spawn agent |
/// | **Validated** | `Merging`, `PendingMerge`, `MergeConflict`, `MergeIncomplete` | Validate git state first |
/// | **Redirect** | `QaPassed`, `RevisionNeeded`, `PendingReview` | Resume to successor state |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResumeCategory {
    /// Resume directly to the state (spawn agent immediately).
    /// Used for agent-active states where no pre-validation is needed.
    Direct,
    /// Resume with validation (check git state, cleanup agents).
    /// Used for merge-related states that require external state validation.
    Validated,
    /// Resume to a successor state instead (auto-transition states).
    /// These states would auto-transition anyway, so redirect to the target.
    Redirect,
}

impl ResumeCategory {
    /// Returns the resume category for the given status.
    ///
    /// # Arguments
    /// * `status` - The status the task was in when stopped
    ///
    /// # Returns
    /// The appropriate `ResumeCategory` for resume handling.
    pub fn from_status(status: InternalStatus) -> Self {
        use InternalStatus::*;
        match status {
            // Direct resume: agent-active states that don't require validation
            Executing | ReExecuting | Reviewing | QaRefining | QaTesting => ResumeCategory::Direct,

            // Validated resume: merge-related states requiring git validation
            Merging | PendingMerge | MergeConflict | MergeIncomplete => ResumeCategory::Validated,

            // Redirect: auto-transition states
            QaPassed | RevisionNeeded | PendingReview => ResumeCategory::Redirect,

            // All other states fall back to Direct (they shouldn't be stopped from these)
            // but if they are, resuming directly is the safest fallback
            _ => ResumeCategory::Direct,
        }
    }
}

/// Categorize a status for resume handling.
///
/// This is a convenience function that delegates to `ResumeCategory::from_status`.
///
/// # Arguments
/// * `status` - The status the task was in when stopped
///
/// # Returns
/// The appropriate `ResumeCategory` for resume handling.
pub fn categorize_resume_state(status: InternalStatus) -> ResumeCategory {
    ResumeCategory::from_status(status)
}

/// Get the resume target for a stopped task.
///
/// For redirect states, this returns the successor state that the task
/// should be resumed to. For direct and validated states, this returns
/// the original state.
///
/// # Redirect Mapping
/// - `QaPassed` → `PendingReview` (auto-transitions to review anyway)
/// - `RevisionNeeded` → `ReExecuting` (auto-transitions to re-execution)
/// - `PendingReview` → `Reviewing` (spawn reviewer)
///
/// # Arguments
/// * `status` - The status the task was in when stopped
///
/// # Returns
/// The status the task should be resumed to.
pub fn get_resume_target(status: InternalStatus) -> InternalStatus {
    use InternalStatus::*;
    match status {
        // Redirect states: return successor
        QaPassed => PendingReview,
        RevisionNeeded => ReExecuting,
        PendingReview => Reviewing,

        // All other states: resume directly to same state
        other => other,
    }
}

// ============================================================================
// StopMetadata - Captures context when a task is stopped mid-execution
// ============================================================================

/// Metadata captured when a task is stopped mid-execution.
///
/// This enables "smart resume" capability where the system can restore
/// the task to its previous state with context about why it was stopped.
///
/// Schema:
/// ```json
/// {
///   "stopped_from_status": "merging",
///   "stop_reason": "User stopped to protect main branch",
///   "stopped_at": "2026-02-15T10:30:00Z"
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StopMetadata {
    /// The status the task was in when stopped (stored as snake_case string)
    pub stopped_from_status: String,
    /// Optional reason provided by user for stopping
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    /// Timestamp when the task was stopped (RFC3339 format)
    pub stopped_at: String,
}

impl StopMetadata {
    /// Create new stop metadata with the given from_status and optional reason.
    /// The timestamp is automatically set to now.
    pub fn new(from_status: InternalStatus, reason: Option<String>) -> Self {
        Self {
            stopped_from_status: from_status.as_str().to_string(),
            stop_reason: reason,
            stopped_at: Utc::now().to_rfc3339(),
        }
    }

    /// Parse the stopped_from_status string back to InternalStatus.
    /// Returns None if the status string is invalid.
    pub fn parse_from_status(&self) -> Option<InternalStatus> {
        self.stopped_from_status.parse().ok()
    }

    /// Parse the stopped_at timestamp string back to DateTime.
    /// Returns None if the timestamp is invalid.
    pub fn parse_stopped_at(&self) -> Option<DateTime<Utc>> {
        DateTime::parse_from_rfc3339(&self.stopped_at)
            .map(|dt| dt.with_timezone(&Utc))
            .ok()
    }
}

/// A metadata update containing key-value pairs to merge into existing task metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct MetadataUpdate {
    entries: Map<String, Value>,
}

impl MetadataUpdate {
    /// Create a new empty metadata update.
    pub fn new() -> Self {
        Self {
            entries: Map::new(),
        }
    }

    /// Add a string key-value pair to the update.
    pub fn with_string(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.entries.insert(key.into(), Value::String(value.into()));
        self
    }

    /// Add a boolean key-value pair to the update.
    pub fn with_bool(mut self, key: impl Into<String>, value: bool) -> Self {
        self.entries.insert(key.into(), Value::Bool(value));
        self
    }

    /// Add a null value for a key (used to clear/remove keys).
    pub fn with_null(mut self, key: impl Into<String>) -> Self {
        self.entries.insert(key.into(), Value::Null);
        self
    }

    /// Add a u32 numeric key-value pair to the update.
    pub fn with_u32(mut self, key: impl Into<String>, value: u32) -> Self {
        self.entries.insert(key.into(), Value::Number(value.into()));
        self
    }

    /// Add a JSON value for a key.
    pub fn with_value(mut self, key: impl Into<String>, value: Value) -> Self {
        self.entries.insert(key.into(), value);
        self
    }

    /// Merge this update into existing metadata, preserving existing keys.
    ///
    /// # Arguments
    /// * `existing` - Optional JSON string of existing metadata
    ///
    /// # Returns
    /// JSON string with merged metadata (update keys overwrite existing).
    /// Null values remove keys from the result.
    pub fn merge_into(self, existing: Option<&str>) -> String {
        let mut base = if let Some(existing_str) = existing {
            // Parse existing metadata or start fresh if invalid
            serde_json::from_str::<Map<String, Value>>(existing_str).unwrap_or_else(|_| Map::new())
        } else {
            Map::new()
        };

        // Merge update entries (overwrites existing keys, null removes)
        for (key, value) in self.entries {
            if value.is_null() {
                base.remove(&key);
            } else {
                base.insert(key, value);
            }
        }

        serde_json::to_string(&base).unwrap_or_else(|_| "{}".to_string())
    }

    /// Check if a key exists in the given metadata JSON.
    ///
    /// # Arguments
    /// * `key` - The key to check for
    /// * `metadata` - Optional JSON string to check
    ///
    /// # Returns
    /// `true` if the key exists in the metadata, `false` otherwise
    pub fn key_exists_in(key: &str, metadata: Option<&str>) -> bool {
        if let Some(metadata_str) = metadata {
            if let Ok(obj) = serde_json::from_str::<Map<String, Value>>(metadata_str) {
                return obj.contains_key(key);
            }
        }
        false
    }
}

impl Default for MetadataUpdate {
    fn default() -> Self {
        Self::new()
    }
}

/// Build metadata for a failed task.
///
/// Creates metadata with:
/// - `failure_error`: The error message
/// - `failure_details`: Optional additional details
/// - `is_timeout`: Whether the failure was due to a timeout
/// - `attempt_count`: Number of execution attempts (from `FailedData.attempt_count`)
pub fn build_failed_metadata(data: &FailedData) -> MetadataUpdate {
    let mut update = MetadataUpdate::new()
        .with_string("failure_error", &data.error)
        .with_bool("is_timeout", data.is_timeout)
        .with_u32("attempt_count", data.attempt_count);

    if let Some(ref details) = data.details {
        update = update.with_string("failure_details", details);
    }

    update
}

/// Build metadata for trigger origin tracking.
///
/// Creates metadata with:
/// - `trigger_origin`: The origin identifier (e.g., "qa", "scheduler")
pub fn build_trigger_origin_metadata(origin: &str) -> MetadataUpdate {
    MetadataUpdate::new().with_string("trigger_origin", origin)
}

/// Build stop metadata for a task being stopped mid-execution.
///
/// Creates metadata with:
/// - `stopped_from_status`: The status the task was in when stopped
/// - `stop_reason`: Optional reason provided by user
/// - `stopped_at`: RFC3339 timestamp of when the task was stopped
pub fn build_stop_metadata(from_status: InternalStatus, reason: Option<String>) -> MetadataUpdate {
    let stop_data = StopMetadata::new(from_status, reason);
    let json_value = serde_json::to_string(&stop_data).unwrap_or_else(|_| "{}".to_string());
    MetadataUpdate::new().with_string("stop_metadata", json_value)
}

/// Parse stop metadata from a task's metadata JSON string.
///
/// Returns `Some(StopMetadata)` if the stop_metadata key exists and is valid,
/// or `None` if it doesn't exist or parsing fails.
pub fn parse_stop_metadata(metadata: Option<&str>) -> Option<StopMetadata> {
    let metadata_str = metadata?;
    let obj = serde_json::from_str::<Map<String, Value>>(metadata_str).ok()?;
    let stop_json = obj.get("stop_metadata")?.as_str()?;
    serde_json::from_str(stop_json).ok()
}

/// Build metadata update to clear stop metadata (for restart cleanup).
///
/// When a task is restarted, the stop metadata should be cleared
/// to prevent stale context from affecting future operations.
pub fn clear_stop_metadata() -> MetadataUpdate {
    MetadataUpdate::new().with_null("stop_metadata")
}

/// Build metadata update for task restart: clears stop metadata and optionally stores restart note.
///
/// When `note` is `Some`, stores it as `restart_note` for downstream consumers
/// (agent prompts, context hints) to read. The note is ephemeral — cleared after first read.
///
/// When `note` is `None`, behaves identically to `clear_stop_metadata()`.
pub fn build_restart_metadata(note: Option<&str>) -> MetadataUpdate {
    let base = MetadataUpdate::new().with_null("stop_metadata");
    match note {
        Some(n) => base.with_string("restart_note", n),
        None => base,
    }
}
