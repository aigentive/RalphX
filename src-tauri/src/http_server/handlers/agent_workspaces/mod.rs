//! Agent workspace HTTP handlers.

mod workspace_review_diff;
pub use workspace_review_diff::*;
mod pr_review;
mod workspace_review_context;

#[cfg(test)]
use pr_review::ensure_review_artifact_for_head;
pub use pr_review::*;
pub use workspace_review_context::get_agent_workspace_review_context;

use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    path::PathBuf,
    pin::Pin,
    str::FromStr,
    sync::Arc,
    time::Instant,
};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};

use super::*;
use crate::application::agent_conversation_workspace::AgentConversationWorkspaceBaseSelection;
use crate::application::agent_workspace_local_commit::{
    commit_agent_workspace_locally, AgentWorkspaceLocalCommitRequest,
};
use crate::application::agent_workspace_pr_autofix_attempt::{
    load_pr_autofix_completion_authority, PrAutofixCompletionAuthority,
};
use crate::application::agent_workspace_pr_supervision_recovery::{
    build_agent_workspace_pr_supervision_recovery_deps,
    schedule_agent_workspace_pr_supervision_recovery, AgentWorkspacePrSupervisionRecoveryTrigger,
};
use crate::application::agent_workspace_publish_repair_state::{
    abort_agent_workspace_pr_fix_review_handoff, block_agent_workspace_pr_fix_claim,
    complete_agent_workspace_pr_fix_claim, complete_agent_workspace_repair_claim,
    continue_agent_workspace_pr_fix_after_review_handoff,
    current_agent_workspace_repair_claim_for_completion,
    settle_agent_workspace_failure_without_repair, AgentWorkspaceRepairClaim,
};
use crate::application::agent_workspace_review::{
    apply_review_artifact_to_monitor, complete_agent_workspace_review_run_unlocked,
    load_agent_workspace_review_context, load_current_workspace_review_eligible,
    lock_workspace_review_lifecycle, review_gate_publish_blocker,
    start_agent_workspace_review_blocking_fixer_with_override, workspace_review_mode_is_eligible,
    AgentWorkspaceReviewGoalContext, AgentWorkspaceReviewHunkAnchor, AgentWorkspaceReviewStart,
    AgentWorkspaceReviewTarget, WorkspaceReviewFixerConfirmation,
};
use crate::application::agent_workspace_review_auto_merge::{
    preview_manual_workspace_review_start, restore_guarded_auto_merge_after_publish,
    start_guarded_agent_workspace_review,
    start_guarded_agent_workspace_review_with_runtime_override, WorkspaceReviewStartConfirmation,
    WorkspaceReviewStartOrigin,
};
use crate::application::agent_workspace_review_diff::{
    ensure_workspace_review_snapshot_current, full_hunk_anchors_for_requests,
};
use crate::application::agent_workspace_review_publish_handoff::{
    resume_pr_fix_publish_after_passed_workspace_review, workspace_review_authorization_kind,
};
use crate::application::interactive_notification_producer::pr_review_notification_key;
use crate::application::publish_resilience::{
    inspect_publish_branch_freshness_for_source, push_agent_workspace_publish_branch,
    verify_agent_workspace_repair_completion, verify_agent_workspace_settled_current_head,
    AgentWorkspaceRepairCompletionCheck, AgentWorkspaceSettledHeadCheck,
};
use crate::application::services::pr_merge_poller::import_agent_workspace_pr_comment_evidence;
use crate::application::{AppState, ChatService, GitService};
use crate::commands::unified_chat_commands::{
    agent_workspace_post_repair_action_from_events, agent_workspace_response_for_state,
    get_agent_conversation_workspace_freshness_for_app_state,
    publish_agent_conversation_workspace_for_app_state, resolve_agent_workspace_publish_target,
    update_agent_conversation_workspace_from_base_for_app_state_with_caller,
    AgentConversationWorkspaceFreshnessResponse,
    AgentConversationWorkspacePublicationEventResponse, AgentConversationWorkspaceResponse,
    AgentWorkspacePostRepairAction, AgentWorkspacePrFixReviewPublishCommandResumer,
    AGENT_WORKSPACE_PUBLISH_IN_PROGRESS_MESSAGE,
};
use crate::domain::agents::{
    AgentHarnessKind, LogicalEffort, ManualRoleRuntimeOverride, ManualServiceTier,
};
use crate::domain::entities::plan_branch::{PrPushStatus, PrStatus as PlanDbPrStatus};
use crate::domain::entities::{
    is_publication_push_active, pr_comment_body_excerpt, AgentConversationWorkspace,
    AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentWorkspacePrCommentEvidence,
    AgentWorkspacePrMetadataDecision, AgentWorkspacePrReviewAction, AgentWorkspacePrReviewActionKind,
    AgentWorkspacePrReviewActionStatus, AgentWorkspacePrReviewMonitor,
    AgentWorkspacePrReviewMonitorStatus, AgentWorkspaceReviewGateStatus,
    AgentWorkspaceReviewHunkAnnotation, AgentWorkspaceReviewMonitor,
    AgentWorkspaceReviewTargetScope, Artifact, ArtifactId, ArtifactType, ChatConversationId,
    IdeationAnalysisBaseRefKind, NewNotification, NotificationCategory, NotificationSeverity,
    NotificationTarget, NotificationTargetKind, PlanBranch, ProjectId,
};
use crate::domain::repositories::{
    AgentWorkspacePrReviewActionMutation, AgentWorkspaceRepairStateGuard,
};
use crate::domain::services::github_service::{
    GithubServiceTrait, PrHealth, PrReviewFeedback, PrReviewSubmissionEvent, PrStatus,
};
use crate::error::AppError;

#[derive(Debug, serde::Deserialize)]
pub struct CompleteAgentWorkspaceRepairRequest {
    pub repair_commit_sha: String,
    pub resolved_base_ref: String,
    pub resolved_base_commit: String,
    pub summary: String,
}

