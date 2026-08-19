use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::entities::{
    ArtifactId, ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSessionId, PlanBranchId,
    ProjectId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentConversationWorkspaceMode {
    Chat,
    Edit,
    Plan,
    Tasks,
    Autopilot,
    Ideation,
    ReviewPr,
    Automation,
    PersonaBuilder,
}

impl std::fmt::Display for AgentConversationWorkspaceMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentConversationWorkspaceMode::Chat => write!(f, "chat"),
            AgentConversationWorkspaceMode::Edit => write!(f, "edit"),
            AgentConversationWorkspaceMode::Plan => write!(f, "plan"),
            AgentConversationWorkspaceMode::Tasks => write!(f, "tasks"),
            AgentConversationWorkspaceMode::Autopilot => write!(f, "autopilot"),
            AgentConversationWorkspaceMode::Ideation => write!(f, "ideation"),
            AgentConversationWorkspaceMode::ReviewPr => write!(f, "review_pr"),
            AgentConversationWorkspaceMode::Automation => write!(f, "automation"),
            AgentConversationWorkspaceMode::PersonaBuilder => write!(f, "persona_builder"),
        }
    }
}

impl FromStr for AgentConversationWorkspaceMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "chat" => Ok(Self::Chat),
            "edit" => Ok(Self::Edit),
            "plan" => Ok(Self::Plan),
            "tasks" => Ok(Self::Tasks),
            "autopilot" => Ok(Self::Autopilot),
            "ideation" => Ok(Self::Ideation),
            "review_pr" => Ok(Self::ReviewPr),
            "automation" => Ok(Self::Automation),
            "persona_builder" => Ok(Self::PersonaBuilder),
            _ => Err(format!(
                "unknown agent conversation workspace mode: '{value}'"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentConversationWorkspaceStatus {
    Active,
    Archived,
    Missing,
}

impl std::fmt::Display for AgentConversationWorkspaceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentConversationWorkspaceStatus::Active => write!(f, "active"),
            AgentConversationWorkspaceStatus::Archived => write!(f, "archived"),
            AgentConversationWorkspaceStatus::Missing => write!(f, "missing"),
        }
    }
}

impl FromStr for AgentConversationWorkspaceStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "active" => Ok(Self::Active),
            "archived" => Ok(Self::Archived),
            "missing" => Ok(Self::Missing),
            _ => Err(format!(
                "unknown agent conversation workspace status: '{value}'"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentWorkspacePrReviewMonitorStatus {
    Idle,
    Reviewing,
    AwaitingUser,
    Watching,
    Submitting,
    Blocked,
    Paused,
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentWorkspaceReviewMonitorStatus {
    Idle,
    Reviewing,
    Ready,
    Blocked,
}

pub const WORKSPACE_REVIEW_FIXER_STATUS_ROUTING: &str = "routing";
pub const WORKSPACE_REVIEW_FIXER_STATUS_QUEUED: &str = "queued";
pub const WORKSPACE_REVIEW_FIXER_STATUS_RUNNING: &str = "running";
pub const WORKSPACE_REVIEW_FIXER_STATUS_CYCLE_CAPPED: &str = "cycle_capped";

pub fn workspace_review_fixer_status_is_active(status: Option<&str>) -> bool {
    matches!(
        status,
        Some(
            WORKSPACE_REVIEW_FIXER_STATUS_ROUTING
                | WORKSPACE_REVIEW_FIXER_STATUS_QUEUED
                | WORKSPACE_REVIEW_FIXER_STATUS_RUNNING
        )
    )
}

/// Response-only classification of whether the current runtime owns Review mutations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentWorkspaceReviewRuntimeState {
    ActiveOwned,
    Terminal,
    MissingRuntimeIdentity,
    MalformedRuntimeIdentity,
    StaleRuntime,
}

impl std::fmt::Display for AgentWorkspaceReviewRuntimeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ActiveOwned => write!(f, "active_owned"),
            Self::Terminal => write!(f, "terminal"),
            Self::MissingRuntimeIdentity => write!(f, "missing_runtime_identity"),
            Self::MalformedRuntimeIdentity => write!(f, "malformed_runtime_identity"),
            Self::StaleRuntime => write!(f, "stale_runtime"),
        }
    }
}

impl std::fmt::Display for AgentWorkspaceReviewMonitorStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "idle"),
            Self::Reviewing => write!(f, "reviewing"),
            Self::Ready => write!(f, "ready"),
            Self::Blocked => write!(f, "blocked"),
        }
    }
}

impl FromStr for AgentWorkspaceReviewMonitorStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "idle" => Ok(Self::Idle),
            "reviewing" => Ok(Self::Reviewing),
            "ready" => Ok(Self::Ready),
            "blocked" => Ok(Self::Blocked),
            _ => Err(format!(
                "unknown workspace review monitor status: '{value}'"
            )),
        }
    }
}

/// Durable GitHub auto-merge state owned by an authoritative workspace Review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentWorkspaceReviewAutoMergeGuardStatus {
    Pausing,
    PausedForReview,
    AwaitingPublish,
    Restoring,
    RestoreFailed,
}

impl std::fmt::Display for AgentWorkspaceReviewAutoMergeGuardStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pausing => write!(f, "pausing"),
            Self::PausedForReview => write!(f, "paused_for_review"),
            Self::AwaitingPublish => write!(f, "awaiting_publish"),
            Self::Restoring => write!(f, "restoring"),
            Self::RestoreFailed => write!(f, "restore_failed"),
        }
    }
}

