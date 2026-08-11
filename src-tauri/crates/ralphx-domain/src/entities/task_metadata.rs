// Task metadata types for structured data stored in tasks.metadata JSON field
// Provides type-safe handling of merge recovery events and other metadata

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::TaskId;

/// Merge recovery metadata stored in tasks.metadata
/// Tracks the full history of merge deferral and retry attempts
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MergeRecoveryMetadata {
    /// Schema version for future compatibility
    pub version: u32,
    /// Append-only event log (capped at 50 events, oldest trimmed)
    pub events: Vec<MergeRecoveryEvent>,
    /// Current recovery state
    pub last_state: MergeRecoveryState,
    /// ISO 8601 timestamp: do not retry merge until this time (provider rate limit)
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub rate_limit_retry_after: Option<String>,
    /// Whether the circuit breaker has fired (prevents auto-retry until manual reset)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub circuit_breaker_active: bool,
    /// Human-readable reason why the circuit breaker fired
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub circuit_breaker_reason: Option<String>,
}

impl MergeRecoveryMetadata {
    /// Create new merge recovery metadata with empty event log
    pub fn new() -> Self {
        Self {
            version: 1,
            events: Vec::new(),
            last_state: MergeRecoveryState::Succeeded,
            rate_limit_retry_after: None,
            circuit_breaker_active: false,
            circuit_breaker_reason: None,
        }
    }

    /// Maximum number of events to keep in the log
    pub const MAX_EVENTS: usize = 50;

    /// Append a new event to the log
    /// Automatically trims oldest events if cap is exceeded
    pub fn append_event(&mut self, event: MergeRecoveryEvent) {
        self.events.push(event);
        self.trim_if_needed();
    }

    /// Append a new event and update last_state
    pub fn append_event_with_state(
        &mut self,
        event: MergeRecoveryEvent,
        state: MergeRecoveryState,
    ) {
        self.append_event(event);
        self.last_state = state;
    }

    /// Trim events if count exceeds MAX_EVENTS
    /// Removes oldest events (from the beginning of the vector)
    fn trim_if_needed(&mut self) {
        if self.events.len() > Self::MAX_EVENTS {
            let excess = self.events.len() - Self::MAX_EVENTS;
            self.events.drain(0..excess);
        }
    }

    /// Parse metadata from task's metadata JSON string
    /// Returns Ok(Some(metadata)) if merge_recovery key exists and is valid
    /// Returns Ok(None) if merge_recovery key doesn't exist
    /// Returns Err if JSON is invalid or merge_recovery value can't be parsed
    pub fn from_task_metadata(
        metadata_json: Option<&str>,
    ) -> Result<Option<Self>, serde_json::Error> {
        let Some(json_str) = metadata_json else {
            return Ok(None);
        };

        let value: serde_json::Value = serde_json::from_str(json_str)?;

        if let Some(merge_recovery) = value.get("merge_recovery") {
            let recovery: MergeRecoveryMetadata = serde_json::from_value(merge_recovery.clone())?;
            Ok(Some(recovery))
        } else {
            Ok(None)
        }
    }

    /// Update task's metadata JSON string with this merge recovery metadata
    /// Preserves other keys in the metadata object
    /// Returns updated JSON string
    pub fn update_task_metadata(
        &self,
        existing_metadata: Option<&str>,
    ) -> Result<String, serde_json::Error> {
        let mut metadata_obj = if let Some(json_str) = existing_metadata {
            serde_json::from_str::<serde_json::Value>(json_str)
                .unwrap_or_else(|_| serde_json::json!({}))
        } else {
            serde_json::json!({})
        };

        if let Some(obj) = metadata_obj.as_object_mut() {
            obj.insert("merge_recovery".to_string(), serde_json::to_value(self)?);
        }

        serde_json::to_string(&metadata_obj)
    }
}

impl Default for MergeRecoveryMetadata {
    fn default() -> Self {
        Self::new()
    }
}

/// Review-time scope snapshot stored in task metadata for later merge-time verification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewScopeMetadata {
    /// The proposal's coarse planned scope at review time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub planned_paths: Vec<String>,
    /// Backend-computed out-of-scope files seen by the reviewer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reviewed_out_of_scope_files: Vec<String>,
    /// Reviewer's explicit drift classification, if any.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub drift_classification: Option<String>,
    /// Optional reviewer notes explaining the drift decision.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub drift_notes: Option<String>,
    /// When this review snapshot was recorded.
    pub reviewed_at: DateTime<Utc>,
}