#[derive(Debug, serde::Serialize)]
pub struct CompleteAgentWorkspaceRepairResponse {
    pub success: bool,
    pub message: String,
    pub new_status: String,
    pub base_commit: String,
    pub repair_commit_sha: String,
    pub auto_publish_status: Option<String>,
    pub auto_publish_error: Option<String>,
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct SubmitAgentWorkspacePrDescriptionRequest {
    pub decision: String,
    pub title: Option<String>,
    pub body_markdown: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct SubmitAgentWorkspacePrDescriptionResponse {
    pub success: bool,
}

#[derive(Debug, serde::Deserialize)]
pub struct UpdateAgentWorkspaceFromBaseRequest {
    pub base_ref_kind: Option<String>,
    pub base_ref: Option<String>,
    pub base_display_name: Option<String>,
    /// Transport-owned runtime identity; intentionally absent from the model-facing tool schema.
    pub created_by_run_id: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommitAgentWorkspaceLocallyRequest {
    pub expected_head_sha: String,
    pub review_artifact_id: Option<String>,
    pub review_artifact_version: Option<u32>,
    pub reviewed_head_sha: Option<String>,
    pub reviewed_diff_fingerprint: Option<String>,
    pub attempt_token: String,
}

#[derive(Debug, serde::Serialize)]
pub struct CommitAgentWorkspaceLocallyResponse {
    pub success: bool,
    pub workspace: AgentConversationWorkspaceResponse,
    pub outcome: String,
    pub branch_name: String,
    pub previous_head_sha: String,
    pub commit_sha: String,
    pub had_changes: bool,
    pub attempt_token: String,
}

#[derive(Debug, serde::Serialize)]
pub struct AgentWorkspacePublishStatusResponse {
    pub success: bool,
    pub workspace: AgentConversationWorkspaceResponse,
    pub events: Vec<AgentConversationWorkspacePublicationEventResponse>,
    pub publish_in_progress: bool,
    pub needs_agent_repair: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct AgentWorkspacePublishReadinessResponse {
    pub success: bool,
    pub workspace: AgentConversationWorkspaceResponse,
    pub freshness: AgentConversationWorkspaceFreshnessResponse,
    pub review_gate_status: Option<String>,
    pub can_publish: bool,
    pub blockers: Vec<String>,
    pub needs_base_update: bool,
    pub recommended_actions: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct AgentWorkspacePublishActionResponse {
    pub success: bool,
    pub status: String,
    pub message: String,
    pub repair_queued: bool,
    pub workspace: Option<AgentConversationWorkspaceResponse>,
    pub freshness: Option<AgentConversationWorkspaceFreshnessResponse>,
    pub updated: Option<bool>,
    pub target_ref: Option<String>,
    pub base_commit: Option<String>,
    pub commit_sha: Option<String>,
    pub pushed: Option<bool>,
    pub created_pr: Option<bool>,
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct AgentWorkspacePrFixContextResponse {
    pub success: bool,
    pub workspace: AgentConversationWorkspaceResponse,
    pub events: Vec<AgentConversationWorkspacePublicationEventResponse>,
    pub target_kind: Option<String>,
    pub target_branch: Option<String>,
    pub target_base_branch: Option<String>,
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
    pub health: Option<PrHealth>,
    pub review_feedback: Option<PrReviewFeedback>,
    pub issue_comment_evidence: Vec<AgentWorkspacePrCommentEvidenceResponse>,
}

#[derive(Debug, serde::Serialize)]
pub struct AgentWorkspacePrCommentEvidenceResponse {
    pub comment_id: String,
    pub author: Option<String>,
    pub url: Option<String>,
    pub github_created_at: Option<String>,
    pub github_updated_at: Option<String>,
    pub is_codecov: bool,
    pub is_bot: bool,
    pub body_excerpt: String,
    pub body_length_chars: usize,
    pub body_sha256: String,
    pub edit_count: i64,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub last_included_at: Option<String>,
    pub last_read_at: Option<String>,
    pub has_more: bool,
    pub full_body_available: bool,
    pub is_untrusted: bool,
    pub read_tool: String,
}

impl AgentWorkspacePrCommentEvidenceResponse {
    fn from_evidence(value: AgentWorkspacePrCommentEvidence) -> Self {
        let compact_body = value.body.split_whitespace().collect::<Vec<_>>().join(" ");
        let has_more = compact_body != value.body_excerpt;
        let body_length_chars = value.body.chars().count();
        Self {
            read_tool: "read_agent_workspace_pr_comment".to_string(),
            comment_id: value.comment_id,
            author: value.author,
            url: value.url,
            github_created_at: value.github_created_at,
            github_updated_at: value.github_updated_at,
            is_codecov: value.is_codecov,
            is_bot: value.is_bot,
            body_excerpt: value.body_excerpt,
            body_length_chars,
            body_sha256: value.body_sha256,
            edit_count: value.edit_count,
            first_seen_at: value.first_seen_at.to_rfc3339(),
            last_seen_at: value.last_seen_at.to_rfc3339(),
            last_included_at: value.last_included_at.map(|value| value.to_rfc3339()),
            last_read_at: value.last_read_at.map(|value| value.to_rfc3339()),
            has_more,
            full_body_available: true,
            is_untrusted: true,
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct ReadAgentWorkspacePrCommentResponse {
    pub success: bool,
    pub conversation_id: String,
    pub pr_number: i64,
    pub comment_id: String,
    pub author: Option<String>,
    pub url: Option<String>,
    pub github_created_at: Option<String>,
    pub github_updated_at: Option<String>,
    pub is_codecov: bool,
    pub is_bot: bool,
    pub body: String,
    pub body_length_chars: usize,
    pub body_sha256: String,
    pub edit_count: i64,
    pub is_untrusted: bool,
}

#[derive(Debug, serde::Deserialize)]
pub struct CompleteAgentWorkspacePrFixRequest {
    pub summary: String,
    pub blocker: Option<String>,
    pub fix_commit_sha: Option<String>,
    /// Transport-owned runtime identity; intentionally absent from the model-facing tool schema.
    pub created_by_run_id: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct CompleteAgentWorkspacePrFixResponse {
    pub success: bool,
    pub status: String,
    pub message: String,
    pub workspace: Option<AgentConversationWorkspaceResponse>,
    pub publish_status: Option<String>,
    pub publish_error: Option<String>,
    pub commit_sha: Option<String>,
    pub pushed: Option<bool>,
    pub created_pr: Option<bool>,
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentWorkspacePrFixTargetKind {
    DirectWorkspace,
    IdeationPlan,
}

#[derive(Debug, Clone)]
struct AgentWorkspacePrFixTarget {
    kind: AgentWorkspacePrFixTargetKind,
    pr_number: i64,
    pr_url: Option<String>,
    working_dir: PathBuf,
    branch_name: String,
    base_branch: String,
    plan_branch: Option<PlanBranch>,
}

impl AgentWorkspacePrFixTarget {
    fn kind_name(&self) -> &'static str {
        match self.kind {
            AgentWorkspacePrFixTargetKind::DirectWorkspace => "direct_workspace_pr",
            AgentWorkspacePrFixTargetKind::IdeationPlan => "ideation_plan_pr",
        }
    }

    fn is_ideation_plan(&self) -> bool {
        self.kind == AgentWorkspacePrFixTargetKind::IdeationPlan
    }
}

#[derive(Debug, serde::Serialize)]
pub struct AgentWorkspacePrReviewMonitorResponse {
    pub conversation_id: String,
    pub project_id: String,
    pub pr_number: i64,
    pub status: String,
    pub monitor_enabled: bool,
    pub auto_approve_enabled: bool,
    pub first_review_completed: bool,
    pub first_action_resolved: bool,
    pub last_seen_head_sha: Option<String>,
    pub last_reviewed_head_sha: Option<String>,
    pub last_review_run_id: Option<String>,
    pub last_review_outcome: Option<String>,
    pub last_submitted_review_id: Option<String>,
    pub review_artifact_id: Option<String>,
    pub review_artifact_head_sha: Option<String>,
    pub review_artifact_version: Option<u32>,
    pub review_artifact_updated_at: Option<String>,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<AgentWorkspacePrReviewMonitor> for AgentWorkspacePrReviewMonitorResponse {
    fn from(value: AgentWorkspacePrReviewMonitor) -> Self {
        Self {
            conversation_id: value.conversation_id.as_str(),
            project_id: value.project_id.as_str().to_string(),
            pr_number: value.pr_number,
            status: value.status.to_string(),
            monitor_enabled: value.monitor_enabled,
            auto_approve_enabled: value.auto_approve_enabled,
            first_review_completed: value.first_review_completed,
            first_action_resolved: value.first_action_resolved,
            last_seen_head_sha: value.last_seen_head_sha,
            last_reviewed_head_sha: value.last_reviewed_head_sha,
            last_review_run_id: value.last_review_run_id,
            last_review_outcome: value.last_review_outcome,
            last_submitted_review_id: value.last_submitted_review_id,
            review_artifact_id: value
                .review_artifact_id
                .map(|artifact_id| artifact_id.as_str().to_string()),
            review_artifact_head_sha: value.review_artifact_head_sha,
            review_artifact_version: value.review_artifact_version,
            review_artifact_updated_at: value
                .review_artifact_updated_at
                .map(|value| value.to_rfc3339()),
            last_error: value.last_error,
            created_at: value.created_at.to_rfc3339(),
            updated_at: value.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct AgentWorkspacePrReviewActionResponse {
    pub id: String,
    pub conversation_id: String,
    pub pr_number: i64,
    pub head_sha: String,
    pub proposed_action: String,
    pub summary: String,
    pub review_body: String,
    pub findings_json: Option<String>,
    pub status: String,
    pub submitted_review_id: Option<String>,
    pub created_by_run_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub resolved_at: Option<String>,
}

impl From<AgentWorkspacePrReviewAction> for AgentWorkspacePrReviewActionResponse {
    fn from(value: AgentWorkspacePrReviewAction) -> Self {
        Self {
            id: value.id,
            conversation_id: value.conversation_id.as_str(),
            pr_number: value.pr_number,
            head_sha: value.head_sha,
            proposed_action: value.proposed_action.to_string(),
            summary: value.summary,
            review_body: value.review_body,
            findings_json: value.findings_json,
            status: value.status.to_string(),
            submitted_review_id: value.submitted_review_id,
            created_by_run_id: value.created_by_run_id,
            created_at: value.created_at.to_rfc3339(),
            updated_at: value.updated_at.to_rfc3339(),
            resolved_at: value.resolved_at.map(|value| value.to_rfc3339()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentWorkspacePrReviewActionHeadStatus {
    Current,
    Stale,
    Unverified,
}

#[derive(Debug, serde::Serialize)]
pub struct AgentWorkspacePrReviewContextResponse {
    pub success: bool,
    pub workspace: AgentConversationWorkspaceResponse,
    pub events: Vec<AgentConversationWorkspacePublicationEventResponse>,
    pub pr_number: i64,
    pub pr_url: Option<String>,
    pub current_head_sha: Option<String>,
    pub health: Option<PrHealth>,
    pub review_feedback: Option<PrReviewFeedback>,
    pub monitor: Option<AgentWorkspacePrReviewMonitorResponse>,
    pub pending_action: Option<AgentWorkspacePrReviewActionResponse>,
    pub pending_action_head_status: Option<AgentWorkspacePrReviewActionHeadStatus>,
    pub recent_actions: Vec<AgentWorkspacePrReviewActionResponse>,
    pub issue_comment_evidence: Vec<AgentWorkspacePrCommentEvidenceResponse>,
}

#[derive(Debug, serde::Serialize)]
pub struct AgentWorkspaceReviewTargetResponse {
    pub scope: String,
    pub base_ref: String,
    pub base_sha: Option<String>,
    pub head_ref: String,
    pub head_sha: Option<String>,
    pub diff_fingerprint: String,
    pub source_pull_request_number: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_packet: Option<AgentWorkspaceReviewPacketResponse>,
}

#[derive(Debug, serde::Serialize)]
pub struct AgentWorkspaceReviewPacketResponse {
    pub summary: AgentWorkspaceReviewDiffSummaryResponse,
    pub changed_files: Vec<AgentWorkspaceReviewChangedFileResponse>,
    pub changed_files_truncated: bool,
    pub hunk_anchors: Vec<AgentWorkspaceReviewHunkAnchorResponse>,
    pub hunk_anchors_truncated: bool,
    pub patch_excerpt: String,
    pub patch_excerpt_truncated: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct AgentWorkspaceReviewDiffSummaryResponse {
    pub files_changed: u32,
    pub insertions: u32,
    pub deletions: u32,
}

#[derive(Debug, serde::Serialize)]
pub struct AgentWorkspaceReviewChangedFileResponse {
    pub path: String,
    pub status: String,
    pub sources: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct AgentWorkspaceReviewHunkAnchorResponse {
    pub path: String,
    pub source: String,
    pub hunk_header: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
}

impl From<AgentWorkspaceReviewHunkAnchor> for AgentWorkspaceReviewHunkAnchorResponse {
    fn from(value: AgentWorkspaceReviewHunkAnchor) -> Self {
        Self {
            path: value.path,
            source: value.source,
            hunk_header: value.hunk_header,
            old_start: value.old_start,
            old_lines: value.old_lines,
            new_start: value.new_start,
            new_lines: value.new_lines,
        }
    }
}

impl From<&AgentWorkspaceReviewHunkAnchor> for AgentWorkspaceReviewHunkAnchorResponse {
    fn from(value: &AgentWorkspaceReviewHunkAnchor) -> Self {
        Self {
            path: value.path.clone(),
            source: value.source.clone(),
            hunk_header: value.hunk_header.clone(),
            old_start: value.old_start,
            old_lines: value.old_lines,
            new_start: value.new_start,
            new_lines: value.new_lines,
        }
    }
}

impl From<crate::application::agent_workspace_review::AgentWorkspaceReviewPacket>
    for AgentWorkspaceReviewPacketResponse
{
    fn from(value: crate::application::agent_workspace_review::AgentWorkspaceReviewPacket) -> Self {
        Self {
            summary: AgentWorkspaceReviewDiffSummaryResponse {
                files_changed: value.summary.files_changed,
                insertions: value.summary.insertions,
                deletions: value.summary.deletions,
            },
            changed_files: value
                .changed_files
                .into_iter()
                .map(|file| AgentWorkspaceReviewChangedFileResponse {
                    path: file.path,
                    status: file.status,
                    sources: file.sources,
                })
                .collect(),
            changed_files_truncated: value.changed_files_truncated,
            hunk_anchors: value
                .hunk_anchors
                .into_iter()
                .map(AgentWorkspaceReviewHunkAnchorResponse::from)
                .collect(),
            hunk_anchors_truncated: value.hunk_anchors_truncated,
            patch_excerpt: value.patch_excerpt,
            patch_excerpt_truncated: value.patch_excerpt_truncated,
            notes: value.notes,
        }
    }
}

impl From<crate::application::agent_workspace_review::AgentWorkspaceReviewTarget>
    for AgentWorkspaceReviewTargetResponse
{
    fn from(value: crate::application::agent_workspace_review::AgentWorkspaceReviewTarget) -> Self {
        Self::from_target(value, false)
    }
}

impl AgentWorkspaceReviewTargetResponse {
    fn from_target(
        value: crate::application::agent_workspace_review::AgentWorkspaceReviewTarget,
        include_review_packet: bool,
    ) -> Self {
        let review_packet = include_review_packet
            .then(|| AgentWorkspaceReviewPacketResponse::from(value.review_packet));
        Self {
            scope: value.scope.to_string(),
            base_ref: value.base_ref,
            base_sha: value.base_sha,
            head_ref: value.head_ref,
            head_sha: value.head_sha,
            diff_fingerprint: value.diff_fingerprint,
            source_pull_request_number: value.source_pull_request_number,
            review_packet,
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct AgentWorkspaceReviewMonitorResponse {
    pub conversation_id: String,
    pub project_id: String,
    pub status: String,
    pub review_outcome: String,
    pub review_gate_status: String,
    pub current_target_scope: Option<String>,
    pub reviewed_target_scope: Option<String>,
    pub review_conversation_id: Option<String>,
    pub review_artifact_id: Option<String>,
    pub review_artifact_version: Option<u32>,
    pub review_artifact_updated_at: Option<String>,
    pub review_gate_bypassed_at: Option<String>,
    pub review_gate_bypassed_target_scope: Option<String>,
    pub review_gate_bypassed_diff_fingerprint: Option<String>,
    pub review_gate_bypassed_artifact_id: Option<String>,
    pub review_gate_bypassed_artifact_version: Option<u32>,
    pub reviewed_head_sha: Option<String>,
    pub reviewed_diff_fingerprint: Option<String>,
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
    pub previous_version_id: Option<String>,
    pub review_blocking_summary: Option<String>,
    pub review_blocking_fingerprint: Option<String>,
    pub review_fixer_run_id: Option<String>,
    pub review_fixer_conversation_id: Option<String>,
    pub review_fixer_status: Option<String>,
    pub last_run_id: Option<String>,
    pub last_error: Option<String>,
    pub auto_merge_guard_status: Option<String>,
    pub auto_merge_guard_pr_number: Option<i64>,
    pub auto_merge_guard_method: Option<String>,
    pub auto_merge_guard_target_scope: Option<String>,
    pub auto_merge_guard_diff_fingerprint: Option<String>,
    pub auto_merge_guard_head_sha: Option<String>,
    pub auto_merge_guard_last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<AgentWorkspaceReviewMonitor> for AgentWorkspaceReviewMonitorResponse {
    fn from(value: AgentWorkspaceReviewMonitor) -> Self {
        Self {
            conversation_id: value.conversation_id.as_str(),
            project_id: value.project_id.as_str().to_string(),
            status: value.status.to_string(),
            review_outcome: value.review_outcome.to_string(),
            review_gate_status: value.review_gate_status.to_string(),
            current_target_scope: value.current_target_scope.map(|scope| scope.to_string()),
            reviewed_target_scope: value.reviewed_target_scope.map(|scope| scope.to_string()),
            review_conversation_id: value
                .review_conversation_id
                .map(|conversation_id| conversation_id.as_str()),
            review_artifact_id: value
                .review_artifact_id
                .map(|artifact_id| artifact_id.as_str().to_string()),
            review_artifact_version: value.review_artifact_version,
            review_artifact_updated_at: value
                .review_artifact_updated_at
                .map(|value| value.to_rfc3339()),
            review_gate_bypassed_at: value
                .review_gate_bypassed_at
                .map(|value| value.to_rfc3339()),
            review_gate_bypassed_target_scope: value
                .review_gate_bypassed_target_scope
                .map(|scope| scope.to_string()),
            review_gate_bypassed_diff_fingerprint: value.review_gate_bypassed_diff_fingerprint,
            review_gate_bypassed_artifact_id: value
                .review_gate_bypassed_artifact_id
                .map(|artifact_id| artifact_id.as_str().to_string()),
            review_gate_bypassed_artifact_version: value.review_gate_bypassed_artifact_version,
            reviewed_head_sha: value.reviewed_head_sha,
            reviewed_diff_fingerprint: value.reviewed_diff_fingerprint,
            selected_source_base_ref: value.selected_source_base_ref,
            selected_source_base_sha: value.selected_source_base_sha,
            selected_source_head_ref: value.selected_source_head_ref,
            selected_source_head_sha: value.selected_source_head_sha,
            selected_source_pull_request_number: value.selected_source_pull_request_number,
            workspace_base_ref: value.workspace_base_ref,
            workspace_base_sha: value.workspace_base_sha,
            workspace_head_ref: value.workspace_head_ref,
            workspace_head_sha: value.workspace_head_sha,
            current_diff_fingerprint: value.current_diff_fingerprint,
            previous_version_id: value
                .previous_version_id
                .map(|artifact_id| artifact_id.as_str().to_string()),
            review_blocking_summary: value.review_blocking_summary,
            review_blocking_fingerprint: value.review_blocking_fingerprint,
            review_fixer_run_id: value.review_fixer_run_id,
            review_fixer_conversation_id: value
                .review_fixer_conversation_id
                .map(|conversation_id| conversation_id.as_str()),
            review_fixer_status: value.review_fixer_status,
            last_run_id: value.last_run_id,
            last_error: value.last_error,
            auto_merge_guard_status: value
                .auto_merge_guard
                .as_ref()
                .map(|guard| guard.status.to_string()),
            auto_merge_guard_pr_number: value
                .auto_merge_guard
                .as_ref()
                .map(|guard| guard.pr_number),
            auto_merge_guard_method: value
                .auto_merge_guard
                .as_ref()
                .map(|guard| guard.merge_method.clone()),
            auto_merge_guard_target_scope: value
                .auto_merge_guard
                .as_ref()
                .map(|guard| guard.target_scope.to_string()),
            auto_merge_guard_diff_fingerprint: value
                .auto_merge_guard
                .as_ref()
                .map(|guard| guard.diff_fingerprint.clone()),
            auto_merge_guard_head_sha: value
                .auto_merge_guard
                .as_ref()
                .and_then(|guard| guard.head_sha.clone()),
            auto_merge_guard_last_error: value
                .auto_merge_guard
                .as_ref()
                .and_then(|guard| guard.last_error.clone()),
            created_at: value.created_at.to_rfc3339(),
            updated_at: value.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct AgentWorkspaceReviewContextResponse {
    pub success: bool,
    pub workspace: AgentConversationWorkspaceResponse,
    pub events: Vec<AgentConversationWorkspacePublicationEventResponse>,
    pub target: Option<AgentWorkspaceReviewTargetResponse>,
    pub monitor: AgentWorkspaceReviewMonitorResponse,
    pub goal_context: AgentWorkspaceReviewGoalContext,
    pub is_current: bool,
    pub is_outdated: bool,
    pub review_artifact_is_current: bool,
    pub review_artifact_is_outdated: bool,
    pub can_mutate_review_state: bool,
    pub review_runtime_state: String,
    pub should_show_tab: bool,
}

#[derive(Debug, serde::Deserialize, Default)]
pub struct AgentWorkspaceReviewContextQuery {
    pub include_review_packet: Option<bool>,
    pub refresh_target: Option<bool>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartAgentWorkspaceReviewRequest {
    pub force: Option<bool>,
    pub confirmation: Option<StartAgentWorkspaceReviewConfirmationRequest>,
    pub runtime_override: Option<ManualRoleRuntimeOverrideRequest>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartAgentWorkspaceReviewConfirmationRequest {
    pub target_scope: Option<String>,
    pub diff_fingerprint: Option<String>,
    pub head_sha: Option<String>,
    pub pr_number: Option<i64>,
    pub will_disable_auto_merge: bool,
    pub merge_method: Option<String>,
    #[serde(default)]
    pub restore_after_publish: bool,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManualRoleRuntimeOverrideRequest {
    pub provider: AgentHarnessKind,
    pub model: Option<String>,
    pub effort: Option<LogicalEffort>,
    pub service_tier: ManualServiceTier,
    pub coordination_mode: Option<crate::domain::entities::CoordinationMode>,
    pub persona_id: Option<String>,
}

impl From<ManualRoleRuntimeOverrideRequest> for ManualRoleRuntimeOverride {
    fn from(value: ManualRoleRuntimeOverrideRequest) -> Self {
        Self {
            harness: value.provider,
            model: value.model,
            effort: value.effort,
            service_tier: value.service_tier,
            coordination_mode: value.coordination_mode,
            persona_id: value
                .persona_id
                .map(crate::domain::entities::PersonaId::from_string),
        }
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartAgentWorkspaceReviewFixerRequest {
    pub confirmation: StartAgentWorkspaceReviewFixerConfirmationRequest,
    pub runtime_override: Option<ManualRoleRuntimeOverrideRequest>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartAgentWorkspaceReviewFixerConfirmationRequest {
    pub target_scope: String,
    pub diff_fingerprint: String,
    pub artifact_id: String,
    pub artifact_version: u32,
    pub blocking_fingerprint: String,
}

impl TryFrom<StartAgentWorkspaceReviewFixerConfirmationRequest>
    for WorkspaceReviewFixerConfirmation
{
    type Error = AppError;

    fn try_from(
        value: StartAgentWorkspaceReviewFixerConfirmationRequest,
    ) -> Result<Self, Self::Error> {
        Ok(Self {
            target_scope: AgentWorkspaceReviewTargetScope::from_str(&value.target_scope)
                .map_err(AppError::Validation)?,
            diff_fingerprint: value.diff_fingerprint,
            artifact_id: value.artifact_id,
            artifact_version: value.artifact_version,
            blocking_fingerprint: value.blocking_fingerprint,
        })
    }
}

impl TryFrom<StartAgentWorkspaceReviewConfirmationRequest> for WorkspaceReviewStartConfirmation {
    type Error = AppError;

    fn try_from(value: StartAgentWorkspaceReviewConfirmationRequest) -> Result<Self, Self::Error> {
        let target_scope = value
            .target_scope
            .as_deref()
            .map(AgentWorkspaceReviewTargetScope::from_str)
            .transpose()
            .map_err(AppError::Validation)?;
        Ok(Self {
            target_scope,
            diff_fingerprint: value.diff_fingerprint,
            head_sha: value.head_sha,
            pr_number: value.pr_number,
            will_disable_auto_merge: value.will_disable_auto_merge,
            merge_method: value.merge_method,
            restore_after_publish: value.restore_after_publish,
        })
    }
}

#[derive(Debug, serde::Serialize)]
pub struct StartAgentWorkspaceReviewConfirmationResponse {
    pub target_scope: Option<String>,
    pub diff_fingerprint: Option<String>,
    pub head_sha: Option<String>,
    pub pr_number: Option<i64>,
    pub will_disable_auto_merge: bool,
    pub merge_method: Option<String>,
    pub restore_after_publish: bool,
}

impl From<WorkspaceReviewStartConfirmation> for StartAgentWorkspaceReviewConfirmationResponse {
    fn from(value: WorkspaceReviewStartConfirmation) -> Self {
        Self {
            target_scope: value.target_scope.map(|scope| scope.to_string()),
            diff_fingerprint: value.diff_fingerprint,
            head_sha: value.head_sha,
            pr_number: value.pr_number,
            will_disable_auto_merge: value.will_disable_auto_merge,
            merge_method: value.merge_method,
            restore_after_publish: value.restore_after_publish,
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct AgentWorkspaceReviewStartPreviewResponse {
    pub success: bool,
    pub target: Option<AgentWorkspaceReviewTargetResponse>,
    pub will_disable_auto_merge: bool,
    pub pr_number: Option<i64>,
    pub merge_method: Option<String>,
    pub restore_after_publish: bool,
    pub confirmation: StartAgentWorkspaceReviewConfirmationResponse,
}

#[derive(Debug, serde::Serialize)]
pub struct StartAgentWorkspaceReviewResponse {
    pub success: bool,
    pub target: Option<AgentWorkspaceReviewTargetResponse>,
    pub monitor: AgentWorkspaceReviewMonitorResponse,
    pub goal_context: AgentWorkspaceReviewGoalContext,
    pub is_current: bool,
    pub is_outdated: bool,
    pub review_artifact_is_current: bool,
    pub review_artifact_is_outdated: bool,
    pub can_mutate_review_state: bool,
    pub review_runtime_state: String,
    pub should_show_tab: bool,
    pub started: bool,
    pub skipped_reason: Option<String>,
    pub was_queued: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct StartAgentWorkspaceReviewFixerResponse {
    pub success: bool,
    pub target: Option<AgentWorkspaceReviewTargetResponse>,
    pub monitor: AgentWorkspaceReviewMonitorResponse,
    pub goal_context: AgentWorkspaceReviewGoalContext,
    pub is_current: bool,
    pub is_outdated: bool,
    pub review_artifact_is_current: bool,
    pub review_artifact_is_outdated: bool,
    pub can_mutate_review_state: bool,
    pub review_runtime_state: String,
    pub should_show_tab: bool,
    pub started: bool,
    pub skipped_reason: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct WriteAgentWorkspaceReviewArtifactRequest {
    pub title: Option<String>,
    pub content: String,
    pub target_scope: Option<String>,
    pub head_sha: Option<String>,
    pub diff_fingerprint: Option<String>,
    pub created_by_run_id: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct WriteAgentWorkspaceReviewHunkAnnotationRequest {
    pub path: String,
    pub source: String,
    pub hunk_header: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub title: Option<String>,
    pub message: String,
    pub level: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct WriteAgentWorkspaceReviewHunkAnnotationsRequest {
    pub target_scope: Option<String>,
    pub head_sha: Option<String>,
    pub diff_fingerprint: Option<String>,
    pub created_by_run_id: Option<String>,
    pub annotations: Vec<WriteAgentWorkspaceReviewHunkAnnotationRequest>,
}

#[derive(Debug, serde::Serialize)]
pub struct WriteAgentWorkspaceReviewHunkAnnotationResult {
    pub index: usize,
    pub accepted: bool,
    pub annotation_id: Option<String>,
    pub path: Option<String>,
    pub source: Option<String>,
    pub hunk_header: Option<String>,
    pub old_start: Option<u32>,
    pub old_lines: Option<u32>,
    pub new_start: Option<u32>,
    pub new_lines: Option<u32>,
    pub reason: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct WriteAgentWorkspaceReviewHunkAnnotationsResponse {
    pub success: bool,
    pub accepted_count: usize,
    pub rejected_count: usize,
    pub stored_count: usize,
    pub missing_required_count: usize,
    pub artifact_id: String,
    pub artifact_version: u32,
    pub monitor: AgentWorkspaceReviewMonitorResponse,
    pub results: Vec<WriteAgentWorkspaceReviewHunkAnnotationResult>,
    pub missing_required_hunks: Vec<AgentWorkspaceReviewHunkAnchorResponse>,
}

#[derive(Debug, serde::Serialize)]
pub struct WriteAgentWorkspaceReviewArtifactResponse {
    pub success: bool,
    pub monitor: AgentWorkspaceReviewMonitorResponse,
    pub artifact: ArtifactResponse,
    pub previous_artifact_id: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct CompleteAgentWorkspaceReviewRunRequest {
    pub outcome: Option<String>,
    pub summary: String,
    pub blocker: Option<String>,
    pub created_by_run_id: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct CompleteAgentWorkspaceReviewRunResponse {
    pub success: bool,
    pub monitor: AgentWorkspaceReviewMonitorResponse,
}

#[derive(Debug, serde::Deserialize)]
pub struct ProposeAgentWorkspacePrReviewActionRequest {
    pub head_sha: String,
    pub proposed_action: String,
    pub summary: String,
    pub review_body: String,
    pub findings_json: Option<String>,
    pub created_by_run_id: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct ProposeAgentWorkspacePrReviewActionResponse {
    pub success: bool,
    pub monitor: AgentWorkspacePrReviewMonitorResponse,
    pub action: AgentWorkspacePrReviewActionResponse,
}

#[derive(Debug, serde::Deserialize)]
pub struct WriteAgentWorkspacePrReviewArtifactRequest {
    pub title: Option<String>,
    pub content: String,
    pub head_sha: Option<String>,
    pub created_by_run_id: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct WriteAgentWorkspacePrReviewArtifactResponse {
    pub success: bool,
    pub monitor: AgentWorkspacePrReviewMonitorResponse,
    pub artifact: ArtifactResponse,
    pub previous_artifact_id: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct CompleteAgentWorkspacePrReviewRunRequest {
    pub head_sha: Option<String>,
    pub outcome: Option<String>,
    pub summary: String,
    pub blocker: Option<String>,
    pub created_by_run_id: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct CompleteAgentWorkspacePrReviewRunResponse {
    pub success: bool,
    pub monitor: AgentWorkspacePrReviewMonitorResponse,
}

#[derive(Debug, serde::Deserialize)]
pub struct SubmitAgentWorkspacePrReviewActionRequest {
    pub action_kind: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct UpdateAgentWorkspacePrReviewSettingsRequest {
    pub auto_approve_enabled: Option<bool>,
    pub monitor_enabled: Option<bool>,
    pub active_review_policy: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct UpdateAgentWorkspacePrReviewSettingsResponse {
    pub success: bool,
    pub monitor: AgentWorkspacePrReviewMonitorResponse,
}

#[derive(Debug, serde::Serialize)]
pub struct SubmitAgentWorkspacePrReviewActionResponse {
    pub success: bool,
    pub monitor: AgentWorkspacePrReviewMonitorResponse,
    pub action: AgentWorkspacePrReviewActionResponse,
    pub submitted_review_id: String,
    pub submitted_review_url: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct SkipAgentWorkspacePrReviewActionRequest {
    pub reason: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct SkipAgentWorkspacePrReviewActionResponse {
    pub success: bool,
    pub monitor: AgentWorkspacePrReviewMonitorResponse,
    pub action: AgentWorkspacePrReviewActionResponse,
}

/// POST /api/agent-workspaces/{conversation_id}/pr-description
///
/// Called by the dedicated PR describer agent after it writes the body for an
/// agent workspace publish.
pub async fn submit_agent_workspace_pr_description(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
    Json(req): Json<SubmitAgentWorkspacePrDescriptionRequest>,
) -> Result<Json<SubmitAgentWorkspacePrDescriptionResponse>, JsonError> {
    let decision = match req.decision.as_str() {
        "preserve" if req.title.is_none() && req.body_markdown.is_none() => {
            AgentWorkspacePrMetadataDecision::Preserve
        }
        "patch" => AgentWorkspacePrMetadataDecision::patch(req.title, req.body_markdown)
            .ok_or_else(|| {
                json_error(
                    StatusCode::BAD_REQUEST,
                    "PR metadata patch requires a non-empty title or body",
                    None,
                )
            })?,
        "preserve" => {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                "preserve cannot include title or body",
                None,
            ))
        }
        _ => {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                "PR metadata decision must be preserve or patch",
                None,
            ))
        }
    };

    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Agent workspace not found", None))?;

    state
        .app_state
        .agent_conversation_workspace_repo
        .save_pr_metadata_decision(&workspace.conversation_id, decision)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;

    Ok(Json(SubmitAgentWorkspacePrDescriptionResponse {
        success: true,
    }))
}

/// GET /api/agent-workspaces/{conversation_id}/publish-status
pub async fn get_agent_workspace_publish_status(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
) -> Result<Json<AgentWorkspacePublishStatusResponse>, JsonError> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace =
        load_agent_workspace_response(state.app_state.as_ref(), &conversation_id).await?;
    let events =
        load_agent_workspace_publication_events(state.app_state.as_ref(), &conversation_id).await?;
    Ok(Json(AgentWorkspacePublishStatusResponse {
        success: true,
        publish_in_progress: is_publish_in_progress(workspace.publication_push_status.as_deref()),
        needs_agent_repair: workspace.publication_push_status.as_deref() == Some("needs_agent"),
        workspace,
        events,
    }))
}

/// GET /api/agent-workspaces/{conversation_id}/publish-readiness
pub async fn check_agent_workspace_publish_readiness(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
) -> Result<Json<AgentWorkspacePublishReadinessResponse>, JsonError> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace =
        load_agent_workspace_response(state.app_state.as_ref(), &conversation_id).await?;
    let freshness = get_agent_conversation_workspace_freshness_for_app_state(
        &conversation_id,
        Some("full"),
        state.app_state.as_ref(),
    )
    .await
    .map_err(|error| json_error(StatusCode::CONFLICT, error, None))?;
    let workspace_entity =
        load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
    let review_context =
        load_agent_workspace_review_context(state.app_state.as_ref(), &workspace_entity)
            .await
            .map_err(|error| {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
            })?;
    let review_settings = state
        .app_state
        .review_settings_repo
        .get_settings()
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let review_gate_status = Some(review_context.monitor.review_gate_status.to_string());
    let review_gate_blocker = if review_settings.require_workspace_review {
        review_gate_publish_blocker(&review_context)
    } else {
        None
    };
    let blockers = publish_readiness_blockers(&freshness, review_gate_blocker);
    let recommended_actions = publish_readiness_recommended_actions(&freshness);
    Ok(Json(AgentWorkspacePublishReadinessResponse {
        success: true,
        can_publish: blockers.is_empty(),
        workspace,
        freshness,
        review_gate_status,
        blockers,
        needs_base_update: recommended_actions
            .iter()
            .any(|action| action == "update_from_base"),
        recommended_actions,
    }))
}

/// POST /api/agent-workspaces/{conversation_id}/update-from-base
pub async fn update_agent_workspace_from_base(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
    Json(req): Json<UpdateAgentWorkspaceFromBaseRequest>,
) -> Result<Json<AgentWorkspacePublishActionResponse>, JsonError> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let selection = AgentConversationWorkspaceBaseSelection {
        kind: parse_update_base_kind(req.base_ref_kind.as_deref())
            .map_err(|error| json_error(StatusCode::BAD_REQUEST, error, None))?,
        branch_mode: None,
        base_ref: req.base_ref,
        display_name: req.base_display_name,
        source_pull_request: None,
    };
    match update_agent_conversation_workspace_from_base_for_app_state_with_caller(
        state.app_state.as_ref(),
        &state.execution_state,
        conversation_id,
        selection,
        req.created_by_run_id.as_deref(),
    )
    .await
    {
        Ok(result) => Ok(Json(AgentWorkspacePublishActionResponse {
            success: true,
            status: if result.updated {
                "updated"
            } else {
                "base_current"
            }
            .to_string(),
            message: if result.updated {
                "Workspace branch updated from base".to_string()
            } else {
                "Workspace branch is current with base".to_string()
            },
            repair_queued: false,
            freshness: None,
            updated: Some(result.updated),
            target_ref: Some(result.target_ref),
            base_commit: Some(result.base_commit),
            workspace: Some(result.workspace),
            commit_sha: None,
            pushed: None,
            created_pr: None,
            pr_number: None,
            pr_url: None,
        })),
        Err(error) => {
            action_response_for_needs_repair(state.app_state.as_ref(), &conversation_id, error)
                .await
        }
    }
}

/// POST /api/agent-workspaces/{conversation_id}/publish
pub async fn publish_agent_workspace(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
) -> Result<Json<AgentWorkspacePublishActionResponse>, JsonError> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace =
        load_agent_workspace_response(state.app_state.as_ref(), &conversation_id).await?;
    if let Some(response) = publish_action_response_for_existing_workspace_state(
        state.app_state.as_ref(),
        &conversation_id,
        workspace,
    )
    .await?
    {
        return Ok(Json(response));
    }

    match publish_agent_conversation_workspace_for_app_state(
        state.app_state.as_ref(),
        &state.execution_state,
        conversation_id,
        true,
    )
    .await
    {
        Ok(result) => Ok(Json(AgentWorkspacePublishActionResponse {
            success: true,
            status: "published".to_string(),
            message: "Draft pull request is ready".to_string(),
            repair_queued: false,
            workspace: Some(result.workspace),
            freshness: None,
            updated: None,
            target_ref: None,
            base_commit: None,
            commit_sha: result.commit_sha,
            pushed: Some(result.pushed),
            created_pr: Some(result.created_pr),
            pr_number: result.pr_number,
            pr_url: result.pr_url,
        })),
        Err(error) if error == AGENT_WORKSPACE_PUBLISH_IN_PROGRESS_MESSAGE => {
            let workspace =
                load_agent_workspace_response(state.app_state.as_ref(), &conversation_id).await?;
            Ok(Json(publish_in_progress_response(workspace)))
        }
        Err(error) => {
            action_response_for_needs_repair(state.app_state.as_ref(), &conversation_id, error)
                .await
        }
    }
}

/// POST /api/agent-workspaces/{conversation_id}/commit-local
pub async fn commit_agent_workspace_locally_handler(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
    Json(req): Json<CommitAgentWorkspaceLocallyRequest>,
) -> Result<Json<CommitAgentWorkspaceLocallyResponse>, JsonError> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let result = commit_agent_workspace_locally(
        state.app_state.as_ref(),
        conversation_id,
        AgentWorkspaceLocalCommitRequest {
            expected_head_sha: req.expected_head_sha,
            review_artifact_id: req.review_artifact_id,
            review_artifact_version: req.review_artifact_version,
            reviewed_head_sha: req.reviewed_head_sha,
            reviewed_diff_fingerprint: req.reviewed_diff_fingerprint,
            attempt_token: req.attempt_token,
            #[cfg(test)]
            before_staging: None,
        },
    )
    .await
    .map_err(|error| json_error(StatusCode::CONFLICT, error, None))?;
    let workspace = agent_workspace_response_for_state(state.app_state.as_ref(), result.workspace)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error, None))?;
    Ok(Json(CommitAgentWorkspaceLocallyResponse {
        success: true,
        workspace,
        outcome: result.outcome.as_str().to_string(),
        branch_name: result.branch_name,
        previous_head_sha: result.previous_head_sha,
        commit_sha: result.commit_sha,
        had_changes: result.had_changes,
        attempt_token: result.attempt_token,
    }))
}

/// GET /api/agent-workspaces/{conversation_id}/pr-fix-context
pub async fn get_agent_workspace_pr_fix_context(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
) -> Result<Json<AgentWorkspacePrFixContextResponse>, JsonError> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace_entity =
        load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
    let target =
        resolve_agent_workspace_pr_fix_target(state.app_state.as_ref(), &workspace_entity).await?;
    let workspace = agent_workspace_response_for_state(state.app_state.as_ref(), workspace_entity)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error, None))?;
    let events =
        load_agent_workspace_publication_events(state.app_state.as_ref(), &conversation_id).await?;

    let (health, review_feedback) = match (state.app_state.github_service.as_ref(), target.as_ref())
    {
        (Some(github), Some(target)) => {
            let mut health = github
                .fetch_pr_health(&target.working_dir, target.pr_number)
                .await
                .ok();
            if let Some(health) = health.as_ref() {
                import_agent_workspace_pr_comment_evidence(
                    Arc::clone(&state.app_state.agent_conversation_workspace_repo),
                    &conversation_id,
                    target.pr_number,
                    health,
                )
                .await
                .map_err(|error| {
                    json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
                })?;
            }
            if let Some(health) = health.as_mut() {
                truncate_pr_health_issue_comments(health);
            }
            let review_feedback = github
                .check_pr_review_feedback(&target.working_dir, target.pr_number)
                .await
                .ok()
                .flatten();
            (health, review_feedback)
        }
        _ => (None, None),
    };

    let pr_number = target.as_ref().map(|target| target.pr_number);
    let pr_url = target.as_ref().and_then(|target| target.pr_url.clone());
    let issue_comment_evidence = match pr_number {
        Some(pr_number) => {
            let comments = state
                .app_state
                .agent_conversation_workspace_repo
                .list_pr_comment_evidence(&conversation_id, pr_number, 20)
                .await
                .map_err(|error| {
                    json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
                })?;
            let comment_ids = comments
                .iter()
                .map(|comment| comment.comment_id.clone())
                .collect::<Vec<_>>();
            state
                .app_state
                .agent_conversation_workspace_repo
                .mark_pr_comments_included(&conversation_id, pr_number, &comment_ids)
                .await
                .map_err(|error| {
                    json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
                })?;
            comments
                .into_iter()
                .map(AgentWorkspacePrCommentEvidenceResponse::from_evidence)
                .collect()
        }
        None => Vec::new(),
    };
    Ok(Json(AgentWorkspacePrFixContextResponse {
        success: true,
        workspace,
        events,
        target_kind: target.as_ref().map(|target| target.kind_name().to_string()),
        target_branch: target.as_ref().map(|target| target.branch_name.clone()),
        target_base_branch: target.as_ref().map(|target| target.base_branch.clone()),
        pr_number,
        pr_url,
        health,
        review_feedback,
        issue_comment_evidence,
    }))
}

fn workspace_review_action_error(error: AppError) -> JsonError {
    let status = match &error {
        AppError::Validation(_)
        | AppError::Conflict(_)
        | AppError::WorkspaceReviewUnfinishedGitOperation => StatusCode::CONFLICT,
        AppError::NotFound(_) | AppError::ProjectNotFound(_) => StatusCode::NOT_FOUND,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    let message = match error {
        AppError::Conflict(message) => message,
        error => error.to_string(),
    };
    json_error(status, message, None)
}

/// GET /api/agent-workspaces/{conversation_id}/workspace-review-start-preview
pub async fn get_agent_workspace_review_start_preview(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
) -> Result<Json<AgentWorkspaceReviewStartPreviewResponse>, JsonError> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
    let _lifecycle_guard = lock_workspace_review_lifecycle(&conversation_id).await;
    let preview = preview_manual_workspace_review_start(state.app_state.as_ref(), &workspace)
        .await
        .map_err(workspace_review_action_error)?;
    let will_disable_auto_merge = preview.auto_merge.is_some();
    let pr_number = preview.auto_merge.as_ref().map(|effect| effect.pr_number);
    let merge_method = preview
        .auto_merge
        .as_ref()
        .map(|effect| effect.merge_method.clone());
    let restore_after_publish = preview
        .auto_merge
        .as_ref()
        .map(|effect| effect.restore_after_publish)
        .unwrap_or_else(|| {
            preview.target.as_ref().is_some_and(|target| {
                target.scope == AgentWorkspaceReviewTargetScope::WorkspaceDelta
            })
        });
    Ok(Json(AgentWorkspaceReviewStartPreviewResponse {
        success: true,
        target: preview.target.map(AgentWorkspaceReviewTargetResponse::from),
        will_disable_auto_merge,
        pr_number,
        merge_method,
        restore_after_publish,
        confirmation: preview.confirmation.into(),
    }))
}

/// POST /api/agent-workspaces/{conversation_id}/workspace-review-runs
pub async fn start_agent_workspace_review_run(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
    Json(req): Json<StartAgentWorkspaceReviewRequest>,
) -> Result<Json<StartAgentWorkspaceReviewResponse>, JsonError> {
    let started = Instant::now();
    let force = req.force.unwrap_or(false);
    let confirmation = req
        .confirmation
        .map(WorkspaceReviewStartConfirmation::try_from)
        .transpose()
        .map_err(workspace_review_action_error)?;
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
    let runtime_override = req.runtime_override.map(ManualRoleRuntimeOverride::from);
    let start = start_guarded_agent_workspace_review_with_runtime_override(
        std::sync::Arc::clone(&state.app_state),
        &workspace,
        force,
        WorkspaceReviewStartOrigin::Manual,
        confirmation.as_ref(),
        runtime_override.as_ref(),
    )
    .await
    .map_err(workspace_review_action_error)?;
    let target_scope = workspace_review_target_scope_log(start.context.target.as_ref());
    let diff_fingerprint = compact_workspace_review_log_fingerprint(
        start
            .context
            .target
            .as_ref()
            .map(|target| target.diff_fingerprint.as_str()),
    );
    let skipped_reason = start
        .skipped_reason
        .as_deref()
        .unwrap_or("none")
        .to_string();
    tracing::info!(
        target: "ralphx_lib::http_server::agent_workspaces",
        operation = "workspace_review_start_http",
        conversation_id = %conversation_id,
        project_id = %workspace.project_id,
        branch = %workspace.branch_name,
        elapsed_ms = started.elapsed().as_millis(),
        force,
        started = start.started,
        skipped_reason = %skipped_reason,
        was_queued = start.was_queued,
        monitor_status = %start.context.monitor.status,
        target_scope = %target_scope,
        diff_fingerprint = %diff_fingerprint,
        is_current = start.context.is_current,
        is_outdated = start.context.is_outdated,
        has_artifact = start.context.monitor.review_artifact_id.is_some(),
        "Handled workspace Review start request"
    );
    Ok(Json(StartAgentWorkspaceReviewResponse {
        success: true,
        target: start
            .context
            .target
            .map(AgentWorkspaceReviewTargetResponse::from),
        monitor: AgentWorkspaceReviewMonitorResponse::from(start.context.monitor),
        goal_context: start.context.goal_context,
        is_current: start.context.is_current,
        is_outdated: start.context.is_outdated,
        review_artifact_is_current: start.context.review_artifact_is_current,
        review_artifact_is_outdated: start.context.review_artifact_is_outdated,
        can_mutate_review_state: start.context.can_mutate_review_state,
        review_runtime_state: start.context.review_runtime_state.to_string(),
        should_show_tab: start.context.should_show_tab,
        started: start.started,
        skipped_reason: start.skipped_reason,
        was_queued: start.was_queued,
    }))
}

/// POST /api/agent-workspaces/{conversation_id}/workspace-review-fixer-runs
pub async fn start_agent_workspace_review_fixer_run(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
    Json(req): Json<StartAgentWorkspaceReviewFixerRequest>,
) -> Result<Json<StartAgentWorkspaceReviewFixerResponse>, JsonError> {
    let started = Instant::now();
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
    let confirmation = WorkspaceReviewFixerConfirmation::try_from(req.confirmation)
        .map_err(workspace_review_action_error)?;
    let runtime_override = req.runtime_override.map(ManualRoleRuntimeOverride::from);
    let start = start_agent_workspace_review_blocking_fixer_with_override(
        state.app_state.as_ref(),
        &workspace,
        Some(&confirmation),
        runtime_override.as_ref(),
    )
    .await
    .map_err(workspace_review_action_error)?;
    let target_scope = workspace_review_target_scope_log(start.context.target.as_ref());
    let diff_fingerprint = compact_workspace_review_log_fingerprint(
        start
            .context
            .target
            .as_ref()
            .map(|target| target.diff_fingerprint.as_str()),
    );
    let skipped_reason = start
        .skipped_reason
        .as_deref()
        .unwrap_or("none")
        .to_string();
    tracing::info!(
        target: "ralphx_lib::http_server::agent_workspaces",
        operation = "workspace_review_fixer_start_http",
        conversation_id = %conversation_id,
        project_id = %workspace.project_id,
        branch = %workspace.branch_name,
        elapsed_ms = started.elapsed().as_millis(),
        started = start.started,
        skipped_reason = %skipped_reason,
        monitor_status = %start.context.monitor.status,
        review_fixer_status = %start.context.monitor.review_fixer_status.as_deref().unwrap_or("none"),
        target_scope = %target_scope,
        diff_fingerprint = %diff_fingerprint,
        is_current = start.context.is_current,
        is_outdated = start.context.is_outdated,
        has_artifact = start.context.monitor.review_artifact_id.is_some(),
        "Handled workspace Review fixer start request"
    );

    Ok(Json(StartAgentWorkspaceReviewFixerResponse {
        success: true,
        target: start
            .context
            .target
            .map(AgentWorkspaceReviewTargetResponse::from),
        monitor: AgentWorkspaceReviewMonitorResponse::from(start.context.monitor),
        goal_context: start.context.goal_context,
        is_current: start.context.is_current,
        is_outdated: start.context.is_outdated,
        review_artifact_is_current: start.context.review_artifact_is_current,
        review_artifact_is_outdated: start.context.review_artifact_is_outdated,
        can_mutate_review_state: start.context.can_mutate_review_state,
        review_runtime_state: start.context.review_runtime_state.to_string(),
        should_show_tab: start.context.should_show_tab,
        started: start.started,
        skipped_reason: start.skipped_reason,
    }))
}

/// POST /api/agent-workspaces/{conversation_id}/workspace-review-artifact
pub async fn write_agent_workspace_review_artifact(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
    Json(req): Json<WriteAgentWorkspaceReviewArtifactRequest>,
) -> Result<Json<WriteAgentWorkspaceReviewArtifactResponse>, JsonError> {
    let started = Instant::now();
    let requested_diff_fingerprint = req.diff_fingerprint.clone();
    let created_by_run_id = req.created_by_run_id.clone();
    let content = non_empty_string(
        normalize_workspace_review_artifact_content(req.content),
        "content",
    )?;
    let content_bytes = content.len();
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
    let _lifecycle_guard = lock_workspace_review_lifecycle(&conversation_id).await;
    let workspace = load_current_workspace_review_eligible(state.app_state.as_ref(), &workspace)
        .await
        .map_err(workspace_review_action_error)?;
    let context = load_agent_workspace_review_context(state.app_state.as_ref(), &workspace)
        .await
        .map_err(workspace_review_action_error)?;
    let mut monitor = context.monitor;
    let created_by_run_id = validate_workspace_review_tool_run_id(
        &monitor,
        created_by_run_id.as_deref(),
        "workspace Review artifact write",
    )?;
    let target = context.target.as_ref().ok_or_else(|| {
        json_error(
            StatusCode::BAD_REQUEST,
            "workspace Review artifact writes require a current review target",
            None,
        )
    })?;
    let (target_scope, target_head_sha, target_diff_fingerprint) =
        validate_workspace_review_tool_target_metadata(
            target,
            req.target_scope.as_deref(),
            req.head_sha.as_deref(),
            req.diff_fingerprint.as_deref(),
            "workspace Review artifact write",
        )?;

    let previous_artifact = match monitor.review_artifact_id.clone() {
        Some(artifact_id) => {
            let latest_id = state
                .app_state
                .artifact_repo
                .resolve_latest_artifact_id(&artifact_id)
                .await
                .map_err(|error| {
                    json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
                })?;
            state
                .app_state
                .artifact_repo
                .get_by_id(&latest_id)
                .await
                .map_err(|error| {
                    json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
                })?
        }
        None => None,
    };

    let title = workspace_review_artifact_title(
        req.title,
        previous_artifact
            .as_ref()
            .map(|artifact| artifact.name.as_str()),
        monitor.reviewed_target_scope,
        target_scope,
        context.target.as_ref(),
    );
    let previous_artifact_id = previous_artifact
        .as_ref()
        .map(|artifact| artifact.id.as_str().to_string());
    let previous_artifact_entity_id = previous_artifact
        .as_ref()
        .map(|artifact| artifact.id.clone());
    let next_version = previous_artifact
        .as_ref()
        .map(|artifact| artifact.metadata.version.saturating_add(1))
        .unwrap_or(1);
    let mut artifact = Artifact::new_inline(
        title,
        ArtifactType::PrReview,
        content,
        "ralphx-workspace-reviewer",
    );
    artifact.metadata.version = next_version;

    let created = if let Some(previous) = previous_artifact {
        state
            .app_state
            .artifact_repo
            .create_with_previous_version(artifact, previous.id)
            .await
            .map_err(|error| {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
            })?
    } else {
        state
            .app_state
            .artifact_repo
            .create(artifact)
            .await
            .map_err(|error| {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
            })?
    };

    apply_review_artifact_to_monitor(
        &mut monitor,
        target_scope,
        target_head_sha.clone(),
        target_diff_fingerprint.clone(),
        created_by_run_id.clone(),
        created.id.clone(),
        created.metadata.version,
        created.metadata.created_at,
        previous_artifact_entity_id,
    );
    let monitor = state
        .app_state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;

    let content_text = match &created.content {
        crate::domain::entities::ArtifactContent::Inline { text } => text.clone(),
        crate::domain::entities::ArtifactContent::File { path } => format!("[File: {}]", path),
    };
    let event_name = if previous_artifact_id.is_some() {
        "workspace_review_artifact:updated"
    } else {
        "workspace_review_artifact:created"
    };
    crate::http_server::emit_http_event(
        &state,
        event_name,
        serde_json::json!({
            "conversationId": conversation_id.as_str(),
            "targetScope": target_scope.to_string(),
            "headSha": target_head_sha,
            "diffFingerprint": target_diff_fingerprint,
            "previousArtifactId": previous_artifact_id,
            "artifact": {
                "id": created.id.as_str(),
                "name": created.name.clone(),
                "content": content_text,
                "version": created.metadata.version,
            }
        }),
    );

    let mut artifact_response = ArtifactResponse::from(created);
    artifact_response.previous_artifact_id = previous_artifact_id.clone();
    tracing::info!(
        target: "ralphx_lib::http_server::agent_workspaces",
        operation = "workspace_review_artifact_write_http",
        conversation_id = %conversation_id,
        project_id = %workspace.project_id,
        branch = %workspace.branch_name,
        elapsed_ms = started.elapsed().as_millis(),
        target_scope = %target_scope,
        diff_fingerprint = %compact_workspace_review_log_fingerprint(Some(&target_diff_fingerprint)),
        requested_diff_fingerprint = %compact_workspace_review_log_fingerprint(requested_diff_fingerprint.as_deref()),
        artifact_id = %artifact_response.id,
        artifact_version = artifact_response.version,
        previous_artifact_id = %previous_artifact_id.as_deref().unwrap_or("none"),
        created_by_run_id = %created_by_run_id.as_deref().unwrap_or("none"),
        content_bytes,
        monitor_status = %monitor.status,
        "Wrote workspace Review artifact"
    );

    Ok(Json(WriteAgentWorkspaceReviewArtifactResponse {
        success: true,
        monitor: AgentWorkspaceReviewMonitorResponse::from(monitor),
        artifact: artifact_response,
        previous_artifact_id,
    }))
}

/// POST /api/agent-workspaces/{conversation_id}/workspace-review-hunk-annotations
pub async fn write_agent_workspace_review_hunk_annotations(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
    Json(req): Json<WriteAgentWorkspaceReviewHunkAnnotationsRequest>,
) -> Result<Json<WriteAgentWorkspaceReviewHunkAnnotationsResponse>, JsonError> {
    let started = Instant::now();
    let requested_diff_fingerprint = req.diff_fingerprint.clone();
    let created_by_run_id = req.created_by_run_id.clone();
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
    let _lifecycle_guard = lock_workspace_review_lifecycle(&conversation_id).await;
    let workspace = load_current_workspace_review_eligible(state.app_state.as_ref(), &workspace)
        .await
        .map_err(workspace_review_action_error)?;
    let project = state
        .app_state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
        .ok_or_else(|| {
            json_error(
                StatusCode::NOT_FOUND,
                format!("Project not found: {}", workspace.project_id),
                None,
            )
        })?;
    let context = load_agent_workspace_review_context(state.app_state.as_ref(), &workspace)
        .await
        .map_err(workspace_review_action_error)?;

    if !context.is_current {
        return Err(json_error(
            StatusCode::CONFLICT,
            "Write the current workspace Review artifact before writing hunk annotations",
            None,
        ));
    }

    let target = context.target.as_ref().ok_or_else(|| {
        json_error(
            StatusCode::BAD_REQUEST,
            "workspace review hunk annotations require a current review target",
            None,
        )
    })?;
    let monitor = context.monitor;
    let artifact_id = monitor.review_artifact_id.clone().ok_or_else(|| {
        json_error(
            StatusCode::CONFLICT,
            "workspace review hunk annotations require a current Review artifact",
            None,
        )
    })?;
    let artifact_version = monitor.review_artifact_version.ok_or_else(|| {
        json_error(
            StatusCode::CONFLICT,
            "workspace review hunk annotations require a current Review artifact version",
            None,
        )
    })?;
    let created_by_run_id = validate_workspace_review_tool_run_id(
        &monitor,
        created_by_run_id.as_deref(),
        "workspace Review hunk annotations write",
    )?;
    let (target_scope, target_head_sha, target_diff_fingerprint) =
        validate_workspace_review_tool_target_metadata(
            target,
            req.target_scope.as_deref(),
            req.head_sha.as_deref(),
            req.diff_fingerprint.as_deref(),
            "workspace Review hunk annotations write",
        )?;
    let hunk_selections = req
        .annotations
        .iter()
        .map(|annotation| (annotation.path.clone(), annotation.source.clone()))
        .collect::<BTreeSet<_>>();
    let mut validation_target = target.clone();
    let (full_hunk_anchors, source_fingerprint) = full_hunk_anchors_for_requests(
        &workspace,
        &project,
        &target_diff_fingerprint,
        &hunk_selections,
    )
    .await
    .map_err(workspace_review_action_error)?;
    validation_target.review_packet.hunk_anchors = full_hunk_anchors;
    let validation = validate_workspace_review_hunk_annotation_requests(
        req.annotations,
        Some(&validation_target),
        target_scope,
        target_head_sha.as_deref(),
        &target_diff_fingerprint,
    )?;
    let accepted_count = validation.accepted.len();
    let rejected_count = validation.rejected.len();
    let annotation_entities = build_workspace_review_hunk_annotation_entities(
        validation.accepted.clone(),
        WorkspaceReviewHunkAnnotationEntityContext {
            conversation_id: &conversation_id,
            project_id: &workspace.project_id,
            artifact_id: &artifact_id,
            artifact_version,
            target_scope,
            head_sha: target_head_sha.clone(),
            diff_fingerprint: &target_diff_fingerprint,
            created_by_run_id,
        },
    );

    let mut results = validation.rejected;
    results.extend(
        validation
            .accepted
            .iter()
            .zip(annotation_entities.iter())
            .map(|(validated, entity)| {
                accepted_workspace_review_hunk_annotation_result(validated, entity)
            }),
    );
    results.sort_by_key(|result| result.index);

    let existing = state
        .app_state
        .agent_conversation_workspace_repo
        .list_workspace_review_hunk_annotations(&conversation_id, &artifact_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let merged = merge_workspace_review_hunk_annotations(existing, annotation_entities);
    let stored_count = merged.len();
    let missing_required_hunks = missing_workspace_review_hunk_anchors(target, &merged)
        .into_iter()
        .map(AgentWorkspaceReviewHunkAnchorResponse::from)
        .collect::<Vec<_>>();
    let missing_required_count = missing_required_hunks.len();
    ensure_workspace_review_snapshot_current(
        &workspace,
        &project,
        &target_diff_fingerprint,
        &source_fingerprint,
    )
    .await
    .map_err(workspace_review_action_error)?;
    state
        .app_state
        .agent_conversation_workspace_repo
        .replace_workspace_review_hunk_annotations(&conversation_id, &artifact_id, merged)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;

    crate::http_server::emit_http_event(
        &state,
        "workspace_review_artifact:updated",
        serde_json::json!({
            "conversationId": conversation_id.as_str(),
            "targetScope": target_scope.to_string(),
            "headSha": target_head_sha,
            "diffFingerprint": target_diff_fingerprint,
            "artifact": {
                "id": artifact_id.as_str(),
                "version": artifact_version,
                "hunkAnnotationCount": stored_count,
            }
        }),
    );

    tracing::info!(
        target: "ralphx_lib::http_server::agent_workspaces",
        operation = "workspace_review_hunk_annotations_write_http",
        conversation_id = %conversation_id,
        project_id = %workspace.project_id,
        branch = %workspace.branch_name,
        elapsed_ms = started.elapsed().as_millis(),
        target_scope = %target_scope,
        diff_fingerprint = %compact_workspace_review_log_fingerprint(Some(&target_diff_fingerprint)),
        requested_diff_fingerprint = %compact_workspace_review_log_fingerprint(requested_diff_fingerprint.as_deref()),
        artifact_id = %artifact_id,
        artifact_version,
        accepted_count,
        rejected_count,
        stored_count,
        missing_required_count,
        "Wrote workspace Review hunk annotations"
    );

    Ok(Json(WriteAgentWorkspaceReviewHunkAnnotationsResponse {
        success: rejected_count == 0,
        accepted_count,
        rejected_count,
        stored_count,
        missing_required_count,
        artifact_id: artifact_id.as_str().to_string(),
        artifact_version,
        monitor: AgentWorkspaceReviewMonitorResponse::from(monitor),
        results,
        missing_required_hunks,
    }))
}

/// POST /api/agent-workspaces/{conversation_id}/complete-workspace-review-run
pub async fn complete_agent_workspace_review_run(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
    Json(req): Json<CompleteAgentWorkspaceReviewRunRequest>,
) -> Result<Json<CompleteAgentWorkspaceReviewRunResponse>, JsonError> {
    let started = Instant::now();
    let summary = non_empty_string(req.summary, "summary")?;
    let summary_bytes = summary.len();
    let has_outcome = req
        .outcome
        .as_ref()
        .is_some_and(|value| !value.trim().is_empty());
    let has_blocker = req
        .blocker
        .as_ref()
        .is_some_and(|value| !value.trim().is_empty());
    let outcome = req.outcome.clone();
    let blocker = req.blocker.clone();
    let created_by_run_id = req.created_by_run_id.clone();
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
    let lifecycle_guard = lock_workspace_review_lifecycle(&conversation_id).await;
    let workspace = load_current_workspace_review_eligible(state.app_state.as_ref(), &workspace)
        .await
        .map_err(workspace_review_action_error)?;
    let context = load_agent_workspace_review_context(state.app_state.as_ref(), &workspace)
        .await
        .map_err(workspace_review_action_error)?;
    let created_by_run_id = validate_workspace_review_tool_run_id(
        &context.monitor,
        created_by_run_id.as_deref(),
        "workspace Review completion",
    )?;
    ensure_workspace_review_hunk_annotation_coverage_for_completion(
        state.app_state.as_ref(),
        &workspace,
        outcome.as_deref(),
    )
    .await?;
    let monitor = complete_agent_workspace_review_run_unlocked(
        state.app_state.as_ref(),
        &workspace,
        outcome,
        Some(summary),
        blocker,
        created_by_run_id.clone(),
    )
    .await
    .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    tracing::info!(
        target: "ralphx_lib::http_server::agent_workspaces",
        operation = "workspace_review_complete_http",
        conversation_id = %conversation_id,
        project_id = %workspace.project_id,
        branch = %workspace.branch_name,
        elapsed_ms = started.elapsed().as_millis(),
        monitor_status = %monitor.status,
        has_artifact = monitor.review_artifact_id.is_some(),
        artifact_id = %monitor.review_artifact_id.as_ref().map(|id| id.as_str()).unwrap_or("none"),
        created_by_run_id = %created_by_run_id.as_deref().unwrap_or("none"),
        has_outcome,
        has_blocker,
        summary_bytes,
        "Handled workspace Review completion"
    );
    // Publishing takes the same lifecycle lock to serialize against review mutations. The review
    // result is durable now, so release this handler's guard before resuming publication.
    drop(lifecycle_guard);
    settle_workspace_review_publish_authorization(&state, &conversation_id, &workspace, &monitor)
        .await?;
    // R3: on a Blocking/Failed gate for an automation-owned conversation, pause the automation and
    // terminalize the stuck run. Classify by the gate ENUM, never the blocker string. No-op for
    // non-automation conversations (handled inside the helper via the run bridge).
    if matches!(
        monitor.review_gate_status,
        AgentWorkspaceReviewGateStatus::Blocking | AgentWorkspaceReviewGateStatus::Failed
    ) {
        let detail = workspace_review_block_detail(&monitor);
        if let Err(error) =
            crate::application::automation::review_gate::pause_automation_for_blocked_workspace_review(
                state.app_state.as_ref(),
                &conversation_id,
                detail.as_deref(),
            )
            .await
        {
            tracing::warn!(
                target: "ralphx_lib::http_server::agent_workspaces",
                operation = "pause_automation_on_workspace_review_block_failed",
                conversation_id = %conversation_id,
                error = %error,
                "Failed to pause automation after blocked workspace review"
            );
        }
    }
    Ok(Json(CompleteAgentWorkspaceReviewRunResponse {
        success: true,
        monitor: AgentWorkspaceReviewMonitorResponse::from(monitor),
    }))
}

fn truncate_pr_health_issue_comments(health: &mut PrHealth) {
    for comment in &mut health.issue_comments {
        comment.body = pr_comment_body_excerpt(&comment.body, 480);
    }
}

/// GET /api/agent-workspaces/{conversation_id}/pr-comments/{comment_id}
pub async fn read_agent_workspace_pr_comment(
    State(state): State<HttpServerState>,
    Path((conversation_id, comment_id)): Path<(String, String)>,
) -> Result<Json<ReadAgentWorkspacePrCommentResponse>, JsonError> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Agent workspace not found", None))?;
    let target = resolve_agent_workspace_pr_fix_target(state.app_state.as_ref(), &workspace)
        .await?
        .ok_or_else(|| {
            json_error(
                StatusCode::BAD_REQUEST,
                "Agent workspace has no linked pull request",
                None,
            )
        })?;
    let pr_number = target.pr_number;
    let comment = state
        .app_state
        .agent_conversation_workspace_repo
        .get_pr_comment_evidence(&conversation_id, pr_number, &comment_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "PR comment not found", None))?;
    state
        .app_state
        .agent_conversation_workspace_repo
        .mark_pr_comment_read(&conversation_id, pr_number, &comment_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;

    Ok(Json(ReadAgentWorkspacePrCommentResponse {
        success: true,
        conversation_id: conversation_id.as_str(),
        pr_number,
        comment_id: comment.comment_id,
        author: comment.author,
        url: comment.url,
        github_created_at: comment.github_created_at,
        github_updated_at: comment.github_updated_at,
        is_codecov: comment.is_codecov,
        is_bot: comment.is_bot,
        body_length_chars: comment.body.chars().count(),
        body: comment.body,
        body_sha256: comment.body_sha256,
        edit_count: comment.edit_count,
        is_untrusted: true,
    }))
}

/// POST /api/agent-workspaces/{conversation_id}/complete-pr-fix
///
/// Called by the PR fixer agent after it has addressed PR health/review issues.
/// RalphX then republishes the workspace branch and resumes PR supervision.
pub async fn complete_agent_workspace_pr_fix(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
    Json(req): Json<CompleteAgentWorkspacePrFixRequest>,
) -> Result<Json<CompleteAgentWorkspacePrFixResponse>, JsonError> {
    let summary = req.summary.trim();
    if summary.is_empty() {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "summary must describe the PR fix outcome",
            None,
        ));
    }

    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Agent workspace not found", None))?;
    let target = resolve_agent_workspace_pr_fix_target(state.app_state.as_ref(), &workspace)
        .await?
        .ok_or_else(|| {
            json_error(
                StatusCode::BAD_REQUEST,
                "Agent workspace has no linked pull request",
                None,
            )
        })?;

    let authority = match load_pr_autofix_completion_authority(
        state.app_state.agent_run_repo.as_ref(),
        &conversation_id,
        target.pr_number,
        req.created_by_run_id.as_deref(),
    )
    .await
    {
        Ok(authority) => authority,
        Err(error) => {
            schedule_pr_autofix_completion_recovery(&state, &conversation_id);
            return Err(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                error.to_string(),
                None,
            ));
        }
    };
    let claim = match authority {
        PrAutofixCompletionAuthority::Superseded => {
            return Ok(Json(CompleteAgentWorkspacePrFixResponse {
                success: true,
                status: "superseded".to_string(),
                message: "This PR fixer attempt was superseded; RalphX supervision continues automatically."
                    .to_string(),
                workspace: None,
                publish_status: Some("skipped".to_string()),
                publish_error: None,
                commit_sha: None,
                pushed: None,
                created_pr: None,
                pr_number: Some(target.pr_number),
                pr_url: target.pr_url.clone(),
            }));
        }
        PrAutofixCompletionAuthority::AlreadyCompleted => {
            return Ok(Json(CompleteAgentWorkspacePrFixResponse {
                success: true,
                status: "already_completed".to_string(),
                message: "This PR fixer attempt was already settled.".to_string(),
                workspace: None,
                publish_status: Some("skipped".to_string()),
                publish_error: None,
                commit_sha: None,
                pushed: None,
                created_pr: None,
                pr_number: Some(target.pr_number),
                pr_url: target.pr_url.clone(),
            }));
        }
        PrAutofixCompletionAuthority::Invalid => {
            schedule_pr_autofix_completion_recovery(&state, &conversation_id);
            return Err(json_error(
                StatusCode::CONFLICT,
                "Agent workspace PR fix attempt is no longer current",
                None,
            ));
        }
        PrAutofixCompletionAuthority::Current => {
            if workspace.publication_push_status.as_deref() != Some("needs_agent")
                || workspace.pr_supervision_status.as_deref() != Some("fixing")
            {
                schedule_pr_autofix_completion_recovery(&state, &conversation_id);
                return Err(json_error(
                    StatusCode::CONFLICT,
                    "Agent workspace PR fix claim is no longer current",
                    None,
                ));
            }
            AgentWorkspaceRepairClaim {
                conversation_id: conversation_id.clone(),
                guard: AgentWorkspaceRepairStateGuard::from_workspace(&workspace),
            }
        }
    };

    if let Some(blocker) = req
        .blocker
        .as_deref()
        .map(str::trim)
        .filter(|blocker| !blocker.is_empty())
    {
        block_agent_workspace_pr_fix_claim(
            Arc::clone(&state.app_state.agent_conversation_workspace_repo),
            &claim,
            blocker,
        )
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
        .ok_or_else(|| {
            schedule_pr_autofix_completion_recovery(&state, &conversation_id);
            json_error(
                StatusCode::CONFLICT,
                "Agent workspace PR fix attempt was replaced before blocker settlement",
                None,
            )
        })?;
        let workspace =
            load_agent_workspace_response(state.app_state.as_ref(), &conversation_id).await?;
        return Ok(Json(CompleteAgentWorkspacePrFixResponse {
            success: true,
            status: "blocked".to_string(),
            message: blocker.to_string(),
            workspace: Some(workspace),
            publish_status: Some("skipped".to_string()),
            publish_error: None,
            commit_sha: None,
            pushed: None,
            created_pr: None,
            pr_number: Some(target.pr_number),
            pr_url: target.pr_url.clone(),
        }));
    }

    if let Some(github) = state.app_state.github_service.as_ref() {
        match github
            .check_pr_status(&target.working_dir, target.pr_number)
            .await
        {
            Ok(PrStatus::Merged { .. }) => {
                if target.is_ideation_plan() {
                    return complete_ideation_plan_pr_fix_for_terminal_pr(
                        state.app_state.as_ref(),
                        &conversation_id,
                        &workspace,
                        &target,
                        "merged",
                        "Pull request already merged; skipping PR fix publish.",
                    )
                    .await;
                }
                return complete_pr_fix_for_terminal_pr(
                    state.app_state.as_ref(),
                    &conversation_id,
                    &workspace,
                    "merged",
                    "Pull request already merged; skipping PR fix publish.",
                )
                .await;
            }
            Ok(PrStatus::Closed) => {
                if target.is_ideation_plan() {
                    return complete_ideation_plan_pr_fix_for_terminal_pr(
                        state.app_state.as_ref(),
                        &conversation_id,
                        &workspace,
                        &target,
                        "closed",
                        "Pull request already closed; skipping PR fix publish.",
                    )
                    .await;
                }
                return complete_pr_fix_for_terminal_pr(
                    state.app_state.as_ref(),
                    &conversation_id,
                    &workspace,
                    "closed",
                    "Pull request already closed; skipping PR fix publish.",
                )
                .await;
            }
            Ok(PrStatus::Open) => {}
            Err(error) => {
                tracing::warn!(
                    conversation_id = conversation_id.as_str(),
                    pr_number = target.pr_number,
                    error = %error,
                    "complete_agent_workspace_pr_fix: failed to recheck PR status before publish"
                );
                return Err(json_error(
                    StatusCode::BAD_GATEWAY,
                    format!("Failed to verify current pull request status: {error}"),
                    None,
                ));
            }
        }
    }

    let fix_commit_sha = req
        .fix_commit_sha
        .as_deref()
        .map(str::trim)
        .filter(|sha| !sha.is_empty())
        .ok_or_else(|| {
            json_error(
                StatusCode::BAD_REQUEST,
                "fix_commit_sha is required when blocker is not provided",
                None,
            )
        })?;
    if !is_valid_git_sha(fix_commit_sha) {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "fix_commit_sha must be a full 40-character SHA (use `git rev-parse HEAD`)",
            None,
        ));
    }
    let workspace_head_sha = GitService::get_head_sha(&target.working_dir)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let has_uncommitted_changes = GitService::has_uncommitted_changes(&target.working_dir)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let unfinished_operation = GitService::unfinished_operation_state(&target.working_dir)
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let has_conflict_files = !GitService::get_conflict_files(&target.working_dir)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
        .is_empty();
    let has_conflict_markers = GitService::has_conflict_markers(&target.working_dir)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    verify_agent_workspace_settled_current_head(AgentWorkspaceSettledHeadCheck {
        reported_head_sha: fix_commit_sha,
        workspace_head_sha: &workspace_head_sha,
        has_uncommitted_changes,
        is_merge_in_progress: unfinished_operation.merge_in_progress,
        is_rebase_in_progress: unfinished_operation.rebase_in_progress,
        has_conflict_files,
        has_conflict_markers,
    })
    .map_err(|error| json_error(StatusCode::CONFLICT, error, None))?;

    let review_settings = state
        .app_state
        .review_settings_repo
        .get_settings()
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let workspace_review_required = review_settings.require_workspace_review;
    let post_completion_claim = complete_agent_workspace_pr_fix_claim(
        Arc::clone(&state.app_state.agent_conversation_workspace_repo),
        &claim,
        summary,
        workspace_review_required,
        workspace.auto_publish_enabled,
    )
    .await
    .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
    .ok_or_else(|| {
        schedule_pr_autofix_completion_recovery(&state, &conversation_id);
        json_error(
            StatusCode::CONFLICT,
            "Agent workspace PR fix attempt was replaced before completion",
            None,
        )
    })?;
    let settled_workspace =
        load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;

    if workspace_review_required {
        if let Some(response) = settle_pr_fix_workspace_review_handoff(
            &state,
            &conversation_id,
            &settled_workspace,
            &post_completion_claim,
        )
        .await?
        {
            return Ok(response);
        }
    }

    if !workspace.auto_publish_enabled {
        return completed_pr_fix_paused_response(state.app_state.as_ref(), &conversation_id).await;
    }

    if target.is_ideation_plan() {
        return complete_ideation_plan_pr_fix_publish(
            &state,
            &conversation_id,
            &workspace,
            &target,
            summary,
        )
        .await;
    }

    match publish_agent_conversation_workspace_for_app_state(
        state.app_state.as_ref(),
        &state.execution_state,
        conversation_id.clone(),
        false,
    )
    .await
    {
        Ok(result) => {
            state
                .app_state
                .agent_conversation_workspace_repo
                .update_pr_auto_merge_state(
                    &conversation_id,
                    result.workspace.pr_auto_merge_current,
                    Some("monitoring"),
                    Some("PR fix published; RalphX is monitoring the pull request."),
                )
                .await
                .map_err(|error| {
                    json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
                })?;
            let workspace =
                load_agent_workspace_response(state.app_state.as_ref(), &conversation_id).await?;
            Ok(Json(CompleteAgentWorkspacePrFixResponse {
                success: true,
                status: "published".to_string(),
                message: "PR fix published; RalphX is monitoring the pull request.".to_string(),
                workspace: Some(workspace),
                publish_status: Some("succeeded".to_string()),
                publish_error: None,
                commit_sha: result.commit_sha,
                pushed: Some(result.pushed),
                created_pr: Some(result.created_pr),
                pr_number: result.pr_number,
                pr_url: result.pr_url,
            }))
        }
        Err(error) => {
            state
                .app_state
                .agent_conversation_workspace_repo
                .update_pr_auto_merge_state(
                    &conversation_id,
                    workspace.pr_auto_merge_current,
                    Some("blocked"),
                    Some(&format!("PR fix publish failed: {error}")),
                )
                .await
                .map_err(|repo_error| {
                    json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        repo_error.to_string(),
                        None,
                    )
                })?;
            state
                .app_state
                .agent_conversation_workspace_repo
                .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                    conversation_id.clone(),
                    "pr_autofix_publish_failed",
                    "failed",
                    error.clone(),
                    Some("pr_autofix_publish_failed".to_string()),
                ))
                .await
                .map_err(|repo_error| {
                    json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        repo_error.to_string(),
                        None,
                    )
                })?;
            let workspace =
                load_agent_workspace_response(state.app_state.as_ref(), &conversation_id).await?;
            Ok(Json(CompleteAgentWorkspacePrFixResponse {
                success: true,
                status: "publish_failed".to_string(),
                message: format!("PR fix publish failed: {error}"),
                workspace: Some(workspace),
                publish_status: Some("failed".to_string()),
                publish_error: Some(error),
                commit_sha: None,
                pushed: None,
                created_pr: None,
                pr_number: None,
                pr_url: None,
            }))
        }
    }
}

fn schedule_pr_autofix_completion_recovery(
    state: &HttpServerState,
    conversation_id: &ChatConversationId,
) {
    let resumer = Arc::new(AgentWorkspacePrFixReviewPublishCommandResumer {
        app_state: state.app_state.as_ref().clone(),
        execution_state: Arc::clone(&state.execution_state),
    });
    let runtime_app_handle = state.app_state.app_handle.clone();
    let transition_service = Arc::new(
        state.app_state.build_transition_service_for_runtime(
            Arc::clone(&state.execution_state),
            runtime_app_handle.clone(),
        ),
    );
    let chat_service: Arc<dyn ChatService> = Arc::new(state.app_state.build_chat_service_for_runtime(
        Some(Arc::clone(&state.execution_state)),
        runtime_app_handle.clone(),
    ));
    let Some(deps) = build_agent_workspace_pr_supervision_recovery_deps(
        state.app_state.as_ref(),
        runtime_app_handle,
        Some(transition_service),
        Some(chat_service),
        Some(resumer),
    ) else {
        return;
    };
    schedule_agent_workspace_pr_supervision_recovery(
        deps,
        conversation_id.clone(),
        AgentWorkspacePrSupervisionRecoveryTrigger::AgentRunCompleted,
        false,
    );
}

async fn complete_ideation_plan_pr_fix_for_terminal_pr(
    state: &AppState,
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    target: &AgentWorkspacePrFixTarget,
    terminal_status: &str,
    message: &str,
) -> Result<Json<CompleteAgentWorkspacePrFixResponse>, JsonError> {
    let plan_branch = target.plan_branch.as_ref().ok_or_else(|| {
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "PR fix target is missing its linked plan branch",
            None,
        )
    })?;
    let db_status = match terminal_status {
        "merged" => PlanDbPrStatus::Merged,
        _ => PlanDbPrStatus::Closed,
    };
    state
        .plan_branch_repo
        .update_pr_status(&plan_branch.id, db_status)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    clear_terminal_plan_pr_auto_merge_marker(state, plan_branch, terminal_status).await;
    state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "pr_autofix_skipped_terminal",
            "skipped",
            message,
            Some(format!("pr_autofix_skipped_terminal:{terminal_status}")),
        ))
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    state
        .agent_conversation_workspace_repo
        .update_pr_auto_merge_state(
            conversation_id,
            workspace.pr_auto_merge_current,
            None,
            Some(message),
        )
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let workspace = load_agent_workspace_response(state, conversation_id).await?;
    Ok(Json(CompleteAgentWorkspacePrFixResponse {
        success: true,
        status: "skipped_terminal".to_string(),
        message: message.to_string(),
        workspace: Some(workspace),
        publish_status: Some("skipped".to_string()),
        publish_error: None,
        commit_sha: None,
        pushed: None,
        created_pr: None,
        pr_number: Some(target.pr_number),
        pr_url: target.pr_url.clone(),
    }))
}

async fn clear_terminal_plan_pr_auto_merge_marker(
    state: &AppState,
    plan_branch: &PlanBranch,
    pr_status: &str,
) {
    let Some(task_id) = plan_branch.merge_task_id.as_ref() else {
        return;
    };

    let mut task = match state.task_repo.get_by_id(task_id).await {
        Ok(Some(task)) => task,
        Ok(None) => return,
        Err(error) => {
            tracing::warn!(
                task_id = task_id.as_str(),
                pr_status,
                error = %error,
                "complete_agent_workspace_pr_fix: failed to load terminal auto-merge correction marker task"
            );
            return;
        }
    };

    let changed =
        crate::domain::state_machine::transition_handler::clear_github_auto_merge_correction_marker_for_terminal_pr(
            &mut task,
            pr_status,
        );
    if changed {
        if let Err(error) = state.task_repo.update(&task).await {
            tracing::warn!(
                task_id = task_id.as_str(),
                pr_status,
                error = %error,
                "complete_agent_workspace_pr_fix: failed to clear terminal auto-merge correction marker"
            );
        }
    }
}

async fn complete_ideation_plan_pr_fix_publish(
    state: &HttpServerState,
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    target: &AgentWorkspacePrFixTarget,
    summary: &str,
) -> Result<Json<CompleteAgentWorkspacePrFixResponse>, JsonError> {
    let plan_branch = target.plan_branch.as_ref().ok_or_else(|| {
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "PR fix target is missing its linked plan branch",
            None,
        )
    })?;
    if plan_branch.status != crate::domain::entities::PlanBranchStatus::Active {
        return finish_ideation_plan_pr_fix_publish_failed(
            state,
            conversation_id,
            workspace,
            target,
            "Cannot publish a plan branch that is no longer active".to_string(),
            None,
        )
        .await;
    }

    let current_branch = GitService::get_current_branch(&target.working_dir)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    if current_branch != target.branch_name {
        return finish_ideation_plan_pr_fix_publish_failed(
            state,
            conversation_id,
            workspace,
            target,
            format!(
                "PR fix workspace is on branch `{current_branch}`, expected `{}`",
                target.branch_name
            ),
            None,
        )
        .await;
    }
    if GitService::has_uncommitted_changes(&target.working_dir)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
    {
        return finish_ideation_plan_pr_fix_publish_failed(
            state,
            conversation_id,
            workspace,
            target,
            "PR fix has uncommitted changes; commit the focused fix before completing.".to_string(),
            None,
        )
        .await;
    }
    if GitService::has_conflict_markers(&target.working_dir)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
    {
        return finish_ideation_plan_pr_fix_publish_failed(
            state,
            conversation_id,
            workspace,
            target,
            "PR fix workspace still contains conflict markers.".to_string(),
            None,
        )
        .await;
    }

    let commit_sha = GitService::get_head_sha(&target.working_dir)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let Some(github) = state.app_state.github_service.as_ref() else {
        state
            .app_state
            .plan_branch_repo
            .update_pr_push_status(&plan_branch.id, PrPushStatus::Failed)
            .await
            .map_err(|error| {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
            })?;
        return finish_ideation_plan_pr_fix_publish_failed(
            state,
            conversation_id,
            workspace,
            target,
            "GitHub integration is not available".to_string(),
            Some(commit_sha),
        )
        .await;
    };

    state
        .app_state
        .plan_branch_repo
        .update_pr_push_status(&plan_branch.id, PrPushStatus::Pending)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;

    if let Err(error) = push_agent_workspace_publish_branch(
        github,
        &target.working_dir,
        &target.branch_name,
        &state.app_state.agent_conversation_workspace_repo,
        conversation_id,
    )
    .await
    {
        let message = format!("PR fix push failed: {error}");
        state
            .app_state
            .plan_branch_repo
            .update_pr_push_status(&plan_branch.id, PrPushStatus::Failed)
            .await
            .map_err(|repo_error| {
                json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    repo_error.to_string(),
                    None,
                )
            })?;
        return finish_ideation_plan_pr_fix_publish_failed(
            state,
            conversation_id,
            workspace,
            target,
            message,
            Some(commit_sha),
        )
        .await;
    }

    state
        .app_state
        .plan_branch_repo
        .update_pr_push_status(&plan_branch.id, PrPushStatus::Pushed)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    state
        .app_state
        .agent_conversation_workspace_repo
        .update_pr_auto_merge_state(
            conversation_id,
            workspace.pr_auto_merge_current,
            Some("monitoring"),
            Some("PR fix pushed to the linked plan branch; RalphX is monitoring the pull request."),
        )
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    state
        .app_state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "pr_autofix_published",
            "succeeded",
            format!("PR fix pushed to the linked plan branch. Fix summary: {summary}"),
            Some(format!(
                "pr_autofix_published:{}:{commit_sha}",
                target.pr_number
            )),
        ))
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;

    if let Some(task_id) = plan_branch.merge_task_id.as_ref() {
        if let Some(project) = state
            .app_state
            .project_repo
            .get_by_id(&plan_branch.project_id)
            .await
            .map_err(|error| {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
            })?
        {
            let transition_service = state
                .app_state
                .build_transition_service_with_execution_state(Arc::clone(&state.execution_state))
                .into_arc();
            state.app_state.pr_poller_registry.start_polling(
                task_id.clone(),
                plan_branch.id.clone(),
                target.pr_number,
                PathBuf::from(project.working_directory),
                plan_branch.source_branch.clone(),
                transition_service,
            );
        }
    }

    let workspace_response =
        load_agent_workspace_response(state.app_state.as_ref(), conversation_id).await?;
    Ok(Json(CompleteAgentWorkspacePrFixResponse {
        success: true,
        status: "published".to_string(),
        message: "PR fix pushed to the linked plan branch; RalphX is monitoring the pull request."
            .to_string(),
        workspace: Some(workspace_response),
        publish_status: Some("succeeded".to_string()),
        publish_error: None,
        commit_sha: Some(commit_sha),
        pushed: Some(true),
        created_pr: Some(false),
        pr_number: Some(target.pr_number),
        pr_url: target.pr_url.clone(),
    }))
}

async fn finish_ideation_plan_pr_fix_publish_failed(
    state: &HttpServerState,
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    target: &AgentWorkspacePrFixTarget,
    message: String,
    commit_sha: Option<String>,
) -> Result<Json<CompleteAgentWorkspacePrFixResponse>, JsonError> {
    state
        .app_state
        .agent_conversation_workspace_repo
        .update_pr_auto_merge_state(
            conversation_id,
            workspace.pr_auto_merge_current,
            Some("blocked"),
            Some(&message),
        )
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    state
        .app_state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "pr_autofix_publish_failed",
            "failed",
            message.clone(),
            Some(format!("pr_autofix_publish_failed:{}", target.pr_number)),
        ))
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let workspace_response =
        load_agent_workspace_response(state.app_state.as_ref(), conversation_id).await?;
    Ok(Json(CompleteAgentWorkspacePrFixResponse {
        success: true,
        status: "publish_failed".to_string(),
        message: message.clone(),
        workspace: Some(workspace_response),
        publish_status: Some("failed".to_string()),
        publish_error: Some(message),
        commit_sha,
        pushed: Some(false),
        created_pr: Some(false),
        pr_number: Some(target.pr_number),
        pr_url: target.pr_url.clone(),
    }))
}

#[cfg(test)]
async fn start_workspace_review_for_pr_fix_if_required(
    state: &HttpServerState,
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    summary: &str,
) -> Result<Option<Json<CompleteAgentWorkspacePrFixResponse>>, JsonError> {
    match workspace_review_action_after_fix_if_required(state, workspace).await? {
        WorkspaceReviewAfterFixAction::Continue => Ok(None),
        WorkspaceReviewAfterFixAction::Waiting { started } => {
            let status = if started {
                "workspace_review_started"
            } else {
                "workspace_reviewing"
            };
            let message = if started {
                "PR fix completed; Workspace Review started before publishing resumes."
            } else {
                "PR fix completed; Workspace Review is already running before publishing resumes."
            };
            finish_pr_fix_waiting_for_workspace_review(
                state,
                conversation_id,
                workspace,
                message,
                summary,
                status,
            )
            .await
            .map(Some)
        }
        WorkspaceReviewAfterFixAction::Blocked {
            blocker,
            classification,
        } => finish_pr_fix_blocked_by_workspace_review(
            state,
            conversation_id,
            workspace,
            &blocker,
            summary,
            classification,
        )
        .await
        .map(Some),
    }
}

async fn settle_pr_fix_workspace_review_handoff(
    state: &HttpServerState,
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    handoff_claim: &AgentWorkspaceRepairClaim,
) -> Result<Option<Json<CompleteAgentWorkspacePrFixResponse>>, JsonError> {
    let action = match workspace_review_action_after_fix_if_required(state, workspace).await {
        Ok(action) => action,
        Err(error) => {
            let blocker = "PR fix was verified, but Workspace Review could not start. Settle or abort any Git operation, then retry Review.";
            abort_agent_workspace_pr_fix_review_handoff(
                Arc::clone(&state.app_state.agent_conversation_workspace_repo),
                handoff_claim,
                blocker,
            )
            .await
            .map_err(|repo_error| {
                json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    repo_error.to_string(),
                    None,
                )
            })?
            .ok_or_else(|| {
                schedule_pr_autofix_completion_recovery(state, conversation_id);
                json_error(
                    StatusCode::CONFLICT,
                    "Workspace Review handoff was replaced while start failure was settling",
                    None,
                )
            })?;
            tracing::warn!(
                conversation_id = conversation_id.as_str(),
                "Workspace Review start failed after verified PR fix; handoff was aborted"
            );
            let response = pr_fix_workspace_review_blocked_response(
                state.app_state.as_ref(),
                conversation_id,
                blocker,
                "workspace_review_failed",
            )
            .await?;
            let _ = error;
            return Ok(Some(response));
        }
    };

    match action {
        WorkspaceReviewAfterFixAction::Continue => {
            continue_agent_workspace_pr_fix_after_review_handoff(
                Arc::clone(&state.app_state.agent_conversation_workspace_repo),
                handoff_claim,
                "Workspace Review found no reviewable changes; PR fix publishing may continue.",
            )
            .await
            .map_err(|error| {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
            })?
            .ok_or_else(|| {
                schedule_pr_autofix_completion_recovery(state, conversation_id);
                json_error(
                    StatusCode::CONFLICT,
                    "Workspace Review handoff was replaced before publish continuation",
                    None,
                )
            })?;
            Ok(None)
        }
        WorkspaceReviewAfterFixAction::Waiting { started } => {
            let status = if started {
                "workspace_review_started"
            } else {
                "workspace_reviewing"
            };
            let message = if started {
                "PR fix completed; Workspace Review started before publishing resumes."
            } else {
                "PR fix completed; Workspace Review is already running before publishing resumes."
            };
            pr_fix_workspace_review_waiting_response(
                state.app_state.as_ref(),
                conversation_id,
                message,
                status,
            )
            .await
            .map(Some)
        }
        WorkspaceReviewAfterFixAction::Blocked {
            blocker,
            classification,
        } => {
            abort_agent_workspace_pr_fix_review_handoff(
                Arc::clone(&state.app_state.agent_conversation_workspace_repo),
                handoff_claim,
                &blocker,
            )
            .await
            .map_err(|error| {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
            })?
            .ok_or_else(|| {
                schedule_pr_autofix_completion_recovery(state, conversation_id);
                json_error(
                    StatusCode::CONFLICT,
                    "Workspace Review handoff was replaced before blocker settlement",
                    None,
                )
            })?;
            pr_fix_workspace_review_blocked_response(
                state.app_state.as_ref(),
                conversation_id,
                &blocker,
                classification,
            )
            .await
            .map(Some)
        }
    }
}

async fn pr_fix_workspace_review_waiting_response(
    state: &AppState,
    conversation_id: &ChatConversationId,
    message: &str,
    classification: &str,
) -> Result<Json<CompleteAgentWorkspacePrFixResponse>, JsonError> {
    let workspace = load_agent_workspace_response(state, conversation_id).await?;
    let pr_number = workspace.publication_pr_number;
    let pr_url = workspace.publication_pr_url.clone();
    Ok(Json(CompleteAgentWorkspacePrFixResponse {
        success: true,
        status: classification.to_string(),
        message: message.to_string(),
        workspace: Some(workspace),
        publish_status: Some("waiting_for_workspace_review".to_string()),
        publish_error: None,
        commit_sha: None,
        pushed: None,
        created_pr: None,
        pr_number,
        pr_url,
    }))
}

async fn pr_fix_workspace_review_blocked_response(
    state: &AppState,
    conversation_id: &ChatConversationId,
    message: &str,
    classification: &str,
) -> Result<Json<CompleteAgentWorkspacePrFixResponse>, JsonError> {
    let workspace = load_agent_workspace_response(state, conversation_id).await?;
    let pr_number = workspace.publication_pr_number;
    let pr_url = workspace.publication_pr_url.clone();
    Ok(Json(CompleteAgentWorkspacePrFixResponse {
        success: true,
        status: classification.to_string(),
        message: message.to_string(),
        workspace: Some(workspace),
        publish_status: Some("blocked_by_workspace_review".to_string()),
        publish_error: Some(message.to_string()),
        commit_sha: None,
        pushed: None,
        created_pr: None,
        pr_number,
        pr_url,
    }))
}

enum WorkspaceReviewAfterFixAction {
    Continue,
    Waiting {
        started: bool,
    },
    Blocked {
        blocker: String,
        classification: &'static str,
    },
}

type WorkspaceReviewStartFuture<'a> =
    Pin<Box<dyn Future<Output = crate::error::AppResult<AgentWorkspaceReviewStart>> + Send + 'a>>;

trait WorkspaceReviewStarter {
    fn start<'a>(
        &'a self,
        state: Arc<AppState>,
        workspace: &'a AgentConversationWorkspace,
        force: bool,
    ) -> WorkspaceReviewStartFuture<'a>;
}

struct DefaultWorkspaceReviewStarter;

impl WorkspaceReviewStarter for DefaultWorkspaceReviewStarter {
    fn start<'a>(
        &'a self,
        state: Arc<AppState>,
        workspace: &'a AgentConversationWorkspace,
        force: bool,
    ) -> WorkspaceReviewStartFuture<'a> {
        Box::pin(start_guarded_agent_workspace_review(
            state,
            workspace,
            force,
            WorkspaceReviewStartOrigin::Automated,
            None,
        ))
    }
}

async fn workspace_review_action_after_fix_if_required(
    state: &HttpServerState,
    workspace: &AgentConversationWorkspace,
) -> Result<WorkspaceReviewAfterFixAction, JsonError> {
    workspace_review_action_after_fix_if_required_with_starter(
        state,
        workspace,
        &DefaultWorkspaceReviewStarter,
    )
    .await
}

async fn workspace_review_action_after_fix_if_required_with_starter<S>(
    state: &HttpServerState,
    workspace: &AgentConversationWorkspace,
    starter: &S,
) -> Result<WorkspaceReviewAfterFixAction, JsonError>
where
    S: WorkspaceReviewStarter + ?Sized,
{
    let review_settings = state
        .app_state
        .review_settings_repo
        .get_settings()
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    if !review_settings.require_workspace_review {
        return Ok(WorkspaceReviewAfterFixAction::Continue);
    }

    let review_context = load_agent_workspace_review_context(state.app_state.as_ref(), workspace)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    match review_context.monitor.review_gate_status {
        AgentWorkspaceReviewGateStatus::Required => {
            let start = starter
                .start(Arc::clone(&state.app_state), workspace, false)
                .await
                .map_err(|error| {
                    json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
                })?;
            match start.context.monitor.review_gate_status {
                AgentWorkspaceReviewGateStatus::NotRequired
                | AgentWorkspaceReviewGateStatus::Passed => {
                    Ok(WorkspaceReviewAfterFixAction::Continue)
                }
                AgentWorkspaceReviewGateStatus::Reviewing
                | AgentWorkspaceReviewGateStatus::Required => {
                    Ok(WorkspaceReviewAfterFixAction::Waiting {
                        started: start.started,
                    })
                }
                AgentWorkspaceReviewGateStatus::Blocking
                | AgentWorkspaceReviewGateStatus::Failed => {
                    let classification = pr_fix_workspace_review_block_classification(
                        start.context.monitor.review_gate_status,
                    );
                    let blocker = review_gate_publish_blocker(&start.context)
                        .unwrap_or_else(|| "Workspace Review blocks publishing".to_string());
                    Ok(WorkspaceReviewAfterFixAction::Blocked {
                        blocker,
                        classification,
                    })
                }
            }
        }
        AgentWorkspaceReviewGateStatus::Reviewing => {
            Ok(WorkspaceReviewAfterFixAction::Waiting { started: false })
        }
        AgentWorkspaceReviewGateStatus::NotRequired | AgentWorkspaceReviewGateStatus::Passed => {
            Ok(WorkspaceReviewAfterFixAction::Continue)
        }
        AgentWorkspaceReviewGateStatus::Blocking | AgentWorkspaceReviewGateStatus::Failed => {
            let classification = pr_fix_workspace_review_block_classification(
                review_context.monitor.review_gate_status,
            );
            let blocker = review_gate_publish_blocker(&review_context)
                .unwrap_or_else(|| "Workspace Review blocks publishing".to_string());
            Ok(WorkspaceReviewAfterFixAction::Blocked {
                blocker,
                classification,
            })
        }
    }
}

#[cfg(test)]
async fn finish_pr_fix_waiting_for_workspace_review(
    state: &HttpServerState,
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    message: &str,
    summary: &str,
    classification: &str,
) -> Result<Json<CompleteAgentWorkspacePrFixResponse>, JsonError> {
    state
        .app_state
        .agent_conversation_workspace_repo
        .update_pr_auto_merge_state(
            conversation_id,
            workspace.pr_auto_merge_current,
            Some("reviewing"),
            Some(message),
        )
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    state
        .app_state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "pr_autofix_workspace_review",
            "reviewing",
            format!("{message} Fix summary: {summary}"),
            Some(classification.to_string()),
        ))
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let workspace_response =
        load_agent_workspace_response(state.app_state.as_ref(), conversation_id).await?;
    let pr_number = workspace_response.publication_pr_number;
    let pr_url = workspace_response.publication_pr_url.clone();
    Ok(Json(CompleteAgentWorkspacePrFixResponse {
        success: true,
        status: classification.to_string(),
        message: message.to_string(),
        workspace: Some(workspace_response),
        publish_status: Some("waiting_for_workspace_review".to_string()),
        publish_error: None,
        commit_sha: None,
        pushed: None,
        created_pr: None,
        pr_number,
        pr_url,
    }))
}

fn pr_fix_workspace_review_block_classification(
    status: AgentWorkspaceReviewGateStatus,
) -> &'static str {
    match status {
        AgentWorkspaceReviewGateStatus::Failed => "workspace_review_failed",
        _ => "workspace_review_blocked",
    }
}

async fn complete_repair_workspace_review_response_if_required(
    state: &HttpServerState,
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    base_commit: &str,
    repair_commit_sha: &str,
    summary: &str,
) -> Result<Option<Json<CompleteAgentWorkspaceRepairResponse>>, JsonError> {
    complete_repair_workspace_review_response_if_required_with_starter(
        state,
        conversation_id,
        workspace,
        base_commit,
        repair_commit_sha,
        summary,
        &DefaultWorkspaceReviewStarter,
    )
    .await
}

async fn complete_repair_workspace_review_response_if_required_with_starter<S>(
    state: &HttpServerState,
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    base_commit: &str,
    repair_commit_sha: &str,
    summary: &str,
    starter: &S,
) -> Result<Option<Json<CompleteAgentWorkspaceRepairResponse>>, JsonError>
where
    S: WorkspaceReviewStarter + ?Sized,
{
    let Some(existing_monitor) = state
        .app_state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
    else {
        return Ok(None);
    };
    if !workspace_repair_was_routed_from_workspace_review(&existing_monitor) {
        return Ok(None);
    }

    match workspace_review_action_after_fix_if_required_with_starter(state, workspace, starter)
        .await?
    {
        WorkspaceReviewAfterFixAction::Continue => Ok(None),
        WorkspaceReviewAfterFixAction::Waiting { started } => {
            let message = if started {
                "Agent workspace repair verified; Workspace Review started before publishing resumes."
            } else {
                "Agent workspace repair verified; Workspace Review is already running before publishing resumes."
            };
            let classification = if started {
                "workspace_review_started"
            } else {
                "workspace_reviewing"
            };
            finish_repair_waiting_for_workspace_review(
                state,
                conversation_id,
                workspace,
                message,
                summary,
                base_commit,
                repair_commit_sha,
                classification,
            )
            .await
            .map(Some)
        }
        WorkspaceReviewAfterFixAction::Blocked {
            blocker,
            classification,
        } => {
            let message = format!(
                "Agent workspace repair verified; Workspace Review blocks publishing: {blocker}"
            );
            finish_repair_blocked_by_workspace_review(
                state,
                conversation_id,
                workspace,
                &message,
                summary,
                base_commit,
                repair_commit_sha,
                &blocker,
                classification,
            )
            .await
            .map(Some)
        }
    }
}

fn workspace_repair_was_routed_from_workspace_review(
    monitor: &AgentWorkspaceReviewMonitor,
) -> bool {
    monitor.review_fixer_status.is_some()
        || monitor.review_fixer_run_id.is_some()
        || monitor.review_fixer_conversation_id.is_some()
}

async fn repair_workspace_review_response(
    state: &HttpServerState,
    conversation_id: &ChatConversationId,
    message: &str,
    base_commit: &str,
    repair_commit_sha: &str,
    auto_publish_status: &str,
    auto_publish_error: Option<String>,
) -> Result<Json<CompleteAgentWorkspaceRepairResponse>, JsonError> {
    let workspace_response =
        load_agent_workspace_response(state.app_state.as_ref(), conversation_id).await?;
    let pr_number = workspace_response.publication_pr_number;
    let pr_url = workspace_response.publication_pr_url.clone();
    Ok(Json(CompleteAgentWorkspaceRepairResponse {
        success: true,
        message: message.to_string(),
        new_status: "refreshed".to_string(),
        base_commit: base_commit.to_string(),
        repair_commit_sha: repair_commit_sha.to_string(),
        auto_publish_status: Some(auto_publish_status.to_string()),
        auto_publish_error,
        pr_number,
        pr_url,
    }))
}

async fn finish_repair_waiting_for_workspace_review(
    state: &HttpServerState,
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    message: &str,
    summary: &str,
    base_commit: &str,
    repair_commit_sha: &str,
    classification: &str,
) -> Result<Json<CompleteAgentWorkspaceRepairResponse>, JsonError> {
    state
        .app_state
        .agent_conversation_workspace_repo
        .update_pr_auto_merge_state(
            conversation_id,
            workspace.pr_auto_merge_current,
            Some("reviewing"),
            Some(message),
        )
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    state
        .app_state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "repair_workspace_review",
            "reviewing",
            format!("{message} Repair summary: {summary}"),
            Some(classification.to_string()),
        ))
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    repair_workspace_review_response(
        state,
        conversation_id,
        message,
        base_commit,
        repair_commit_sha,
        "waiting_for_workspace_review",
        None,
    )
    .await
}

async fn finish_repair_blocked_by_workspace_review(
    state: &HttpServerState,
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    message: &str,
    summary: &str,
    base_commit: &str,
    repair_commit_sha: &str,
    blocker: &str,
    classification: &str,
) -> Result<Json<CompleteAgentWorkspaceRepairResponse>, JsonError> {
    state
        .app_state
        .agent_conversation_workspace_repo
        .update_pr_auto_merge_state(
            conversation_id,
            workspace.pr_auto_merge_current,
            Some("blocked"),
            Some(message),
        )
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    state
        .app_state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "repair_workspace_review",
            "blocked",
            format!("{message} Repair summary: {summary}"),
            Some(classification.to_string()),
        ))
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    repair_workspace_review_response(
        state,
        conversation_id,
        message,
        base_commit,
        repair_commit_sha,
        "blocked_by_workspace_review",
        Some(blocker.to_string()),
    )
    .await
}

#[cfg(test)]
async fn finish_pr_fix_blocked_by_workspace_review(
    state: &HttpServerState,
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    message: &str,
    summary: &str,
    classification: &str,
) -> Result<Json<CompleteAgentWorkspacePrFixResponse>, JsonError> {
    state
        .app_state
        .agent_conversation_workspace_repo
        .update_pr_auto_merge_state(
            conversation_id,
            workspace.pr_auto_merge_current,
            Some("blocked"),
            Some(message),
        )
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    state
        .app_state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "pr_autofix_workspace_review",
            "blocked",
            format!("{message} Fix summary: {summary}"),
            Some(classification.to_string()),
        ))
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let workspace_response =
        load_agent_workspace_response(state.app_state.as_ref(), conversation_id).await?;
    let pr_number = workspace_response.publication_pr_number;
    let pr_url = workspace_response.publication_pr_url.clone();
    Ok(Json(CompleteAgentWorkspacePrFixResponse {
        success: true,
        status: classification.to_string(),
        message: message.to_string(),
        workspace: Some(workspace_response),
        publish_status: Some("blocked_by_workspace_review".to_string()),
        publish_error: Some(message.to_string()),
        commit_sha: None,
        pushed: None,
        created_pr: None,
        pr_number,
        pr_url,
    }))
}

async fn resume_pr_fix_publish_after_workspace_review(
    state: &HttpServerState,
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    _monitor: &AgentWorkspaceReviewMonitor,
) -> Result<(), JsonError> {
    let review_context = load_agent_workspace_review_context(state.app_state.as_ref(), workspace)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let app_state = Arc::clone(&state.app_state);
    let execution_state = Arc::clone(&state.execution_state);
    resume_pr_fix_publish_after_passed_workspace_review(
        Arc::clone(&state.app_state.agent_conversation_workspace_repo),
        conversation_id,
        workspace,
        &review_context.monitor,
        review_context.target.as_ref(),
        move |conversation_id| {
            let app_state = Arc::clone(&app_state);
            let execution_state = Arc::clone(&execution_state);
            async move {
                publish_agent_conversation_workspace_for_app_state(
                    app_state.as_ref(),
                    &execution_state,
                    conversation_id,
                    false,
                )
                .await
                .map(|result| result.workspace.pr_auto_merge_current)
            }
        },
    )
    .await
    .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;

    Ok(())
}

/// R2: resume the INITIAL automation/armed publish once the workspace review passes.
///
/// Gated on the armed *initial* auto-publish flag (`auto_publish_initial_pr_enabled`, distinct from
/// `auto_publish_enabled` which governs the PR-fix/update path), no existing publication PR, no
/// terminal publication status, and a `Passed` gate. This is the missing counterpart to the PR-fix
/// resume for workspaces that have no PR yet — without it an initial automation publish stalls
/// because auto-publish fired (and skipped) on the same completion event while the gate was still
/// `Required`.
async fn resume_initial_auto_publish_after_workspace_review(
    state: &HttpServerState,
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    monitor: &AgentWorkspaceReviewMonitor,
) -> Result<(), JsonError> {
    let review_context = load_agent_workspace_review_context(state.app_state.as_ref(), workspace)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let Some(authorization_kind) = review_context
        .target
        .as_ref()
        .and_then(|target| workspace_review_authorization_kind(monitor, target))
    else {
        return Ok(());
    };
    if !auto_publish_can_resume_after_workspace_review(workspace, monitor) {
        return Ok(());
    }

    let publishing_message = authorization_kind.publishing_message("initial pull request");
    state
        .app_state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "initial_auto_publish_workspace_review_passed",
            "publishing",
            publishing_message,
            Some(authorization_kind.classification()),
        ))
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;

    match publish_agent_conversation_workspace_for_app_state(
        state.app_state.as_ref(),
        &state.execution_state,
        conversation_id.clone(),
        false,
    )
    .await
    {
        Ok(_) => Ok(()),
        // R5: a concurrent publish already holds the in-flight guard — treat as a soft no-op, not a
        // failure. The in-flight guard + PR-exists short-circuit make double-publish impossible.
        Err(error) if error == AGENT_WORKSPACE_PUBLISH_IN_PROGRESS_MESSAGE => {
            tracing::debug!(
                target: "ralphx_lib::http_server::agent_workspaces",
                operation = "initial_auto_publish_in_progress_noop",
                conversation_id = %conversation_id,
                "Initial auto-publish resume no-op: publish already in progress"
            );
            Ok(())
        }
        Err(error) => {
            state
                .app_state
                .agent_conversation_workspace_repo
                .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                    conversation_id.clone(),
                    "initial_auto_publish_failed",
                    "failed",
                    error,
                    Some("initial_auto_publish_failed".to_string()),
                ))
                .await
                .map_err(|repo_error| {
                    json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        repo_error.to_string(),
                        None,
                    )
                })?;
            Ok(())
        }
    }
}

pub(crate) async fn settle_workspace_review_publish_authorization(
    state: &HttpServerState,
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    monitor: &AgentWorkspaceReviewMonitor,
) -> Result<(), JsonError> {
    resume_pr_fix_publish_after_workspace_review(state, conversation_id, workspace, monitor)
        .await?;
    resume_initial_auto_publish_after_workspace_review(state, conversation_id, workspace, monitor)
        .await
}

fn auto_publish_can_resume_after_workspace_review(
    workspace: &AgentConversationWorkspace,
    monitor: &AgentWorkspaceReviewMonitor,
) -> bool {
    workspace_review_mode_is_eligible(workspace.mode)
        && monitor.review_gate_status == AgentWorkspaceReviewGateStatus::Passed
        && workspace.auto_publish_initial_pr_enabled
        && workspace.publication_pr_number.is_none()
        && !workspace.has_terminal_publication_pr_status()
}

/// R3: build the pause detail from the gate ENUM-derived monitor fields (never the raw blocker
/// string as a classifier). Blocking/Failed carry arbitrary reviewer text used only as detail here.
fn workspace_review_block_detail(monitor: &AgentWorkspaceReviewMonitor) -> Option<String> {
    let artifact = monitor.review_artifact_id.as_ref().map(|id| id.as_str());
    let summary = monitor
        .review_blocking_summary
        .as_deref()
        .or(monitor.last_error.as_deref());
    Some(match (artifact, summary) {
        (Some(artifact), Some(summary)) => {
            format!("Workspace review blocked (artifact {artifact}): {summary}")
        }
        (Some(artifact), None) => format!("Workspace review blocked (artifact {artifact})"),
        (None, Some(summary)) => format!("Workspace review blocked: {summary}"),
        (None, None) => "Workspace review blocked".to_string(),
    })
}

#[cfg(test)]
mod tests;

async fn complete_pr_fix_for_terminal_pr(
    state: &AppState,
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    terminal_status: &str,
    message: &str,
) -> Result<Json<CompleteAgentWorkspacePrFixResponse>, JsonError> {
    state
        .agent_conversation_workspace_repo
        .update_publication(
            conversation_id,
            workspace.publication_pr_number,
            workspace.publication_pr_url.as_deref(),
            Some(terminal_status),
            workspace.publication_push_status.as_deref(),
        )
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "pr_autofix_skipped_terminal",
            "skipped",
            message,
            Some(format!("pr_autofix_skipped_terminal:{terminal_status}")),
        ))
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let workspace = load_agent_workspace_response(state, conversation_id).await?;
    let pr_number = workspace.publication_pr_number;
    let pr_url = workspace.publication_pr_url.clone();
    Ok(Json(CompleteAgentWorkspacePrFixResponse {
        success: true,
        status: "skipped_terminal".to_string(),
        message: message.to_string(),
        workspace: Some(workspace),
        publish_status: Some("skipped".to_string()),
        publish_error: None,
        commit_sha: None,
        pushed: None,
        created_pr: None,
        pr_number,
        pr_url,
    }))
}