impl FromStr for AgentWorkspaceReviewAutoMergeGuardStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pausing" => Ok(Self::Pausing),
            "paused_for_review" => Ok(Self::PausedForReview),
            "awaiting_publish" => Ok(Self::AwaitingPublish),
            "restoring" => Ok(Self::Restoring),
            "restore_failed" => Ok(Self::RestoreFailed),
            _ => Err(format!(
                "unknown workspace review auto-merge guard status: '{value}'"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentWorkspaceReviewAutoMergeGuard {
    pub status: AgentWorkspaceReviewAutoMergeGuardStatus,
    pub pr_number: i64,
    pub merge_method: String,
    pub target_scope: AgentWorkspaceReviewTargetScope,
    pub diff_fingerprint: String,
    pub head_sha: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentWorkspaceReviewOutcome {
    None,
    Passed,
    Blocking,
    NoChanges,
    RunFailed,
}

impl std::fmt::Display for AgentWorkspaceReviewOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "none"),
            Self::Passed => write!(f, "passed"),
            Self::Blocking => write!(f, "blocking"),
            Self::NoChanges => write!(f, "no_changes"),
            Self::RunFailed => write!(f, "run_failed"),
        }
    }
}

impl FromStr for AgentWorkspaceReviewOutcome {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "none" => Ok(Self::None),
            "passed" | "reviewed" => Ok(Self::Passed),
            "blocking" => Ok(Self::Blocking),
            "no_changes" => Ok(Self::NoChanges),
            "run_failed" | "failed" | "blocked" => Ok(Self::RunFailed),
            _ => Err(format!("unknown workspace review outcome: '{value}'")),
        }
    }
}

/// What the last settled review produced, frozen at the start of the next review run.
///
/// Lets a re-review triage delta-first instead of re-reading a delta it already cleared.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentWorkspacePreviousReviewSnapshot {
    pub overview_artifact_id: ArtifactId,
    pub requested_changes_artifact_id: Option<ArtifactId>,
    pub artifact_version: Option<u32>,
    pub reviewed_diff_fingerprint: Option<String>,
    pub reviewed_head_sha: Option<String>,
    pub outcome: AgentWorkspaceReviewOutcome,
}

/// Disposition the reviewer recorded on its final Review artifact write.
///
/// Deliberately narrower than [`AgentWorkspaceReviewOutcome`]: `run_failed` and `no_changes`
/// are backend-derived states a reviewer must never be able to assert about its own artifact,
/// so they are unrepresentable here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentWorkspaceReviewArtifactOutcome {
    Passed,
    Blocking,
}

impl std::fmt::Display for AgentWorkspaceReviewArtifactOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Passed => write!(f, "passed"),
            Self::Blocking => write!(f, "blocking"),
        }
    }
}

impl FromStr for AgentWorkspaceReviewArtifactOutcome {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "passed" => Ok(Self::Passed),
            "blocking" => Ok(Self::Blocking),
            _ => Err(format!(
                "unknown workspace review artifact outcome: '{value}'"
            )),
        }
    }
}

impl From<AgentWorkspaceReviewArtifactOutcome> for AgentWorkspaceReviewOutcome {
    fn from(value: AgentWorkspaceReviewArtifactOutcome) -> Self {
        match value {
            AgentWorkspaceReviewArtifactOutcome::Passed => Self::Passed,
            AgentWorkspaceReviewArtifactOutcome::Blocking => Self::Blocking,
        }
    }
}

/// How the review gate reached its settled value.
///
/// `Typed` is the reviewer calling `complete_workspace_review_run`. `ArtifactDegraded` is the
/// backend settling from a current artifact pair carrying a recorded outcome after the reviewer
/// wrapper timed out; it deliberately withholds auto-merge arming and fixer routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentWorkspaceReviewSettlementSource {
    Typed,
    ArtifactDegraded,
}

impl std::fmt::Display for AgentWorkspaceReviewSettlementSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Typed => write!(f, "typed"),
            Self::ArtifactDegraded => write!(f, "artifact_degraded"),
        }
    }
}

impl FromStr for AgentWorkspaceReviewSettlementSource {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "typed" => Ok(Self::Typed),
            "artifact_degraded" => Ok(Self::ArtifactDegraded),
            _ => Err(format!(
                "unknown workspace review settlement source: '{value}'"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentWorkspaceReviewGateStatus {
    NotRequired,
    Required,
    Reviewing,
    Passed,
    Blocking,
    Failed,
}

impl std::fmt::Display for AgentWorkspaceReviewGateStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotRequired => write!(f, "not_required"),
            Self::Required => write!(f, "required"),
            Self::Reviewing => write!(f, "reviewing"),
            Self::Passed => write!(f, "passed"),
            Self::Blocking => write!(f, "blocking"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

impl FromStr for AgentWorkspaceReviewGateStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "not_required" => Ok(Self::NotRequired),
            "required" => Ok(Self::Required),
            "reviewing" => Ok(Self::Reviewing),
            "passed" => Ok(Self::Passed),
            "blocking" => Ok(Self::Blocking),
            "failed" => Ok(Self::Failed),
            _ => Err(format!("unknown workspace review gate status: '{value}'")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentWorkspaceReviewTargetScope {
    SelectedSource,
    WorkspaceDelta,
}

impl std::fmt::Display for AgentWorkspaceReviewTargetScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SelectedSource => write!(f, "selected_source"),
            Self::WorkspaceDelta => write!(f, "workspace_delta"),
        }
    }
}

impl FromStr for AgentWorkspaceReviewTargetScope {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "selected_source" => Ok(Self::SelectedSource),
            "workspace_delta" => Ok(Self::WorkspaceDelta),
            _ => Err(format!("unknown workspace review target scope: '{value}'")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentWorkspaceReviewHunkAnnotation {
    pub id: String,
    pub conversation_id: ChatConversationId,
    pub project_id: ProjectId,
    pub artifact_id: ArtifactId,
    pub artifact_version: u32,
    pub target_scope: AgentWorkspaceReviewTargetScope,
    pub head_sha: Option<String>,
    pub diff_fingerprint: String,
    pub path: String,
    pub diff_source: String,
    pub hunk_header: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub title: Option<String>,
    pub message: String,
    pub level: String,
    /// Hash of this file's patch-vs-base at the time the annotation was written.
    ///
    /// Hunk anchors are per-file: `@@ -a,b +c,d @@` offsets are relative to that file's own diff,
    /// so a file whose patch text is byte-identical between review cycles has byte-identical
    /// anchors. That is what lets an annotation carry forward verbatim instead of being
    /// regenerated. `None` means "unknown", which fails carry-forward closed.
    pub file_patch_hash: Option<String>,
    pub created_by_run_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl std::fmt::Display for AgentWorkspacePrReviewMonitorStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "idle"),
            Self::Reviewing => write!(f, "reviewing"),
            Self::AwaitingUser => write!(f, "awaiting_user"),
            Self::Watching => write!(f, "watching"),
            Self::Submitting => write!(f, "submitting"),
            Self::Blocked => write!(f, "blocked"),
            Self::Paused => write!(f, "paused"),
            Self::Terminal => write!(f, "terminal"),
        }
    }
}

impl FromStr for AgentWorkspacePrReviewMonitorStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "idle" => Ok(Self::Idle),
            "reviewing" => Ok(Self::Reviewing),
            "awaiting_user" => Ok(Self::AwaitingUser),
            "watching" => Ok(Self::Watching),
            "submitting" => Ok(Self::Submitting),
            "blocked" => Ok(Self::Blocked),
            "paused" => Ok(Self::Paused),
            "terminal" => Ok(Self::Terminal),
            _ => Err(format!("unknown PR review monitor status: '{value}'")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentWorkspacePrReviewActionKind {
    RequestChanges,
    Approve,
    Comment,
}

impl std::fmt::Display for AgentWorkspacePrReviewActionKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RequestChanges => write!(f, "request_changes"),
            Self::Approve => write!(f, "approve"),
            Self::Comment => write!(f, "comment"),
        }
    }
}