impl ReviewScopeMetadata {
    pub fn new(
        planned_paths: Vec<String>,
        reviewed_out_of_scope_files: Vec<String>,
        drift_classification: Option<String>,
        drift_notes: Option<String>,
    ) -> Self {
        Self {
            planned_paths,
            reviewed_out_of_scope_files,
            drift_classification,
            drift_notes,
            reviewed_at: Utc::now(),
        }
    }

    /// Parse metadata from task's metadata JSON string.
    pub fn from_task_metadata(
        metadata_json: Option<&str>,
    ) -> Result<Option<Self>, serde_json::Error> {
        let Some(json_str) = metadata_json else {
            return Ok(None);
        };

        let value: serde_json::Value = serde_json::from_str(json_str)?;

        if let Some(review_scope) = value.get("review_scope") {
            let review_scope: ReviewScopeMetadata = serde_json::from_value(review_scope.clone())?;
            Ok(Some(review_scope))
        } else {
            Ok(None)
        }
    }

    /// Update task metadata with this review scope snapshot.
    pub fn update_task_metadata(
        &self,
        existing_metadata: Option<&str>,
    ) -> Result<String, serde_json::Error> {
        let mut metadata_obj = if let Some(json_str) = existing_metadata {
            serde_json::from_str::<serde_json::Value>(json_str)
                .unwrap_or_else(|_| serde_json::json!({}))
        } else {
            serde_json::json!({})
        };

        if let Some(obj) = metadata_obj.as_object_mut() {
            obj.insert("review_scope".to_string(), serde_json::to_value(self)?);
        }

        serde_json::to_string(&metadata_obj)
    }

    /// Remove any stale review scope snapshot from task metadata.
    pub fn clear_from_task_metadata(
        existing_metadata: Option<&str>,
    ) -> Result<Option<String>, serde_json::Error> {
        let Some(json_str) = existing_metadata else {
            return Ok(None);
        };

        let mut metadata_obj: serde_json::Value =
            serde_json::from_str(json_str).unwrap_or_else(|_| serde_json::json!({}));

        if let Some(obj) = metadata_obj.as_object_mut() {
            if obj.remove("review_scope").is_some() {
                return Ok(Some(serde_json::to_string(&metadata_obj)?));
            }
        }

        Ok(Some(json_str.to_string()))
    }
}

/// Individual merge recovery event
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MergeRecoveryEvent {
    /// When this event occurred
    pub at: DateTime<Utc>,
    /// Type of event
    pub kind: MergeRecoveryEventKind,
    /// Who/what triggered this event
    pub source: MergeRecoverySource,
    /// Reason code for categorization
    pub reason_code: MergeRecoveryReasonCode,
    /// Human-readable message
    pub message: String,
    /// Target branch being merged into
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_branch: Option<String>,
    /// Source branch being merged from
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_branch: Option<String>,
    /// Task blocking this merge (if deferred due to concurrent merge)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocking_task_id: Option<TaskId>,
    /// Attempt number for retries
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    /// Classification of the failure source for smart retry decisions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_source: Option<MergeFailureSource>,
    /// SHA of source branch at time of failure (for SHA comparison guard)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_sha: Option<String>,
}

impl MergeRecoveryEvent {
    /// Create a new merge recovery event
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        kind: MergeRecoveryEventKind,
        source: MergeRecoverySource,
        reason_code: MergeRecoveryReasonCode,
        message: impl Into<String>,
    ) -> Self {
        Self {
            at: Utc::now(),
            kind,
            source,
            reason_code,
            message: message.into(),
            target_branch: None,
            source_branch: None,
            blocking_task_id: None,
            attempt: None,
            failure_source: None,
            source_sha: None,
        }
    }

    /// Builder method to add target branch
    pub fn with_target_branch(mut self, branch: impl Into<String>) -> Self {
        self.target_branch = Some(branch.into());
        self
    }

    /// Builder method to add source branch
    pub fn with_source_branch(mut self, branch: impl Into<String>) -> Self {
        self.source_branch = Some(branch.into());
        self
    }

    /// Builder method to add blocking task
    pub fn with_blocking_task(mut self, task_id: TaskId) -> Self {
        self.blocking_task_id = Some(task_id);
        self
    }

    /// Builder method to add attempt number
    pub fn with_attempt(mut self, attempt: u32) -> Self {
        self.attempt = Some(attempt);
        self
    }

    /// Builder method to set failure source classification
    pub fn with_failure_source(mut self, failure_source: MergeFailureSource) -> Self {
        self.failure_source = Some(failure_source);
        self
    }

    /// Builder method to record source branch SHA at failure time
    pub fn with_source_sha(mut self, sha: impl Into<String>) -> Self {
        self.source_sha = Some(sha.into());
        self
    }
}

