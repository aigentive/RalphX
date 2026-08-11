use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{ChatConversationId, ProjectId};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AutomationId(pub String);

impl AutomationId {
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

impl Default for AutomationId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for AutomationId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AutomationRunId(pub String);

impl AutomationRunId {
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

impl Default for AutomationRunId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for AutomationRunId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationStatus {
    Draft,
    Active,
    Paused,
    Completed,
    Stopped,
}

impl AutomationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Stopped => "stopped",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "draft" => Some(Self::Draft),
            "active" => Some(Self::Active),
            "paused" => Some(Self::Paused),
            "completed" => Some(Self::Completed),
            "stopped" => Some(Self::Stopped),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationRunStatus {
    Pending,
    Provisioning,
    Running,
    AwaitingPlanApproval,
    Published,
    Completed,
    Merged,
    PrClosed,
    AgentFailed,
    Cancelled,
}

impl AutomationRunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Provisioning => "provisioning",
            Self::Running => "running",
            Self::AwaitingPlanApproval => "awaiting_plan_approval",
            Self::Published => "published",
            Self::Completed => "completed",
            Self::Merged => "merged",
            Self::PrClosed => "pr_closed",
            Self::AgentFailed => "agent_failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "provisioning" => Some(Self::Provisioning),
            "running" => Some(Self::Running),
            "awaiting_plan_approval" => Some(Self::AwaitingPlanApproval),
            "published" => Some(Self::Published),
            "completed" => Some(Self::Completed),
            "merged" => Some(Self::Merged),
            "pr_closed" => Some(Self::PrClosed),
            "agent_failed" => Some(Self::AgentFailed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationJudgeState {
    None,
    InProgress,
    Done,
    Failed,
    Skipped,
}

impl AutomationJudgeState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::InProgress => "in_progress",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "in_progress" => Some(Self::InProgress),
            "done" => Some(Self::Done),
            "failed" => Some(Self::Failed),
            "skipped" => Some(Self::Skipped),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationPlanApprovalMode {
    Manual,
    Automatic,
}

impl AutomationPlanApprovalMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Automatic => "automatic",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "manual" => Some(Self::Manual),
            "automatic" => Some(Self::Automatic),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationPrMergeMode {
    Manual,
    Automatic,
}

impl AutomationPrMergeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Automatic => "automatic",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "manual" => Some(Self::Manual),
            "automatic" => Some(Self::Automatic),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationPlanJudgeState {
    None,
    InProgress,
    Done,
    Failed,
}

impl AutomationPlanJudgeState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::InProgress => "in_progress",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "in_progress" => Some(Self::InProgress),
            "done" => Some(Self::Done),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationPromptAuthor {
    SetupAgent,
    Judge,
    SkipJudgeTemplate,
}

impl AutomationPromptAuthor {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SetupAgent => "setup_agent",
            Self::Judge => "judge",
            Self::SkipJudgeTemplate => "skip_judge_template",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "setup_agent" => Some(Self::SetupAgent),
            "judge" => Some(Self::Judge),
            "skip_judge_template" => Some(Self::SkipJudgeTemplate),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationContextRefKind {
    Project,
    Integration,
    Artifact,
}

impl AutomationContextRefKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Integration => "integration",
            Self::Artifact => "artifact",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "project" => Some(Self::Project),
            "integration" => Some(Self::Integration),
            "artifact" => Some(Self::Artifact),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Automation {
    pub id: AutomationId,
    pub project_id: ProjectId,
    pub name: String,
    pub status: AutomationStatus,
    pub paused_reason_code: Option<String>,
    pub paused_reason_detail: Option<String>,
    pub goal_prompt: String,
    pub setup_conversation_id: Option<ChatConversationId>,
    pub provider_harness: String,
    pub model_id: String,
    pub logical_effort: Option<String>,
    pub run_mode: String,
    pub base_ref_kind: String,
    pub base_ref: String,
    pub base_display_name: Option<String>,
    pub base_source_pull_request_json: Option<String>,
    pub goal_items_json: Option<String>,
    pub chain_mode: String,
    pub completion_signal: String,
    pub plan_approval_mode: AutomationPlanApprovalMode,
    pub pr_merge_mode: AutomationPrMergeMode,
    pub plan_deep_verification: bool,
    pub max_runs: i64,
    pub max_consecutive_failures: i64,
    pub first_run_prompt: Option<String>,
    pub setup_analysis_summary: Option<String>,
    pub spec_artifact_id: Option<String>,
    pub authoring_state_json: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationRun {
    pub id: AutomationRunId,
    pub automation_id: AutomationId,
    pub run_index: i64,
    pub status: AutomationRunStatus,
    pub judge_state: AutomationJudgeState,
    pub judge_lease_expires_at: Option<DateTime<Utc>>,
    pub plan_judge_state: AutomationPlanJudgeState,
    pub plan_judge_lease_expires_at: Option<DateTime<Utc>>,
    pub plan_judge_verdict_json: Option<String>,
    pub plan_revision_round: i64,
    pub plan_reminder_count: i64,
    pub plan_pending_instructions: Option<String>,
    pub plan_last_parked_artifact_id: Option<String>,
    #[serde(default)]
    pub plan_last_parked_blueprint_artifact_id: Option<String>,
    pub agent_phase_started_at: Option<DateTime<Utc>>,
    pub conversation_id: Option<ChatConversationId>,
    pub run_prompt: String,
    pub prompt_author: AutomationPromptAuthor,
    pub base_ref_kind: String,
    pub base_ref_used: String,
    pub base_from_run_id: Option<AutomationRunId>,
    /// Goal item this run was created to advance. Stamped once at run creation
    /// by the scheduler; never rewritten by judge or recovery paths. `None` for
    /// phase-less automations and pre-migration history.
    pub goal_item_id: Option<String>,
    pub branch_name: Option<String>,
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
    pub pr_title: Option<String>,
    pub pr_head_ref_name: Option<String>,
    pub pr_base_ref_name: Option<String>,
    pub pr_merged_at: Option<DateTime<Utc>>,
    pub merge_commit_sha: Option<String>,
    pub diff_stats_json: Option<String>,
    pub agent_summary: Option<String>,
    pub judge_verdict_json: Option<String>,
    pub judge_model_id: Option<String>,
    pub error_code: Option<String>,
    pub error_detail: Option<String>,
    pub signal_check_failures: i64,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationAttachment {
    pub id: String,
    pub automation_id: AutomationId,
    pub file_name: String,
    pub file_path: String,
    pub mime_type: Option<String>,
    pub file_size: Option<i64>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationContextRef {
    pub id: String,
    pub automation_id: AutomationId,
    pub ref_kind: AutomationContextRefKind,
    pub payload_json: String,
    pub position: i64,
}

pub fn automation_is_transition_allowed(from: AutomationStatus, to: AutomationStatus) -> bool {
    matches!(
        (from, to),
        (AutomationStatus::Draft, AutomationStatus::Active)
            | (AutomationStatus::Draft, AutomationStatus::Stopped)
            | (AutomationStatus::Active, AutomationStatus::Paused)
            | (AutomationStatus::Active, AutomationStatus::Completed)
            | (AutomationStatus::Active, AutomationStatus::Stopped)
            | (AutomationStatus::Paused, AutomationStatus::Active)
            | (AutomationStatus::Paused, AutomationStatus::Stopped)
            | (AutomationStatus::Stopped, AutomationStatus::Active)
    )
}

pub fn automation_run_is_transition_allowed(
    from: AutomationRunStatus,
    to: AutomationRunStatus,
) -> bool {
    matches!(
        (from, to),
        (
            AutomationRunStatus::Pending,
            AutomationRunStatus::Provisioning
        ) | (AutomationRunStatus::Pending, AutomationRunStatus::Cancelled)
            | (
                AutomationRunStatus::Provisioning,
                AutomationRunStatus::Running
            )
            | (
                AutomationRunStatus::Provisioning,
                AutomationRunStatus::AgentFailed
            )
            | (
                AutomationRunStatus::Provisioning,
                AutomationRunStatus::Cancelled
            )
            | (AutomationRunStatus::Running, AutomationRunStatus::Published)
            | (
                AutomationRunStatus::Running,
                AutomationRunStatus::AwaitingPlanApproval
            )
            | (AutomationRunStatus::Running, AutomationRunStatus::Completed)
            | (
                AutomationRunStatus::Running,
                AutomationRunStatus::AgentFailed
            )
            | (AutomationRunStatus::Running, AutomationRunStatus::Cancelled)
            | (
                AutomationRunStatus::AwaitingPlanApproval,
                AutomationRunStatus::Running
            )
            | (
                AutomationRunStatus::AwaitingPlanApproval,
                AutomationRunStatus::Cancelled
            )
            | (AutomationRunStatus::Published, AutomationRunStatus::Merged)
            | (
                AutomationRunStatus::Published,
                AutomationRunStatus::PrClosed
            )
            | (
                AutomationRunStatus::Published,
                AutomationRunStatus::Cancelled
            )
    )
}

pub fn judge_is_transition_allowed(from: AutomationJudgeState, to: AutomationJudgeState) -> bool {
    matches!(
        (from, to),
        (AutomationJudgeState::None, AutomationJudgeState::InProgress)
            | (AutomationJudgeState::None, AutomationJudgeState::Skipped)
            | (AutomationJudgeState::InProgress, AutomationJudgeState::Done)
            | (
                AutomationJudgeState::InProgress,
                AutomationJudgeState::Failed
            )
            | (AutomationJudgeState::Done, AutomationJudgeState::Failed)
            | (
                AutomationJudgeState::Failed,
                AutomationJudgeState::InProgress
            )
            | (AutomationJudgeState::Failed, AutomationJudgeState::Skipped)
    )
}

pub fn plan_judge_is_transition_allowed(
    from: AutomationPlanJudgeState,
    to: AutomationPlanJudgeState,
) -> bool {
    matches!(
        (from, to),
        (
            AutomationPlanJudgeState::None,
            AutomationPlanJudgeState::InProgress
        ) | (
            AutomationPlanJudgeState::InProgress,
            AutomationPlanJudgeState::Done
        ) | (
            AutomationPlanJudgeState::InProgress,
            AutomationPlanJudgeState::Failed
        ) | (
            AutomationPlanJudgeState::InProgress,
            AutomationPlanJudgeState::None
        ) | (
            AutomationPlanJudgeState::Done,
            AutomationPlanJudgeState::Failed
        ) | (
            AutomationPlanJudgeState::Failed,
            AutomationPlanJudgeState::None
        ) | (
            AutomationPlanJudgeState::Done,
            AutomationPlanJudgeState::None
        )
    )
}

pub fn judge_transition_clears_verdict(
    to: AutomationJudgeState,
    judge_verdict_json: Option<&str>,
) -> bool {
    to == AutomationJudgeState::InProgress && judge_verdict_json.is_none()
}

pub fn is_open_automation_run(
    status: AutomationRunStatus,
    judge_state: AutomationJudgeState,
) -> bool {
    matches!(
        status,
        AutomationRunStatus::Pending
            | AutomationRunStatus::Provisioning
            | AutomationRunStatus::Running
            | AutomationRunStatus::AwaitingPlanApproval
            | AutomationRunStatus::Published
    ) || (matches!(
        status,
        AutomationRunStatus::Merged
            | AutomationRunStatus::PrClosed
            | AutomationRunStatus::AgentFailed
    ) && matches!(
        judge_state,
        AutomationJudgeState::None
            | AutomationJudgeState::InProgress
            | AutomationJudgeState::Failed
    ))
}

pub fn is_signal_terminal_automation_run(status: AutomationRunStatus) -> bool {
    matches!(
        status,
        AutomationRunStatus::Merged
            | AutomationRunStatus::PrClosed
            | AutomationRunStatus::AgentFailed
            | AutomationRunStatus::Completed
    )
}

pub fn latest_run_holds_goal_authority(run: &AutomationRun) -> bool {
    matches!(
        run.status,
        AutomationRunStatus::Pending
            | AutomationRunStatus::Provisioning
            | AutomationRunStatus::Running
            | AutomationRunStatus::AwaitingPlanApproval
            | AutomationRunStatus::Published
    ) || (is_signal_terminal_automation_run(run.status)
        && matches!(
            run.judge_state,
            AutomationJudgeState::None
                | AutomationJudgeState::InProgress
                | AutomationJudgeState::Done
        ))
}