async fn completed_pr_fix_paused_response(
    state: &AppState,
    conversation_id: &ChatConversationId,
) -> Result<Json<CompleteAgentWorkspacePrFixResponse>, JsonError> {
    let message = "PR fix completed, but Auto Publish is paused. Manual Commit & Publish is required to update the pull request.";
    let workspace = load_agent_workspace_response(state, conversation_id).await?;
    let pr_number = workspace.publication_pr_number;
    let pr_url = workspace.publication_pr_url.clone();
    Ok(Json(CompleteAgentWorkspacePrFixResponse {
        success: true,
        status: "publish_paused".to_string(),
        message: message.to_string(),
        workspace: Some(workspace),
        publish_status: Some("skipped".to_string()),
        publish_error: None,
        commit_sha: None,
        pushed: None,
        created_pr: None,
        pr_number,
        pr_url,
    }))
}

async fn load_agent_workspace_response(
    state: &AppState,
    conversation_id: &ChatConversationId,
) -> Result<AgentConversationWorkspaceResponse, JsonError> {
    let workspace = load_agent_workspace_entity(state, conversation_id).await?;
    agent_workspace_response_for_state(state, workspace)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error, None))
}

async fn load_agent_workspace_entity(
    state: &AppState,
    conversation_id: &ChatConversationId,
) -> Result<AgentConversationWorkspace, JsonError> {
    state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Agent workspace not found", None))
}