/// Classification of why a merge failure occurred.
/// Used by the reconciler to decide whether auto-retry is safe.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MergeFailureSource {
    /// Transient git operation error (lock files, timeouts, network) — safe to auto-retry
    TransientGit,
    /// Agent explicitly called report_conflict — human deliberate decision, do NOT auto-retry
    AgentReported,
    /// System detected failure (conflict markers, MERGE_HEAD, stale rebase) — safe to auto-retry
    SystemDetected,
    /// Post-merge validation reverted — do not auto-retry until code changes
    ValidationFailed,
    /// Worktree path deleted but git entry still exists (TOCTOU race) — safe to auto-retry after prune
    WorktreeMissing,
    /// Git process spawn failed (ENOENT or permission denied) — may indicate worktree issue
    SpawnFailure,
    /// Git lock file contention — transient, safe to retry
    LockContention,
    /// Provider rate limit hit — respect retry_after timestamp
    RateLimited,
    /// Target branch is busy with another merge — deferral-based, never counts toward circuit breaker
    TargetBranchBusy,
    /// Merge cleanup step timed out (pre-merge worktree scan or deferred kill) — safe to auto-retry
    CleanupTimeout,
    /// Running count leaked due to reviewer/executor exit after task already transitioned — safe to auto-retry
    TeardownRace,
    /// merge_pipeline_active TTL expired — possible crash during prior merge attempt — safe to auto-retry
    PipelineActiveExpired,
    /// Repository hook failed because its worktree environment/tooling could not bootstrap
    HookEnvironment,
    /// Same repository hook failure repeated after a corrective re-execution attempt
    RepeatedHookFailure,
    /// Git authentication/credential failure requiring user or operator action
    AuthFailure,
    /// Disk full / ENOSPC while git tried to write repository state
    DiskFull,
    /// Deterministic database or infrastructure failure surfaced during merge handling
    DeterministicInfra,
    /// Unrecognized failure source from stored metadata (backward compat)
    #[serde(other)]
    Unknown,
}

/// Phase of merge cleanup that encountered an issue.
/// Used alongside `merge_failure_source: cleanup_timeout` to pinpoint where cleanup failed.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CleanupPhase {
    /// Phase 3: deferred_merge_cleanup kill of worktree processes (post-merge)
    DeferredWorktreeKill,
    /// Phase 0b: pre_merge_cleanup lsof/kill scan before merge pipeline starts
    PreMergeWorktreeScan,
}

/// Whether the system will automatically retry a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryStrategy {
    /// System will auto-retry this failure
    AutoRetry,
    /// System will auto-retry but does NOT count toward the circuit breaker threshold (deferral-based)
    AutoRetryNoCB,
    /// System will NOT auto-retry; user manual retry is always allowed
    NoAutomaticRetry,
}

impl MergeFailureSource {
    /// Returns the retry strategy for this failure source.
    pub fn retry_strategy(&self) -> RetryStrategy {
        match self {
            Self::AgentReported
            | Self::ValidationFailed
            | Self::HookEnvironment
            | Self::RepeatedHookFailure
            | Self::AuthFailure
            | Self::DiskFull
            | Self::DeterministicInfra
            | Self::Unknown => RetryStrategy::NoAutomaticRetry,
            Self::TargetBranchBusy => RetryStrategy::AutoRetryNoCB,
            _ => RetryStrategy::AutoRetry,
        }
    }
}

/// Type of merge recovery event
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MergeRecoveryEventKind {
    /// Merge was deferred due to conflict or blocking condition
    Deferred,
    /// Automatic retry was triggered by system
    AutoRetryTriggered,
    /// Retry attempt started
    AttemptStarted,
    /// Retry attempt failed
    AttemptFailed,
    /// Retry attempt succeeded
    AttemptSucceeded,
    /// Manual retry initiated by user
    ManualRetry,
    /// Main-branch merge deferred because agents are running
    MainMergeDeferred,
    /// Main-branch merge retry triggered after agents went idle
    MainMergeRetry,
}