impl FromStr for AgentWorkspacePrReviewActionKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "request_changes" => Ok(Self::RequestChanges),
            "approve" => Ok(Self::Approve),
            "comment" => Ok(Self::Comment),
            _ => Err(format!("unknown PR review action: '{value}'")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentWorkspacePrReviewActionStatus {
    Pending,
    Approved,
    Skipped,
    Submitting,
    Submitted,
    Failed,
    Superseded,
}

impl std::fmt::Display for AgentWorkspacePrReviewActionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Approved => write!(f, "approved"),
            Self::Skipped => write!(f, "skipped"),
            Self::Submitting => write!(f, "submitting"),
            Self::Submitted => write!(f, "submitted"),
            Self::Failed => write!(f, "failed"),
            Self::Superseded => write!(f, "superseded"),
        }
    }
}

impl FromStr for AgentWorkspacePrReviewActionStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "approved" => Ok(Self::Approved),
            "skipped" => Ok(Self::Skipped),
            "submitting" => Ok(Self::Submitting),
            "submitted" => Ok(Self::Submitted),
            "failed" => Ok(Self::Failed),
            "superseded" => Ok(Self::Superseded),
            _ => Err(format!("unknown PR review action status: '{value}'")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentConversationWorkspaceBranchMode {
    Isolated,
    Linked,
}

impl Default for AgentConversationWorkspaceBranchMode {
    fn default() -> Self {
        Self::Isolated
    }
}

impl std::fmt::Display for AgentConversationWorkspaceBranchMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentConversationWorkspaceBranchMode::Isolated => write!(f, "isolated"),
            AgentConversationWorkspaceBranchMode::Linked => write!(f, "linked"),
        }
    }
}