async fn resolve_agent_workspace_pr_fix_target(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> Result<Option<AgentWorkspacePrFixTarget>, JsonError> {
    if !workspace.allows_owned_pr_mutation() {
        let message = if workspace.mode == AgentConversationWorkspaceMode::ReviewPr {
            "PR fixer workflows are unavailable in Review PR mode"
        } else {
            "PR fixer workflows are unavailable for this workspace"
        };
        return Err(json_error(StatusCode::BAD_REQUEST, message, None));
    }

    if workspace.mode == AgentConversationWorkspaceMode::Ideation {
        let Some(plan_branch_id) = workspace.linked_plan_branch_id.as_ref() else {
            return Ok(None);
        };
        let plan_branch = state
            .plan_branch_repo
            .get_by_id(plan_branch_id)
            .await
            .map_err(|error| {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
            })?
            .ok_or_else(|| {
                json_error(
                    StatusCode::NOT_FOUND,
                    format!("Linked plan branch not found: {plan_branch_id}"),
                    None,
                )
            })?;
        let Some(pr_number) = plan_branch.pr_number else {
            return Ok(None);
        };
        let project = state
            .project_repo
            .get_by_id(&workspace.project_id)
            .await
            .map_err(|error| {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
            })?
            .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Project not found", None))?;
        let working_dir =
            crate::application::agent_conversation_workspace::ensure_linked_plan_branch_agent_worktree(
                &project,
                &plan_branch,
            )
            .await
            .map_err(|error| json_error(StatusCode::CONFLICT, error.to_string(), None))?;
        return Ok(Some(AgentWorkspacePrFixTarget {
            kind: AgentWorkspacePrFixTargetKind::IdeationPlan,
            pr_number,
            pr_url: plan_branch.pr_url.clone(),
            working_dir,
            branch_name: plan_branch.branch_name.clone(),
            base_branch: plan_branch.source_branch.clone(),
            plan_branch: Some(plan_branch),
        }));
    }

    let Some(pr_number) = workspace.publication_pr_number else {
        return Ok(None);
    };
    Ok(Some(AgentWorkspacePrFixTarget {
        kind: AgentWorkspacePrFixTargetKind::DirectWorkspace,
        pr_number,
        pr_url: workspace.publication_pr_url.clone(),
        working_dir: PathBuf::from(&workspace.worktree_path),
        branch_name: workspace.branch_name.clone(),
        base_branch: workspace.base_ref.clone(),
        plan_branch: None,
    }))
}

async fn load_agent_workspace_publication_events(
    state: &AppState,
    conversation_id: &ChatConversationId,
) -> Result<Vec<AgentConversationWorkspacePublicationEventResponse>, JsonError> {
    state
        .agent_conversation_workspace_repo
        .list_publication_events(conversation_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))
        .map(|events| {
            events
                .into_iter()
                .map(AgentConversationWorkspacePublicationEventResponse::from)
                .collect()
        })
}