/// Source of the merge recovery event
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MergeRecoverySource {
    /// Triggered by system logic
    System,
    /// Triggered by automatic retry mechanism
    Auto,
    /// Triggered by user action
    User,
}

/// Reason code for merge recovery event
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MergeRecoveryReasonCode {
    /// Target branch is busy with another merge
    TargetBranchBusy,
    /// Git operation error
    GitError,
    /// Validation failed
    ValidationFailed,
    /// Source or target branch does not exist
    BranchNotFound,
    /// Merge deferred because agents are running globally
    AgentsRunning,
    /// Deferred merge forced retry after timeout expired
    DeferredTimeout,
    /// Provider rate limit hit during merge agent run
    ProviderRateLimited,
    /// Unknown/unclassified reason
    Unknown,
}

/// Current state of merge recovery
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MergeRecoveryState {
    /// Merge is deferred, waiting for retry
    Deferred,
    /// Currently retrying
    Retrying,
    /// Recovery failed
    Failed,
    /// Recovery succeeded
    Succeeded,
    /// Waiting for provider rate limit to clear
    RateLimited,
}

/// Execution recovery metadata stored in tasks.metadata
/// Tracks the full history of execution timeout and retry attempts
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionRecoveryMetadata {
    /// Schema version for future compatibility
    pub version: u32,
    /// Append-only event log (capped at 50 events, oldest trimmed)
    pub events: Vec<ExecutionRecoveryEvent>,
    /// Current recovery state
    pub last_state: ExecutionRecoveryState,
    /// When true, reconciler will not auto-retry (user stopped retrying or max retries exceeded)
    pub stop_retrying: bool,
    /// How many times this task has been auto-recovered from permanent git errors.
    /// Incremented each time auto_recover_task() resets the task to Ready.
    /// Capped at MAX_AUTO_RECOVERIES before permanently failing.
    #[serde(default)]
    pub auto_recovery_count: u32,
    /// Reason why stop_retrying was set to true (for diagnostics/UI display).
    /// None if stop_retrying is false or was set before this field existed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unrecoverable_reason: Option<StopRetryingReason>,
}

impl ExecutionRecoveryMetadata {
    /// Create new execution recovery metadata with empty event log
    pub fn new() -> Self {
        Self {
            version: 1,
            events: Vec::new(),
            last_state: ExecutionRecoveryState::Retrying,
            stop_retrying: false,
            auto_recovery_count: 0,
            unrecoverable_reason: None,
        }
    }

    /// Maximum number of events to keep in the log
    pub const MAX_EVENTS: usize = 50;

    /// Append a new event to the log
    /// Automatically trims oldest events if cap is exceeded
    pub fn append_event(&mut self, event: ExecutionRecoveryEvent) {
        self.events.push(event);
        self.trim_if_needed();
    }

    /// Append a new event and update last_state
    pub fn append_event_with_state(
        &mut self,
        event: ExecutionRecoveryEvent,
        state: ExecutionRecoveryState,
    ) {
        self.append_event(event);
        self.last_state = state;
    }

    /// Returns the number of times this task has been auto-recovered from permanent git errors.
    pub fn get_auto_recovery_count(&self) -> u32 {
        self.auto_recovery_count
    }

    /// Trim events if count exceeds MAX_EVENTS
    /// Removes oldest events (from the beginning of the vector)
    fn trim_if_needed(&mut self) {
        if self.events.len() > Self::MAX_EVENTS {
            let excess = self.events.len() - Self::MAX_EVENTS;
            self.events.drain(0..excess);
        }
    }

    /// Count `AutoRetryTriggered` events filtered by failure source.
    /// Used for per-source retry budgets (e.g. GitIsolation has its own cap, separate from timeout retries).
    pub fn auto_retry_count_for_source(&self, source: ExecutionFailureSource) -> u32 {
        self.events
            .iter()
            .filter(|e| {
                matches!(e.kind, ExecutionRecoveryEventKind::AutoRetryTriggered)
                    && e.failure_source == Some(source)
            })
            .count() as u32
    }

    /// Returns true if the last recorded failure is transient (safe to auto-retry)
    /// Checks only the most recent event — not historical ones — to avoid stale state.
    pub fn last_failure_is_transient(&self) -> bool {
        self.events
            .last()
            .and_then(|e| e.failure_source.as_ref())
            .map(|source| source.is_transient())
            .unwrap_or(false)
    }

