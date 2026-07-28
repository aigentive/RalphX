use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::entities::types::ProjectId;
use crate::error::AppError;

pub const RECURRENCE_KEY_FIELD: &str = "recurrence_key";
pub const RECURRENCE_SESSION_FIELD: &str = "recurrence_session";
pub const RECURRENCE_KEY_PREFIX: &str = "token-set-v1:";

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
pub enum TaskOutcomeSource {
    AgentSession,
    AgentWorkspace,
    AgentWorkspacePr,
    GithubPrReview,
    AgentConversation,
    Review,
    GitCommitHistory,
    GithubPrHistory,
    PlanMode,
    Merge,
    MergeValidation,
    Verification,
    TaskPipeline,
}

impl TaskOutcomeSource {
    pub fn is_live(self) -> bool {
        self != Self::TaskPipeline
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::AgentSession => "agent_session",
            Self::AgentWorkspace => "agent_workspace",
            Self::AgentWorkspacePr => "agent_workspace_pr",
            Self::GithubPrReview => "github_pr_review",
            Self::AgentConversation => "agent_conversation",
            Self::Review => "review",
            Self::GitCommitHistory => "git_commit_history",
            Self::GithubPrHistory => "github_pr_history",
            Self::PlanMode => "plan_mode",
            Self::Merge => "merge",
            Self::MergeValidation => "merge_validation",
            Self::Verification => "verification",
            Self::TaskPipeline => "task_pipeline",
        }
    }
}