async fn load_agent_workspace_pr_comment_evidence(
    state: &AppState,
    conversation_id: &ChatConversationId,
    pr_number: i64,
) -> Result<Vec<AgentWorkspacePrCommentEvidenceResponse>, JsonError> {
    let comments = state
        .agent_conversation_workspace_repo
        .list_pr_comment_evidence(conversation_id, pr_number, 20)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let comment_ids = comments
        .iter()
        .map(|comment| comment.comment_id.clone())
        .collect::<Vec<_>>();
    state
        .agent_conversation_workspace_repo
        .mark_pr_comments_included(conversation_id, pr_number, &comment_ids)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    Ok(comments
        .into_iter()
        .map(AgentWorkspacePrCommentEvidenceResponse::from_evidence)
        .collect())
}

fn parse_workspace_review_target_scope(
    value: Option<&str>,
) -> Option<AgentWorkspaceReviewTargetScope> {
    value.and_then(|value| AgentWorkspaceReviewTargetScope::from_str(value.trim()).ok())
}

fn validate_workspace_review_tool_run_id(
    monitor: &AgentWorkspaceReviewMonitor,
    created_by_run_id: Option<&str>,
    operation: &str,
) -> Result<Option<String>, JsonError> {
    let created_by_run_id = created_by_run_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    crate::application::agent_workspace_review::ensure_workspace_review_run_is_active(
        monitor,
        created_by_run_id.as_deref(),
        operation,
    )
    .map_err(|error| json_error(StatusCode::CONFLICT, error.to_string(), None))?;
    Ok(created_by_run_id)
}