    /// Parse metadata from task's metadata JSON string
    /// Returns Ok(Some(metadata)) if execution_recovery key exists and is valid
    /// Returns Ok(None) if execution_recovery key doesn't exist
    /// Returns Err if JSON is invalid or execution_recovery value can't be parsed
    pub fn from_task_metadata(
        metadata_json: Option<&str>,
    ) -> Result<Option<Self>, serde_json::Error> {
        let Some(json_str) = metadata_json else {
            return Ok(None);
        };
        Self::from_json(json_str)
    }

    /// Parse metadata from a JSON string
    /// Returns Ok(Some(metadata)) if execution_recovery key exists and is valid
    /// Returns Ok(None) if execution_recovery key doesn't exist
    pub fn from_json(json_str: &str) -> Result<Option<Self>, serde_json::Error> {
        let value: serde_json::Value = serde_json::from_str(json_str)?;

        if let Some(execution_recovery) = value.get("execution_recovery") {
            let recovery: ExecutionRecoveryMetadata =
                serde_json::from_value(execution_recovery.clone())?;
            Ok(Some(recovery))
        } else {
            Ok(None)
        }
    }

    /// Update task's metadata JSON string with this execution recovery metadata
    /// Preserves other keys in the metadata object
    /// Returns updated JSON string
    pub fn update_task_metadata(
        &self,
        existing_metadata: Option<&str>,
    ) -> Result<String, serde_json::Error> {
        let mut metadata_obj = if let Some(json_str) = existing_metadata {
            serde_json::from_str::<serde_json::Value>(json_str)
                .unwrap_or_else(|_| serde_json::json!({}))
        } else {
            serde_json::json!({})
        };

        if let Some(obj) = metadata_obj.as_object_mut() {
            obj.insert(
                "execution_recovery".to_string(),
                serde_json::to_value(self)?,
            );
        }

        serde_json::to_string(&metadata_obj)
    }
}

impl Default for ExecutionRecoveryMetadata {
    fn default() -> Self {
        Self::new()
    }
}

/// Individual execution recovery event
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionRecoveryEvent {
    /// When this event occurred
    pub at: DateTime<Utc>,
    /// Type of event
    pub kind: ExecutionRecoveryEventKind,
    /// Who/what triggered this event
    pub source: ExecutionRecoverySource,
    /// Reason code for categorization
    pub reason_code: ExecutionRecoveryReasonCode,
    /// Human-readable message
    pub message: String,
    /// Attempt number for retries
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    /// Classification of the failure source for smart retry decisions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_source: Option<ExecutionFailureSource>,
}

impl ExecutionRecoveryEvent {
    /// Create a new execution recovery event
    pub fn new(
        kind: ExecutionRecoveryEventKind,
        source: ExecutionRecoverySource,
        reason_code: ExecutionRecoveryReasonCode,
        message: impl Into<String>,
    ) -> Self {
        Self {
            at: Utc::now(),
            kind,
            source,
            reason_code,
            message: message.into(),
            attempt: None,
            failure_source: None,
        }
    }

    /// Builder method to add attempt number
    pub fn with_attempt(mut self, attempt: u32) -> Self {
        self.attempt = Some(attempt);
        self
    }

    /// Builder method to set failure source classification
    pub fn with_failure_source(mut self, failure_source: ExecutionFailureSource) -> Self {
        self.failure_source = Some(failure_source);
        self
    }
}

/// Error prefix for git isolation failures.
/// Use this constant at both the generation site (on_enter_states.rs) and
/// classification site (task_transition_service.rs) to avoid fragile string matching.
pub const GIT_ISOLATION_ERROR_PREFIX: &str = "Git isolation failed";

/// Classification of why an execution failure occurred.
/// Used by the reconciler to decide whether auto-retry is safe.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionFailureSource {
    /// Agent stream produced no output within timeout — safe to auto-retry
    TransientTimeout,
    /// Agent stream stalled during parse — safe to auto-retry
    ParseStall,
    /// Agent process exited unexpectedly — safe to auto-retry
    AgentCrash,
    /// Provider returned an error (handled by Paused) — do NOT auto-retry here
    ProviderError,
    /// Wall-clock (C5) timeout fired — do NOT auto-retry (would loop infinitely)
    WallClockTimeout,
    /// Transient git isolation failure (stale index.lock, leftover worktree dir, concurrent git op) — safe to auto-retry after cleanup
    GitIsolation,
    /// Agent exited successfully without producing enough work to satisfy execution completion
    AgentIncomplete,
    /// A local command or MCP tool failed without proving that the agent process crashed
    LocalToolFailed,
    /// Backend-owned validation rejected the execution result; not an agent crash.
    ValidationFailed,
    /// Unknown/unclassified failure
    Unknown,
}