impl fmt::Display for TaskOutcomeSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TaskOutcomeSource {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "agent_session" => Ok(Self::AgentSession),
            "agent_workspace" => Ok(Self::AgentWorkspace),
            "agent_workspace_pr" => Ok(Self::AgentWorkspacePr),
            "github_pr_review" => Ok(Self::GithubPrReview),
            "agent_conversation" => Ok(Self::AgentConversation),
            "review" => Ok(Self::Review),
            "git_commit_history" => Ok(Self::GitCommitHistory),
            "github_pr_history" => Ok(Self::GithubPrHistory),
            "plan_mode" => Ok(Self::PlanMode),
            "merge" => Ok(Self::Merge),
            "merge_validation" => Ok(Self::MergeValidation),
            "verification" => Ok(Self::Verification),
            "task_pipeline" => Ok(Self::TaskPipeline),
            _ => Err(AppError::Validation(format!(
                "invalid task outcome source: {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TaskOutcomeClass {
    TaskExecutionCompleted,
    TaskExecutionNoOutput,
    TaskExecutionFailed,
    WorkspaceCodeChanges,
    WorkspacePrPublished,
    WorkspacePrChangesRequested,
    WorkspacePrTerminal,
    WorkspacePrClosed,
    WorkspacePrFailed,
    WorkspacePrMerged,
    WorkspacePrMergedClean,
    WorkspacePrMergedWithFollowups,
    WorkspaceSessionAbandoned,
    WorkspacePublishFailed,
    WorkspacePrStaleRepair,
    PlanModeAccepted,
    PlanModeDeclined,
    PlanModeRevisionRequested,
    ReviewApproved,
    ReviewApprovedNoChanges,
    ReviewChangesRequested,
    ReviewEscalated,
    GithubPrChangesRequested,
    ConversationSkillCandidate,
    GitHistoryCommit,
    GithubPrHistory,
    VerificationGapRecurring,
    MergeCompleted,
    MergeValidationFailed,
    MergeConflict,
    MergeQaFailed,
    MergeTimeout,
    Other(String),
}

impl TaskOutcomeClass {
    pub fn as_str(&self) -> &str {
        match self {
            Self::TaskExecutionCompleted => "task_execution_completed",
            Self::TaskExecutionNoOutput => "task_execution_no_output",
            Self::TaskExecutionFailed => "task_execution_failed",
            Self::WorkspaceCodeChanges => "workspace_code_changes",
            Self::WorkspacePrPublished => "workspace_pr_published",
            Self::WorkspacePrChangesRequested => "workspace_pr_changes_requested",
            Self::WorkspacePrTerminal => "workspace_pr_terminal",
            Self::WorkspacePrClosed => "workspace_pr_closed",
            Self::WorkspacePrFailed => "workspace_pr_failed",
            Self::WorkspacePrMerged => "workspace_pr_merged",
            Self::WorkspacePrMergedClean => "workspace_pr_merged_clean",
            Self::WorkspacePrMergedWithFollowups => "workspace_pr_merged_with_followups",
            Self::WorkspaceSessionAbandoned => "workspace_session_abandoned",
            Self::WorkspacePublishFailed => "workspace_publish_failed",
            Self::WorkspacePrStaleRepair => "workspace_pr_stale_repair",
            Self::PlanModeAccepted => "plan_mode_accepted",
            Self::PlanModeDeclined => "plan_mode_declined",
            Self::PlanModeRevisionRequested => "plan_mode_revision_requested",
            Self::ReviewApproved => "review_approved",
            Self::ReviewApprovedNoChanges => "review_approved_no_changes",
            Self::ReviewChangesRequested => "review_changes_requested",
            Self::ReviewEscalated => "review_escalated",
            Self::GithubPrChangesRequested => "github_pr_changes_requested",
            Self::ConversationSkillCandidate => "conversation_skill_candidate",
            Self::GitHistoryCommit => "git_history_commit",
            Self::GithubPrHistory => "github_pr_history",
            Self::VerificationGapRecurring => "verification_gap_recurring",
            Self::MergeCompleted => "merge_completed",
            Self::MergeValidationFailed => "merge_validation_failed",
            Self::MergeConflict => "merge_conflict",
            Self::MergeQaFailed => "merge_qa_failed",
            Self::MergeTimeout => "merge_timeout",
            Self::Other(value) => value,
        }
    }
}

impl fmt::Display for TaskOutcomeClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The short Plan-mode verdict strings `"accepted"`, `"declined"`, and
/// `"revision_requested"` are retired in favour of the canonical `plan_mode_*`
/// classes. Rows persisted under the retired strings intentionally fall through
/// to `Other(...)`, which round-trips losslessly; they are not backfilled.
impl From<&str> for TaskOutcomeClass {
    fn from(value: &str) -> Self {
        match value {
            "task_execution_completed" => Self::TaskExecutionCompleted,
            "task_execution_no_output" => Self::TaskExecutionNoOutput,
            "task_execution_failed" => Self::TaskExecutionFailed,
            "workspace_code_changes" => Self::WorkspaceCodeChanges,
            "workspace_pr_published" => Self::WorkspacePrPublished,
            "workspace_pr_changes_requested" => Self::WorkspacePrChangesRequested,
            "workspace_pr_terminal" => Self::WorkspacePrTerminal,
            "workspace_pr_closed" => Self::WorkspacePrClosed,
            "workspace_pr_failed" => Self::WorkspacePrFailed,
            "workspace_pr_merged" => Self::WorkspacePrMerged,
            "workspace_pr_merged_clean" => Self::WorkspacePrMergedClean,
            "workspace_pr_merged_with_followups" => Self::WorkspacePrMergedWithFollowups,
            "workspace_session_abandoned" => Self::WorkspaceSessionAbandoned,
            "workspace_publish_failed" => Self::WorkspacePublishFailed,
            "workspace_pr_stale_repair" => Self::WorkspacePrStaleRepair,
            "plan_mode_accepted" => Self::PlanModeAccepted,
            "plan_mode_declined" => Self::PlanModeDeclined,
            "plan_mode_revision_requested" => Self::PlanModeRevisionRequested,
            "review_approved" => Self::ReviewApproved,
            "review_approved_no_changes" => Self::ReviewApprovedNoChanges,
            "review_changes_requested" => Self::ReviewChangesRequested,
            "review_escalated" => Self::ReviewEscalated,
            "github_pr_changes_requested" => Self::GithubPrChangesRequested,
            "conversation_skill_candidate" => Self::ConversationSkillCandidate,
            "git_history_commit" => Self::GitHistoryCommit,
            "github_pr_history" => Self::GithubPrHistory,
            "verification_gap_recurring" => Self::VerificationGapRecurring,
            "merge_completed" => Self::MergeCompleted,
            "merge_validation_failed" => Self::MergeValidationFailed,
            "merge_conflict" => Self::MergeConflict,
            "merge_qa_failed" => Self::MergeQaFailed,
            "merge_timeout" => Self::MergeTimeout,
            value => Self::Other(value.to_string()),
        }
    }
}

impl Serialize for TaskOutcomeClass {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for TaskOutcomeClass {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Self::from(String::deserialize(deserializer)?.as_str()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillUsageInjectionKind {
    CompactIndex,
    FullLoad,
    ComposerDirective,
    InteractiveStdinUnattributed,
}

impl SkillUsageInjectionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CompactIndex => "compact_index",
            Self::FullLoad => "full_load",
            Self::ComposerDirective => "composer_directive",
            Self::InteractiveStdinUnattributed => "interactive_stdin_unattributed",
        }
    }
}

impl fmt::Display for SkillUsageInjectionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SkillUsageInjectionKind {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "compact_index" => Ok(Self::CompactIndex),
            "full_load" => Ok(Self::FullLoad),
            "composer_directive" => Ok(Self::ComposerDirective),
            "interactive_stdin_unattributed" => Ok(Self::InteractiveStdinUnattributed),
            _ => Err(AppError::Validation(format!(
                "invalid skill usage injection kind: {value}"
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
    pub source: TaskOutcomeSource,
    pub source_ref_kind: String,
    pub source_ref_id: String,
    pub task_id: Option<String>,
    pub conversation_id: Option<String>,
    pub agent_run_id: Option<String>,
    pub pull_request_id: Option<String>,
    pub proposal_id: Option<String>,
    pub verification_id: Option<String>,
    pub review_id: Option<String>,
    pub outcome_class: Option<TaskOutcomeClass>,
    pub status: TaskOutcomeStatus,
    pub evidence_json: Value,
    pub failure_fingerprint: Option<String>,
    pub provider_harness: Option<String>,
    pub provider_session_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TaskOutcomeRecurrenceCorpus {
    pub eligible_observations: u64,
    pub distinct_sessions: u64,
}

pub fn is_valid_recurrence_key(value: &str) -> bool {
    let Some(digest) = value.strip_prefix(RECURRENCE_KEY_PREFIX) else {
        return false;
    };
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub fn task_outcome_recurrence_metadata(outcome: &TaskOutcome) -> Option<(&str, &str)> {
    let key = outcome
        .evidence_json
        .get(RECURRENCE_KEY_FIELD)?
        .as_str()?
        .trim();
    let session = outcome
        .evidence_json
        .get(RECURRENCE_SESSION_FIELD)?
        .as_str()?
        .trim();
    (is_valid_recurrence_key(key) && !session.is_empty()).then_some((key, session))
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
    pub injection_kind: SkillUsageInjectionKind,
    pub outcome_id: Option<TaskOutcomeId>,
    pub metadata_json: Value,
    pub created_at: DateTime<Utc>,
}