fn validate_workspace_review_tool_target_metadata(
    target: &AgentWorkspaceReviewTarget,
    target_scope: Option<&str>,
    head_sha: Option<&str>,
    diff_fingerprint: Option<&str>,
    operation: &str,
) -> Result<(AgentWorkspaceReviewTargetScope, Option<String>, String), JsonError> {
    let target_scope = parse_workspace_review_target_scope(target_scope).ok_or_else(|| {
        json_error(
            StatusCode::BAD_REQUEST,
            format!("{operation} requires target_scope from get_workspace_review_context"),
            None,
        )
    })?;
    let head_sha = head_sha
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let diff_fingerprint = diff_fingerprint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            json_error(
                StatusCode::BAD_REQUEST,
                format!("{operation} requires diff_fingerprint from get_workspace_review_context"),
                None,
            )
        })?;

    if target.scope != target_scope
        || target.head_sha.as_deref() != head_sha.as_deref()
        || target.diff_fingerprint != diff_fingerprint
    {
        return Err(json_error(
            StatusCode::CONFLICT,
            format!(
                "{operation} target metadata does not match the current workspace Review target"
            ),
            None,
        ));
    }

    Ok((target_scope, head_sha, diff_fingerprint))
}

const WORKSPACE_REVIEW_MAX_HUNK_ANNOTATIONS: usize = 600;
const WORKSPACE_REVIEW_HUNK_ANNOTATION_MAX_PATH_CHARS: usize = 512;
const WORKSPACE_REVIEW_HUNK_ANNOTATION_MAX_SOURCE_CHARS: usize = 64;
const WORKSPACE_REVIEW_HUNK_ANNOTATION_MAX_HEADER_CHARS: usize = 300;
const WORKSPACE_REVIEW_HUNK_ANNOTATION_MAX_TITLE_CHARS: usize = 160;
const WORKSPACE_REVIEW_HUNK_ANNOTATION_MAX_MESSAGE_CHARS: usize = 1200;

#[derive(Debug, Clone)]
struct ValidatedWorkspaceReviewHunkAnnotation {
    index: usize,
    path: String,
    source: String,
    hunk_header: String,
    old_start: u32,
    old_lines: u32,
    new_start: u32,
    new_lines: u32,
    title: Option<String>,
    message: String,
    level: String,
}

#[derive(Debug, Default)]
struct WorkspaceReviewHunkAnnotationValidation {
    accepted: Vec<ValidatedWorkspaceReviewHunkAnnotation>,
    rejected: Vec<WriteAgentWorkspaceReviewHunkAnnotationResult>,
}

fn validate_workspace_review_hunk_annotation_requests(
    requests: Vec<WriteAgentWorkspaceReviewHunkAnnotationRequest>,
    target: Option<&AgentWorkspaceReviewTarget>,
    target_scope: AgentWorkspaceReviewTargetScope,
    target_head_sha: Option<&str>,
    target_diff_fingerprint: &str,
) -> Result<WorkspaceReviewHunkAnnotationValidation, JsonError> {
    if requests.is_empty() {
        return Ok(WorkspaceReviewHunkAnnotationValidation::default());
    }
    if requests.len() > WORKSPACE_REVIEW_MAX_HUNK_ANNOTATIONS {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            format!("annotations is limited to {WORKSPACE_REVIEW_MAX_HUNK_ANNOTATIONS} items"),
            None,
        ));
    }
    let target = target.ok_or_else(|| {
        json_error(
            StatusCode::BAD_REQUEST,
            "annotations require a current workspace review target",
            None,
        )
    })?;
    if target.scope != target_scope
        || target.diff_fingerprint != target_diff_fingerprint
        || (target_scope == AgentWorkspaceReviewTargetScope::SelectedSource
            && target.head_sha.as_deref() != target_head_sha)
    {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "annotations target metadata does not match the current workspace review target",
            None,
        ));
    }

    let mut validation = WorkspaceReviewHunkAnnotationValidation::default();
    for (index, request) in requests.into_iter().enumerate() {
        match validate_workspace_review_hunk_annotation_request(index, request, target) {
            Ok(validated) => validation.accepted.push(validated),
            Err(rejected) => validation.rejected.push(rejected),
        }
    }
    Ok(validation)
}