impl ExecutionFailureSource {
    /// Returns true if this failure source is transient and safe to auto-retry
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            ExecutionFailureSource::TransientTimeout
                | ExecutionFailureSource::ParseStall
                | ExecutionFailureSource::AgentCrash
                | ExecutionFailureSource::GitIsolation
        )
    }
}

/// Type of execution recovery event
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionRecoveryEventKind {
    /// Execution failed and was recorded
    Failed,
    /// Automatic retry was triggered by the reconciler
    AutoRetryTriggered,
    /// Retry attempt started
    AttemptStarted,
    /// Retry attempt succeeded
    AttemptSucceeded,
    /// Manual retry initiated by user
    ManualRetry,
    /// User or system stopped further retries
    StopRetrying,
    /// A false terminal failure was corrected after current-attempt proof succeeded.
    CompletedWorkRecovered,
}

/// Source of the execution recovery event
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionRecoverySource {
    /// Triggered by system logic
    System,
    /// Triggered by automatic retry mechanism
    Auto,
    /// Triggered by user action
    User,
    /// Triggered by startup recovery (recover_timeout_failures) — GAP M5 sentinel
    Startup,
}

/// Reason code for execution recovery event
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionRecoveryReasonCode {
    /// Agent stream timed out (no output)
    Timeout,
    /// Agent stream stalled during parse
    ParseStall,
    /// Agent process exited unexpectedly
    AgentExit,
    /// Provider returned an error
    ProviderError,
    /// Wall-clock (C5) hard limit exceeded
    WallClockExceeded,
    /// Maximum retry budget exhausted
    MaxRetriesExceeded,
    /// User explicitly stopped retrying
    UserStopped,
    /// Git isolation failure (stale index.lock, leftover worktree dir, concurrent git op)
    GitIsolationFailed,
    /// Permanent git error — branch lost or deleted, auto-recovery exhausted
    GitBranchLost,
    /// Structural git error (missing branch, no commits) — retrying cannot help
    StructuralGitError,
    /// Git-isolation retry budget exhausted (3/3 retries failed)
    GitIsolationExhausted,
    /// Agent exited successfully before task steps were complete
    IncompleteSteps,
    /// Agent made a deterministic local command usage error
    AgentCommandInvalid,
    /// Backend-owned validation returned failed evidence
    ValidationFailed,
    /// Local command or MCP tool failed without proving an agent process crash
    LocalToolFailed,
    /// Preserved work was proven complete by current run, git, step, and validation evidence.
    ValidatedCompletedWork,
    /// Unknown/unclassified reason
    Unknown,
}

/// Current state of execution recovery
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionRecoveryState {
    /// Currently retrying (auto-retry in progress)
    Retrying,
    /// Recovery permanently failed (max retries exceeded or user stopped)
    Failed,
    /// Execution completed successfully after retries
    Succeeded,
}

/// Reason why auto-retry was permanently stopped.
/// Stored in `ExecutionRecoveryMetadata.unrecoverable_reason` for diagnostics.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StopRetryingReason {
    /// Normal retry budget exhaustion
    MaxRetriesExceeded,
    /// Branch deleted/corrupted, auto-recovery exhausted
    GitBranchLost,
    /// User-initiated stop
    ManualStop,
    /// Structural git error (missing branch, no commits) — retrying cannot help
    StructuralGitError,
    /// Git-isolation retry budget exhausted (3/3 retries failed)
    GitIsolationExhausted,
    /// Deterministic local command usage error by the agent; retrying is expected to repeat it.
    AgentCommandInvalid,
    /// Unknown variant from newer code — MUST be treated as stop reason (safe default:
    /// unknown variants were explicitly set as stop reasons by newer code).
    #[serde(other)]
    Unknown,
}