impl FromStr for AgentConversationWorkspaceBranchMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "isolated" => Ok(Self::Isolated),
            "linked" => Ok(Self::Linked),
            _ => Err(format!("unknown agent workspace branch mode: '{value}'")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentWorkspaceSourcePullRequest {
    pub number: i64,
    pub url: Option<String>,
    pub title: Option<String>,
    pub head_ref_name: String,
    pub base_ref_name: Option<String>,
    pub head_ref_oid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentWorkspaceFollowupProvenance {
    pub origin_conversation_id: ChatConversationId,
    pub source_task_id: Option<String>,
    pub source_context_type: Option<String>,
    pub source_context_id: Option<String>,
    pub source_agent_name: Option<String>,
    pub spawn_reason: Option<String>,
    pub blocker_fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentWorkspacePrReviewMonitor {
    pub conversation_id: ChatConversationId,
    pub project_id: ProjectId,
    pub pr_number: i64,
    pub status: AgentWorkspacePrReviewMonitorStatus,
    pub monitor_enabled: bool,
    pub auto_approve_enabled: bool,
    pub first_review_completed: bool,
    pub first_action_resolved: bool,
    pub last_seen_head_sha: Option<String>,
    pub last_reviewed_head_sha: Option<String>,
    pub last_review_run_id: Option<String>,
    pub last_review_outcome: Option<String>,
    pub last_submitted_review_id: Option<String>,
    pub review_artifact_id: Option<ArtifactId>,
    pub review_artifact_head_sha: Option<String>,
    pub review_artifact_version: Option<u32>,
    pub review_artifact_updated_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentWorkspaceReviewMonitor {
    pub conversation_id: ChatConversationId,
    pub project_id: ProjectId,
    pub status: AgentWorkspaceReviewMonitorStatus,
    pub review_outcome: AgentWorkspaceReviewOutcome,
    pub review_gate_status: AgentWorkspaceReviewGateStatus,
    pub current_target_scope: Option<AgentWorkspaceReviewTargetScope>,
    pub reviewed_target_scope: Option<AgentWorkspaceReviewTargetScope>,
    pub review_conversation_id: Option<ChatConversationId>,
    pub review_artifact_id: Option<ArtifactId>,
    pub review_artifact_version: Option<u32>,
    pub review_artifact_updated_at: Option<DateTime<Utc>>,
    pub review_requested_changes_artifact_id: Option<ArtifactId>,
    pub review_requested_changes_artifact_version: Option<u32>,
    pub review_requested_changes_artifact_updated_at: Option<DateTime<Utc>>,
    pub review_gate_bypassed_at: Option<DateTime<Utc>>,
    pub review_gate_bypassed_target_scope: Option<AgentWorkspaceReviewTargetScope>,
    pub review_gate_bypassed_diff_fingerprint: Option<String>,
    pub review_gate_bypassed_artifact_id: Option<ArtifactId>,
    pub review_gate_bypassed_artifact_version: Option<u32>,
    pub reviewed_head_sha: Option<String>,
    pub reviewed_diff_fingerprint: Option<String>,
    pub reviewed_plan_context_fingerprint: Option<String>,
    pub selected_source_base_ref: Option<String>,
    pub selected_source_base_sha: Option<String>,
    pub selected_source_head_ref: Option<String>,
    pub selected_source_head_sha: Option<String>,
    pub selected_source_pull_request_number: Option<i64>,
    pub workspace_base_ref: Option<String>,
    pub workspace_base_sha: Option<String>,
    pub workspace_head_ref: Option<String>,
    pub workspace_head_sha: Option<String>,
    pub current_diff_fingerprint: Option<String>,
    pub current_plan_context_fingerprint: Option<String>,
    pub previous_version_id: Option<ArtifactId>,
    pub review_requested_changes_previous_version_id: Option<ArtifactId>,
    pub review_blocking_summary: Option<String>,
    pub review_blocking_fingerprint: Option<String>,
    pub review_fixer_run_id: Option<String>,
    pub review_fixer_conversation_id: Option<ChatConversationId>,
    pub review_fixer_status: Option<String>,
    /// Backend-owned identity for the exact blocker repair reservation.
    pub review_fixer_attempt_id: Option<String>,
    /// Number of automatic or manual workspace Review fixer attempts since the last clean gate.
    pub review_fixer_cycle_count: i64,
    /// Typed disposition recorded on the reviewer's final Review artifact write.
    ///
    /// Never parsed from artifact markdown. Consumed only by degraded settlement, and only
    /// together with `review_artifact_recorded_outcome_run_id`.
    pub review_artifact_recorded_outcome: Option<AgentWorkspaceReviewArtifactOutcome>,
    /// The run that recorded `review_artifact_recorded_outcome`.
    ///
    /// Load-bearing: `apply_current_target_to_monitor` does not clear artifact identity on an
    /// unchanged target, so a re-review of the same delta would otherwise inherit the previous
    /// run's recorded outcome. Degraded settlement requires this to equal the settling run.
    /// Deliberately NOT `last_run_id`, which the artifact-write path can populate from a prior run.
    pub review_artifact_recorded_outcome_run_id: Option<String>,
    /// Blocking summary captured at the final artifact write.
    ///
    /// Required for degraded `blocking` settlement: the artifact write itself clears live blocking
    /// state, and the fixer-start path fails closed without a summary and fingerprint.
    pub review_artifact_recorded_blocking_summary: Option<String>,
    /// How the current gate value was settled. Presentation + diagnostics only.
    pub review_settlement_source: Option<AgentWorkspaceReviewSettlementSource>,
    /// Run registered by the backend as the post-settlement hunk annotator for the reviewed target.
    pub annotation_run_id: Option<String>,
    /// Snapshot of the previously settled review, captured once when a new review run starts.
    ///
    /// Must be a snapshot, not a live read of `reviewed_*`/`review_artifact_*`: the current run's
    /// artifact write overwrites those fields before it completes, so serving `previous_review`
    /// from them would make a later context fetch return the run's own review as its "previous"
    /// one. `None` until a review has settled at least once.
    pub previous_review: Option<AgentWorkspacePreviousReviewSnapshot>,
    pub last_run_id: Option<String>,
    pub last_error: Option<String>,
    pub auto_merge_guard: Option<AgentWorkspaceReviewAutoMergeGuard>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AgentWorkspaceReviewMonitor {
    pub fn new(conversation_id: ChatConversationId, project_id: ProjectId) -> Self {
        let now = Utc::now();
        Self {
            conversation_id,
            project_id,
            status: AgentWorkspaceReviewMonitorStatus::Idle,
            review_outcome: AgentWorkspaceReviewOutcome::None,
            review_gate_status: AgentWorkspaceReviewGateStatus::NotRequired,
            current_target_scope: None,
            reviewed_target_scope: None,
            review_conversation_id: None,
            review_artifact_id: None,
            review_artifact_version: None,
            review_artifact_updated_at: None,
            review_requested_changes_artifact_id: None,
            review_requested_changes_artifact_version: None,
            review_requested_changes_artifact_updated_at: None,
            review_gate_bypassed_at: None,
            review_gate_bypassed_target_scope: None,
            review_gate_bypassed_diff_fingerprint: None,
            review_gate_bypassed_artifact_id: None,
            review_gate_bypassed_artifact_version: None,
            reviewed_head_sha: None,
            reviewed_diff_fingerprint: None,
            reviewed_plan_context_fingerprint: None,
            selected_source_base_ref: None,
            selected_source_base_sha: None,
            selected_source_head_ref: None,
            selected_source_head_sha: None,
            selected_source_pull_request_number: None,
            workspace_base_ref: None,
            workspace_base_sha: None,
            workspace_head_ref: None,
            workspace_head_sha: None,
            current_diff_fingerprint: None,
            current_plan_context_fingerprint: None,
            previous_version_id: None,
            review_requested_changes_previous_version_id: None,
            review_blocking_summary: None,
            review_blocking_fingerprint: None,
            review_fixer_run_id: None,
            review_fixer_conversation_id: None,
            review_fixer_status: None,
            review_fixer_attempt_id: None,
            review_fixer_cycle_count: 0,
            review_artifact_recorded_outcome: None,
            review_artifact_recorded_outcome_run_id: None,
            review_artifact_recorded_blocking_summary: None,
            review_settlement_source: None,
            annotation_run_id: None,
            previous_review: None,
            last_run_id: None,
            last_error: None,
            auto_merge_guard: None,
            created_at: now,
            updated_at: now,
        }
    }
}

impl AgentWorkspaceReviewMonitor {
    pub fn has_review_artifact_pair(&self) -> bool {
        self.review_artifact_id.is_some()
            && self.review_artifact_version.is_some()
            && self.review_requested_changes_artifact_id.is_some()
            && self.review_requested_changes_artifact_version.is_some()
    }

    pub fn is_current_for_target(
        &self,
        target_scope: AgentWorkspaceReviewTargetScope,
        head_sha: Option<&str>,
        diff_fingerprint: &str,
    ) -> bool {
        if self.reviewed_target_scope != Some(target_scope)
            || self.reviewed_diff_fingerprint.as_deref() != Some(diff_fingerprint)
            || self.reviewed_plan_context_fingerprint != self.current_plan_context_fingerprint
        {
            return false;
        }

        match target_scope {
            AgentWorkspaceReviewTargetScope::WorkspaceDelta => true,
            AgentWorkspaceReviewTargetScope::SelectedSource => {
                self.reviewed_head_sha.as_deref() == head_sha
            }
        }
    }

    pub fn has_current_passing_review_for_target(
        &self,
        target_scope: AgentWorkspaceReviewTargetScope,
        head_sha: Option<&str>,
        diff_fingerprint: &str,
    ) -> bool {
        self.review_outcome == AgentWorkspaceReviewOutcome::Passed
            && self.is_current_for_target(target_scope, head_sha, diff_fingerprint)
            && self.has_review_artifact_pair()
    }

    pub fn has_current_review_bypass_for_target(
        &self,
        target_scope: AgentWorkspaceReviewTargetScope,
        head_sha: Option<&str>,
        diff_fingerprint: &str,
    ) -> bool {
        self.status == AgentWorkspaceReviewMonitorStatus::Ready
            && self.review_outcome == AgentWorkspaceReviewOutcome::Blocking
            && self.review_gate_bypassed_at.is_some()
            && self.review_gate_bypassed_target_scope == Some(target_scope)
            && self.review_gate_bypassed_diff_fingerprint.as_deref() == Some(diff_fingerprint)
            && self.review_gate_bypassed_artifact_id == self.review_artifact_id
            && self.review_gate_bypassed_artifact_version == self.review_artifact_version
            && self.has_review_artifact_pair()
            && self.is_current_for_target(target_scope, head_sha, diff_fingerprint)
    }

    pub fn has_current_review_publish_authorization_for_target(
        &self,
        target_scope: AgentWorkspaceReviewTargetScope,
        head_sha: Option<&str>,
        diff_fingerprint: &str,
    ) -> bool {
        self.has_current_passing_review_for_target(target_scope, head_sha, diff_fingerprint)
            || self.has_current_review_bypass_for_target(target_scope, head_sha, diff_fingerprint)
    }

    pub fn clear_review_gate_bypass(&mut self) {
        self.review_gate_bypassed_at = None;
        self.review_gate_bypassed_target_scope = None;
        self.review_gate_bypassed_diff_fingerprint = None;
        self.review_gate_bypassed_artifact_id = None;
        self.review_gate_bypassed_artifact_version = None;
    }

    /// Drops every piece of durable evidence a degraded settlement or annotator write could
    /// authorize itself from.
    ///
    /// Called on target refresh and on any artifact write that carries no typed outcome, so
    /// stale evidence can never authorize a later run. The three recorded fields and
    /// `annotation_run_id` must always clear together.
    pub fn clear_recorded_review_evidence(&mut self) {
        self.review_artifact_recorded_outcome = None;
        self.review_artifact_recorded_outcome_run_id = None;
        self.review_artifact_recorded_blocking_summary = None;
        self.annotation_run_id = None;
    }

    /// Freezes the currently settled review so the next run can triage against it.
    ///
    /// Call exactly once, at review start, before the run touches `reviewed_*`. Returns `false`
    /// and changes nothing when there is no settled prior review to capture.
    pub fn capture_previous_review_snapshot(&mut self) -> bool {
        let Some(overview_artifact_id) = self.review_artifact_id.clone() else {
            return false;
        };
        self.previous_review = Some(AgentWorkspacePreviousReviewSnapshot {
            overview_artifact_id,
            requested_changes_artifact_id: self.review_requested_changes_artifact_id.clone(),
            artifact_version: self.review_artifact_version,
            reviewed_diff_fingerprint: self.reviewed_diff_fingerprint.clone(),
            reviewed_head_sha: self.reviewed_head_sha.clone(),
            outcome: self.review_outcome,
        });
        true
    }

    /// True when `run_id` is the exact run that recorded a typed outcome on the current artifact.
    ///
    /// Fails closed on a missing outcome, a missing run id, or any mismatch.
    pub fn has_recorded_outcome_for_run(&self, run_id: &str) -> bool {
        self.review_artifact_recorded_outcome.is_some()
            && self.review_artifact_recorded_outcome_run_id.as_deref() == Some(run_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentWorkspaceReviewApprovalSnapshot {
    pub target_scope: AgentWorkspaceReviewTargetScope,
    pub diff_fingerprint: String,
    pub artifact_id: ArtifactId,
    pub artifact_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentWorkspaceReviewFixerSnapshot {
    pub target_scope: AgentWorkspaceReviewTargetScope,
    pub diff_fingerprint: String,
    pub artifact_id: ArtifactId,
    pub artifact_version: u32,
    pub requested_changes_artifact_id: ArtifactId,
    pub requested_changes_artifact_version: u32,
    pub blocking_fingerprint: String,
    pub plan_context_fingerprint: Option<String>,
}

impl AgentWorkspaceReviewFixerSnapshot {
    pub fn from_monitor(monitor: &AgentWorkspaceReviewMonitor) -> Option<Self> {
        let target_scope = monitor.current_target_scope?;
        if monitor.reviewed_target_scope != Some(target_scope) {
            return None;
        }
        let diff_fingerprint = monitor
            .current_diff_fingerprint
            .clone()
            .filter(|value| !value.trim().is_empty())?;
        if monitor.reviewed_diff_fingerprint.as_deref() != Some(diff_fingerprint.as_str()) {
            return None;
        }
        if monitor.reviewed_plan_context_fingerprint != monitor.current_plan_context_fingerprint {
            return None;
        }
        let artifact_id = monitor
            .review_artifact_id
            .clone()
            .filter(|value| !value.as_str().trim().is_empty())?;
        let artifact_version = monitor
            .review_artifact_version
            .filter(|version| *version > 0)?;
        let requested_changes_artifact_id = monitor
            .review_requested_changes_artifact_id
            .clone()
            .filter(|value| !value.as_str().trim().is_empty())?;
        let requested_changes_artifact_version = monitor
            .review_requested_changes_artifact_version
            .filter(|version| *version > 0)?;
        let blocking_fingerprint = monitor
            .review_blocking_fingerprint
            .clone()
            .filter(|value| !value.trim().is_empty())?;
        Some(Self {
            target_scope,
            diff_fingerprint,
            artifact_id,
            artifact_version,
            requested_changes_artifact_id,
            requested_changes_artifact_version,
            blocking_fingerprint,
            plan_context_fingerprint: monitor.current_plan_context_fingerprint.clone(),
        })
    }
}

impl AgentWorkspaceReviewApprovalSnapshot {
    pub fn audit_event(
        &self,
        conversation_id: ChatConversationId,
        approved_at: DateTime<Utc>,
    ) -> AgentConversationWorkspacePublicationEvent {
        let mut event = AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "workspace_review_approved_anyway",
            "succeeded",
            format!(
                "Human approved blocking Workspace Review artifact {} v{} for {} at diff {}.",
                self.artifact_id.as_str(),
                self.artifact_version,
                self.target_scope,
                self.diff_fingerprint
            ),
            Some("workspace_review_approved_anyway".to_string()),
        );
        event.created_at = approved_at;
        event
    }
}

impl AgentWorkspacePrReviewMonitor {
    pub fn new(
        conversation_id: ChatConversationId,
        project_id: ProjectId,
        pr_number: i64,
        head_sha: Option<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            conversation_id,
            project_id,
            pr_number,
            status: AgentWorkspacePrReviewMonitorStatus::Idle,
            monitor_enabled: false,
            auto_approve_enabled: true,
            first_review_completed: false,
            first_action_resolved: false,
            last_seen_head_sha: head_sha,
            last_reviewed_head_sha: None,
            last_review_run_id: None,
            last_review_outcome: None,
            last_submitted_review_id: None,
            review_artifact_id: None,
            review_artifact_head_sha: None,
            review_artifact_version: None,
            review_artifact_updated_at: None,
            last_error: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn can_auto_approve(&self, action: &AgentWorkspacePrReviewAction) -> bool {
        self.auto_approve_enabled
            && self.first_action_resolved
            && action.proposed_action == AgentWorkspacePrReviewActionKind::Approve
            && action.status == AgentWorkspacePrReviewActionStatus::Pending
            && action.created_by_run_id.is_some()
            && self.last_review_run_id == action.created_by_run_id
            && self.review_artifact_id.is_some()
            && self.review_artifact_head_sha.as_deref() == Some(action.head_sha.as_str())
    }

    pub fn settlement_status(&self) -> AgentWorkspacePrReviewMonitorStatus {
        if self.status == AgentWorkspacePrReviewMonitorStatus::Terminal {
            return AgentWorkspacePrReviewMonitorStatus::Terminal;
        }
        if self.monitor_enabled {
            if self.last_error.is_some() {
                AgentWorkspacePrReviewMonitorStatus::Blocked
            } else {
                AgentWorkspacePrReviewMonitorStatus::Watching
            }
        } else {
            AgentWorkspacePrReviewMonitorStatus::Paused
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentWorkspacePrReviewAction {
    pub id: String,
    pub conversation_id: ChatConversationId,
    pub pr_number: i64,
    pub head_sha: String,
    pub proposed_action: AgentWorkspacePrReviewActionKind,
    pub summary: String,
    pub review_body: String,
    pub findings_json: Option<String>,
    pub status: AgentWorkspacePrReviewActionStatus,
    pub submitted_review_id: Option<String>,
    pub created_by_run_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

impl AgentWorkspacePrReviewAction {
    pub fn new(
        conversation_id: ChatConversationId,
        pr_number: i64,
        head_sha: String,
        proposed_action: AgentWorkspacePrReviewActionKind,
        summary: String,
        review_body: String,
        findings_json: Option<String>,
        created_by_run_id: Option<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            conversation_id,
            pr_number,
            head_sha,
            proposed_action,
            summary,
            review_body,
            findings_json,
            status: AgentWorkspacePrReviewActionStatus::Pending,
            submitted_review_id: None,
            created_by_run_id,
            created_at: now,
            updated_at: now,
            resolved_at: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConversationWorkspace {
    pub conversation_id: ChatConversationId,
    pub project_id: ProjectId,
    pub mode: AgentConversationWorkspaceMode,
    pub branch_mode: AgentConversationWorkspaceBranchMode,
    pub base_ref_kind: IdeationAnalysisBaseRefKind,
    pub base_ref: String,
    pub base_display_name: Option<String>,
    pub base_commit: Option<String>,
    pub branch_name: String,
    pub worktree_path: String,
    pub linked_ideation_session_id: Option<IdeationSessionId>,
    pub task_pipeline_session_id: Option<IdeationSessionId>,
    pub linked_plan_branch_id: Option<PlanBranchId>,
    pub source_pull_request: Option<AgentWorkspaceSourcePullRequest>,
    pub publication_pr_number: Option<i64>,
    pub publication_pr_url: Option<String>,
    pub publication_pr_status: Option<String>,
    pub publication_push_status: Option<String>,
    /// Durable publication authority. Legacy rows have no owner and use the timestamp fallback.
    pub publish_lease_owner_run_id: Option<String>,
    pub publish_lease_token: Option<String>,
    pub publish_lease_heartbeat_at: Option<DateTime<Utc>>,
    pub publication_metadata_phase: Option<AgentWorkspacePublicationMetadataPhase>,
    pub publication_metadata_state: Option<AgentWorkspacePublicationMetadataState>,
    pub publication_metadata_attempt_id: Option<String>,
    pub auto_publish_enabled: bool,
    pub auto_publish_initial_pr_enabled: bool,
    pub auto_publish_paused_pr_autofix_enabled: Option<bool>,
    pub auto_publish_paused_pr_auto_merge_desired: Option<bool>,
    pub pr_autofix_enabled: bool,
    /// Per-workspace automation choice: None inherits global Review settings.
    pub review_automation_override: Option<bool>,
    pub pr_auto_merge_desired: bool,
    pub pr_auto_merge_method: String,
    pub pr_auto_merge_current: Option<bool>,
    pub pr_supervision_status: Option<String>,
    pub pr_supervision_summary: Option<String>,
    pub pr_supervision_updated_at: Option<DateTime<Utc>>,
    /// The failure identity the most recent PR autofix streak exhausted itself against. Repair
    /// attempts are per-streak, so without this the next streak has no memory of what already
    /// failed and re-spends agents on identical evidence.
    pub last_blocked_pr_health_fingerprint: Option<String>,
    pub last_blocked_pr_health_at: Option<DateTime<Utc>>,
    /// When the unattended base-freshness scan last observed this unpublished workspace's base
    /// as ahead of its effective checkout ref. Cleared when the base becomes current again;
    /// untouched while base resolution is blocked.
    pub stale_base_detected_at: Option<DateTime<Utc>>,
    pub status: AgentConversationWorkspaceStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AgentConversationWorkspace {
    pub fn new(
        conversation_id: ChatConversationId,
        project_id: ProjectId,
        mode: AgentConversationWorkspaceMode,
        base_ref_kind: IdeationAnalysisBaseRefKind,
        base_ref: String,
        base_display_name: Option<String>,
        base_commit: Option<String>,
        branch_name: String,
        worktree_path: String,
    ) -> Self {
        let now = Utc::now();
        Self {
            conversation_id,
            project_id,
            mode,
            branch_mode: AgentConversationWorkspaceBranchMode::Isolated,
            base_ref_kind,
            base_ref,
            base_display_name,
            base_commit,
            branch_name,
            worktree_path,
            linked_ideation_session_id: None,
            task_pipeline_session_id: None,
            linked_plan_branch_id: None,
            source_pull_request: None,
            publication_pr_number: None,
            publication_pr_url: None,
            publication_pr_status: None,
            publication_push_status: None,
            publish_lease_owner_run_id: None,
            publish_lease_token: None,
            publish_lease_heartbeat_at: None,
            publication_metadata_phase: None,
            publication_metadata_state: None,
            publication_metadata_attempt_id: None,
            auto_publish_enabled: true,
            auto_publish_initial_pr_enabled: false,
            auto_publish_paused_pr_autofix_enabled: None,
            auto_publish_paused_pr_auto_merge_desired: None,
            pr_autofix_enabled: false,
            review_automation_override: None,
            pr_auto_merge_desired: false,
            pr_auto_merge_method: DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD.to_string(),
            pr_auto_merge_current: None,
            pr_supervision_status: None,
            pr_supervision_summary: None,
            pr_supervision_updated_at: None,
            last_blocked_pr_health_fingerprint: None,
            last_blocked_pr_health_at: None,
            stale_base_detected_at: None,
            status: AgentConversationWorkspaceStatus::Active,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn is_execution_owned(&self) -> bool {
        self.linked_plan_branch_id.is_some()
    }

    /// Whether this active workspace owns a publication PR mutation surface.
    ///
    /// Keep this positive and shape-aware: direct Edit workspaces and linked
    /// Ideation workspaces are the only established owned-PR mutation modes.
    pub fn allows_owned_pr_mutation(&self) -> bool {
        if self.status != AgentConversationWorkspaceStatus::Active {
            return false;
        }

        match self.mode {
            AgentConversationWorkspaceMode::Edit => self.linked_plan_branch_id.is_none(),
            AgentConversationWorkspaceMode::Ideation => {
                self.linked_plan_branch_id.is_some() && self.linked_ideation_session_id.is_some()
            }
            _ => false,
        }
    }

    pub fn has_terminal_publication_pr_status(&self) -> bool {
        is_terminal_publication_pr_status(self.publication_pr_status.as_deref())
    }

    pub fn has_pr_status_pollable_push_status(&self) -> bool {
        is_pr_status_pollable_push_status(self.publication_push_status.as_deref())
    }

    /// Whether this workspace currently has an open (non-terminal) publication PR.
    pub fn has_open_pr(&self) -> bool {
        is_open_pr(
            self.publication_pr_number,
            self.publication_pr_status.as_deref(),
        )
    }
}

pub fn is_terminal_publication_pr_status(status: Option<&str>) -> bool {
    matches!(status, Some("merged" | "closed"))
}

/// A workspace has an OPEN pull request when a PR number exists and its status is
/// not terminal (merged/closed). Draft PRs count as open. Single source of truth
/// for the "has an open PR" rule across ticketing, sidebar, and chat header.
pub fn is_open_pr(publication_pr_number: Option<i64>, publication_pr_status: Option<&str>) -> bool {
    publication_pr_number.is_some() && !is_terminal_publication_pr_status(publication_pr_status)
}

pub fn is_pr_status_pollable_push_status(status: Option<&str>) -> bool {
    matches!(status, None | Some("pushed" | "refreshed"))
}

/// Whether a workspace publication workflow is actively mutating its branch.
pub fn is_publication_push_active(status: Option<&str>) -> bool {
    matches!(
        status,
        Some("checking" | "committing" | "refreshing" | "describing" | "pushing")
    )
}

pub const DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD: &str = "squash";

/// Durable stage for an existing-PR metadata publication receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentWorkspacePublicationMetadataPhase {
    Prepared,
    Mutating,
    Reconciling,
    Settled,
}

impl std::fmt::Display for AgentWorkspacePublicationMetadataPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Prepared => write!(f, "prepared"),
            Self::Mutating => write!(f, "mutating"),
            Self::Reconciling => write!(f, "reconciling"),
            Self::Settled => write!(f, "settled"),
        }
    }
}

impl FromStr for AgentWorkspacePublicationMetadataPhase {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "prepared" => Ok(Self::Prepared),
            "mutating" => Ok(Self::Mutating),
            "reconciling" => Ok(Self::Reconciling),
            "settled" => Ok(Self::Settled),
            _ => Err(format!(
                "unknown workspace publication metadata phase: '{value}'"
            )),
        }
    }
}

/// Durable outcome for an existing-PR metadata publication receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentWorkspacePublicationMetadataState {
    NotAttempted,
    Applied,
    NotApplied,
    Unknown,
    Reconciled,
    Conflicted,
}

impl std::fmt::Display for AgentWorkspacePublicationMetadataState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAttempted => write!(f, "not_attempted"),
            Self::Applied => write!(f, "applied"),
            Self::NotApplied => write!(f, "not_applied"),
            Self::Unknown => write!(f, "unknown"),
            Self::Reconciled => write!(f, "reconciled"),
            Self::Conflicted => write!(f, "conflicted"),
        }
    }
}

impl FromStr for AgentWorkspacePublicationMetadataState {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "not_attempted" => Ok(Self::NotAttempted),
            "applied" => Ok(Self::Applied),
            "not_applied" => Ok(Self::NotApplied),
            "unknown" => Ok(Self::Unknown),
            "reconciled" => Ok(Self::Reconciled),
            "conflicted" => Ok(Self::Conflicted),
            _ => Err(format!(
                "unknown workspace publication metadata state: '{value}'"
            )),
        }
    }
}

/// Durable, non-secret authority used to recover an existing-PR metadata mutation.
///
/// The selected title/body decision is persisted separately because its existing columns are
/// intentionally shared with the publication workflow; this receipt keeps fingerprints only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentWorkspacePublicationMetadataReceipt {
    pub attempt_id: String,
    pub phase: AgentWorkspacePublicationMetadataPhase,
    pub state: AgentWorkspacePublicationMetadataState,
    pub target_pr_number: i64,
    pub before_authority_sha256: String,
    pub before_title_sha256: String,
    pub before_editable_body_sha256: String,
    pub before_managed_suffix_sha256: Option<String>,
    pub intended_title_sha256: Option<String>,
    pub intended_editable_body_sha256: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConversationWorkspacePublicationEvent {
    pub id: String,
    pub conversation_id: ChatConversationId,
    pub step: String,
    pub status: String,
    pub summary: String,
    pub classification: Option<String>,
    /// Backend-owned receipt identity. `None` keeps pre-receipt events readable.
    pub attempt_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentWorkspacePrDescription {
    pub title: Option<String>,
    pub body_markdown: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentWorkspacePrCommentEvidence {
    pub conversation_id: ChatConversationId,
    pub pr_number: i64,
    pub comment_id: String,
    pub author: Option<String>,
    pub body: String,
    pub body_excerpt: String,
    pub body_sha256: String,
    pub url: Option<String>,
    pub github_created_at: Option<String>,
    pub github_updated_at: Option<String>,
    pub is_codecov: bool,
    pub is_bot: bool,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub last_included_at: Option<DateTime<Utc>>,
    pub last_read_at: Option<DateTime<Utc>>,
    pub edit_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentWorkspacePrCommentEvidenceUpsert {
    pub pr_number: i64,
    pub comment_id: String,
    pub author: Option<String>,
    pub body: String,
    pub body_excerpt: String,
    pub body_sha256: String,
    pub url: Option<String>,
    pub github_created_at: Option<String>,
    pub github_updated_at: Option<String>,
    pub is_codecov: bool,
    pub is_bot: bool,
}

impl AgentWorkspacePrCommentEvidenceUpsert {
    pub fn new(
        pr_number: i64,
        comment_id: String,
        author: Option<String>,
        body: String,
        url: Option<String>,
        github_created_at: Option<String>,
        github_updated_at: Option<String>,
        is_codecov: bool,
        is_bot: bool,
    ) -> Self {
        let body_excerpt = pr_comment_body_excerpt(&body, 480);
        let body_sha256 = pr_comment_body_sha256(&body);
        Self {
            pr_number,
            comment_id,
            author,
            body,
            body_excerpt,
            body_sha256,
            url,
            github_created_at,
            github_updated_at,
            is_codecov,
            is_bot,
        }
    }
}

pub fn pr_comment_body_sha256(body: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(body.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn pr_comment_body_excerpt(body: &str, max_chars: usize) -> String {
    let compact = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= max_chars {
        return compact;
    }
    if max_chars == 0 {
        return String::new();
    }
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }
    let truncated: String = compact.chars().take(max_chars - 3).collect();
    format!("{truncated}...")
}

impl AgentWorkspacePrDescription {
    pub fn new(title: Option<String>, body_markdown: String) -> Self {
        Self {
            title: title.and_then(|value| {
                let trimmed = value.trim();
                (!trimmed.is_empty()).then(|| trimmed.to_string())
            }),
            body_markdown,
        }
    }
}

impl AgentConversationWorkspacePublicationEvent {
    pub fn new(
        conversation_id: ChatConversationId,
        step: impl Into<String>,
        status: impl Into<String>,
        summary: impl Into<String>,
        classification: Option<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            conversation_id,
            step: step.into(),
            status: status.into(),
            summary: summary.into(),
            classification,
            attempt_id: None,
            created_at: Utc::now(),
        }
    }
}