#[allow(clippy::result_large_err)] // Rejections are serialized response payloads; keep the local API unboxed.
fn validate_workspace_review_hunk_annotation_request(
    index: usize,
    request: WriteAgentWorkspaceReviewHunkAnnotationRequest,
    target: &AgentWorkspaceReviewTarget,
) -> Result<ValidatedWorkspaceReviewHunkAnnotation, WriteAgentWorkspaceReviewHunkAnnotationResult> {
    let field = |name: &str| format!("annotations[{index}].{name}");
    let path = bounded_trimmed_string(
        request.path.clone(),
        &field("path"),
        WORKSPACE_REVIEW_HUNK_ANNOTATION_MAX_PATH_CHARS,
    )
    .map_err(|reason| rejected_workspace_review_hunk_annotation_result(index, &request, reason))?;
    validate_workspace_review_annotation_path(&path, &field("path")).map_err(|reason| {
        rejected_workspace_review_hunk_annotation_result(index, &request, reason)
    })?;
    let source = bounded_trimmed_string(
        request.source.clone(),
        &field("source"),
        WORKSPACE_REVIEW_HUNK_ANNOTATION_MAX_SOURCE_CHARS,
    )
    .map_err(|reason| rejected_workspace_review_hunk_annotation_result(index, &request, reason))?;
    let hunk_header = bounded_trimmed_string(
        request.hunk_header.clone(),
        &field("hunk_header"),
        WORKSPACE_REVIEW_HUNK_ANNOTATION_MAX_HEADER_CHARS,
    )
    .map_err(|reason| rejected_workspace_review_hunk_annotation_result(index, &request, reason))?;
    let message = bounded_trimmed_string(
        request.message.clone(),
        &field("message"),
        WORKSPACE_REVIEW_HUNK_ANNOTATION_MAX_MESSAGE_CHARS,
    )
    .map_err(|reason| rejected_workspace_review_hunk_annotation_result(index, &request, reason))?;
    let title = request
        .title
        .clone()
        .map(|title| {
            bounded_trimmed_string(
                title,
                &field("title"),
                WORKSPACE_REVIEW_HUNK_ANNOTATION_MAX_TITLE_CHARS,
            )
        })
        .transpose()
        .map_err(|reason| {
            rejected_workspace_review_hunk_annotation_result(index, &request, reason)
        })?;
    let level = validate_workspace_review_hunk_annotation_level(
        request
            .level
            .clone()
            .unwrap_or_else(|| "notice".to_string()),
        &field("level"),
    )
    .map_err(|reason| rejected_workspace_review_hunk_annotation_result(index, &request, reason))?;

    let anchor_matches = target.review_packet.hunk_anchors.iter().any(|anchor| {
        anchor.path == path
            && anchor.source == source
            && anchor.hunk_header == hunk_header
            && anchor.old_start == request.old_start
            && anchor.old_lines == request.old_lines
            && anchor.new_start == request.new_start
            && anchor.new_lines == request.new_lines
    });
    if !anchor_matches {
        return Err(rejected_workspace_review_hunk_annotation_result(
            index,
            &request,
            format!(
                "{} does not match any current workspace review hunk anchor",
                field("hunk_header")
            ),
        ));
    }

    Ok(ValidatedWorkspaceReviewHunkAnnotation {
        index,
        path,
        source,
        hunk_header,
        old_start: request.old_start,
        old_lines: request.old_lines,
        new_start: request.new_start,
        new_lines: request.new_lines,
        title,
        message,
        level,
    })
}

fn rejected_workspace_review_hunk_annotation_result(
    index: usize,
    request: &WriteAgentWorkspaceReviewHunkAnnotationRequest,
    reason: impl Into<String>,
) -> WriteAgentWorkspaceReviewHunkAnnotationResult {
    WriteAgentWorkspaceReviewHunkAnnotationResult {
        index,
        accepted: false,
        annotation_id: None,
        path: Some(request.path.clone()),
        source: Some(request.source.clone()),
        hunk_header: Some(request.hunk_header.clone()),
        old_start: Some(request.old_start),
        old_lines: Some(request.old_lines),
        new_start: Some(request.new_start),
        new_lines: Some(request.new_lines),
        reason: Some(reason.into()),
    }
}

fn accepted_workspace_review_hunk_annotation_result(
    validated: &ValidatedWorkspaceReviewHunkAnnotation,
    entity: &AgentWorkspaceReviewHunkAnnotation,
) -> WriteAgentWorkspaceReviewHunkAnnotationResult {
    WriteAgentWorkspaceReviewHunkAnnotationResult {
        index: validated.index,
        accepted: true,
        annotation_id: Some(entity.id.clone()),
        path: Some(validated.path.clone()),
        source: Some(validated.source.clone()),
        hunk_header: Some(validated.hunk_header.clone()),
        old_start: Some(validated.old_start),
        old_lines: Some(validated.old_lines),
        new_start: Some(validated.new_start),
        new_lines: Some(validated.new_lines),
        reason: None,
    }
}

fn bounded_trimmed_string(value: String, field: &str, max_chars: usize) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{field} is required"));
    }
    if trimmed.chars().count() > max_chars {
        return Err(format!("{field} is limited to {max_chars} characters"));
    }
    Ok(trimmed.to_string())
}

fn validate_workspace_review_annotation_path(path: &str, field: &str) -> Result<(), String> {
    let candidate = std::path::Path::new(path);
    if candidate.is_absolute()
        || candidate.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(format!(
            "{field} must be a relative path inside the reviewed workspace"
        ));
    }
    Ok(())
}

fn validate_workspace_review_hunk_annotation_level(
    value: String,
    field: &str,
) -> Result<String, String> {
    let level = value.trim();
    match level {
        "info" | "notice" | "warning" => Ok(level.to_string()),
        _ => Err(format!("{field} must be one of: info, notice, warning")),
    }
}

struct WorkspaceReviewHunkAnnotationEntityContext<'a> {
    conversation_id: &'a ChatConversationId,
    project_id: &'a ProjectId,
    artifact_id: &'a ArtifactId,
    artifact_version: u32,
    target_scope: AgentWorkspaceReviewTargetScope,
    head_sha: Option<String>,
    diff_fingerprint: &'a str,
    created_by_run_id: Option<String>,
}

fn build_workspace_review_hunk_annotation_entities(
    annotations: Vec<ValidatedWorkspaceReviewHunkAnnotation>,
    context: WorkspaceReviewHunkAnnotationEntityContext<'_>,
) -> Vec<AgentWorkspaceReviewHunkAnnotation> {
    let created_at = chrono::Utc::now();
    annotations
        .into_iter()
        .map(|annotation| AgentWorkspaceReviewHunkAnnotation {
            id: uuid::Uuid::new_v4().to_string(),
            conversation_id: context.conversation_id.clone(),
            project_id: context.project_id.clone(),
            artifact_id: context.artifact_id.clone(),
            artifact_version: context.artifact_version,
            target_scope: context.target_scope,
            head_sha: context.head_sha.clone(),
            diff_fingerprint: context.diff_fingerprint.to_string(),
            path: annotation.path,
            diff_source: annotation.source,
            hunk_header: annotation.hunk_header,
            old_start: annotation.old_start,
            old_lines: annotation.old_lines,
            new_start: annotation.new_start,
            new_lines: annotation.new_lines,
            title: annotation.title,
            message: annotation.message,
            level: annotation.level,
            created_by_run_id: context.created_by_run_id.clone(),
            created_at,
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct WorkspaceReviewHunkAnnotationKey {
    path: String,
    source: String,
    hunk_header: String,
    old_start: u32,
    old_lines: u32,
    new_start: u32,
    new_lines: u32,
}

impl From<&AgentWorkspaceReviewHunkAnnotation> for WorkspaceReviewHunkAnnotationKey {
    fn from(value: &AgentWorkspaceReviewHunkAnnotation) -> Self {
        Self {
            path: value.path.clone(),
            source: value.diff_source.clone(),
            hunk_header: value.hunk_header.clone(),
            old_start: value.old_start,
            old_lines: value.old_lines,
            new_start: value.new_start,
            new_lines: value.new_lines,
        }
    }
}

impl From<&AgentWorkspaceReviewHunkAnchor> for WorkspaceReviewHunkAnnotationKey {
    fn from(value: &AgentWorkspaceReviewHunkAnchor) -> Self {
        Self {
            path: value.path.clone(),
            source: value.source.clone(),
            hunk_header: value.hunk_header.clone(),
            old_start: value.old_start,
            old_lines: value.old_lines,
            new_start: value.new_start,
            new_lines: value.new_lines,
        }
    }
}

fn merge_workspace_review_hunk_annotations(
    existing: Vec<AgentWorkspaceReviewHunkAnnotation>,
    updates: Vec<AgentWorkspaceReviewHunkAnnotation>,
) -> Vec<AgentWorkspaceReviewHunkAnnotation> {
    let mut merged = BTreeMap::new();
    for annotation in existing {
        merged.insert(
            WorkspaceReviewHunkAnnotationKey::from(&annotation),
            annotation,
        );
    }
    for annotation in updates {
        merged.insert(
            WorkspaceReviewHunkAnnotationKey::from(&annotation),
            annotation,
        );
    }
    merged.into_values().collect()
}

fn missing_workspace_review_hunk_anchors(
    target: &AgentWorkspaceReviewTarget,
    annotations: &[AgentWorkspaceReviewHunkAnnotation],
) -> Vec<AgentWorkspaceReviewHunkAnchor> {
    let covered = annotations
        .iter()
        .map(WorkspaceReviewHunkAnnotationKey::from)
        .collect::<BTreeSet<_>>();
    target
        .review_packet
        .hunk_anchors
        .iter()
        .filter(|anchor| !covered.contains(&WorkspaceReviewHunkAnnotationKey::from(*anchor)))
        .cloned()
        .collect()
}

fn workspace_review_completion_requires_hunk_coverage(_outcome: Option<&str>) -> bool {
    false
}

async fn ensure_workspace_review_hunk_annotation_coverage_for_completion(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    outcome: Option<&str>,
) -> Result<(), JsonError> {
    if !workspace_review_completion_requires_hunk_coverage(outcome) {
        return Ok(());
    }

    let context = load_agent_workspace_review_context(state, workspace)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let Some(target) = context.target.as_ref() else {
        return Ok(());
    };
    if !context.is_current {
        return Err(json_error(
            StatusCode::CONFLICT,
            "Write the current workspace Review artifact before completing this review outcome",
            None,
        ));
    }
    if target.review_packet.hunk_anchors.is_empty() {
        return Ok(());
    }
    let artifact_id = context.monitor.review_artifact_id.clone().ok_or_else(|| {
        json_error(
            StatusCode::CONFLICT,
            "Write the current workspace Review artifact before completing this review outcome",
            None,
        )
    })?;
    let annotations = state
        .agent_conversation_workspace_repo
        .list_workspace_review_hunk_annotations(&workspace.conversation_id, &artifact_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let missing = missing_workspace_review_hunk_anchors(target, &annotations);
    if missing.is_empty() {
        return Ok(());
    }

    let preview = missing
        .iter()
        .take(5)
        .map(|anchor| format!("{} {} {}", anchor.source, anchor.path, anchor.hunk_header))
        .collect::<Vec<_>>()
        .join("; ");
    Err(json_error(
        StatusCode::CONFLICT,
        format!(
            "workspace Review hunk annotations are incomplete: {} current hunk(s) still need descriptions. Call write_workspace_review_hunk_annotations for the missing target.review_packet.hunk_anchors before completing. Missing: {}",
            missing.len(),
            preview
        ),
        None,
    ))
}

fn compact_workspace_review_log_fingerprint(value: Option<&str>) -> String {
    value
        .map(|value| value.chars().take(12).collect())
        .unwrap_or_else(|| "none".to_string())
}

fn workspace_review_target_scope_log(target: Option<&AgentWorkspaceReviewTarget>) -> String {
    target
        .map(|target| target.scope.to_string())
        .unwrap_or_else(|| "none".to_string())
}

fn default_workspace_review_artifact_title(
    target_scope: AgentWorkspaceReviewTargetScope,
    target: Option<&AgentWorkspaceReviewTarget>,
) -> String {
    match target_scope {
        AgentWorkspaceReviewTargetScope::SelectedSource => target
            .and_then(|target| target.source_pull_request_number)
            .map(|pr_number| format!("PR #{pr_number}"))
            .or_else(|| {
                target
                    .map(|target| compact_workspace_review_ref_title(&target.head_ref))
                    .filter(|title| !title.is_empty())
            })
            .unwrap_or_else(|| "Selected source".to_string()),
        AgentWorkspaceReviewTargetScope::WorkspaceDelta => "Workspace changes".to_string(),
    }
}

fn workspace_review_artifact_title(
    requested_title: Option<String>,
    previous_title: Option<&str>,
    previous_target_scope: Option<AgentWorkspaceReviewTargetScope>,
    target_scope: AgentWorkspaceReviewTargetScope,
    target: Option<&AgentWorkspaceReviewTarget>,
) -> String {
    requested_title
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .filter(|value| !is_legacy_workspace_review_artifact_title(value))
        .or_else(|| {
            if previous_target_scope != Some(target_scope) {
                return None;
            }
            previous_title
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .filter(|value| !is_legacy_workspace_review_artifact_title(value))
        })
        .unwrap_or_else(|| default_workspace_review_artifact_title(target_scope, target))
}

fn compact_workspace_review_ref_title(ref_name: &str) -> String {
    let mut value = ref_name.trim();
    for prefix in ["refs/heads/", "refs/remotes/", "origin/"] {
        if let Some(stripped) = value.strip_prefix(prefix) {
            value = stripped;
            break;
        }
    }
    value.trim().to_string()
}

fn normalize_workspace_review_artifact_content(content: String) -> String {
    let content = content.trim().to_string();
    let first_line_end = content.find('\n').unwrap_or(content.len());
    let first_line = content[..first_line_end].trim_end_matches('\r').trim();
    if !is_redundant_workspace_review_heading(first_line) {
        return content;
    }
    content[first_line_end..]
        .trim_start_matches(['\r', '\n'])
        .trim()
        .to_string()
}

fn is_redundant_workspace_review_heading(line: &str) -> bool {
    let Some(title) = line.strip_prefix("# ") else {
        return false;
    };
    is_legacy_workspace_review_artifact_title(title)
}

fn is_legacy_workspace_review_artifact_title(title: &str) -> bool {
    matches!(
        title.trim(),
        "Review" | "Workspace Review" | "Selected Source Review"
    )
}

fn non_empty_string(value: String, field: &str) -> Result<String, JsonError> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            format!("{field} must not be empty"),
            None,
        ));
    }
    Ok(value)
}

fn parse_update_base_kind(
    value: Option<&str>,
) -> Result<Option<IdeationAnalysisBaseRefKind>, String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::parse::<IdeationAnalysisBaseRefKind>)
        .transpose()
}

fn is_publish_in_progress(push_status: Option<&str>) -> bool {
    is_publication_push_active(push_status)
}

fn update_only_repair_pr_supervision_state(
    workspace: &AgentConversationWorkspace,
) -> Option<(&'static str, &'static str)> {
    if workspace.publication_pr_number.is_none()
        || matches!(
            workspace.publication_pr_status.as_deref(),
            Some("merged" | "closed")
        )
    {
        return None;
    }

    if !workspace.auto_publish_enabled {
        return Some((
            "paused",
            "Agent workspace repair verified; Auto Publish is paused.",
        ));
    }

    if workspace.pr_autofix_enabled
        || workspace.pr_auto_merge_desired
        || workspace.pr_auto_merge_current.is_some()
    {
        return Some((
            "monitoring",
            "Agent workspace repair verified; RalphX is monitoring the pull request.",
        ));
    }

    None
}

fn should_auto_publish_after_update_only_repair(workspace: &AgentConversationWorkspace) -> bool {
    if workspace.publication_pr_number.is_some() {
        workspace.auto_publish_enabled
    } else {
        workspace.auto_publish_initial_pr_enabled
    }
}

fn publish_in_progress_response(
    workspace: AgentConversationWorkspaceResponse,
) -> AgentWorkspacePublishActionResponse {
    let pr_number = workspace.publication_pr_number;
    let pr_url = workspace.publication_pr_url.clone();
    AgentWorkspacePublishActionResponse {
        success: true,
        status: "publish_in_progress".to_string(),
        message: "Publish is already in progress for this agent workspace".to_string(),
        repair_queued: false,
        workspace: Some(workspace),
        freshness: None,
        updated: None,
        target_ref: None,
        base_commit: None,
        commit_sha: None,
        pushed: None,
        created_pr: None,
        pr_number,
        pr_url,
    }
}

fn repair_queued_from_publication_events(
    events: &[AgentConversationWorkspacePublicationEventResponse],
) -> bool {
    match events.iter().rev().find(|event| {
        matches!(
            event.step.as_str(),
            "repair_requested" | "repair_deferred" | "repair_sent"
        )
    }) {
        Some(event) if event.step == "repair_sent" => {
            matches!(event.status.as_str(), "started" | "succeeded")
        }
        Some(event) => matches!(event.status.as_str(), "started" | "succeeded"),
        None => false,
    }
}

fn needs_agent_repair_response(
    workspace: AgentConversationWorkspaceResponse,
    repair_queued: bool,
) -> AgentWorkspacePublishActionResponse {
    let pr_number = workspace.publication_pr_number;
    let pr_url = workspace.publication_pr_url.clone();
    AgentWorkspacePublishActionResponse {
        success: true,
        status: "needs_agent_repair".to_string(),
        message: "Workspace needs agent repair before publishing can continue".to_string(),
        repair_queued,
        workspace: Some(workspace),
        freshness: None,
        updated: None,
        target_ref: None,
        base_commit: None,
        commit_sha: None,
        pushed: None,
        created_pr: None,
        pr_number,
        pr_url,
    }
}

async fn publish_action_response_for_existing_workspace_state(
    state: &AppState,
    conversation_id: &ChatConversationId,
    workspace: AgentConversationWorkspaceResponse,
) -> Result<Option<AgentWorkspacePublishActionResponse>, JsonError> {
    match workspace.publication_push_status.as_deref() {
        status if is_publish_in_progress(status) => {
            Ok(Some(publish_in_progress_response(workspace)))
        }
        Some("needs_agent") => {
            let events = load_agent_workspace_publication_events(state, conversation_id).await?;
            Ok(Some(needs_agent_repair_response(
                workspace,
                repair_queued_from_publication_events(&events),
            )))
        }
        _ => Ok(None),
    }
}

fn publish_readiness_blockers(
    freshness: &AgentConversationWorkspaceFreshnessResponse,
    review_blocker: Option<String>,
) -> Vec<String> {
    let mut blockers = Vec::new();
    if let Some(blocker) = review_blocker {
        blockers.push(blocker);
    }
    if freshness.base_status == "blocked" {
        blockers.push(
            freshness
                .base_block_reason
                .clone()
                .unwrap_or_else(|| "Workspace base is blocked".to_string()),
        );
    }
    if !freshness.has_uncommitted_changes
        && freshness.unpublished_commit_count.unwrap_or_default() == 0
    {
        blockers.push("No committed or uncommitted workspace changes to publish".to_string());
    }
    blockers
}

fn publish_readiness_recommended_actions(
    freshness: &AgentConversationWorkspaceFreshnessResponse,
) -> Vec<String> {
    let mut actions = Vec::new();
    if freshness.base_status != "blocked" && freshness.is_base_ahead {
        actions.push("update_from_base".to_string());
    }
    actions
}

async fn action_response_for_needs_repair(
    state: &AppState,
    conversation_id: &ChatConversationId,
    error: String,
) -> Result<Json<AgentWorkspacePublishActionResponse>, JsonError> {
    let workspace = load_agent_workspace_response(state, conversation_id).await?;
    if workspace.publication_push_status.as_deref() != Some("needs_agent") {
        return Err(json_error(StatusCode::CONFLICT, error, None));
    }
    let events = load_agent_workspace_publication_events(state, conversation_id).await?;
    let repair_queued = repair_queued_from_publication_events(&events);

    let freshness = get_agent_conversation_workspace_freshness_for_app_state(
        conversation_id,
        Some("local"),
        state,
    )
    .await
    .ok();
    Ok(Json(AgentWorkspacePublishActionResponse {
        success: true,
        status: "needs_agent_repair".to_string(),
        message: error,
        repair_queued,
        workspace: Some(workspace),
        freshness,
        updated: None,
        target_ref: None,
        base_commit: None,
        commit_sha: None,
        pushed: None,
        created_pr: None,
        pr_number: None,
        pr_url: None,
    }))
}