/// Validation cache metadata stored in tasks.metadata
/// Stores test execution results + commit SHA captured at execution completion.
/// Used by reviewer and merger agents to skip redundant test runs when SHA hasn't changed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ValidationCacheMetadata {
    /// Schema version for future compatibility
    pub version: u32,
    /// HEAD commit SHA at time of capture (from git rev-parse HEAD on task branch)
    pub commit_sha: String,
    /// Whether any tests were run during execution
    pub tests_ran: bool,
    /// Whether all tests passed (only meaningful when tests_ran=true)
    pub tests_passed: bool,
    /// Brief summary of test results (e.g., "6758 passed, 0 failed")
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub test_summary: Option<String>,
    /// When this cache entry was captured
    pub captured_at: DateTime<Utc>,
    /// Which handler captured this entry (e.g., "execution_complete")
    pub captured_by: String,
}

impl ValidationCacheMetadata {
    /// Parse metadata from task's metadata JSON string
    /// Returns Ok(Some(metadata)) if validation_cache key exists and is valid
    /// Returns Ok(None) if validation_cache key doesn't exist
    /// Returns Err if JSON is invalid or validation_cache value can't be parsed
    pub fn from_task_metadata(
        metadata_json: Option<&str>,
    ) -> Result<Option<Self>, serde_json::Error> {
        let Some(json_str) = metadata_json else {
            return Ok(None);
        };

        let value: serde_json::Value = serde_json::from_str(json_str)?;

        if let Some(validation_cache) = value.get("validation_cache") {
            let cache: ValidationCacheMetadata = serde_json::from_value(validation_cache.clone())?;
            Ok(Some(cache))
        } else {
            Ok(None)
        }
    }

    /// Update task's metadata JSON string with this validation cache metadata
    /// Preserves other keys in the metadata object (merge_recovery, execution_recovery, etc.)
    /// Returns updated JSON string
    pub fn update_task_metadata(
        &self,
        existing_metadata: Option<&str>,
    ) -> Result<String, serde_json::Error> {
        let mut metadata_obj = if let Some(json_str) = existing_metadata {
            serde_json::from_str::<serde_json::Value>(json_str)
                .unwrap_or_else(|_| serde_json::json!({}))
        } else {
            serde_json::json!({})
        };

        if let Some(obj) = metadata_obj.as_object_mut() {
            obj.insert("validation_cache".to_string(), serde_json::to_value(self)?);
        }

        serde_json::to_string(&metadata_obj)
    }
}

#[cfg(test)]
#[path = "task_metadata_tests.rs"]
mod tests;

/// Pure function to compute the validation hint and human-readable message from a cache entry
/// and the current HEAD SHA.
///
/// No git calls, no side effects — fully unit-testable.
///
/// Returns `(validation_hint, hint_message)` where `validation_hint` is one of:
/// - `"skip_tests"` — tests passed on the same SHA
/// - `"skip_test_validation"` — no tests ran at execution time
/// - `"run_tests"` — SHA mismatch, tests failed, or cache is otherwise stale
pub fn compute_validation_hint(
    cache: &ValidationCacheMetadata,
    current_sha: &str,
    episode_entered_at: Option<chrono::DateTime<chrono::Utc>>,
) -> (String, String) {
    if current_sha != cache.commit_sha {
        (
            "run_tests".to_string(),
            format!(
                "Cache stale (SHA changed from {} to {}). Run tests normally.",
                &cache.commit_sha[..8.min(cache.commit_sha.len())],
                &current_sha[..8.min(current_sha.len())]
            ),
        )
    } else if episode_entered_at
        .map(|entered_at| cache.captured_at < entered_at)
        .unwrap_or(true)
    {
        (
            "run_tests".to_string(),
            format!(
                "Cache was not captured during the current execution episode (commit {}). Run tests normally.",
                &cache.commit_sha[..8.min(cache.commit_sha.len())]
            ),
        )
    } else if !cache.tests_ran {
        (
            "skip_test_validation".to_string(),
            format!(
                "No tests were run during execution (commit {}). Skip test validation.",
                &cache.commit_sha[..8.min(cache.commit_sha.len())]
            ),
        )
    } else if cache.tests_passed {
        (
            "skip_tests".to_string(),
            format!(
                "Tests passed on commit {}. Skip test re-run.",
                &cache.commit_sha[..8.min(cache.commit_sha.len())]
            ),
        )
    } else {
        // tests_ran=true but tests_passed=false → must re-run
        (
            "run_tests".to_string(),
            format!(
                "Tests failed on commit {}. Run tests normally.",
                &cache.commit_sha[..8.min(cache.commit_sha.len())]
            ),
        )
    }
}