/// POST /api/agent-workspaces/{conversation_id}/complete-repair
///
/// Called by the dedicated agent workspace repair agent after it has resolved a
/// publish/update failure and committed the repair.
pub async fn complete_agent_workspace_repair(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
    Json(req): Json<CompleteAgentWorkspaceRepairRequest>,
) -> Result<Json<CompleteAgentWorkspaceRepairResponse>, JsonError> {
    if !is_valid_git_sha(&req.repair_commit_sha) {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "repair_commit_sha must be a full 40-character SHA (use `git rev-parse HEAD`)",
            None,
        ));
    }
    if !is_valid_git_sha(&req.resolved_base_commit) {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "resolved_base_commit must be a full 40-character SHA",
            None,
        ));
    }

    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Agent workspace not found", None))?;

    let project = state
        .app_state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Project not found", None))?;

    let publish_target =
        resolve_agent_workspace_publish_target(state.app_state.as_ref(), &project, &workspace)
            .await
            .map_err(|error| json_error(StatusCode::BAD_REQUEST, error, None))?;

    let freshness = inspect_publish_branch_freshness_for_source(
        &publish_target.worktree_path,
        &publish_target.base_ref,
        &publish_target.branch_name,
        workspace.base_commit.as_deref(),
    )
    .await
    .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;

    let workspace_head_sha =
        GitService::get_branch_sha(&publish_target.worktree_path, &publish_target.branch_name)
            .await
            .map_err(|error| {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
            })?;
    let has_uncommitted_changes =
        GitService::has_uncommitted_changes(&publish_target.worktree_path)
            .await
            .map_err(|error| {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
            })?;
    let has_conflict_markers = GitService::has_conflict_markers(&publish_target.worktree_path)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;

    verify_agent_workspace_repair_completion(AgentWorkspaceRepairCompletionCheck {
        freshness_status: &freshness,
        workspace_base_ref: &publish_target.base_ref,
        resolved_base_ref: &req.resolved_base_ref,
        resolved_base_commit: &req.resolved_base_commit,
        repair_commit_sha: &req.repair_commit_sha,
        workspace_head_sha: &workspace_head_sha,
        has_uncommitted_changes,
        is_merge_in_progress: GitService::is_merge_in_progress(&publish_target.worktree_path),
        is_rebase_in_progress: GitService::is_rebase_in_progress(&publish_target.worktree_path),
        has_conflict_files: !GitService::get_conflict_files(&publish_target.worktree_path)
            .await
            .map_err(|error| {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
            })?
            .is_empty(),
        has_conflict_markers,
    })
    .map_err(|error| json_error(StatusCode::CONFLICT, error, None))?;

    let publication_events = state
        .app_state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let post_repair_action = agent_workspace_post_repair_action_from_events(&publication_events);
    let completion_supervision = if post_repair_action == AgentWorkspacePostRepairAction::UpdateOnly
        && !should_auto_publish_after_update_only_repair(&workspace)
    {
        update_only_repair_pr_supervision_state(&workspace)
    } else if !workspace.auto_publish_enabled {
        Some((
            "paused",
            "Agent workspace repair verified; Auto Publish is paused.",
        ))
    } else {
        Some((
            "publishing",
            "Agent workspace repair verified; publication is continuing.",
        ))
    };
    let claim = current_agent_workspace_repair_claim_for_completion(
        Arc::clone(&state.app_state.agent_conversation_workspace_repo),
        Arc::clone(&state.app_state.agent_run_repo),
        &workspace,
    )
    .await
    .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
    .ok_or_else(|| {
        json_error(
            StatusCode::CONFLICT,
            "Agent workspace repair attempt is no longer current",
            None,
        )
    })?;
    if !complete_agent_workspace_repair_claim(
        Arc::clone(&state.app_state.agent_conversation_workspace_repo),
        &claim,
        &freshness.target_base_commit,
        completion_supervision.map(|(status, _)| status),
        completion_supervision.map(|(_, summary)| summary),
    )
    .await
    .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
    {
        return Err(json_error(
            StatusCode::CONFLICT,
            "Agent workspace repair attempt was replaced before completion",
            None,
        ));
    }
    let updated_workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Agent workspace not found", None))?;
    state
        .app_state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "repair_completed",
            "succeeded",
            req.summary.clone(),
            Some("agent_fixable".to_string()),
        ))
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    if let Some(response) = complete_repair_workspace_review_response_if_required(
        &state,
        &conversation_id,
        &updated_workspace,
        &freshness.target_base_commit,
        &req.repair_commit_sha,
        &req.summary,
    )
    .await?
    {
        return Ok(response);
    }
    let (
        message,
        new_status,
        base_commit,
        auto_publish_status,
        auto_publish_error,
        pr_number,
        pr_url,
    ) = if let Some(plan_branch) = publish_target.plan_branch.as_ref() {
        let pr_number = plan_branch.pr_number;
        let pr_url = plan_branch.pr_url.clone();
        let pr_status = plan_branch
            .pr_status
            .as_ref()
            .map(|status| status.to_db_string());

        if pr_number.is_none() {
            (
                "Agent workspace repair verified".to_string(),
                "refreshed".to_string(),
                freshness.target_base_commit.clone(),
                Some("skipped".to_string()),
                None,
                pr_number,
                pr_url,
            )
        } else if let (Some(github), Some(published_pr_number)) =
            (state.app_state.github_service.as_ref(), pr_number)
        {
            match push_agent_workspace_publish_branch(
                github,
                &publish_target.worktree_path,
                &publish_target.branch_name,
                &state.app_state.agent_conversation_workspace_repo,
                &conversation_id,
            )
            .await
            {
                Ok(_) => {
                    state
                        .app_state
                        .plan_branch_repo
                        .update_pr_push_status(&plan_branch.id, PrPushStatus::Pushed)
                        .await
                        .map_err(|error| {
                            json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
                        })?;
                    state
                        .app_state
                        .agent_conversation_workspace_repo
                        .update_publication(
                            &conversation_id,
                            pr_number,
                            pr_url.as_deref(),
                            pr_status,
                            Some("pushed"),
                        )
                        .await
                        .map_err(|error| {
                            json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
                        })?;
                    state
                        .app_state
                        .agent_conversation_workspace_repo
                        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                            conversation_id.clone(),
                            "published",
                            "succeeded",
                            "Plan branch repair pushed",
                            Some(format!("published:{published_pr_number}")),
                        ))
                        .await
                        .map_err(|error| {
                            json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
                        })?;
                    if let Some(refreshed) = state
                        .app_state
                        .agent_conversation_workspace_repo
                        .get_by_conversation_id(&conversation_id)
                        .await
                        .map_err(|error| {
                            json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
                        })?
                    {
                        if let Err(error) = restore_guarded_auto_merge_after_publish(
                            state.app_state.as_ref(),
                            &refreshed,
                        )
                        .await
                        {
                            tracing::warn!(
                                conversation_id = %conversation_id,
                                pr_number = published_pr_number,
                                error = %error,
                                "Failed to restore guarded auto-merge after plan branch repair publish"
                            );
                        }
                    }
                    (
                        "Agent workspace repair verified and pushed".to_string(),
                        "pushed".to_string(),
                        freshness.target_base_commit.clone(),
                        Some("succeeded".to_string()),
                        None,
                        pr_number,
                        pr_url,
                    )
                }
                Err(error) => {
                    let message = error.to_string();
                    state
                        .app_state
                        .plan_branch_repo
                        .update_pr_push_status(&plan_branch.id, PrPushStatus::Failed)
                        .await
                        .map_err(|repo_error| {
                            json_error(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                repo_error.to_string(),
                                None,
                            )
                        })?;
                    state
                        .app_state
                        .agent_conversation_workspace_repo
                        .update_publication(
                            &conversation_id,
                            pr_number,
                            pr_url.as_deref(),
                            pr_status,
                            Some("failed"),
                        )
                        .await
                        .map_err(|repo_error| {
                            json_error(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                repo_error.to_string(),
                                None,
                            )
                        })?;
                    settle_agent_workspace_failure_without_repair(
                        Arc::clone(&state.app_state.agent_conversation_workspace_repo),
                        &conversation_id,
                        "failed",
                        &message,
                    )
                    .await
                    .map_err(|repo_error| {
                        json_error(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            repo_error.to_string(),
                            None,
                        )
                    })?;
                    state
                        .app_state
                        .agent_conversation_workspace_repo
                        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                            conversation_id,
                            "failed",
                            "failed",
                            message.clone(),
                            Some("operational".to_string()),
                        ))
                        .await
                        .map_err(|repo_error| {
                            json_error(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                repo_error.to_string(),
                                None,
                            )
                        })?;
                    (
                        format!(
                            "Agent workspace repair verified; automatic push failed: {message}"
                        ),
                        "failed".to_string(),
                        freshness.target_base_commit.clone(),
                        Some("failed".to_string()),
                        Some(message),
                        pr_number,
                        pr_url,
                    )
                }
            }
        } else {
            let message = "GitHub integration is not available".to_string();
            state
                .app_state
                .plan_branch_repo
                .update_pr_push_status(&plan_branch.id, PrPushStatus::Failed)
                .await
                .map_err(|error| {
                    json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
                })?;
            state
                .app_state
                .agent_conversation_workspace_repo
                .update_publication(
                    &conversation_id,
                    pr_number,
                    pr_url.as_deref(),
                    pr_status,
                    Some("failed"),
                )
                .await
                .map_err(|error| {
                    json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
                })?;
            settle_agent_workspace_failure_without_repair(
                Arc::clone(&state.app_state.agent_conversation_workspace_repo),
                &conversation_id,
                "failed",
                &message,
            )
            .await
            .map_err(|error| {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
            })?;
            state
                .app_state
                .agent_conversation_workspace_repo
                .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                    conversation_id,
                    "failed",
                    "failed",
                    message.clone(),
                    Some("operational".to_string()),
                ))
                .await
                .map_err(|error| {
                    json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
                })?;
            (
                format!("Agent workspace repair verified; automatic push failed: {message}"),
                "failed".to_string(),
                freshness.target_base_commit.clone(),
                Some("failed".to_string()),
                Some(message),
                pr_number,
                pr_url,
            )
        }
    } else if post_repair_action == AgentWorkspacePostRepairAction::UpdateOnly
        && !should_auto_publish_after_update_only_repair(&workspace)
    {
        if let Some((status, summary)) = update_only_repair_pr_supervision_state(&workspace) {
            state
                .app_state
                .agent_conversation_workspace_repo
                .update_pr_auto_merge_state(
                    &conversation_id,
                    workspace.pr_auto_merge_current,
                    Some(status),
                    Some(summary),
                )
                .await
                .map_err(|error| {
                    json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
                })?;
        }
        (
            "Agent workspace repair verified".to_string(),
            "refreshed".to_string(),
            freshness.target_base_commit.clone(),
            Some("skipped".to_string()),
            None,
            workspace.publication_pr_number,
            workspace.publication_pr_url.clone(),
        )
    } else if !workspace.auto_publish_enabled {
        let message = "Agent workspace repair verified; Auto Publish is paused. Manual Commit & Publish is required to update the pull request.";
        state
            .app_state
            .agent_conversation_workspace_repo
            .update_pr_auto_merge_state(
                &conversation_id,
                workspace.pr_auto_merge_current,
                Some("paused"),
                Some(message),
            )
            .await
            .map_err(|error| {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
            })?;
        state
            .app_state
            .agent_conversation_workspace_repo
            .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                conversation_id,
                "repair_publish_skipped",
                "skipped",
                message,
                Some("auto_publish_paused".to_string()),
            ))
            .await
            .map_err(|error| {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
            })?;
        (
            message.to_string(),
            "refreshed".to_string(),
            freshness.target_base_commit.clone(),
            Some("skipped".to_string()),
            None,
            workspace.publication_pr_number,
            workspace.publication_pr_url.clone(),
        )
    } else {
        let auto_publish = publish_agent_conversation_workspace_for_app_state(
            state.app_state.as_ref(),
            &state.execution_state,
            conversation_id,
            false,
        )
        .await;

        match auto_publish {
            Ok(result) => {
                let status = result
                    .workspace
                    .publication_push_status
                    .clone()
                    .unwrap_or_else(|| "pushed".to_string());
                let base_commit = result
                    .workspace
                    .base_commit
                    .clone()
                    .unwrap_or_else(|| freshness.target_base_commit.clone());
                (
                    "Agent workspace repair verified and published".to_string(),
                    status,
                    base_commit,
                    Some("succeeded".to_string()),
                    None,
                    result.pr_number,
                    result.pr_url,
                )
            }
            Err(error) => {
                let refreshed = state
                    .app_state
                    .agent_conversation_workspace_repo
                    .get_by_conversation_id(&conversation_id)
                    .await
                    .map_err(|repo_error| {
                        json_error(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            repo_error.to_string(),
                            None,
                        )
                    })?;
                let final_status = refreshed
                    .as_ref()
                    .and_then(|workspace| workspace.publication_push_status.clone())
                    .unwrap_or_else(|| "failed".to_string());
                let final_base_commit = refreshed
                    .as_ref()
                    .and_then(|workspace| workspace.base_commit.clone())
                    .unwrap_or_else(|| freshness.target_base_commit.clone());
                let publish_status = if final_status == "no_changes" {
                    "skipped"
                } else {
                    "failed"
                };
                (
                    format!("Agent workspace repair verified; automatic publish failed: {error}"),
                    final_status,
                    final_base_commit,
                    Some(publish_status.to_string()),
                    Some(error),
                    refreshed
                        .as_ref()
                        .and_then(|workspace| workspace.publication_pr_number),
                    refreshed
                        .as_ref()
                        .and_then(|workspace| workspace.publication_pr_url.clone()),
                )
            }
        }
    };

    Ok(Json(CompleteAgentWorkspaceRepairResponse {
        success: true,
        message,
        new_status,
        base_commit,
        repair_commit_sha: req.repair_commit_sha,
        auto_publish_status,
        auto_publish_error,
        pr_number,
        pr_url,
    }))
}

// =========================================================================
// Extension A — Staged / Unstaged diff HTTP handlers
// =========================================================================

/// GET /api/agent-workspaces/{conversation_id}/staged-changes
pub async fn get_agent_workspace_staged_file_changes(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
) -> Result<Json<Vec<crate::application::FileChange>>, JsonError> {
    let conversation_id = crate::domain::entities::ChatConversationId::from_string(conversation_id);
    crate::commands::diff_commands::get_agent_conversation_workspace_staged_file_changes_for_state(
        state.app_state.as_ref(),
        &conversation_id,
    )
    .await
    .map(Json)
    .map_err(|e| {
        json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            e.to_string(),
            None,
        )
    })
}

/// GET /api/agent-workspaces/{conversation_id}/unstaged-changes
pub async fn get_agent_workspace_unstaged_file_changes(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
) -> Result<Json<Vec<crate::application::FileChange>>, JsonError> {
    let conversation_id = crate::domain::entities::ChatConversationId::from_string(conversation_id);
    crate::commands::diff_commands::get_agent_conversation_workspace_unstaged_file_changes_for_state(
        state.app_state.as_ref(),
        &conversation_id,
    )
    .await
    .map(Json)
    .map_err(|e| json_error(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string(), None))
}

/// GET /api/agent-workspaces/{conversation_id}/staged-changes/{*file_path}
pub async fn get_agent_workspace_staged_file_diff(
    State(state): State<HttpServerState>,
    Path((conversation_id, file_path)): Path<(String, String)>,
) -> Result<Json<crate::application::FileDiff>, JsonError> {
    let conversation_id = crate::domain::entities::ChatConversationId::from_string(conversation_id);
    crate::commands::diff_commands::get_agent_conversation_workspace_staged_file_diff_for_state(
        state.app_state.as_ref(),
        &conversation_id,
        file_path,
    )
    .await
    .map(Json)
    .map_err(|e| {
        json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            e.to_string(),
            None,
        )
    })
}

/// GET /api/agent-workspaces/{conversation_id}/unstaged-changes/{*file_path}
pub async fn get_agent_workspace_unstaged_file_diff(
    State(state): State<HttpServerState>,
    Path((conversation_id, file_path)): Path<(String, String)>,
) -> Result<Json<crate::application::FileDiff>, JsonError> {
    let conversation_id = crate::domain::entities::ChatConversationId::from_string(conversation_id);
    crate::commands::diff_commands::get_agent_conversation_workspace_unstaged_file_diff_for_state(
        state.app_state.as_ref(),
        &conversation_id,
        file_path,
    )
    .await
    .map(Json)
    .map_err(|e| {
        json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            e.to_string(),
            None,
        )
    })
}

// =========================================================================
// Extension B — Cumulative diff HTTP handlers
// =========================================================================

/// GET /api/agent-workspaces/{conversation_id}/cumulative-changes
pub async fn get_agent_workspace_cumulative_file_changes(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
) -> Result<Json<Vec<crate::application::FileChange>>, JsonError> {
    let conversation_id = crate::domain::entities::ChatConversationId::from_string(conversation_id);
    crate::commands::diff_commands::get_agent_conversation_workspace_cumulative_file_changes_for_state(
        state.app_state.as_ref(),
        &conversation_id,
    )
    .await
    .map(Json)
    .map_err(|e| json_error(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string(), None))
}

/// GET /api/agent-workspaces/{conversation_id}/cumulative-changes/{*file_path}
pub async fn get_agent_workspace_cumulative_file_diff(
    State(state): State<HttpServerState>,
    Path((conversation_id, file_path)): Path<(String, String)>,
) -> Result<Json<crate::application::FileDiff>, JsonError> {
    let conversation_id = crate::domain::entities::ChatConversationId::from_string(conversation_id);
    crate::commands::diff_commands::get_agent_conversation_workspace_cumulative_file_diff_for_state(
        state.app_state.as_ref(),
        &conversation_id,
        file_path,
    )
    .await
    .map(Json)
    .map_err(|e| {
        json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            e.to_string(),
            None,
        )
    })
}

/// Query parameters for the file content range endpoint.
#[derive(Debug, serde::Deserialize)]
pub struct FileContentRangeQuery {
    /// "old" or "new"
    pub side: String,
    /// Relative file path within the workspace
    pub path: String,
    /// "head" | "staged" | "unstaged" | "commit" | "cumulative_base" | "cumulative_head"
    pub ref_kind: String,
    /// Commit SHA — required when ref_kind == "commit"
    pub sha: Option<String>,
    /// First line to fetch (1-indexed, inclusive)
    pub from: u32,
    /// Last line to fetch (1-indexed, inclusive)
    pub to: u32,
}

fn parse_diff_ref_kind(
    ref_kind: &str,
    sha: Option<String>,
) -> Result<crate::application::DiffRefKind, String> {
    match ref_kind {
        "head" => Ok(crate::application::DiffRefKind::Head),
        "staged" => Ok(crate::application::DiffRefKind::Staged),
        "unstaged" => Ok(crate::application::DiffRefKind::Unstaged),
        "commit" => {
            let sha = sha.ok_or_else(|| {
                "ref_kind 'commit' requires 'sha' query parameter".to_string()
            })?;
            Ok(crate::application::DiffRefKind::Commit { sha })
        }
        "cumulative_base" => Ok(crate::application::DiffRefKind::CumulativeBase),
        "cumulative_head" => Ok(crate::application::DiffRefKind::CumulativeHead),
        other => Err(format!(
            "Invalid ref_kind '{other}': expected head|staged|unstaged|commit|cumulative_base|cumulative_head"
        )),
    }
}

impl FileContentRangeQuery {
    fn into_service_params(
        self,
    ) -> Result<
        (
            crate::application::DiffSide,
            String,
            crate::application::DiffRefKind,
            u32,
            u32,
        ),
        String,
    > {
        let side = match self.side.as_str() {
            "old" => crate::application::DiffSide::Old,
            "new" => crate::application::DiffSide::New,
            other => return Err(format!("Invalid side '{other}': expected 'old' or 'new'")),
        };
        let ref_kind = parse_diff_ref_kind(&self.ref_kind, self.sha)?;
        Ok((side, self.path, ref_kind, self.from, self.to))
    }
}

/// Query parameters for the file diff page endpoint.
#[derive(Debug, serde::Deserialize)]
pub struct FileDiffPageQuery {
    /// Relative file path within the workspace
    pub path: String,
    /// "head" | "staged" | "unstaged" | "commit" | "cumulative_head"
    pub ref_kind: String,
    /// Commit SHA — required when ref_kind == "commit"
    pub sha: Option<String>,
    /// Flattened diff-row offset
    pub offset: usize,
    /// Maximum number of rows to fetch
    pub limit: usize,
}

impl FileDiffPageQuery {
    fn into_service_params(
        self,
    ) -> Result<(String, crate::application::DiffRefKind, usize, usize), String> {
        let ref_kind = parse_diff_ref_kind(&self.ref_kind, self.sha)?;
        Ok((self.path, ref_kind, self.offset, self.limit))
    }
}

/// GET /api/agent-workspaces/{conversation_id}/file-content-range
///
/// Fetch a line range from a specific version of a file in the workspace.
///
/// Query params: `side`, `path`, `ref_kind`, `sha` (required for commit), `from`, `to`.
pub async fn get_agent_workspace_file_content_range(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
    axum::extract::Query(params): axum::extract::Query<FileContentRangeQuery>,
) -> Result<Json<Vec<crate::application::RangeLine>>, JsonError> {
    let (side, file_path, ref_kind, from, to) = params
        .into_service_params()
        .map_err(|msg| json_error(axum::http::StatusCode::BAD_REQUEST, msg, None))?;
    let conversation_id = crate::domain::entities::ChatConversationId::from_string(conversation_id);
    crate::commands::diff_commands::get_agent_conversation_workspace_file_content_range_for_state(
        state.app_state.as_ref(),
        &conversation_id,
        side,
        file_path,
        ref_kind,
        from,
        to,
    )
    .await
    .map(Json)
    .map_err(|e| {
        let status = if e.to_string().to_lowercase().contains("validation")
            || e.to_string().to_lowercase().contains("unsafe")
            || e.to_string().to_lowercase().contains("relative")
            || e.to_string().to_lowercase().contains("too large")
        {
            axum::http::StatusCode::BAD_REQUEST
        } else {
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        };
        json_error(status, e.to_string(), None)
    })
}

/// GET /api/agent-workspaces/{conversation_id}/file-diff-page
///
/// Fetch a bounded page of flattened diff rows for one workspace file.
///
/// Query params: `path`, `ref_kind`, `sha` (required for commit), `offset`, `limit`.
pub async fn get_agent_workspace_file_diff_page(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
    axum::extract::Query(params): axum::extract::Query<FileDiffPageQuery>,
) -> Result<Json<crate::application::FileDiffPage>, JsonError> {
    let (file_path, ref_kind, offset, limit) = params
        .into_service_params()
        .map_err(|msg| json_error(axum::http::StatusCode::BAD_REQUEST, msg, None))?;
    let conversation_id = crate::domain::entities::ChatConversationId::from_string(conversation_id);
    crate::commands::diff_commands::get_agent_conversation_workspace_file_diff_page_for_state(
        state.app_state.as_ref(),
        &conversation_id,
        file_path,
        ref_kind,
        offset,
        limit,
    )
    .await
    .map(Json)
    .map_err(|e| {
        let status = if e.to_string().to_lowercase().contains("validation")
            || e.to_string().to_lowercase().contains("unsafe")
            || e.to_string().to_lowercase().contains("relative")
            || e.to_string().to_lowercase().contains("too large")
        {
            axum::http::StatusCode::BAD_REQUEST
        } else {
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        };
        json_error(status, e.to_string(), None)
    })
}
