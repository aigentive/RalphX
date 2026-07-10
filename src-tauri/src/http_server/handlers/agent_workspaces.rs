//! Agent workspace HTTP handlers.

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
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};

use super::*;
use crate::application::agent_conversation_workspace::{
    resolve_valid_agent_conversation_workspace_path, AgentConversationWorkspaceBaseSelection,
};
use crate::application::agent_workspace_pr_description::validate_agent_workspace_pr_description_body;
use crate::application::agent_workspace_review::{
    apply_review_artifact_to_monitor, load_agent_workspace_review_context,
    review_gate_publish_blocker, start_agent_workspace_review,
    start_agent_workspace_review_blocking_fixer, AgentWorkspaceReviewGoalContext,
    AgentWorkspaceReviewHunkAnchor, AgentWorkspaceReviewStart, AgentWorkspaceReviewTarget,
};
use crate::application::agent_workspace_review_publish_handoff::resume_pr_fix_publish_after_passed_workspace_review;
use crate::application::publish_resilience::{
    inspect_publish_branch_freshness_for_source, push_publish_branch,
    verify_agent_workspace_repair_completion, AgentWorkspaceRepairCompletionCheck,
};
use crate::application::services::pr_merge_poller::import_agent_workspace_pr_comment_evidence;
use crate::application::{AppState, GitService};
use crate::commands::unified_chat_commands::{
    agent_workspace_post_repair_action_from_events, agent_workspace_response_for_state,
    get_agent_conversation_workspace_freshness_for_app_state,
    publish_agent_conversation_workspace_for_app_state, resolve_agent_workspace_publish_target,
    update_agent_conversation_workspace_from_base_for_app_state,
    AgentConversationWorkspaceFreshnessResponse,
    AgentConversationWorkspacePublicationEventResponse, AgentConversationWorkspaceResponse,
    AgentWorkspacePostRepairAction, AGENT_WORKSPACE_PUBLISH_IN_PROGRESS_MESSAGE,
};
use crate::domain::entities::plan_branch::{PrPushStatus, PrStatus as PlanDbPrStatus};
use crate::domain::entities::{
    pr_comment_body_excerpt, AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentWorkspacePrCommentEvidence,
    AgentWorkspacePrDescription, AgentWorkspacePrReviewAction, AgentWorkspacePrReviewActionKind,
    AgentWorkspacePrReviewActionStatus, AgentWorkspacePrReviewMonitor,
    AgentWorkspacePrReviewMonitorStatus, AgentWorkspaceReviewGateStatus,
    AgentWorkspaceReviewHunkAnnotation, AgentWorkspaceReviewMonitor,
    AgentWorkspaceReviewMonitorStatus, AgentWorkspaceReviewTargetScope, Artifact, ArtifactId,
    ArtifactType, ChatConversationId, IdeationAnalysisBaseRefKind, PlanBranch, ProjectId,
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
    pub title: Option<String>,
    pub body_markdown: String,
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
    pub first_review_completed: bool,
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
            first_review_completed: value.first_review_completed,
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
    pub hunk_anchors: Vec<AgentWorkspaceReviewHunkAnchorResponse>,
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
            hunk_anchors: value
                .hunk_anchors
                .into_iter()
                .map(AgentWorkspaceReviewHunkAnchorResponse::from)
                .collect(),
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
    pub should_show_tab: bool,
}

#[derive(Debug, serde::Deserialize, Default)]
pub struct AgentWorkspaceReviewContextQuery {
    pub include_review_packet: Option<bool>,
}

#[derive(Debug, serde::Deserialize)]
pub struct StartAgentWorkspaceReviewRequest {
    pub force: Option<bool>,
}

#[derive(Debug, serde::Serialize)]
pub struct StartAgentWorkspaceReviewResponse {
    pub success: bool,
    pub target: Option<AgentWorkspaceReviewTargetResponse>,
    pub monitor: AgentWorkspaceReviewMonitorResponse,
    pub goal_context: AgentWorkspaceReviewGoalContext,
    pub is_current: bool,
    pub is_outdated: bool,
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
    validate_agent_workspace_pr_description_body(&req.body_markdown)
        .map_err(|error| json_error(StatusCode::BAD_REQUEST, error.to_string(), None))?;

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
        .save_pr_description(
            &workspace.conversation_id,
            AgentWorkspacePrDescription::new(req.title, req.body_markdown),
        )
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
    match update_agent_conversation_workspace_from_base_for_app_state(
        state.app_state.as_ref(),
        &state.execution_state,
        Some(state.team_service.clone()),
        conversation_id,
        selection,
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
        Some(state.team_service.clone()),
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

/// GET /api/agent-workspaces/{conversation_id}/pr-review-context
pub async fn get_agent_workspace_pr_review_context(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
) -> Result<Json<AgentWorkspacePrReviewContextResponse>, JsonError> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
    let workspace_response =
        agent_workspace_response_for_state(state.app_state.as_ref(), workspace.clone())
            .await
            .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error, None))?;
    let events =
        load_agent_workspace_publication_events(state.app_state.as_ref(), &conversation_id).await?;
    let pr_number = review_pr_number(&workspace).ok_or_else(|| {
        json_error(
            StatusCode::BAD_REQUEST,
            "Review PR mode requires a linked pull request",
            None,
        )
    })?;
    let pr_url = review_pr_url(&workspace);
    let source_head_sha = review_pr_head_sha(&workspace);
    let (mut health, review_feedback) =
        fetch_review_pr_remote_context(state.app_state.as_ref(), &workspace, pr_number).await?;
    let current_head_sha = health
        .as_ref()
        .and_then(|health| health.sync_state.head_ref_oid.clone())
        .or(source_head_sha);
    if let Some(health) = health.as_mut() {
        truncate_pr_health_issue_comments(health);
    }
    let issue_comment_evidence = load_agent_workspace_pr_comment_evidence(
        state.app_state.as_ref(),
        &conversation_id,
        pr_number,
    )
    .await?;
    let monitor = state
        .app_state
        .agent_conversation_workspace_repo
        .get_pr_review_monitor(&conversation_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let pending_action = match current_head_sha.as_deref() {
        Some(head_sha) => state
            .app_state
            .agent_conversation_workspace_repo
            .get_pending_pr_review_action_for_head(&conversation_id, pr_number, head_sha)
            .await
            .map_err(|error| {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
            })?,
        None => None,
    };
    let recent_actions = state
        .app_state
        .agent_conversation_workspace_repo
        .list_pr_review_actions(&conversation_id, 20)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;

    Ok(Json(AgentWorkspacePrReviewContextResponse {
        success: true,
        workspace: workspace_response,
        events,
        pr_number,
        pr_url,
        current_head_sha,
        health,
        review_feedback,
        monitor: monitor.map(AgentWorkspacePrReviewMonitorResponse::from),
        pending_action: pending_action.map(AgentWorkspacePrReviewActionResponse::from),
        recent_actions: recent_actions
            .into_iter()
            .map(AgentWorkspacePrReviewActionResponse::from)
            .collect(),
        issue_comment_evidence,
    }))
}

/// GET /api/agent-workspaces/{conversation_id}/workspace-review-context
pub async fn get_agent_workspace_review_context(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
    Query(query): Query<AgentWorkspaceReviewContextQuery>,
) -> Result<Json<AgentWorkspaceReviewContextResponse>, JsonError> {
    let started = Instant::now();
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
    let workspace_response =
        agent_workspace_response_for_state(state.app_state.as_ref(), workspace.clone())
            .await
            .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error, None))?;
    let events =
        load_agent_workspace_publication_events(state.app_state.as_ref(), &conversation_id).await?;
    let context = load_agent_workspace_review_context(state.app_state.as_ref(), &workspace)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let target_scope = workspace_review_target_scope_log(context.target.as_ref());
    let diff_fingerprint = compact_workspace_review_log_fingerprint(
        context
            .target
            .as_ref()
            .map(|target| target.diff_fingerprint.as_str()),
    );
    tracing::info!(
        target: "ralphx_lib::http_server::agent_workspaces",
        operation = "workspace_review_context_http",
        conversation_id = %conversation_id,
        project_id = %workspace.project_id,
        branch = %workspace.branch_name,
        elapsed_ms = started.elapsed().as_millis(),
        monitor_status = %context.monitor.status,
        target_scope = %target_scope,
        diff_fingerprint = %diff_fingerprint,
        is_current = context.is_current,
        is_outdated = context.is_outdated,
        should_show_tab = context.should_show_tab,
        has_artifact = context.monitor.review_artifact_id.is_some(),
        "Served workspace Review context"
    );

    Ok(Json(AgentWorkspaceReviewContextResponse {
        success: true,
        workspace: workspace_response,
        events,
        target: context.target.map(|target| {
            AgentWorkspaceReviewTargetResponse::from_target(
                target,
                query.include_review_packet.unwrap_or(false),
            )
        }),
        monitor: AgentWorkspaceReviewMonitorResponse::from(context.monitor),
        goal_context: context.goal_context,
        is_current: context.is_current,
        is_outdated: context.is_outdated,
        should_show_tab: context.should_show_tab,
    }))
}

fn workspace_review_action_error(error: AppError) -> JsonError {
    let status = match &error {
        AppError::Validation(_) | AppError::Conflict(_) => StatusCode::CONFLICT,
        AppError::NotFound(_) | AppError::ProjectNotFound(_) => StatusCode::NOT_FOUND,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    json_error(status, error.to_string(), None)
}

/// POST /api/agent-workspaces/{conversation_id}/workspace-review-runs
pub async fn start_agent_workspace_review_run(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
    Json(req): Json<StartAgentWorkspaceReviewRequest>,
) -> Result<Json<StartAgentWorkspaceReviewResponse>, JsonError> {
    let started = Instant::now();
    let force = req.force.unwrap_or(false);
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
    let start =
        start_agent_workspace_review(std::sync::Arc::clone(&state.app_state), &workspace, force)
            .await
            .map_err(|error| {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
            })?;
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
) -> Result<Json<StartAgentWorkspaceReviewFixerResponse>, JsonError> {
    let started = Instant::now();
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
    let start = start_agent_workspace_review_blocking_fixer(state.app_state.as_ref(), &workspace)
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
    let context = load_agent_workspace_review_context(state.app_state.as_ref(), &workspace)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
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
    let context = load_agent_workspace_review_context(state.app_state.as_ref(), &workspace)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;

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
    let validation = validate_workspace_review_hunk_annotation_requests(
        req.annotations,
        Some(target),
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
    let context = load_agent_workspace_review_context(state.app_state.as_ref(), &workspace)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
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
    let monitor = crate::application::agent_workspace_review::complete_agent_workspace_review_run(
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
    resume_pr_fix_publish_after_workspace_review(&state, &conversation_id, &workspace, &monitor)
        .await?;
    // R2: resume an initial (no-PR-yet) armed/automation publish once the gate is Passed.
    resume_initial_auto_publish_after_workspace_review(
        &state,
        &conversation_id,
        &workspace,
        &monitor,
    )
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

/// POST /api/agent-workspaces/{conversation_id}/pr-review-artifact
pub async fn write_agent_workspace_pr_review_artifact(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
    Json(req): Json<WriteAgentWorkspacePrReviewArtifactRequest>,
) -> Result<Json<WriteAgentWorkspacePrReviewArtifactResponse>, JsonError> {
    let content = non_empty_string(req.content, "content")?;
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
    let pr_number = review_pr_number(&workspace).ok_or_else(|| {
        json_error(
            StatusCode::BAD_REQUEST,
            "Review PR mode requires a linked pull request",
            None,
        )
    })?;
    let head_sha = req.head_sha.or_else(|| review_pr_head_sha(&workspace));
    let mut monitor = load_or_create_pr_review_monitor(
        state.app_state.as_ref(),
        &workspace,
        pr_number,
        head_sha.clone(),
    )
    .await?;

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

    let title = req
        .title
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            previous_artifact
                .as_ref()
                .map(|artifact| artifact.name.clone())
        })
        .unwrap_or_else(|| format!("PR #{} Review", pr_number));
    let previous_artifact_id = previous_artifact
        .as_ref()
        .map(|artifact| artifact.id.as_str().to_string());
    let next_version = previous_artifact
        .as_ref()
        .map(|artifact| artifact.metadata.version.saturating_add(1))
        .unwrap_or(1);
    let mut artifact =
        Artifact::new_inline(title, ArtifactType::PrReview, content, "ralphx-pr-reviewer");
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

    monitor.last_seen_head_sha = head_sha.clone().or(monitor.last_seen_head_sha);
    monitor.last_review_run_id = req.created_by_run_id.or(monitor.last_review_run_id);
    monitor.review_artifact_id = Some(created.id.clone());
    monitor.review_artifact_head_sha = head_sha.clone();
    monitor.review_artifact_version = Some(created.metadata.version);
    monitor.review_artifact_updated_at = Some(created.metadata.created_at);
    let monitor = state
        .app_state
        .agent_conversation_workspace_repo
        .upsert_pr_review_monitor(monitor)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;

    let content_text = match &created.content {
        crate::domain::entities::ArtifactContent::Inline { text } => text.clone(),
        crate::domain::entities::ArtifactContent::File { path } => format!("[File: {}]", path),
    };
    let event_name = if previous_artifact_id.is_some() {
        "pr_review_artifact:updated"
    } else {
        "pr_review_artifact:created"
    };
    crate::http_server::emit_http_event(
        &state,
        event_name,
        serde_json::json!({
            "conversationId": conversation_id.as_str(),
            "prNumber": pr_number,
            "headSha": head_sha,
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

    Ok(Json(WriteAgentWorkspacePrReviewArtifactResponse {
        success: true,
        monitor: AgentWorkspacePrReviewMonitorResponse::from(monitor),
        artifact: artifact_response,
        previous_artifact_id,
    }))
}

/// POST /api/agent-workspaces/{conversation_id}/pr-review-actions
pub async fn propose_agent_workspace_pr_review_action(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
    Json(req): Json<ProposeAgentWorkspacePrReviewActionRequest>,
) -> Result<Json<ProposeAgentWorkspacePrReviewActionResponse>, JsonError> {
    let head_sha = non_empty_string(req.head_sha, "head_sha")?;
    let summary = non_empty_string(req.summary, "summary")?;
    let review_body = non_empty_string(req.review_body, "review_body")?;
    let proposed_action = AgentWorkspacePrReviewActionKind::from_str(req.proposed_action.trim())
        .map_err(|error| json_error(StatusCode::BAD_REQUEST, error, None))?;
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
    let pr_number = review_pr_number(&workspace).ok_or_else(|| {
        json_error(
            StatusCode::BAD_REQUEST,
            "Review PR mode requires a linked pull request",
            None,
        )
    })?;
    let mut monitor = load_or_create_pr_review_monitor(
        state.app_state.as_ref(),
        &workspace,
        pr_number,
        Some(head_sha.clone()),
    )
    .await?;
    ensure_review_artifact_for_head(&monitor, &head_sha)?;

    let action = AgentWorkspacePrReviewAction::new(
        conversation_id.clone(),
        pr_number,
        head_sha.clone(),
        proposed_action,
        summary,
        review_body,
        req.findings_json,
        req.created_by_run_id.clone(),
    );
    let action = state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update_pr_review_action(action)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    monitor.status = AgentWorkspacePrReviewMonitorStatus::AwaitingUser;
    monitor.first_review_completed = true;
    monitor.last_reviewed_head_sha = Some(action.head_sha.clone());
    monitor.last_review_run_id = req.created_by_run_id;
    monitor.last_review_outcome = Some(proposed_action.to_string());
    monitor.last_error = None;
    let monitor = state
        .app_state
        .agent_conversation_workspace_repo
        .upsert_pr_review_monitor(monitor)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;

    Ok(Json(ProposeAgentWorkspacePrReviewActionResponse {
        success: true,
        monitor: AgentWorkspacePrReviewMonitorResponse::from(monitor),
        action: AgentWorkspacePrReviewActionResponse::from(action),
    }))
}

/// POST /api/agent-workspaces/{conversation_id}/complete-pr-review-run
pub async fn complete_agent_workspace_pr_review_run(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
    Json(req): Json<CompleteAgentWorkspacePrReviewRunRequest>,
) -> Result<Json<CompleteAgentWorkspacePrReviewRunResponse>, JsonError> {
    let summary = non_empty_string(req.summary, "summary")?;
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
    let pr_number = review_pr_number(&workspace).ok_or_else(|| {
        json_error(
            StatusCode::BAD_REQUEST,
            "Review PR mode requires a linked pull request",
            None,
        )
    })?;
    let head_sha = req.head_sha.or_else(|| review_pr_head_sha(&workspace));
    let mut monitor = load_or_create_pr_review_monitor(
        state.app_state.as_ref(),
        &workspace,
        pr_number,
        head_sha.clone(),
    )
    .await?;
    monitor.first_review_completed = true;
    monitor.last_seen_head_sha = head_sha.clone().or(monitor.last_seen_head_sha);
    monitor.last_reviewed_head_sha = head_sha.or(monitor.last_reviewed_head_sha);
    monitor.last_review_run_id = req.created_by_run_id;
    monitor.last_review_outcome = req.outcome.or_else(|| Some("no_action".to_string()));
    monitor.last_error = req.blocker.or_else(|| {
        if summary.trim().is_empty() {
            Some("Review run completed without a summary".to_string())
        } else {
            None
        }
    });
    monitor.status = if monitor.last_error.is_some() {
        AgentWorkspacePrReviewMonitorStatus::Blocked
    } else if monitor.monitor_enabled {
        AgentWorkspacePrReviewMonitorStatus::Watching
    } else {
        AgentWorkspacePrReviewMonitorStatus::Terminal
    };
    let monitor = state
        .app_state
        .agent_conversation_workspace_repo
        .upsert_pr_review_monitor(monitor)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    maybe_start_pr_review_monitor_polling(state.app_state.as_ref(), &workspace, &monitor).await;

    Ok(Json(CompleteAgentWorkspacePrReviewRunResponse {
        success: true,
        monitor: AgentWorkspacePrReviewMonitorResponse::from(monitor),
    }))
}

/// POST /api/agent-workspaces/{conversation_id}/pr-review-actions/{action_id}/submit
pub async fn submit_agent_workspace_pr_review_action(
    State(state): State<HttpServerState>,
    Path((conversation_id, action_id)): Path<(String, String)>,
    Json(req): Json<SubmitAgentWorkspacePrReviewActionRequest>,
) -> Result<Json<SubmitAgentWorkspacePrReviewActionResponse>, JsonError> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
    let action = state
        .app_state
        .agent_conversation_workspace_repo
        .get_pr_review_action(&action_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "PR review action not found", None))?;
    if action.conversation_id != conversation_id {
        return Err(json_error(
            StatusCode::NOT_FOUND,
            "PR review action not found for this workspace",
            None,
        ));
    }
    if action.status != AgentWorkspacePrReviewActionStatus::Pending {
        return Err(json_error(
            StatusCode::CONFLICT,
            "PR review action is no longer pending",
            None,
        ));
    }
    let override_kind = req
        .action_kind
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(AgentWorkspacePrReviewActionKind::from_str)
        .transpose()
        .map_err(|error| json_error(StatusCode::BAD_REQUEST, error, None))?;
    let action_kind = override_kind.unwrap_or(action.proposed_action);
    let event = pr_review_submission_event(action_kind);
    let pr_number = review_pr_number(&workspace).ok_or_else(|| {
        json_error(
            StatusCode::BAD_REQUEST,
            "Review PR mode requires a linked pull request",
            None,
        )
    })?;
    if action.pr_number != pr_number {
        return Err(json_error(
            StatusCode::CONFLICT,
            "PR review action belongs to a different pull request",
            None,
        ));
    }
    let github = state.app_state.github_service.as_ref().ok_or_else(|| {
        json_error(
            StatusCode::BAD_REQUEST,
            "GitHub review submission is unavailable",
            None,
        )
    })?;
    let current_head_sha = fetch_current_review_pr_head_sha(
        state.app_state.as_ref(),
        &workspace,
        pr_number,
        github.as_ref(),
    )
    .await?;
    if current_head_sha.as_deref() != Some(action.head_sha.as_str()) {
        return Err(json_error(
            StatusCode::CONFLICT,
            "Pull request head changed; run a fresh review before submitting",
            None,
        ));
    }
    let mut monitor = load_or_create_pr_review_monitor(
        state.app_state.as_ref(),
        &workspace,
        pr_number,
        Some(action.head_sha.clone()),
    )
    .await?;
    ensure_review_artifact_for_head(&monitor, &action.head_sha)?;

    state
        .app_state
        .agent_conversation_workspace_repo
        .update_pr_review_action_status(
            &action.id,
            AgentWorkspacePrReviewActionStatus::Submitting,
            None,
        )
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    monitor.status = AgentWorkspacePrReviewMonitorStatus::Submitting;
    monitor.last_error = None;
    let monitor = state
        .app_state
        .agent_conversation_workspace_repo
        .upsert_pr_review_monitor(monitor)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let submitted = match github
        .submit_pr_review(
            std::path::Path::new(&workspace.worktree_path),
            pr_number,
            event,
            &action.review_body,
        )
        .await
    {
        Ok(submitted) => submitted,
        Err(error) => {
            let error_message = error.to_string();
            state
                .app_state
                .agent_conversation_workspace_repo
                .update_pr_review_action_status(
                    &action.id,
                    AgentWorkspacePrReviewActionStatus::Pending,
                    None,
                )
                .await
                .map_err(|error| {
                    json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
                })?;
            let mut retry_monitor =
                monitor_for_retryable_submission_failure(monitor, error_message.clone());
            retry_monitor.last_seen_head_sha = Some(action.head_sha.clone());
            state
                .app_state
                .agent_conversation_workspace_repo
                .upsert_pr_review_monitor(retry_monitor)
                .await
                .map_err(|error| {
                    json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
                })?;
            return Err(json_error(
                StatusCode::BAD_GATEWAY,
                "Failed to submit GitHub PR review",
                Some(error_message),
            ));
        }
    };
    state
        .app_state
        .agent_conversation_workspace_repo
        .update_pr_review_action_status(
            &action.id,
            AgentWorkspacePrReviewActionStatus::Submitted,
            Some(&submitted.id),
        )
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let mut monitor = load_or_create_pr_review_monitor(
        state.app_state.as_ref(),
        &workspace,
        pr_number,
        Some(action.head_sha.clone()),
    )
    .await?;
    monitor.first_review_completed = true;
    monitor.last_seen_head_sha = Some(action.head_sha.clone());
    monitor.last_reviewed_head_sha = Some(action.head_sha.clone());
    monitor.last_review_outcome = Some(action_kind.to_string());
    monitor.last_submitted_review_id = Some(submitted.id.clone());
    monitor.last_error = None;
    if action_kind == AgentWorkspacePrReviewActionKind::RequestChanges {
        monitor.monitor_enabled = true;
        monitor.status = AgentWorkspacePrReviewMonitorStatus::Watching;
    } else {
        monitor.status = if monitor.monitor_enabled {
            AgentWorkspacePrReviewMonitorStatus::Watching
        } else {
            AgentWorkspacePrReviewMonitorStatus::Terminal
        };
    }
    let monitor = state
        .app_state
        .agent_conversation_workspace_repo
        .upsert_pr_review_monitor(monitor)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    maybe_start_pr_review_monitor_polling(state.app_state.as_ref(), &workspace, &monitor).await;
    let action = state
        .app_state
        .agent_conversation_workspace_repo
        .get_pr_review_action(&action.id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "PR review action not found", None))?;

    Ok(Json(SubmitAgentWorkspacePrReviewActionResponse {
        success: true,
        monitor: AgentWorkspacePrReviewMonitorResponse::from(monitor),
        action: AgentWorkspacePrReviewActionResponse::from(action),
        submitted_review_id: submitted.id,
        submitted_review_url: submitted.url,
    }))
}

/// POST /api/agent-workspaces/{conversation_id}/pr-review-actions/{action_id}/skip
pub async fn skip_agent_workspace_pr_review_action(
    State(state): State<HttpServerState>,
    Path((conversation_id, action_id)): Path<(String, String)>,
    Json(req): Json<SkipAgentWorkspacePrReviewActionRequest>,
) -> Result<Json<SkipAgentWorkspacePrReviewActionResponse>, JsonError> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
    let action = state
        .app_state
        .agent_conversation_workspace_repo
        .get_pr_review_action(&action_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "PR review action not found", None))?;
    if action.conversation_id != conversation_id {
        return Err(json_error(
            StatusCode::NOT_FOUND,
            "PR review action not found for this workspace",
            None,
        ));
    }
    if action.status != AgentWorkspacePrReviewActionStatus::Pending {
        return Err(json_error(
            StatusCode::CONFLICT,
            "PR review action is no longer pending",
            None,
        ));
    }
    state
        .app_state
        .agent_conversation_workspace_repo
        .update_pr_review_action_status(
            &action.id,
            AgentWorkspacePrReviewActionStatus::Skipped,
            None,
        )
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let mut monitor = load_or_create_pr_review_monitor(
        state.app_state.as_ref(),
        &workspace,
        action.pr_number,
        Some(action.head_sha.clone()),
    )
    .await?;
    monitor.first_review_completed = true;
    monitor.last_seen_head_sha = Some(action.head_sha.clone());
    monitor.last_reviewed_head_sha = Some(action.head_sha.clone());
    monitor.last_review_outcome = Some("skipped".to_string());
    monitor.last_error = req.reason;
    monitor.status = if monitor.monitor_enabled {
        AgentWorkspacePrReviewMonitorStatus::Watching
    } else {
        AgentWorkspacePrReviewMonitorStatus::Terminal
    };
    let monitor = state
        .app_state
        .agent_conversation_workspace_repo
        .upsert_pr_review_monitor(monitor)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    maybe_start_pr_review_monitor_polling(state.app_state.as_ref(), &workspace, &monitor).await;
    let action = state
        .app_state
        .agent_conversation_workspace_repo
        .get_pr_review_action(&action.id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "PR review action not found", None))?;

    Ok(Json(SkipAgentWorkspacePrReviewActionResponse {
        success: true,
        monitor: AgentWorkspacePrReviewMonitorResponse::from(monitor),
        action: AgentWorkspacePrReviewActionResponse::from(action),
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
            }
        }
    }

    if let Some(blocker) = req
        .blocker
        .as_deref()
        .map(str::trim)
        .filter(|blocker| !blocker.is_empty())
    {
        state
            .app_state
            .agent_conversation_workspace_repo
            .update_pr_auto_merge_state(
                &conversation_id,
                workspace.pr_auto_merge_current,
                Some("blocked"),
                Some(blocker),
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
                "pr_autofix_blocked",
                "blocked",
                blocker,
                Some("pr_autofix_blocker".to_string()),
            ))
            .await
            .map_err(|error| {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
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

    if !workspace.auto_publish_enabled {
        return complete_pr_fix_for_paused_auto_publish(
            state.app_state.as_ref(),
            &conversation_id,
            &workspace,
            summary,
        )
        .await;
    }

    state
        .app_state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "pr_autofix_completed",
            "succeeded",
            summary,
            Some("pr_autofix_completed".to_string()),
        ))
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;

    if let Some(response) =
        start_workspace_review_for_pr_fix_if_required(&state, &conversation_id, &workspace, summary)
            .await?
    {
        return Ok(response);
    }

    state
        .app_state
        .agent_conversation_workspace_repo
        .update_pr_auto_merge_state(
            &conversation_id,
            workspace.pr_auto_merge_current,
            Some("publishing"),
            Some("PR fix completed; publishing updates."),
        )
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;

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
        Some(state.team_service.clone()),
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

    if let Err(error) = push_publish_branch(github, &target.working_dir, &target.branch_name).await
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
        Box::pin(start_agent_workspace_review(state, workspace, force))
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
    let team_service = Some(Arc::clone(&state.team_service));
    resume_pr_fix_publish_after_passed_workspace_review(
        Arc::clone(&state.app_state.agent_conversation_workspace_repo),
        conversation_id,
        workspace,
        &review_context.monitor,
        review_context.target.as_ref(),
        move |conversation_id| {
            let app_state = Arc::clone(&app_state);
            let execution_state = Arc::clone(&execution_state);
            let team_service = team_service.clone();
            async move {
                publish_agent_conversation_workspace_for_app_state(
                    app_state.as_ref(),
                    &execution_state,
                    team_service,
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
    if !auto_publish_can_resume_after_workspace_review(workspace, monitor) {
        return Ok(());
    }

    let publishing_message = "Workspace Review passed; publishing initial pull request.";
    state
        .app_state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "initial_auto_publish_workspace_review_passed",
            "publishing",
            publishing_message,
            Some("workspace_review_passed".to_string()),
        ))
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;

    match publish_agent_conversation_workspace_for_app_state(
        state.app_state.as_ref(),
        &state.execution_state,
        Some(state.team_service.clone()),
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

fn auto_publish_can_resume_after_workspace_review(
    workspace: &AgentConversationWorkspace,
    monitor: &AgentWorkspaceReviewMonitor,
) -> bool {
    monitor.review_gate_status == AgentWorkspaceReviewGateStatus::Passed
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

async fn complete_pr_fix_for_paused_auto_publish(
    state: &AppState,
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    summary: &str,
) -> Result<Json<CompleteAgentWorkspacePrFixResponse>, JsonError> {
    let message = "PR fix completed, but Auto Publish is paused. Manual Commit & Publish is required to update the pull request.";
    state
        .agent_conversation_workspace_repo
        .update_pr_auto_merge_state(
            conversation_id,
            workspace.pr_auto_merge_current,
            Some("paused"),
            Some(message),
        )
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "pr_autofix_publish_skipped",
            "skipped",
            format!("{message} Fix summary: {summary}"),
            Some("auto_publish_paused".to_string()),
        ))
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let workspace_response = load_agent_workspace_response(state, conversation_id).await?;
    let pr_number = workspace_response.publication_pr_number;
    let pr_url = workspace_response.publication_pr_url.clone();
    Ok(Json(CompleteAgentWorkspacePrFixResponse {
        success: true,
        status: "publish_paused".to_string(),
        message: message.to_string(),
        workspace: Some(workspace_response),
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

fn review_pr_number(workspace: &AgentConversationWorkspace) -> Option<i64> {
    workspace
        .source_pull_request
        .as_ref()
        .map(|pull_request| pull_request.number)
        .or(workspace.publication_pr_number)
}

fn review_pr_url(workspace: &AgentConversationWorkspace) -> Option<String> {
    workspace
        .source_pull_request
        .as_ref()
        .and_then(|pull_request| pull_request.url.clone())
        .or_else(|| workspace.publication_pr_url.clone())
}

fn review_pr_head_sha(workspace: &AgentConversationWorkspace) -> Option<String> {
    workspace
        .source_pull_request
        .as_ref()
        .and_then(|pull_request| pull_request.head_ref_oid.clone())
}

async fn maybe_start_pr_review_monitor_polling(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    monitor: &AgentWorkspacePrReviewMonitor,
) {
    if workspace.mode != AgentConversationWorkspaceMode::ReviewPr
        || !monitor.monitor_enabled
        || monitor.status != AgentWorkspacePrReviewMonitorStatus::Watching
    {
        return;
    }
    if state
        .pr_poller_registry
        .is_agent_workspace_polling(&workspace.conversation_id)
    {
        return;
    }

    let Some(pr_number) = review_pr_number(workspace) else {
        tracing::warn!(
            conversation_id = workspace.conversation_id.as_str(),
            "Review PR monitor could not start because the workspace has no PR number"
        );
        return;
    };
    if monitor.pr_number != pr_number {
        tracing::warn!(
            conversation_id = workspace.conversation_id.as_str(),
            monitor_pr_number = monitor.pr_number,
            workspace_pr_number = pr_number,
            "Review PR monitor could not start because monitor/workspace PR numbers differ"
        );
        return;
    }

    let project = match state.project_repo.get_by_id(&workspace.project_id).await {
        Ok(Some(project)) => project,
        Ok(None) => {
            tracing::warn!(
                conversation_id = workspace.conversation_id.as_str(),
                project_id = workspace.project_id.as_str(),
                "Review PR monitor could not start because the project was not found"
            );
            return;
        }
        Err(error) => {
            tracing::warn!(
                conversation_id = workspace.conversation_id.as_str(),
                project_id = workspace.project_id.as_str(),
                error = %error,
                "Review PR monitor failed to load project before poller start"
            );
            return;
        }
    };
    let worktree_path =
        match resolve_valid_agent_conversation_workspace_path(&project, workspace).await {
            Ok(path) => path,
            Err(error) => {
                tracing::warn!(
                    conversation_id = workspace.conversation_id.as_str(),
                    pr_number,
                    error = %error,
                    "Review PR monitor could not start because the workspace path is not usable"
                );
                return;
            }
        };

    let chat_service: Arc<dyn crate::application::chat_service::ChatService> =
        Arc::new(state.build_chat_service());
    state.pr_poller_registry.start_agent_workspace_polling(
        workspace.conversation_id.clone(),
        pr_number,
        project,
        worktree_path,
        Arc::clone(&state.agent_conversation_workspace_repo),
        Arc::clone(&state.agent_run_repo),
        chat_service,
    );
}

async fn fetch_review_pr_remote_context(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    pr_number: i64,
) -> Result<(Option<PrHealth>, Option<PrReviewFeedback>), JsonError> {
    let Some(github) = state.github_service.as_ref() else {
        return Ok((None, None));
    };
    let working_dir = std::path::Path::new(&workspace.worktree_path);
    let health = github.fetch_pr_health(working_dir, pr_number).await.ok();
    if let Some(health) = health.as_ref() {
        import_agent_workspace_pr_comment_evidence(
            Arc::clone(&state.agent_conversation_workspace_repo),
            &workspace.conversation_id,
            pr_number,
            health,
        )
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    }
    let review_feedback = github
        .check_pr_review_feedback(working_dir, pr_number)
        .await
        .ok()
        .flatten();
    Ok((health, review_feedback))
}

async fn fetch_current_review_pr_head_sha(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    pr_number: i64,
    github: &dyn GithubServiceTrait,
) -> Result<Option<String>, JsonError> {
    let working_dir = std::path::Path::new(&workspace.worktree_path);
    let remote_head = github
        .fetch_pr_health(working_dir, pr_number)
        .await
        .ok()
        .and_then(|health| health.sync_state.head_ref_oid);
    let head_sha = remote_head.or_else(|| review_pr_head_sha(workspace));
    if head_sha.is_none() {
        tracing::warn!(
            conversation_id = workspace.conversation_id.as_str(),
            pr_number,
            "Review PR submit could not resolve current head SHA"
        );
    }
    let _ = state;
    Ok(head_sha)
}

async fn load_or_create_pr_review_monitor(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    pr_number: i64,
    head_sha: Option<String>,
) -> Result<AgentWorkspacePrReviewMonitor, JsonError> {
    let existing = state
        .agent_conversation_workspace_repo
        .get_pr_review_monitor(&workspace.conversation_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    Ok(existing.unwrap_or_else(|| {
        AgentWorkspacePrReviewMonitor::new(
            workspace.conversation_id.clone(),
            workspace.project_id.clone(),
            pr_number,
            head_sha,
        )
    }))
}

fn ensure_review_artifact_for_head(
    monitor: &AgentWorkspacePrReviewMonitor,
    head_sha: &str,
) -> Result<(), JsonError> {
    let has_matching_artifact = monitor.review_artifact_id.is_some()
        && monitor.review_artifact_head_sha.as_deref() == Some(head_sha);
    if has_matching_artifact {
        return Ok(());
    }

    Err(json_error(
        StatusCode::CONFLICT,
        "Write the Review for the current PR head before proposing or submitting a PR review action",
        None,
    ))
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
    if monitor.status != AgentWorkspaceReviewMonitorStatus::Reviewing {
        return Ok(created_by_run_id);
    }
    let Some(active_run_id) = monitor.last_run_id.as_deref() else {
        return Err(json_error(
            StatusCode::CONFLICT,
            format!("{operation} requires an active workspace Review run id"),
            None,
        ));
    };
    match created_by_run_id.as_deref() {
        Some(run_id) if run_id == active_run_id => Ok(created_by_run_id),
        Some(_) => Err(json_error(
            StatusCode::CONFLICT,
            format!("{operation} run id does not match the active workspace Review run"),
            None,
        )),
        None => Err(json_error(
            StatusCode::BAD_REQUEST,
            format!("{operation} requires created_by_run_id for the active workspace Review run"),
            None,
        )),
    }
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

fn pr_review_submission_event(
    action_kind: AgentWorkspacePrReviewActionKind,
) -> PrReviewSubmissionEvent {
    match action_kind {
        AgentWorkspacePrReviewActionKind::RequestChanges => PrReviewSubmissionEvent::RequestChanges,
        AgentWorkspacePrReviewActionKind::Approve => PrReviewSubmissionEvent::Approve,
        AgentWorkspacePrReviewActionKind::Comment => PrReviewSubmissionEvent::Comment,
    }
}

fn monitor_for_retryable_submission_failure(
    mut monitor: AgentWorkspacePrReviewMonitor,
    error: String,
) -> AgentWorkspacePrReviewMonitor {
    monitor.status = AgentWorkspacePrReviewMonitorStatus::AwaitingUser;
    monitor.last_error = Some(error);
    monitor
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
    matches!(
        push_status,
        Some("checking" | "committing" | "refreshing" | "describing" | "pushing")
    )
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
        Some(event) if event.step == "repair_sent" => event.status == "succeeded",
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
        has_conflict_markers,
    })
    .map_err(|error| json_error(StatusCode::CONFLICT, error, None))?;

    let mut updated_workspace = workspace.clone();
    updated_workspace.base_commit = Some(freshness.target_base_commit.clone());
    updated_workspace.publication_push_status = Some("refreshed".to_string());
    state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(updated_workspace.clone())
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
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
    let publication_events = state
        .app_state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let post_repair_action = agent_workspace_post_repair_action_from_events(&publication_events);

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
        } else if let Some(github) = state.app_state.github_service.as_ref() {
            match push_publish_branch(
                github,
                &publish_target.worktree_path,
                &publish_target.branch_name,
            )
            .await
            {
                Ok(()) => {
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
                            conversation_id,
                            "published",
                            "succeeded",
                            "Plan branch repair pushed",
                            None,
                        ))
                        .await
                        .map_err(|error| {
                            json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
                        })?;
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
            Some(state.team_service.clone()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path as StdPath, PathBuf};
    use std::pin::Pin;
    use std::process::Command;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use crate::application::agent_conversation_workspace::{
        resolve_agent_conversation_workspace_path, resolve_linked_plan_branch_agent_worktree_path,
    };
    use crate::application::agent_workspace_review::{
        AgentWorkspaceReviewChangedFile, AgentWorkspaceReviewContext,
        AgentWorkspaceReviewDiffSummary, AgentWorkspaceReviewHunkAnchor,
        AgentWorkspaceReviewPacket,
    };
    use crate::application::agent_workspace_review_publish_handoff::pr_fix_publish_can_resume_after_workspace_review;
    use crate::application::{AppState, TeamService, TeamStateTracker};
    use crate::commands::ExecutionState;
    use crate::domain::agents::{
        AgentConfig, AgentHandle, AgentOutput, AgentResponse, AgentResult, AgenticClient,
        ClientCapabilities, ResponseChunk,
    };
    use crate::domain::entities::plan_branch::{
        PrPushStatus as PlanPrPushStatus, PrStatus as PlanPrStatus,
    };
    use crate::domain::entities::{
        AgentConversationWorkspace, AgentConversationWorkspaceMode,
        AgentWorkspacePrCommentEvidenceUpsert, AgentWorkspacePrDescription,
        AgentWorkspaceReviewGateStatus, AgentWorkspaceReviewMonitorStatus,
        AgentWorkspaceReviewOutcome, AgentWorkspaceSourcePullRequest, ArtifactId, ChatContextType,
        ChatConversation, IdeationAnalysisBaseRefKind, IdeationSessionId, PlanBranch, PlanBranchId,
        Project, ProjectId, TaskId,
    };
    use crate::domain::repositories::AgentConversationWorkspaceRepository;
    use crate::domain::review::ReviewSettings;
    use crate::domain::services::github_service::{
        GithubServiceTrait, PrHealth, PrIssueCommentSummary, PrReviewSubmissionEvent, PrStatus,
        PrSyncState,
    };
    use crate::tests::mock_github_service::MockGithubService;
    use async_trait::async_trait;
    use futures::{stream, Stream};

    fn git(repo: impl AsRef<StdPath>, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("git command should spawn");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn test_http_state(app_state: Arc<AppState>) -> HttpServerState {
        let tracker = TeamStateTracker::new();
        let team_service = Arc::new(TeamService::new_without_events(Arc::new(tracker.clone())));
        HttpServerState {
            app_state,
            execution_state: Arc::new(ExecutionState::new()),
            team_tracker: tracker,
            team_service,
            delegation_service: Default::default(),
        }
    }

    struct RecordingWorkspaceReviewStarter {
        calls: AtomicUsize,
    }

    impl RecordingWorkspaceReviewStarter {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl WorkspaceReviewStarter for RecordingWorkspaceReviewStarter {
        fn start<'a>(
            &'a self,
            state: Arc<AppState>,
            workspace: &'a AgentConversationWorkspace,
            force: bool,
        ) -> WorkspaceReviewStartFuture<'a> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                assert!(!force, "repair completion should start a normal refresh");
                let context =
                    load_agent_workspace_review_context(state.as_ref(), workspace).await?;
                let target = context.target;
                assert!(
                    target.is_some(),
                    "repair refresh should still have reviewable changes"
                );
                let mut monitor = context.monitor;
                monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
                monitor.review_outcome = AgentWorkspaceReviewOutcome::None;
                monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
                monitor.review_blocking_summary = None;
                monitor.review_blocking_fingerprint = None;
                monitor.review_fixer_run_id = None;
                monitor.review_fixer_conversation_id = None;
                monitor.review_fixer_status = None;
                monitor.last_run_id = Some("workspace-review-run-after-repair".to_string());
                let monitor = state
                    .agent_conversation_workspace_repo
                    .upsert_workspace_review_monitor(monitor)
                    .await?;
                Ok(AgentWorkspaceReviewStart {
                    context: AgentWorkspaceReviewContext {
                        monitor,
                        target,
                        goal_context: AgentWorkspaceReviewGoalContext::default(),
                        is_current: false,
                        is_outdated: false,
                        should_show_tab: true,
                    },
                    started: true,
                    skipped_reason: None,
                    was_queued: false,
                })
            })
        }
    }

    struct SubmittingPrDescriptionClient {
        repo: Arc<dyn AgentConversationWorkspaceRepository>,
        conversation_id: ChatConversationId,
    }

    impl SubmittingPrDescriptionClient {
        fn new(
            repo: Arc<dyn AgentConversationWorkspaceRepository>,
            conversation_id: ChatConversationId,
        ) -> Self {
            Self {
                repo,
                conversation_id,
            }
        }
    }

    #[async_trait]
    impl AgenticClient for SubmittingPrDescriptionClient {
        async fn spawn_agent(&self, config: AgentConfig) -> AgentResult<AgentHandle> {
            Ok(AgentHandle::mock(config.role))
        }

        async fn stop_agent(&self, _handle: &AgentHandle) -> AgentResult<()> {
            Ok(())
        }

        async fn wait_for_completion(&self, _handle: &AgentHandle) -> AgentResult<AgentOutput> {
            self.repo
                .save_pr_description(
                    &self.conversation_id,
                    AgentWorkspacePrDescription::new(
                        Some("Cached publication title".to_string()),
                        "## Summary\n\nReady to publish.".to_string(),
                    ),
                )
                .await
                .expect("test PR description should save");
            Ok(AgentOutput::success("submitted"))
        }

        async fn send_prompt(
            &self,
            _handle: &AgentHandle,
            _prompt: &str,
        ) -> AgentResult<AgentResponse> {
            Ok(AgentResponse::new(""))
        }

        fn stream_response(
            &self,
            _handle: &AgentHandle,
            _prompt: &str,
        ) -> Pin<Box<dyn Stream<Item = AgentResult<ResponseChunk>> + Send>> {
            Box::pin(stream::empty())
        }

        fn capabilities(&self) -> &ClientCapabilities {
            static CAPS: std::sync::OnceLock<ClientCapabilities> = std::sync::OnceLock::new();
            CAPS.get_or_init(ClientCapabilities::mock)
        }

        async fn is_available(&self) -> AgentResult<bool> {
            Ok(true)
        }
    }

    #[test]
    fn workspace_review_default_title_uses_target_identity() {
        let selected_pr_target =
            crate::application::agent_workspace_review::AgentWorkspaceReviewTarget {
                scope: AgentWorkspaceReviewTargetScope::SelectedSource,
                base_ref: "main".to_string(),
                base_sha: Some("base".to_string()),
                head_ref: "refs/ralphx/pr-heads/347".to_string(),
                head_sha: Some("head".to_string()),
                diff_fingerprint: "fingerprint".to_string(),
                working_directory: PathBuf::from("/tmp/worktree"),
                source_pull_request_number: Some(347),
                review_packet: AgentWorkspaceReviewPacket::default(),
            };
        assert_eq!(
            default_workspace_review_artifact_title(
                AgentWorkspaceReviewTargetScope::SelectedSource,
                Some(&selected_pr_target),
            ),
            "PR #347"
        );

        let selected_branch_target =
            crate::application::agent_workspace_review::AgentWorkspaceReviewTarget {
                scope: AgentWorkspaceReviewTargetScope::SelectedSource,
                base_ref: "main".to_string(),
                base_sha: Some("base".to_string()),
                head_ref: "refs/heads/feature/review-sidecar".to_string(),
                head_sha: Some("head".to_string()),
                diff_fingerprint: "fingerprint".to_string(),
                working_directory: PathBuf::from("/tmp/worktree"),
                source_pull_request_number: None,
                review_packet: AgentWorkspaceReviewPacket::default(),
            };
        assert_eq!(
            default_workspace_review_artifact_title(
                AgentWorkspaceReviewTargetScope::SelectedSource,
                Some(&selected_branch_target),
            ),
            "feature/review-sidecar"
        );

        assert_eq!(
            default_workspace_review_artifact_title(
                AgentWorkspaceReviewTargetScope::WorkspaceDelta,
                Some(&selected_branch_target),
            ),
            "Workspace changes"
        );
    }

    #[test]
    fn workspace_review_target_response_includes_packet_only_when_requested() {
        let target = crate::application::agent_workspace_review::AgentWorkspaceReviewTarget {
            scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
            base_ref: "main".to_string(),
            base_sha: Some("base".to_string()),
            head_ref: "HEAD".to_string(),
            head_sha: Some("head".to_string()),
            diff_fingerprint: "fingerprint".to_string(),
            working_directory: PathBuf::from("/tmp/worktree"),
            source_pull_request_number: None,
            review_packet: AgentWorkspaceReviewPacket {
                summary: AgentWorkspaceReviewDiffSummary {
                    files_changed: 1,
                    insertions: 2,
                    deletions: 0,
                },
                changed_files: vec![AgentWorkspaceReviewChangedFile {
                    path: "src/lib.rs".to_string(),
                    status: "modified".to_string(),
                    sources: vec!["committed".to_string()],
                }],
                hunk_anchors: vec![
                    crate::application::agent_workspace_review::AgentWorkspaceReviewHunkAnchor {
                        path: "src/lib.rs".to_string(),
                        source: "committed".to_string(),
                        hunk_header: "@@ -1,1 +1,2 @@".to_string(),
                        old_start: 1,
                        old_lines: 1,
                        new_start: 1,
                        new_lines: 2,
                    },
                ],
                patch_excerpt: "diff --git a/src/lib.rs b/src/lib.rs".to_string(),
                patch_excerpt_truncated: false,
                notes: vec![],
            },
        };

        let default_response = AgentWorkspaceReviewTargetResponse::from(target.clone());
        assert!(default_response.review_packet.is_none());

        let packet_response = AgentWorkspaceReviewTargetResponse::from_target(target, true)
            .review_packet
            .expect("packet should be included when requested");
        assert_eq!(packet_response.summary.files_changed, 1);
        assert_eq!(packet_response.changed_files[0].path, "src/lib.rs");
        assert_eq!(packet_response.hunk_anchors[0].source, "committed");
        assert_eq!(packet_response.hunk_anchors[0].new_lines, 2);
        assert_eq!(
            packet_response.patch_excerpt,
            "diff --git a/src/lib.rs b/src/lib.rs"
        );
    }

    #[test]
    fn workspace_review_tool_target_metadata_requires_current_target() {
        let target = crate::application::agent_workspace_review::AgentWorkspaceReviewTarget {
            scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
            base_ref: "main".to_string(),
            base_sha: Some("base".to_string()),
            head_ref: "HEAD".to_string(),
            head_sha: Some("head".to_string()),
            diff_fingerprint: "fingerprint".to_string(),
            working_directory: PathBuf::from("/tmp/worktree"),
            source_pull_request_number: None,
            review_packet: AgentWorkspaceReviewPacket::default(),
        };

        let accepted = validate_workspace_review_tool_target_metadata(
            &target,
            Some("workspace_delta"),
            Some("head"),
            Some("fingerprint"),
            "workspace Review artifact write",
        )
        .expect("matching target metadata should be accepted");
        assert_eq!(accepted.0, AgentWorkspaceReviewTargetScope::WorkspaceDelta);
        assert_eq!(accepted.1.as_deref(), Some("head"));
        assert_eq!(accepted.2, "fingerprint");

        assert!(validate_workspace_review_tool_target_metadata(
            &target,
            Some("workspace_delta"),
            None,
            Some("fingerprint"),
            "workspace Review artifact write",
        )
        .is_err());
        assert!(validate_workspace_review_tool_target_metadata(
            &target,
            Some("workspace_delta"),
            Some("head"),
            Some("stale-fingerprint"),
            "workspace Review artifact write",
        )
        .is_err());
    }

    #[test]
    fn workspace_review_tool_run_id_requires_active_review_run_match() {
        let conversation_id = ChatConversationId::new();
        let mut monitor =
            AgentWorkspaceReviewMonitor::new(conversation_id.clone(), ProjectId::new());
        monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
        monitor.last_run_id = Some("run-current".to_string());

        assert_eq!(
            validate_workspace_review_tool_run_id(
                &monitor,
                Some("run-current"),
                "workspace Review completion",
            )
            .expect("matching run id should be accepted")
            .as_deref(),
            Some("run-current")
        );
        assert!(validate_workspace_review_tool_run_id(
            &monitor,
            Some("run-stale"),
            "workspace Review completion",
        )
        .is_err());
        assert!(validate_workspace_review_tool_run_id(
            &monitor,
            None,
            "workspace Review completion",
        )
        .is_err());

        monitor.last_run_id = None;
        assert!(validate_workspace_review_tool_run_id(
            &monitor,
            Some("run-current"),
            "workspace Review completion",
        )
        .is_err());

        monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
        assert!(validate_workspace_review_tool_run_id(
            &monitor,
            None,
            "workspace Review completion",
        )
        .expect("idle monitor should not require an active child run id")
        .is_none());
    }

    #[test]
    fn workspace_review_hunk_annotation_validation_accepts_current_anchor() {
        let target = crate::application::agent_workspace_review::AgentWorkspaceReviewTarget {
            scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
            base_ref: "main".to_string(),
            base_sha: Some("base".to_string()),
            head_ref: "HEAD".to_string(),
            head_sha: Some("head".to_string()),
            diff_fingerprint: "fingerprint".to_string(),
            working_directory: PathBuf::from("/tmp/worktree"),
            source_pull_request_number: None,
            review_packet: AgentWorkspaceReviewPacket {
                summary: AgentWorkspaceReviewDiffSummary::default(),
                changed_files: Vec::new(),
                hunk_anchors: vec![AgentWorkspaceReviewHunkAnchor {
                    path: "src/lib.rs".to_string(),
                    source: "committed".to_string(),
                    hunk_header: "@@ -1,1 +1,2 @@".to_string(),
                    old_start: 1,
                    old_lines: 1,
                    new_start: 1,
                    new_lines: 2,
                }],
                patch_excerpt: String::new(),
                patch_excerpt_truncated: false,
                notes: Vec::new(),
            },
        };

        let validation = validate_workspace_review_hunk_annotation_requests(
            vec![WriteAgentWorkspaceReviewHunkAnnotationRequest {
                path: "src/lib.rs".to_string(),
                source: "committed".to_string(),
                hunk_header: "@@ -1,1 +1,2 @@".to_string(),
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 2,
                title: Some("Library update".to_string()),
                message: "Explains the reviewed hunk.".to_string(),
                level: Some("notice".to_string()),
            }],
            Some(&target),
            AgentWorkspaceReviewTargetScope::WorkspaceDelta,
            Some("head"),
            "fingerprint",
        )
        .expect("annotation should match current anchor");

        assert_eq!(validation.accepted.len(), 1);
        assert!(validation.rejected.is_empty());
        assert_eq!(validation.accepted[0].path, "src/lib.rs");
        assert_eq!(validation.accepted[0].level, "notice");
    }

    #[test]
    fn workspace_review_hunk_annotation_validation_partially_rejects_unmatched_anchor() {
        let target = crate::application::agent_workspace_review::AgentWorkspaceReviewTarget {
            scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
            base_ref: "main".to_string(),
            base_sha: Some("base".to_string()),
            head_ref: "HEAD".to_string(),
            head_sha: Some("head".to_string()),
            diff_fingerprint: "fingerprint".to_string(),
            working_directory: PathBuf::from("/tmp/worktree"),
            source_pull_request_number: None,
            review_packet: AgentWorkspaceReviewPacket {
                hunk_anchors: vec![AgentWorkspaceReviewHunkAnchor {
                    path: "src/lib.rs".to_string(),
                    source: "committed".to_string(),
                    hunk_header: "@@ -1,1 +1,2 @@".to_string(),
                    old_start: 1,
                    old_lines: 1,
                    new_start: 1,
                    new_lines: 2,
                }],
                ..AgentWorkspaceReviewPacket::default()
            },
        };

        let validation = validate_workspace_review_hunk_annotation_requests(
            vec![
                WriteAgentWorkspaceReviewHunkAnnotationRequest {
                    path: "src/lib.rs".to_string(),
                    source: "committed".to_string(),
                    hunk_header: "@@ -1,1 +1,2 @@".to_string(),
                    old_start: 1,
                    old_lines: 1,
                    new_start: 1,
                    new_lines: 2,
                    title: None,
                    message: "Explains the reviewed hunk.".to_string(),
                    level: None,
                },
                WriteAgentWorkspaceReviewHunkAnnotationRequest {
                    path: "src/lib.rs".to_string(),
                    source: "committed".to_string(),
                    hunk_header: "@@ -10,1 +10,2 @@".to_string(),
                    old_start: 10,
                    old_lines: 1,
                    new_start: 10,
                    new_lines: 2,
                    title: None,
                    message: "This hunk is not in the current packet.".to_string(),
                    level: None,
                },
            ],
            Some(&target),
            AgentWorkspaceReviewTargetScope::WorkspaceDelta,
            Some("head"),
            "fingerprint",
        )
        .expect("batch metadata should be valid");

        assert_eq!(validation.accepted.len(), 1);
        assert_eq!(validation.rejected.len(), 1);
        assert!(validation.rejected[0]
            .reason
            .as_deref()
            .expect("rejection should include reason")
            .contains("does not match any current workspace review hunk anchor"));
    }

    #[test]
    fn workspace_review_missing_hunk_anchors_requires_every_anchor() {
        let target = crate::application::agent_workspace_review::AgentWorkspaceReviewTarget {
            scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
            base_ref: "main".to_string(),
            base_sha: Some("base".to_string()),
            head_ref: "HEAD".to_string(),
            head_sha: Some("head".to_string()),
            diff_fingerprint: "fingerprint".to_string(),
            working_directory: PathBuf::from("/tmp/worktree"),
            source_pull_request_number: None,
            review_packet: AgentWorkspaceReviewPacket {
                hunk_anchors: vec![
                    AgentWorkspaceReviewHunkAnchor {
                        path: "src/lib.rs".to_string(),
                        source: "committed".to_string(),
                        hunk_header: "@@ -1,1 +1,2 @@".to_string(),
                        old_start: 1,
                        old_lines: 1,
                        new_start: 1,
                        new_lines: 2,
                    },
                    AgentWorkspaceReviewHunkAnchor {
                        path: "src/main.rs".to_string(),
                        source: "committed".to_string(),
                        hunk_header: "@@ -5,1 +5,3 @@".to_string(),
                        old_start: 5,
                        old_lines: 1,
                        new_start: 5,
                        new_lines: 3,
                    },
                ],
                ..AgentWorkspaceReviewPacket::default()
            },
        };
        let annotation = AgentWorkspaceReviewHunkAnnotation {
            id: "annotation-1".to_string(),
            conversation_id: ChatConversationId::from_string("conversation-1".to_string()),
            project_id: ProjectId::from_string("project-1".to_string()),
            artifact_id: ArtifactId::from_string("artifact-1".to_string()),
            artifact_version: 1,
            target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
            head_sha: Some("head".to_string()),
            diff_fingerprint: "fingerprint".to_string(),
            path: "src/lib.rs".to_string(),
            diff_source: "committed".to_string(),
            hunk_header: "@@ -1,1 +1,2 @@".to_string(),
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 2,
            title: None,
            message: "Explains the first hunk.".to_string(),
            level: "notice".to_string(),
            created_by_run_id: Some("run-1".to_string()),
            created_at: chrono::Utc::now(),
        };

        let missing = missing_workspace_review_hunk_anchors(&target, &[annotation]);

        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].path, "src/main.rs");
    }

    #[test]
    fn workspace_review_completion_treats_hunk_coverage_as_best_effort() {
        assert!(!workspace_review_completion_requires_hunk_coverage(Some(
            "passed"
        )));
        assert!(!workspace_review_completion_requires_hunk_coverage(Some(
            "blocking"
        )));
        assert!(!workspace_review_completion_requires_hunk_coverage(Some(
            "no_changes"
        )));
        assert!(!workspace_review_completion_requires_hunk_coverage(Some(
            "run_failed"
        )));
        assert!(!workspace_review_completion_requires_hunk_coverage(None));
        assert!(!workspace_review_completion_requires_hunk_coverage(Some(
            "bogus"
        )));
    }

    #[test]
    fn workspace_review_artifact_title_replaces_legacy_or_stale_titles() {
        let selected_pr_target =
            crate::application::agent_workspace_review::AgentWorkspaceReviewTarget {
                scope: AgentWorkspaceReviewTargetScope::SelectedSource,
                base_ref: "main".to_string(),
                base_sha: Some("base".to_string()),
                head_ref: "refs/ralphx/pr-heads/347".to_string(),
                head_sha: Some("head".to_string()),
                diff_fingerprint: "fingerprint".to_string(),
                working_directory: PathBuf::from("/tmp/worktree"),
                source_pull_request_number: Some(347),
                review_packet: AgentWorkspaceReviewPacket::default(),
            };

        assert_eq!(
            workspace_review_artifact_title(
                Some("Selected Source Review".to_string()),
                None,
                None,
                AgentWorkspaceReviewTargetScope::SelectedSource,
                Some(&selected_pr_target),
            ),
            "PR #347"
        );
        assert_eq!(
            workspace_review_artifact_title(
                None,
                Some("PR #123"),
                Some(AgentWorkspaceReviewTargetScope::SelectedSource),
                AgentWorkspaceReviewTargetScope::WorkspaceDelta,
                Some(&selected_pr_target),
            ),
            "Workspace changes"
        );
        assert_eq!(
            workspace_review_artifact_title(
                None,
                Some("Custom review title"),
                Some(AgentWorkspaceReviewTargetScope::SelectedSource),
                AgentWorkspaceReviewTargetScope::SelectedSource,
                Some(&selected_pr_target),
            ),
            "Custom review title"
        );
    }

    #[test]
    fn workspace_review_content_normalization_removes_redundant_h1() {
        assert_eq!(
            normalize_workspace_review_artifact_content(
                "# Selected Source Review\n\n## Summary\n\nLooks good.".to_string(),
            ),
            "## Summary\n\nLooks good."
        );
        assert_eq!(
            normalize_workspace_review_artifact_content(
                "# Workspace Review\r\n\r\n## Summary\r\n\r\nLooks good.".to_string(),
            ),
            "## Summary\r\n\r\nLooks good."
        );
        assert_eq!(
            normalize_workspace_review_artifact_content(
                "# Review\n\n## Summary\n\nLooks good.".to_string(),
            ),
            "## Summary\n\nLooks good."
        );
        assert_eq!(
            normalize_workspace_review_artifact_content(
                "# Useful Architecture Context\n\n## Summary\n\nKeep this title.".to_string(),
            ),
            "# Useful Architecture Context\n\n## Summary\n\nKeep this title."
        );
    }

    fn test_workspace(conversation_id: ChatConversationId) -> AgentConversationWorkspace {
        AgentConversationWorkspace::new(
            conversation_id,
            ProjectId::new(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("main".to_string()),
            Some("0".repeat(40)),
            "feature/pr-description".to_string(),
            "/tmp/pr-description-worktree".to_string(),
        )
    }

    fn test_workspace_review_target() -> AgentWorkspaceReviewTarget {
        AgentWorkspaceReviewTarget {
            scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
            base_ref: "main".to_string(),
            base_sha: Some("0".repeat(40)),
            head_ref: "HEAD".to_string(),
            head_sha: None,
            diff_fingerprint: "workspace-diff-fingerprint".to_string(),
            working_directory: PathBuf::from("/tmp/pr-description-worktree"),
            source_pull_request_number: None,
            review_packet: Default::default(),
        }
    }

    fn mark_monitor_current_passed(
        monitor: &mut AgentWorkspaceReviewMonitor,
        target: &AgentWorkspaceReviewTarget,
    ) {
        monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
        monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
        monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;
        monitor.review_artifact_id = Some(ArtifactId::from_string("review-artifact"));
        monitor.reviewed_target_scope = Some(target.scope);
        monitor.reviewed_head_sha = target.head_sha.clone();
        monitor.reviewed_diff_fingerprint = Some(target.diff_fingerprint.clone());
        monitor.current_target_scope = Some(target.scope);
        monitor.current_diff_fingerprint = Some(target.diff_fingerprint.clone());
    }

    #[test]
    fn initial_auto_publish_resume_predicate_requires_armed_initial_flag_and_no_pr() {
        let conversation_id = ChatConversationId::new();
        let mut workspace = test_workspace(conversation_id.clone());
        workspace.auto_publish_initial_pr_enabled = true;
        workspace.auto_publish_enabled = false;
        workspace.publication_pr_number = None;
        let mut monitor =
            AgentWorkspaceReviewMonitor::new(conversation_id, workspace.project_id.clone());
        monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;

        // Armed initial flag + no PR + gate Passed → resume.
        assert!(auto_publish_can_resume_after_workspace_review(
            &workspace, &monitor
        ));

        // Not armed for the initial PR → no resume (even if the PR-fix flag is on).
        workspace.auto_publish_initial_pr_enabled = false;
        workspace.auto_publish_enabled = true;
        assert!(!auto_publish_can_resume_after_workspace_review(
            &workspace, &monitor
        ));
        workspace.auto_publish_initial_pr_enabled = true;
        workspace.auto_publish_enabled = false;

        // A publication PR already exists → this is the PR-fix path, not initial publish.
        workspace.publication_pr_number = Some(512);
        assert!(!auto_publish_can_resume_after_workspace_review(
            &workspace, &monitor
        ));
        workspace.publication_pr_number = None;

        // Gate not Passed → no resume.
        monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
        assert!(!auto_publish_can_resume_after_workspace_review(
            &workspace, &monitor
        ));
        monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;

        // Terminal publication status → no resume.
        workspace.publication_pr_status = Some("merged".to_string());
        assert!(!auto_publish_can_resume_after_workspace_review(
            &workspace, &monitor
        ));
    }

    #[test]
    fn review_completion_resume_predicate_only_allows_review_gate_blocks() {
        let conversation_id = ChatConversationId::new();
        let mut workspace = test_workspace(conversation_id.clone());
        workspace.publication_pr_number = Some(267);
        workspace.publication_pr_status = Some("open".to_string());
        workspace.auto_publish_enabled = true;
        workspace.pr_autofix_enabled = true;
        workspace.pr_supervision_status = Some("blocked".to_string());
        workspace.pr_supervision_summary =
            Some("Workspace Review is required before publishing".to_string());
        let mut monitor =
            AgentWorkspaceReviewMonitor::new(conversation_id, workspace.project_id.clone());
        let target = test_workspace_review_target();
        mark_monitor_current_passed(&mut monitor, &target);

        assert!(pr_fix_publish_can_resume_after_workspace_review(
            &workspace,
            &monitor,
            Some(&target),
            &[]
        ));

        workspace.pr_supervision_summary =
            Some("Workspace reviewer completed without writing a current Review".to_string());

        assert!(pr_fix_publish_can_resume_after_workspace_review(
            &workspace,
            &monitor,
            Some(&target),
            &[]
        ));

        workspace.pr_supervision_summary = Some(
            "PR fix publish failed: Workspace reviewer completed without writing a current Review"
                .to_string(),
        );

        assert!(pr_fix_publish_can_resume_after_workspace_review(
            &workspace,
            &monitor,
            Some(&target),
            &[]
        ));

        workspace.pr_supervision_summary = Some("Required checks are still pending.".to_string());

        assert!(!pr_fix_publish_can_resume_after_workspace_review(
            &workspace,
            &monitor,
            Some(&target),
            &[]
        ));
    }

    fn test_freshness(
        is_base_ahead: bool,
        has_uncommitted_changes: bool,
        unpublished_commit_count: Option<u32>,
        base_status: &str,
    ) -> AgentConversationWorkspaceFreshnessResponse {
        AgentConversationWorkspaceFreshnessResponse {
            conversation_id: ChatConversationId::new().as_str(),
            freshness_scope: "full".to_string(),
            base_ref: "main".to_string(),
            base_display_name: Some("main".to_string()),
            target_ref: "origin/main".to_string(),
            captured_base_commit: Some("0".repeat(40)),
            target_base_commit: "1".repeat(40),
            is_base_ahead,
            has_uncommitted_changes,
            unpublished_commit_count,
            remote_refreshed: true,
            worktree_status_checked: true,
            base_status: base_status.to_string(),
            effective_base_ref: Some("main".to_string()),
            effective_base_display_name: Some("main".to_string()),
            base_block_reason: (base_status == "blocked")
                .then_some("Workspace base is blocked".to_string()),
        }
    }

    async fn seed_current_passing_workspace_review(
        app_state: &AppState,
        workspace: &AgentConversationWorkspace,
    ) {
        let context = load_agent_workspace_review_context(app_state, workspace)
            .await
            .expect("review context should load");
        let target = context.target.expect("review target should exist");
        let mut monitor = context.monitor;
        apply_review_artifact_to_monitor(
            &mut monitor,
            target.scope,
            target.head_sha,
            target.diff_fingerprint,
            Some("seeded-passing-review".to_string()),
            ArtifactId::from_string(format!(
                "review-artifact-{}",
                workspace.conversation_id.as_str()
            )),
            1,
            chrono::Utc::now(),
            None,
        );
        monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
        monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;
        monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
        app_state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(monitor)
            .await
            .expect("passing review monitor should persist");
    }

    struct PrFixReviewGateFixture {
        _repo: tempfile::TempDir,
        _worktrees: tempfile::TempDir,
        app_state: Arc<AppState>,
        conversation_id: ChatConversationId,
        github: Arc<MockGithubService>,
    }

    async fn setup_pr_fix_workspace_with_review_gate(
        suffix: &str,
        review_gate_status: AgentWorkspaceReviewGateStatus,
    ) -> PrFixReviewGateFixture {
        let repo = tempfile::TempDir::new().expect("repo tempdir");
        let worktrees = tempfile::TempDir::new().expect("worktree tempdir");
        git(repo.path(), &["init", "-b", "main"]);
        git(repo.path(), &["config", "user.email", "test@example.com"]);
        git(repo.path(), &["config", "user.name", "RalphX Test"]);
        std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
        git(repo.path(), &["add", "README.md"]);
        git(repo.path(), &["commit", "-m", "base"]);
        let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);

        let github = Arc::new(MockGithubService::new());
        let mut state = AppState::new_test();
        state.github_service = Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>);
        let app_state = Arc::new(state);
        let conversation_id = ChatConversationId::new();
        let mut project = Project::new(
            format!("PR Fix Review Gate {suffix}"),
            repo.path().to_string_lossy().to_string(),
        );
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktrees.path().to_string_lossy().to_string());
        app_state
            .project_repo
            .create(project.clone())
            .await
            .expect("seed project");

        let mut conversation = ChatConversation::new_project(project.id.clone());
        conversation.id = conversation_id.clone();
        conversation.context_type = ChatContextType::Project;
        conversation.context_id = project.id.as_str().to_string();
        app_state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("seed conversation");

        let workspace_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
            .expect("workspace path");
        let branch_name = format!("ralphx/test/pr-fix-review-{suffix}");
        git(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                &branch_name,
                workspace_path.to_str().unwrap(),
                "main",
            ],
        );
        std::fs::write(workspace_path.join("fix.txt"), "ci fix\n").expect("write workspace change");
        let mut workspace = AgentConversationWorkspace::new(
            conversation_id.clone(),
            project.id.clone(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some(base_sha),
            branch_name,
            workspace_path.to_string_lossy().to_string(),
        );
        workspace.publication_pr_number = Some(267);
        workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/267".to_string());
        workspace.publication_pr_status = Some("open".to_string());
        workspace.publication_push_status = Some("needs_agent".to_string());
        workspace.pr_supervision_status = Some("fixing".to_string());
        workspace.pr_autofix_enabled = true;
        workspace.pr_auto_merge_desired = true;
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");

        let review_context = load_agent_workspace_review_context(app_state.as_ref(), &workspace)
            .await
            .expect("review context should load");
        let target = review_context.target.expect("review target should exist");
        let mut monitor = review_context.monitor;
        apply_review_artifact_to_monitor(
            &mut monitor,
            target.scope,
            target.head_sha,
            target.diff_fingerprint,
            Some("review-run".to_string()),
            ArtifactId::from_string(format!("review-artifact-{suffix}")),
            1,
            chrono::Utc::now(),
            None,
        );
        match review_gate_status {
            AgentWorkspaceReviewGateStatus::Blocking => {
                monitor.review_outcome = AgentWorkspaceReviewOutcome::Blocking;
                monitor.review_blocking_summary =
                    Some("Workspace Review found blocking changes".to_string());
                monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
            }
            AgentWorkspaceReviewGateStatus::Failed => {
                monitor.review_outcome = AgentWorkspaceReviewOutcome::RunFailed;
                monitor.last_error = Some("Workspace Review failed".to_string());
                monitor.status = AgentWorkspaceReviewMonitorStatus::Blocked;
            }
            other => panic!("unsupported test review gate status: {other:?}"),
        }
        app_state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(monitor)
            .await
            .expect("review monitor should persist");

        PrFixReviewGateFixture {
            _repo: repo,
            _worktrees: worktrees,
            app_state,
            conversation_id,
            github,
        }
    }

    #[test]
    fn review_artifact_gate_accepts_matching_head_sha() {
        let mut monitor = AgentWorkspacePrReviewMonitor::new(
            ChatConversationId::new(),
            ProjectId::new(),
            411,
            Some("head-sha".to_string()),
        );
        monitor.review_artifact_id = Some(ArtifactId::from_string("artifact-1"));
        monitor.review_artifact_head_sha = Some("head-sha".to_string());

        assert!(ensure_review_artifact_for_head(&monitor, "head-sha").is_ok());
    }

    #[test]
    fn review_artifact_gate_rejects_missing_or_stale_artifact() {
        let mut monitor = AgentWorkspacePrReviewMonitor::new(
            ChatConversationId::new(),
            ProjectId::new(),
            411,
            Some("head-sha".to_string()),
        );
        assert!(ensure_review_artifact_for_head(&monitor, "head-sha").is_err());

        monitor.review_artifact_id = Some(ArtifactId::from_string("artifact-1"));
        monitor.review_artifact_head_sha = Some("old-head-sha".to_string());
        assert!(ensure_review_artifact_for_head(&monitor, "head-sha").is_err());
    }

    #[tokio::test]
    async fn propose_pr_review_action_requires_matching_review_artifact() {
        let app_state = Arc::new(AppState::new_test());
        let conversation_id = ChatConversationId::new();
        let mut workspace = test_workspace(conversation_id.clone());
        workspace.mode = AgentConversationWorkspaceMode::ReviewPr;
        workspace.publication_pr_number = Some(411);
        workspace.publication_pr_url = Some("https://github.com/mock/project/pull/411".to_string());
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();
        let state = test_http_state(Arc::clone(&app_state));

        let (status, Json(body)) = propose_agent_workspace_pr_review_action(
            State(state),
            Path(conversation_id.to_string()),
            Json(ProposeAgentWorkspacePrReviewActionRequest {
                head_sha: "head-sha".to_string(),
                proposed_action: "request_changes".to_string(),
                summary: "Found a blocking regression".to_string(),
                review_body: "Please fix the regression before merge.".to_string(),
                findings_json: None,
                created_by_run_id: Some("run-1".to_string()),
            }),
        )
        .await
        .unwrap_err();

        assert_eq!(status, StatusCode::CONFLICT);
        assert!(body["error"].as_str().unwrap().contains("Write the Review"));
        let actions = app_state
            .agent_conversation_workspace_repo
            .list_pr_review_actions(&conversation_id, 10)
            .await
            .unwrap();
        assert!(actions.is_empty());
    }

    #[tokio::test]
    async fn failed_pr_review_submit_keeps_action_pending_for_retry() {
        let mut app_state = AppState::new_test();
        let github = Arc::new(MockGithubService::new());
        github.will_fail_submit_pr_review("network unavailable");
        app_state.github_service = Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>);
        let app_state = Arc::new(app_state);

        let conversation_id = ChatConversationId::new();
        let mut workspace = test_workspace(conversation_id.clone());
        workspace.mode = AgentConversationWorkspaceMode::ReviewPr;
        workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
            number: 411,
            url: Some("https://github.com/mock/project/pull/411".to_string()),
            title: Some("Fix review workflow".to_string()),
            head_ref_name: "feature/review-workflow".to_string(),
            base_ref_name: Some("main".to_string()),
            head_ref_oid: Some("head-sha".to_string()),
        });
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .unwrap();

        let mut monitor = AgentWorkspacePrReviewMonitor::new(
            conversation_id.clone(),
            workspace.project_id.clone(),
            411,
            Some("head-sha".to_string()),
        );
        monitor.status = AgentWorkspacePrReviewMonitorStatus::AwaitingUser;
        monitor.review_artifact_id = Some(ArtifactId::from_string("review-artifact-1"));
        monitor.review_artifact_head_sha = Some("head-sha".to_string());
        monitor.review_artifact_version = Some(1);
        app_state
            .agent_conversation_workspace_repo
            .upsert_pr_review_monitor(monitor)
            .await
            .unwrap();

        let action = app_state
            .agent_conversation_workspace_repo
            .create_or_update_pr_review_action(AgentWorkspacePrReviewAction::new(
                conversation_id.clone(),
                411,
                "head-sha".to_string(),
                AgentWorkspacePrReviewActionKind::RequestChanges,
                "Found a blocking regression".to_string(),
                "Please fix the regression before merge.".to_string(),
                None,
                Some("run-1".to_string()),
            ))
            .await
            .unwrap();
        let state = test_http_state(Arc::clone(&app_state));

        let (status, Json(body)) = submit_agent_workspace_pr_review_action(
            State(state),
            Path((conversation_id.to_string(), action.id.clone())),
            Json(SubmitAgentWorkspacePrReviewActionRequest { action_kind: None }),
        )
        .await
        .unwrap_err();

        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(body["error"], "Failed to submit GitHub PR review");
        assert!(body["details"]
            .as_str()
            .unwrap()
            .contains("network unavailable"));

        let saved_action = app_state
            .agent_conversation_workspace_repo
            .get_pr_review_action(&action.id)
            .await
            .unwrap()
            .expect("action should still exist");
        assert_eq!(
            saved_action.status,
            AgentWorkspacePrReviewActionStatus::Pending
        );
        assert!(saved_action.submitted_review_id.is_none());
        assert!(saved_action.resolved_at.is_none());

        let pending = app_state
            .agent_conversation_workspace_repo
            .get_pending_pr_review_action_for_head(&conversation_id, 411, "head-sha")
            .await
            .unwrap()
            .expect("failed submit should leave a retryable pending action");
        assert_eq!(pending.id, action.id);

        let monitor = app_state
            .agent_conversation_workspace_repo
            .get_pr_review_monitor(&conversation_id)
            .await
            .unwrap()
            .expect("monitor should exist");
        assert_eq!(
            monitor.status,
            AgentWorkspacePrReviewMonitorStatus::AwaitingUser
        );
        assert_eq!(monitor.last_seen_head_sha.as_deref(), Some("head-sha"));
        assert!(monitor
            .last_error
            .as_deref()
            .unwrap()
            .contains("network unavailable"));

        let github_state = github.state();
        assert_eq!(github_state.submit_pr_review_calls, 1);
        assert_eq!(
            github_state
                .last_submit_pr_review_args
                .as_ref()
                .map(|(pr_number, event, body)| (*pr_number, *event, body.as_str())),
            Some((
                411,
                PrReviewSubmissionEvent::RequestChanges,
                "Please fix the regression before merge."
            ))
        );
    }

    #[tokio::test]
    async fn readiness_handler_reports_publishable_workspace_with_uncommitted_changes() {
        let repo = tempfile::TempDir::new().expect("repo tempdir");
        let worktrees = tempfile::TempDir::new().expect("worktree tempdir");
        git(repo.path(), &["init", "-b", "main"]);
        git(repo.path(), &["config", "user.email", "test@example.com"]);
        git(repo.path(), &["config", "user.name", "RalphX Test"]);
        std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
        git(repo.path(), &["add", "README.md"]);
        git(repo.path(), &["commit", "-m", "base"]);
        let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);

        let app_state = Arc::new(AppState::new_test());
        let conversation_id = ChatConversationId::new();
        let mut project = Project::new(
            "Readiness Workspace".to_string(),
            repo.path().to_string_lossy().to_string(),
        );
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktrees.path().to_string_lossy().to_string());
        app_state
            .project_repo
            .create(project.clone())
            .await
            .expect("seed project");

        let mut conversation = ChatConversation::new_project(project.id.clone());
        conversation.id = conversation_id.clone();
        conversation.context_type = ChatContextType::Project;
        conversation.context_id = project.id.as_str().to_string();
        app_state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("seed conversation");

        let workspace_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
            .expect("workspace path");
        let branch_name = "ralphx/test/readiness-workspace";
        git(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                branch_name,
                workspace_path.to_str().unwrap(),
                "main",
            ],
        );
        std::fs::write(workspace_path.join("implementation.txt"), "uncommitted\n")
            .expect("write workspace change");
        let workspace = AgentConversationWorkspace::new(
            conversation_id.clone(),
            project.id.clone(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some(base_sha),
            branch_name.to_string(),
            workspace_path.to_string_lossy().to_string(),
        );
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        seed_current_passing_workspace_review(app_state.as_ref(), &workspace).await;
        let state = test_http_state(app_state);

        let Json(response) = check_agent_workspace_publish_readiness(
            State(state),
            Path(conversation_id.to_string()),
        )
        .await
        .expect("readiness should load");

        assert!(response.success);
        assert!(response.can_publish);
        assert!(response.blockers.is_empty());
        assert!(!response.needs_base_update);
        assert!(response.recommended_actions.is_empty());
        assert!(response.freshness.has_uncommitted_changes);
    }

    #[tokio::test]
    async fn readiness_handler_ignores_required_review_gate_when_policy_is_disabled() {
        let repo = tempfile::TempDir::new().expect("repo tempdir");
        let worktrees = tempfile::TempDir::new().expect("worktree tempdir");
        git(repo.path(), &["init", "-b", "main"]);
        git(repo.path(), &["config", "user.email", "test@example.com"]);
        git(repo.path(), &["config", "user.name", "RalphX Test"]);
        std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
        git(repo.path(), &["add", "README.md"]);
        git(repo.path(), &["commit", "-m", "base"]);
        let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);

        let app_state = Arc::new(AppState::new_test());
        app_state
            .review_settings_repo
            .update_settings(&ReviewSettings {
                require_workspace_review: false,
                ..ReviewSettings::default()
            })
            .await
            .expect("review settings should update");
        let conversation_id = ChatConversationId::new();
        let mut project = Project::new(
            "Readiness Workspace Disabled Review".to_string(),
            repo.path().to_string_lossy().to_string(),
        );
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktrees.path().to_string_lossy().to_string());
        app_state
            .project_repo
            .create(project.clone())
            .await
            .expect("seed project");

        let mut conversation = ChatConversation::new_project(project.id.clone());
        conversation.id = conversation_id.clone();
        conversation.context_type = ChatContextType::Project;
        conversation.context_id = project.id.as_str().to_string();
        app_state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("seed conversation");

        let workspace_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
            .expect("workspace path");
        let branch_name = "ralphx/test/readiness-policy-disabled";
        git(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                branch_name,
                workspace_path.to_str().unwrap(),
                "main",
            ],
        );
        std::fs::write(workspace_path.join("implementation.txt"), "uncommitted\n")
            .expect("write workspace change");
        let workspace = AgentConversationWorkspace::new(
            conversation_id.clone(),
            project.id.clone(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some(base_sha),
            branch_name.to_string(),
            workspace_path.to_string_lossy().to_string(),
        );
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("seed workspace");
        let state = test_http_state(app_state);

        let Json(response) = check_agent_workspace_publish_readiness(
            State(state),
            Path(conversation_id.to_string()),
        )
        .await
        .expect("readiness should load");

        assert!(response.success);
        assert_eq!(response.review_gate_status.as_deref(), Some("required"));
        assert!(response.can_publish);
        assert!(response.blockers.is_empty());
        assert!(response.freshness.has_uncommitted_changes);
    }

    #[tokio::test]
    async fn update_from_base_rejects_invalid_base_kind_before_loading_workspace() {
        let state = test_http_state(Arc::new(AppState::new_test()));

        let (status, Json(body)) = update_agent_workspace_from_base(
            State(state),
            Path(ChatConversationId::new().to_string()),
            Json(UpdateAgentWorkspaceFromBaseRequest {
                base_ref_kind: Some("not-a-kind".to_string()),
                base_ref: Some("main".to_string()),
                base_display_name: Some("main".to_string()),
            }),
        )
        .await
        .unwrap_err();

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["error"]
            .as_str()
            .unwrap()
            .contains("unknown ideation analysis base ref kind"));
    }

    #[tokio::test]
    async fn needs_repair_action_response_preserves_error_payload_without_implying_queue() {
        let app_state = Arc::new(AppState::new_test());
        let conversation_id = ChatConversationId::new();
        let mut workspace = test_workspace(conversation_id.clone());
        workspace.publication_push_status = Some("needs_agent".to_string());
        workspace.publication_pr_number = Some(42);
        workspace.publication_pr_url = Some("https://github.com/mock/project/pull/42".to_string());
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();

        let Json(response) = action_response_for_needs_repair(
            app_state.as_ref(),
            &conversation_id,
            "merge conflict".to_string(),
        )
        .await
        .expect("needs-agent response should be returned");

        assert!(response.success);
        assert_eq!(response.status, "needs_agent_repair");
        assert_eq!(response.message, "merge conflict");
        assert!(!response.repair_queued);
        assert!(response.freshness.is_none());
        assert_eq!(response.pr_number, None);
        assert_eq!(response.pr_url, None);
    }

    #[tokio::test]
    async fn needs_repair_action_response_reports_queue_from_repair_events() {
        let app_state = Arc::new(AppState::new_test());
        let conversation_id = ChatConversationId::new();
        let mut workspace = test_workspace(conversation_id.clone());
        workspace.publication_push_status = Some("needs_agent".to_string());
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();
        app_state
            .agent_conversation_workspace_repo
            .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                conversation_id.clone(),
                "repair_requested",
                "started",
                "Workspace agent repair requested before the base update can complete",
                Some("agent_fixable:update_only".to_string()),
            ))
            .await
            .unwrap();

        let Json(response) = action_response_for_needs_repair(
            app_state.as_ref(),
            &conversation_id,
            "merge conflict".to_string(),
        )
        .await
        .expect("needs-agent response should be returned");

        assert!(response.success);
        assert_eq!(response.status, "needs_agent_repair");
        assert!(response.repair_queued);
    }

    #[tokio::test]
    async fn get_publish_status_reports_in_progress_and_events() {
        let app_state = Arc::new(AppState::new_test());
        let conversation_id = ChatConversationId::new();
        let mut workspace = test_workspace(conversation_id.clone());
        workspace.publication_push_status = Some("checking".to_string());
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();
        app_state
            .agent_conversation_workspace_repo
            .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                conversation_id.clone(),
                "checking",
                "started",
                "Checking workspace changes",
                None,
            ))
            .await
            .unwrap();
        let state = test_http_state(app_state);

        let Json(response) =
            get_agent_workspace_publish_status(State(state), Path(conversation_id.to_string()))
                .await
                .unwrap();

        assert!(response.success);
        assert!(response.publish_in_progress);
        assert!(!response.needs_agent_repair);
        assert_eq!(
            response.workspace.publication_push_status.as_deref(),
            Some("checking")
        );
        assert_eq!(response.events.len(), 1);
        assert_eq!(response.events[0].step, "checking");
    }

    #[tokio::test]
    async fn publish_agent_workspace_returns_in_progress_for_active_publish_state() {
        let app_state = Arc::new(AppState::new_test());
        let conversation_id = ChatConversationId::new();
        let mut workspace = test_workspace(conversation_id.clone());
        workspace.publication_push_status = Some("pushing".to_string());
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();
        let state = test_http_state(app_state);

        let Json(response) =
            publish_agent_workspace(State(state), Path(conversation_id.to_string()))
                .await
                .unwrap();

        assert!(response.success);
        assert_eq!(response.status, "publish_in_progress");
        assert!(!response.repair_queued);
    }

    #[tokio::test]
    async fn publish_agent_workspace_returns_repair_state_without_republishing() {
        let app_state = Arc::new(AppState::new_test());
        let conversation_id = ChatConversationId::new();
        let mut workspace = test_workspace(conversation_id.clone());
        workspace.publication_push_status = Some("needs_agent".to_string());
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();
        let state = test_http_state(app_state);

        let Json(response) =
            publish_agent_workspace(State(state), Path(conversation_id.to_string()))
                .await
                .unwrap();

        assert!(response.success);
        assert_eq!(response.status, "needs_agent_repair");
        assert!(!response.repair_queued);
    }

    #[tokio::test]
    async fn complete_pr_fix_skips_publish_when_pr_is_already_merged() {
        let mut app_state = AppState::new_test();
        let github = Arc::new(MockGithubService::new());
        github.will_return_status(PrStatus::Merged {
            merge_commit_sha: Some("a".repeat(40)),
            merged_at: None,
        });
        app_state.github_service = Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>);
        let app_state = Arc::new(app_state);

        let conversation_id = ChatConversationId::new();
        let mut workspace = test_workspace(conversation_id.clone());
        workspace.publication_pr_number = Some(267);
        workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/267".to_string());
        workspace.publication_pr_status = Some("open".to_string());
        workspace.publication_push_status = Some("needs_agent".to_string());
        workspace.pr_supervision_status = Some("fixing".to_string());
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();
        let state = test_http_state(Arc::clone(&app_state));

        let Json(response) = complete_agent_workspace_pr_fix(
            State(state),
            Path(conversation_id.to_string()),
            Json(CompleteAgentWorkspacePrFixRequest {
                summary: "Investigated post-merge fixer state".to_string(),
                blocker: None,
            }),
        )
        .await
        .expect("terminal PR should be handled without publishing");

        assert_eq!(response.status, "skipped_terminal");
        assert_eq!(response.publish_status.as_deref(), Some("skipped"));
        let updated = app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.publication_pr_status.as_deref(), Some("merged"));
        assert!(updated.pr_supervision_status.is_none());
        assert_eq!(github.state().check_pr_status_calls, 1);
    }

    #[tokio::test]
    async fn complete_pr_fix_skips_publish_when_auto_publish_is_paused() {
        let app_state = Arc::new(AppState::new_test());
        let conversation_id = ChatConversationId::new();
        let mut workspace = test_workspace(conversation_id.clone());
        workspace.publication_pr_number = Some(267);
        workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/267".to_string());
        workspace.publication_pr_status = Some("open".to_string());
        workspace.publication_push_status = Some("needs_agent".to_string());
        workspace.auto_publish_enabled = false;
        workspace.pr_supervision_status = Some("fixing".to_string());
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();
        let state = test_http_state(Arc::clone(&app_state));

        let Json(response) = complete_agent_workspace_pr_fix(
            State(state),
            Path(conversation_id.to_string()),
            Json(CompleteAgentWorkspacePrFixRequest {
                summary: "Fixed requested review change".to_string(),
                blocker: None,
            }),
        )
        .await
        .expect("paused Auto Publish should skip publish");

        assert_eq!(response.status, "publish_paused");
        assert_eq!(response.publish_status.as_deref(), Some("skipped"));
        assert!(response.commit_sha.is_none());
        let updated = app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.pr_supervision_status.as_deref(), Some("paused"));
        let events = app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .unwrap();
        assert!(events.iter().any(|event| {
            event.step == "pr_autofix_publish_skipped"
                && event.classification.as_deref() == Some("auto_publish_paused")
        }));
    }

    #[tokio::test]
    async fn complete_pr_fix_waits_for_running_workspace_review_when_required() {
        let repo = tempfile::TempDir::new().expect("repo tempdir");
        let worktrees = tempfile::TempDir::new().expect("worktree tempdir");
        git(repo.path(), &["init", "-b", "main"]);
        git(repo.path(), &["config", "user.email", "test@example.com"]);
        git(repo.path(), &["config", "user.name", "RalphX Test"]);
        std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
        git(repo.path(), &["add", "README.md"]);
        git(repo.path(), &["commit", "-m", "base"]);
        let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);

        let app_state = Arc::new(AppState::new_test());
        let conversation_id = ChatConversationId::new();
        let mut project = Project::new(
            "PR Fix Review Workspace".to_string(),
            repo.path().to_string_lossy().to_string(),
        );
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktrees.path().to_string_lossy().to_string());
        app_state
            .project_repo
            .create(project.clone())
            .await
            .expect("seed project");

        let mut conversation = ChatConversation::new_project(project.id.clone());
        conversation.id = conversation_id.clone();
        conversation.context_type = ChatContextType::Project;
        conversation.context_id = project.id.as_str().to_string();
        app_state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("seed conversation");

        let workspace_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
            .expect("workspace path");
        let branch_name = "ralphx/test/pr-fix-review-required";
        git(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                branch_name,
                workspace_path.to_str().unwrap(),
                "main",
            ],
        );
        std::fs::write(workspace_path.join("fix.txt"), "ci fix\n").expect("write workspace change");
        let mut workspace = AgentConversationWorkspace::new(
            conversation_id.clone(),
            project.id.clone(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some(base_sha),
            branch_name.to_string(),
            workspace_path.to_string_lossy().to_string(),
        );
        workspace.publication_pr_number = Some(267);
        workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/267".to_string());
        workspace.publication_pr_status = Some("open".to_string());
        workspace.publication_push_status = Some("needs_agent".to_string());
        workspace.pr_supervision_status = Some("fixing".to_string());
        workspace.pr_autofix_enabled = true;
        workspace.pr_auto_merge_desired = true;
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        let review_context = load_agent_workspace_review_context(app_state.as_ref(), &workspace)
            .await
            .expect("review context should load");
        let mut monitor = review_context.monitor;
        monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
        monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
        app_state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(monitor)
            .await
            .expect("running review monitor should persist");
        let state = test_http_state(Arc::clone(&app_state));

        let Json(response) = complete_agent_workspace_pr_fix(
            State(state),
            Path(conversation_id.to_string()),
            Json(CompleteAgentWorkspacePrFixRequest {
                summary: "Fixed failing CI check".to_string(),
                blocker: None,
            }),
        )
        .await
        .expect("running review should wait instead of blocking supervision");

        assert_eq!(response.status, "workspace_reviewing");
        assert_eq!(
            response.publish_status.as_deref(),
            Some("waiting_for_workspace_review")
        );
        assert!(response.publish_error.is_none());
        assert!(response.commit_sha.is_none());
        let updated = app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.pr_supervision_status.as_deref(), Some("reviewing"));
        let review_context = load_agent_workspace_review_context(app_state.as_ref(), &updated)
            .await
            .expect("review context should load");
        assert_eq!(
            review_context.monitor.status,
            AgentWorkspaceReviewMonitorStatus::Reviewing
        );
        assert_eq!(
            review_context.monitor.review_gate_status,
            AgentWorkspaceReviewGateStatus::Reviewing
        );
        let events = app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .unwrap();
        assert!(events.iter().any(|event| {
            event.step == "pr_autofix_workspace_review"
                && event.classification.as_deref() == Some("workspace_reviewing")
        }));
    }

    #[tokio::test]
    async fn complete_repair_starts_fresh_workspace_review_when_blocking_review_is_stale() {
        let repo = tempfile::TempDir::new().expect("repo tempdir");
        let worktrees = tempfile::TempDir::new().expect("worktree tempdir");
        git(repo.path(), &["init", "-b", "main"]);
        git(repo.path(), &["config", "user.email", "test@example.com"]);
        git(repo.path(), &["config", "user.name", "RalphX Test"]);
        std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
        git(repo.path(), &["add", "README.md"]);
        git(repo.path(), &["commit", "-m", "base"]);
        let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);

        let app_state = Arc::new(AppState::new_test());
        app_state
            .review_settings_repo
            .update_settings(&ReviewSettings {
                require_workspace_review: true,
                ..ReviewSettings::default()
            })
            .await
            .expect("review settings should update");
        let conversation_id = ChatConversationId::new();
        let mut project = Project::new(
            "Repair Review Refresh".to_string(),
            repo.path().to_string_lossy().to_string(),
        );
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktrees.path().to_string_lossy().to_string());
        app_state
            .project_repo
            .create(project.clone())
            .await
            .expect("seed project");

        let mut conversation = ChatConversation::new_project(project.id.clone());
        conversation.id = conversation_id.clone();
        conversation.context_type = ChatContextType::Project;
        conversation.context_id = project.id.as_str().to_string();
        app_state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("seed conversation");

        let workspace_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
            .expect("workspace path");
        let branch_name = "ralphx/test/repair-review-refresh";
        git(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                branch_name,
                workspace_path.to_str().unwrap(),
                "main",
            ],
        );
        std::fs::write(workspace_path.join("reviewed.txt"), "blocking\n")
            .expect("write reviewed change");
        git(&workspace_path, &["add", "reviewed.txt"]);
        git(
            &workspace_path,
            &["commit", "-m", "reviewed blocking change"],
        );

        let mut workspace = AgentConversationWorkspace::new(
            conversation_id.clone(),
            project.id.clone(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some(base_sha.clone()),
            branch_name.to_string(),
            workspace_path.to_string_lossy().to_string(),
        );
        workspace.publication_push_status = Some("refreshed".to_string());
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");

        let initial_review = load_agent_workspace_review_context(app_state.as_ref(), &workspace)
            .await
            .expect("initial review context should load");
        let initial_target = initial_review.target.expect("review target should exist");
        let mut monitor = initial_review.monitor;
        apply_review_artifact_to_monitor(
            &mut monitor,
            initial_target.scope,
            initial_target.head_sha,
            initial_target.diff_fingerprint,
            Some("review-run-blocking".to_string()),
            ArtifactId::from_string("artifact-blocking-review"),
            1,
            chrono::Utc::now(),
            None,
        );
        monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
        monitor.review_outcome = AgentWorkspaceReviewOutcome::Blocking;
        monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Blocking;
        monitor.review_blocking_summary = Some("Blocking review finding".to_string());
        monitor.review_blocking_fingerprint = Some("blocking-fingerprint".to_string());
        monitor.review_fixer_status = Some("running".to_string());
        monitor.review_fixer_run_id = Some("fixer-run".to_string());
        monitor.review_fixer_conversation_id = Some(conversation_id.clone());
        app_state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(monitor)
            .await
            .expect("blocking review monitor should persist");

        std::fs::write(workspace_path.join("reviewed.txt"), "fixed\n").expect("write repair");
        git(&workspace_path, &["add", "reviewed.txt"]);
        git(&workspace_path, &["commit", "-m", "repair blocking review"]);
        let repair_sha = git(&workspace_path, &["rev-parse", "HEAD"]);

        let stale_context = load_agent_workspace_review_context(app_state.as_ref(), &workspace)
            .await
            .expect("stale review context should load");
        assert_eq!(
            stale_context.monitor.review_gate_status,
            AgentWorkspaceReviewGateStatus::Required
        );

        let state = test_http_state(Arc::clone(&app_state));
        let starter = RecordingWorkspaceReviewStarter::new();
        let response = complete_repair_workspace_review_response_if_required_with_starter(
            &state,
            &conversation_id,
            &workspace,
            &base_sha,
            &repair_sha,
            "fixed the blocking review finding",
            &starter,
        )
        .await
        .expect("repair review response should succeed")
        .expect("stale required review should pause publish")
        .0;

        assert_eq!(starter.call_count(), 1);
        assert_eq!(response.new_status, "refreshed");
        assert_eq!(
            response.auto_publish_status.as_deref(),
            Some("waiting_for_workspace_review")
        );
        assert_eq!(response.auto_publish_error, None);
        let updated_workspace = app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace should load")
            .expect("workspace should exist");
        assert_eq!(
            updated_workspace.pr_supervision_status.as_deref(),
            Some("reviewing"),
            "repair completion should persist the paused workspace-review supervision state"
        );
        assert_eq!(
            updated_workspace.pr_supervision_summary.as_deref(),
            Some(
                "Agent workspace repair verified; Workspace Review started before publishing resumes."
            )
        );
        let events = app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("publication events should load");
        assert!(
            events.iter().any(|event| {
                event.step == "repair_workspace_review"
                    && event.status == "reviewing"
                    && event.classification.as_deref() == Some("workspace_review_started")
                    && event.summary.contains("fixed the blocking review finding")
            }),
            "repair completion should append a durable workspace-review pause event"
        );
        let refreshed_review = load_agent_workspace_review_context(app_state.as_ref(), &workspace)
            .await
            .expect("refreshed review context should load");
        assert_eq!(
            refreshed_review.monitor.status,
            AgentWorkspaceReviewMonitorStatus::Reviewing
        );
        assert_eq!(
            refreshed_review.monitor.review_gate_status,
            AgentWorkspaceReviewGateStatus::Reviewing
        );
        assert_eq!(
            refreshed_review.monitor.review_fixer_status, None,
            "repair completion should clear stale fixer state before the fresh review"
        );
    }

    #[tokio::test]
    async fn complete_pr_fix_blocks_when_workspace_review_has_blocking_findings() {
        let fixture = setup_pr_fix_workspace_with_review_gate(
            "blocking",
            AgentWorkspaceReviewGateStatus::Blocking,
        )
        .await;
        let state = test_http_state(Arc::clone(&fixture.app_state));

        let Json(response) = complete_agent_workspace_pr_fix(
            State(state),
            Path(fixture.conversation_id.to_string()),
            Json(CompleteAgentWorkspacePrFixRequest {
                summary: "Fixed failing CI check".to_string(),
                blocker: None,
            }),
        )
        .await
        .expect("blocking review should return an authoritative block");

        assert_eq!(response.status, "workspace_review_blocked");
        assert_eq!(
            response.publish_status.as_deref(),
            Some("blocked_by_workspace_review")
        );
        assert!(response.commit_sha.is_none());
        assert_eq!(fixture.github.state().push_branch_calls, 0);
        let updated = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.pr_supervision_status.as_deref(), Some("blocked"));
        assert!(updated
            .pr_supervision_summary
            .as_deref()
            .unwrap()
            .contains("Workspace Review found blocking changes"));
        let events = fixture
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&fixture.conversation_id)
            .await
            .unwrap();
        assert!(events.iter().any(|event| {
            event.step == "pr_autofix_workspace_review"
                && event.status == "blocked"
                && event.classification.as_deref() == Some("workspace_review_blocked")
        }));
        assert!(!events
            .iter()
            .any(|event| event.step == "pr_autofix_publish_failed"));
    }

    #[tokio::test]
    async fn pr_fix_workspace_review_gate_is_skipped_when_policy_is_disabled() {
        let fixture = setup_pr_fix_workspace_with_review_gate(
            "policy-disabled",
            AgentWorkspaceReviewGateStatus::Blocking,
        )
        .await;
        fixture
            .app_state
            .review_settings_repo
            .update_settings(&ReviewSettings {
                require_workspace_review: false,
                ..ReviewSettings::default()
            })
            .await
            .expect("review settings should update");
        let state = test_http_state(Arc::clone(&fixture.app_state));
        let workspace = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");

        let result = start_workspace_review_for_pr_fix_if_required(
            &state,
            &fixture.conversation_id,
            &workspace,
            "Fixed failing CI check",
        )
        .await
        .expect("disabled policy should skip workspace review gate");

        assert!(result.is_none());
        let events = fixture
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&fixture.conversation_id)
            .await
            .unwrap();
        assert!(!events
            .iter()
            .any(|event| event.step == "pr_autofix_workspace_review"));
    }

    #[tokio::test]
    async fn complete_pr_fix_blocks_when_workspace_review_failed() {
        let fixture = setup_pr_fix_workspace_with_review_gate(
            "failed",
            AgentWorkspaceReviewGateStatus::Failed,
        )
        .await;
        let state = test_http_state(Arc::clone(&fixture.app_state));

        let Json(response) = complete_agent_workspace_pr_fix(
            State(state),
            Path(fixture.conversation_id.to_string()),
            Json(CompleteAgentWorkspacePrFixRequest {
                summary: "Fixed failing CI check".to_string(),
                blocker: None,
            }),
        )
        .await
        .expect("failed review should return an authoritative block");

        assert_eq!(response.status, "workspace_review_failed");
        assert_eq!(
            response.publish_status.as_deref(),
            Some("blocked_by_workspace_review")
        );
        assert_eq!(
            response.publish_error.as_deref(),
            Some("Workspace Review failed")
        );
        assert!(response.commit_sha.is_none());
        assert_eq!(fixture.github.state().push_branch_calls, 0);
        let updated = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.pr_supervision_status.as_deref(), Some("blocked"));
        assert!(updated
            .pr_supervision_summary
            .as_deref()
            .unwrap()
            .contains("Workspace Review failed"));
    }

    #[tokio::test]
    async fn passed_workspace_review_resumes_pr_fix_publish_after_missing_review_failure() {
        let repo = tempfile::TempDir::new().expect("repo tempdir");
        let worktrees = tempfile::TempDir::new().expect("worktree tempdir");
        git(repo.path(), &["init", "-b", "main"]);
        git(repo.path(), &["config", "user.email", "test@example.com"]);
        git(repo.path(), &["config", "user.name", "RalphX Test"]);
        std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
        git(repo.path(), &["add", "README.md"]);
        git(repo.path(), &["commit", "-m", "base"]);
        let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);

        let github = Arc::new(MockGithubService::new());
        let conversation_id = ChatConversationId::new();
        let mut state = AppState::new_test();
        state.github_service = Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>);
        let client = Arc::new(SubmittingPrDescriptionClient::new(
            Arc::clone(&state.agent_conversation_workspace_repo),
            conversation_id.clone(),
        ));
        let state = state.with_agent_client(client);
        let app_state = Arc::new(state);
        let mut project = Project::new(
            "Blocked PR Fix Review Resume".to_string(),
            repo.path().to_string_lossy().to_string(),
        );
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktrees.path().to_string_lossy().to_string());
        app_state
            .project_repo
            .create(project.clone())
            .await
            .expect("seed project");

        let mut conversation = ChatConversation::new_project(project.id.clone());
        conversation.id = conversation_id.clone();
        conversation.context_type = ChatContextType::Project;
        conversation.context_id = project.id.as_str().to_string();
        conversation.title = Some("Fix blocked review autopilot".to_string());
        app_state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("seed conversation");

        let workspace_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
            .expect("workspace path");
        let branch_name = "ralphx/test/blocked-pr-fix-review-resume";
        git(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                branch_name,
                workspace_path.to_str().unwrap(),
                "main",
            ],
        );
        std::fs::write(workspace_path.join("fix.txt"), "ci fix\n").expect("write workspace change");
        let mut workspace = AgentConversationWorkspace::new(
            conversation_id.clone(),
            project.id.clone(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some(base_sha),
            branch_name.to_string(),
            workspace_path.to_string_lossy().to_string(),
        );
        workspace.publication_pr_number = Some(267);
        workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/267".to_string());
        workspace.publication_pr_status = Some("open".to_string());
        workspace.publication_push_status = Some("needs_agent".to_string());
        workspace.auto_publish_enabled = true;
        workspace.pr_supervision_status = Some("blocked".to_string());
        workspace.pr_supervision_summary =
            Some("Workspace reviewer completed without writing a current Review".to_string());
        workspace.pr_autofix_enabled = true;
        workspace.pr_auto_merge_desired = true;
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");

        let review_context = load_agent_workspace_review_context(app_state.as_ref(), &workspace)
            .await
            .expect("review context should load");
        let target = review_context.target.expect("review target should exist");
        let mut monitor = review_context.monitor;
        monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
        apply_review_artifact_to_monitor(
            &mut monitor,
            target.scope,
            target.head_sha,
            target.diff_fingerprint,
            Some("review-run".to_string()),
            ArtifactId::from_string("review-artifact-blocked-resume"),
            1,
            chrono::Utc::now(),
            None,
        );
        monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
        monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
        app_state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(monitor)
            .await
            .expect("review monitor should persist");

        let state = test_http_state(Arc::clone(&app_state));
        let Json(response) = complete_agent_workspace_review_run(
            State(state),
            Path(conversation_id.to_string()),
            Json(CompleteAgentWorkspaceReviewRunRequest {
                outcome: Some("passed".to_string()),
                summary: "Review passed".to_string(),
                blocker: None,
                created_by_run_id: Some("review-run".to_string()),
            }),
        )
        .await
        .expect("passed workspace review should complete");

        assert_eq!(response.monitor.review_gate_status, "passed");
        {
            let github_state = github.state();
            assert_eq!(github_state.push_branch_calls, 1);
            assert_eq!(
                github_state.last_push_branch_name.as_deref(),
                Some(branch_name)
            );
            assert_eq!(github_state.enable_pr_auto_merge_calls, 1);
        }
        let updated = app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.pr_supervision_status.as_deref(), Some("monitoring"));
        assert_eq!(updated.publication_push_status.as_deref(), Some("pushed"));
        let events = app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .unwrap();
        assert!(events.iter().any(|event| {
            event.step == "pr_autofix_workspace_review_passed"
                && event.status == "publishing"
                && event.classification.as_deref() == Some("workspace_review_passed")
        }));
        assert!(events
            .iter()
            .any(|event| event.step == "published" && event.status == "succeeded"));
    }

    struct ReviewCompletionFixture {
        _repo: tempfile::TempDir,
        _worktrees: tempfile::TempDir,
        app_state: Arc<AppState>,
        conversation_id: ChatConversationId,
        automation_id: Option<crate::domain::entities::AutomationId>,
        run_id: Option<crate::domain::entities::AutomationRunId>,
        github: Arc<MockGithubService>,
    }

    /// Seed a no-PR workspace whose review monitor is `Reviewing` with a current artifact, so that
    /// calling `complete_agent_workspace_review_run` recomputes the gate from the passed outcome.
    /// Optionally arms initial auto-publish and links an automation run to the conversation.
    async fn setup_workspace_for_review_completion(
        suffix: &str,
        armed_initial: bool,
        seed_automation: bool,
    ) -> ReviewCompletionFixture {
        use crate::domain::entities::{
            Automation, AutomationId, AutomationJudgeState, AutomationPlanApprovalMode,
            AutomationPlanJudgeState, AutomationPrMergeMode, AutomationPromptAuthor, AutomationRun,
            AutomationRunId, AutomationRunStatus, AutomationStatus,
        };

        let repo = tempfile::TempDir::new().expect("repo tempdir");
        let worktrees = tempfile::TempDir::new().expect("worktree tempdir");
        git(repo.path(), &["init", "-b", "main"]);
        git(repo.path(), &["config", "user.email", "test@example.com"]);
        git(repo.path(), &["config", "user.name", "RalphX Test"]);
        std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
        git(repo.path(), &["add", "README.md"]);
        git(repo.path(), &["commit", "-m", "base"]);
        let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);

        let github = Arc::new(MockGithubService::new());
        // When we expect the resume to publish, let the mock create a PR so publish completes
        // instead of blocking on an agent-authored PR description.
        github.will_create_pr(918, "https://github.com/owner/repo/pull/918");
        let conversation_id = ChatConversationId::new();
        let mut state = AppState::new_test();
        state.github_service = Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>);
        let publish_client = Arc::new(SubmittingPrDescriptionClient::new(
            Arc::clone(&state.agent_conversation_workspace_repo),
            conversation_id.clone(),
        ));
        let state = state.with_agent_client(publish_client);
        let app_state = Arc::new(state);
        let mut project = Project::new(
            format!("Review Completion {suffix}"),
            repo.path().to_string_lossy().to_string(),
        );
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktrees.path().to_string_lossy().to_string());
        app_state
            .project_repo
            .create(project.clone())
            .await
            .expect("seed project");

        let mut conversation = ChatConversation::new_project(project.id.clone());
        conversation.id = conversation_id.clone();
        conversation.context_type = ChatContextType::Project;
        conversation.context_id = project.id.as_str().to_string();
        app_state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("seed conversation");

        let workspace_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
            .expect("workspace path");
        let branch_name = format!("ralphx/test/review-completion-{suffix}");
        git(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                &branch_name,
                workspace_path.to_str().unwrap(),
                "main",
            ],
        );
        std::fs::write(workspace_path.join("fix.txt"), "work\n").expect("write workspace change");
        git(&workspace_path, &["add", "fix.txt"]);
        git(&workspace_path, &["commit", "-m", "workspace change"]);
        let mut workspace = AgentConversationWorkspace::new(
            conversation_id.clone(),
            project.id.clone(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some(base_sha),
            branch_name,
            workspace_path.to_string_lossy().to_string(),
        );
        // No publication PR yet — this is the INITIAL publish path.
        workspace.publication_pr_number = None;
        workspace.auto_publish_initial_pr_enabled = armed_initial;
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");

        let review_context = load_agent_workspace_review_context(app_state.as_ref(), &workspace)
            .await
            .expect("review context should load");
        let target = review_context.target.expect("review target should exist");
        let mut monitor = review_context.monitor;
        apply_review_artifact_to_monitor(
            &mut monitor,
            target.scope,
            target.head_sha,
            target.diff_fingerprint,
            Some("review-run".to_string()),
            ArtifactId::from_string(format!("review-artifact-{suffix}")),
            1,
            chrono::Utc::now(),
            None,
        );
        monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
        monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
        app_state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(monitor)
            .await
            .expect("review monitor should persist");

        let (automation_id, run_id) = if seed_automation {
            let now = chrono::Utc::now();
            let automation_id = AutomationId::from_string(format!("automation-{suffix}"));
            app_state
                .automation_repo
                .create(Automation {
                    id: automation_id.clone(),
                    project_id: project.id.clone(),
                    name: "Automation".to_string(),
                    status: AutomationStatus::Active,
                    paused_reason_code: None,
                    paused_reason_detail: None,
                    goal_prompt: "Goal".to_string(),
                    setup_conversation_id: None,
                    provider_harness: "claude".to_string(),
                    model_id: "sonnet".to_string(),
                    logical_effort: None,
                    run_mode: "edit".to_string(),
                    base_ref_kind: "project_default".to_string(),
                    base_ref: String::new(),
                    base_display_name: None,
                    base_source_pull_request_json: None,
                    goal_items_json: None,
                    chain_mode: "merged_base".to_string(),
                    completion_signal: "pr_merged".to_string(),
                    plan_approval_mode: AutomationPlanApprovalMode::Manual,
                    pr_merge_mode: AutomationPrMergeMode::Manual,
                    plan_deep_verification: false,
                    max_runs: 25,
                    max_consecutive_failures: 3,
                    first_run_prompt: None,
                    setup_analysis_summary: None,
                    spec_artifact_id: None,
                    created_at: now,
                    updated_at: now,
                })
                .await
                .expect("seed automation");
            let run_id = AutomationRunId::from_string(format!("run-{suffix}"));
            app_state
                .automation_run_repo
                .create_run(AutomationRun {
                    id: run_id.clone(),
                    automation_id: automation_id.clone(),
                    run_index: 1,
                    status: AutomationRunStatus::Running,
                    judge_state: AutomationJudgeState::None,
                    judge_lease_expires_at: None,
                    plan_judge_state: AutomationPlanJudgeState::None,
                    plan_judge_lease_expires_at: None,
                    plan_judge_verdict_json: None,
                    plan_revision_round: 0,
                    plan_reminder_count: 0,
                    plan_pending_instructions: None,
                    plan_last_parked_artifact_id: None,
                    agent_phase_started_at: None,
                    conversation_id: Some(conversation_id.clone()),
                    run_prompt: "Run".to_string(),
                    prompt_author: AutomationPromptAuthor::SetupAgent,
                    base_ref_kind: "project_default".to_string(),
                    base_ref_used: "main".to_string(),
                    base_from_run_id: None,
                    branch_name: None,
                    pr_number: None,
                    pr_url: None,
                    pr_title: None,
                    pr_head_ref_name: None,
                    pr_base_ref_name: None,
                    pr_merged_at: None,
                    merge_commit_sha: None,
                    diff_stats_json: None,
                    agent_summary: None,
                    judge_verdict_json: None,
                    judge_model_id: None,
                    error_code: None,
                    error_detail: None,
                    signal_check_failures: 0,
                    started_at: Some(now),
                    finished_at: None,
                    created_at: now,
                    updated_at: now,
                })
                .await
                .expect("seed automation run");
            (Some(automation_id), Some(run_id))
        } else {
            (None, None)
        };

        ReviewCompletionFixture {
            _repo: repo,
            _worktrees: worktrees,
            app_state,
            conversation_id,
            automation_id,
            run_id,
            github,
        }
    }

    #[tokio::test]
    async fn passed_review_resumes_initial_auto_publish_when_armed() {
        let fixture = setup_workspace_for_review_completion("armed", true, false).await;
        let state = test_http_state(Arc::clone(&fixture.app_state));

        let _ = complete_agent_workspace_review_run(
            State(state),
            Path(fixture.conversation_id.to_string()),
            Json(CompleteAgentWorkspaceReviewRunRequest {
                outcome: Some("passed".to_string()),
                summary: "Review passed".to_string(),
                blocker: None,
                created_by_run_id: Some("review-run".to_string()),
            }),
        )
        .await
        .expect("passed workspace review should complete");

        // R2: the initial auto-publish resume fired (publishing event appended before publish).
        let events = fixture
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&fixture.conversation_id)
            .await
            .unwrap();
        assert!(
            events.iter().any(|event| {
                event.step == "initial_auto_publish_workspace_review_passed"
                    && event.status == "publishing"
                    && event.classification.as_deref() == Some("workspace_review_passed")
            }),
            "armed initial auto-publish should resume on a passed gate"
        );
        // Publish was invoked exactly once and created the initial PR.
        assert_eq!(fixture.github.state().create_draft_pr_calls, 1);
        let persisted = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(persisted.publication_pr_number, Some(918));
    }

    #[tokio::test]
    async fn passed_review_does_not_resume_initial_auto_publish_when_not_armed() {
        let fixture = setup_workspace_for_review_completion("unarmed", false, false).await;
        let state = test_http_state(Arc::clone(&fixture.app_state));

        let Json(response) = complete_agent_workspace_review_run(
            State(state),
            Path(fixture.conversation_id.to_string()),
            Json(CompleteAgentWorkspaceReviewRunRequest {
                outcome: Some("passed".to_string()),
                summary: "Review passed".to_string(),
                blocker: None,
                created_by_run_id: Some("review-run".to_string()),
            }),
        )
        .await
        .expect("passed workspace review should complete");

        assert_eq!(response.monitor.review_gate_status, "passed");
        let events = fixture
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&fixture.conversation_id)
            .await
            .unwrap();
        assert!(
            !events
                .iter()
                .any(|event| event.step == "initial_auto_publish_workspace_review_passed"),
            "a non-armed workspace must not resume initial auto-publish"
        );
    }

    #[tokio::test]
    async fn blocking_review_pauses_owning_automation_and_terminalizes_run() {
        let fixture = setup_workspace_for_review_completion("block", false, true).await;
        let state = test_http_state(Arc::clone(&fixture.app_state));

        let _ = complete_agent_workspace_review_run(
            State(state),
            Path(fixture.conversation_id.to_string()),
            Json(CompleteAgentWorkspaceReviewRunRequest {
                outcome: Some("blocking".to_string()),
                summary: "Review found blocking changes".to_string(),
                blocker: Some("Fix the failing invariant".to_string()),
                created_by_run_id: Some("review-run".to_string()),
            }),
        )
        .await
        .expect("blocking workspace review should complete");

        // R3 site (a): automation paused with the review-blocked reason.
        let automation = fixture
            .app_state
            .automation_repo
            .get_by_id(fixture.automation_id.as_ref().unwrap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            automation.status,
            crate::domain::entities::AutomationStatus::Paused
        );
        assert_eq!(
            automation.paused_reason_code.as_deref(),
            Some("workspace_review_blocked")
        );

        // Run terminalized as AgentFailed so its wall-clock can't false-timeout on resume.
        let run = fixture
            .app_state
            .automation_run_repo
            .get_by_id(fixture.run_id.as_ref().unwrap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            run.status,
            crate::domain::entities::AutomationRunStatus::AgentFailed
        );
        assert_eq!(run.error_code.as_deref(), Some("workspace_review_blocked"));

        // Publish was NOT invoked (no publishing events at all).
        let events = fixture
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&fixture.conversation_id)
            .await
            .unwrap();
        assert!(
            !events
                .iter()
                .any(|event| event.step == "initial_auto_publish_workspace_review_passed"),
            "a blocking gate must not resume publish"
        );
    }

    #[tokio::test]
    async fn blocking_review_is_noop_for_non_automation_conversation() {
        let fixture =
            setup_workspace_for_review_completion("block-interactive", false, false).await;
        let state = test_http_state(Arc::clone(&fixture.app_state));

        // No automation linked → the handler must still succeed and not attempt any pause.
        let Json(response) = complete_agent_workspace_review_run(
            State(state),
            Path(fixture.conversation_id.to_string()),
            Json(CompleteAgentWorkspaceReviewRunRequest {
                outcome: Some("blocking".to_string()),
                summary: "Review found blocking changes".to_string(),
                blocker: Some("Fix the failing invariant".to_string()),
                created_by_run_id: Some("review-run".to_string()),
            }),
        )
        .await
        .expect("blocking workspace review should complete for interactive conversation");

        assert_eq!(response.monitor.review_gate_status, "blocking");
    }

    #[tokio::test]
    async fn read_pr_comment_returns_full_body_and_marks_read() {
        let app_state = Arc::new(AppState::new_test());
        let conversation_id = ChatConversationId::new();
        let mut workspace = test_workspace(conversation_id.clone());
        workspace.publication_pr_number = Some(267);
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();
        app_state
            .agent_conversation_workspace_repo
            .upsert_pr_comment_evidence(
                &conversation_id,
                vec![AgentWorkspacePrCommentEvidenceUpsert::new(
                    267,
                    "comment-1".to_string(),
                    Some("codecov".to_string()),
                    "Full Codecov report body with detailed coverage table.".to_string(),
                    Some("https://github.com/owner/repo/pull/267#issuecomment-1".to_string()),
                    Some("2026-05-18T22:00:00Z".to_string()),
                    Some("2026-05-18T22:00:00Z".to_string()),
                    true,
                    true,
                )],
            )
            .await
            .unwrap();
        let state = test_http_state(Arc::clone(&app_state));

        let Json(response) = read_agent_workspace_pr_comment(
            State(state),
            Path((conversation_id.to_string(), "comment-1".to_string())),
        )
        .await
        .expect("comment should read");

        assert!(response.success);
        assert_eq!(response.pr_number, 267);
        assert_eq!(
            response.body,
            "Full Codecov report body with detailed coverage table."
        );
        assert_eq!(response.body_length_chars, response.body.chars().count());
        assert!(response.is_untrusted);
        let stored = app_state
            .agent_conversation_workspace_repo
            .get_pr_comment_evidence(&conversation_id, 267, "comment-1")
            .await
            .unwrap()
            .unwrap();
        assert!(stored.last_read_at.is_some());
    }

    #[tokio::test]
    async fn pr_fix_context_imports_bounded_comment_evidence() {
        let mut app_state = AppState::new_test();
        let github = Arc::new(MockGithubService::new());
        let long_body = "Patch coverage report row ".repeat(40);
        github.state().fetch_pr_health_result = Some(Ok(PrHealth {
            sync_state: PrSyncState {
                status: PrStatus::Open,
                merge_state_status: None,
                mergeable: None,
                is_draft: false,
                head_ref_name: "feature/pr-description".to_string(),
                base_ref_name: "main".to_string(),
                head_ref_oid: None,
                base_ref_oid: None,
            },
            review_decision: None,
            checks: Vec::new(),
            issue_comments: vec![PrIssueCommentSummary {
                id: "comment-long".to_string(),
                author: Some("codecov".to_string()),
                body: long_body.clone(),
                url: Some("https://github.com/owner/repo/pull/267#issuecomment-1".to_string()),
                created_at: Some("2026-05-18T22:00:00Z".to_string()),
                updated_at: Some("2026-05-18T22:05:00Z".to_string()),
                is_bot: true,
                is_codecov: true,
            }],
            auto_merge_request: None,
        }));
        app_state.github_service = Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>);
        let app_state = Arc::new(app_state);
        let conversation_id = ChatConversationId::new();
        let mut workspace = test_workspace(conversation_id.clone());
        workspace.publication_pr_number = Some(267);
        workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/267".to_string());
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();
        let state = test_http_state(Arc::clone(&app_state));

        let Json(response) =
            get_agent_workspace_pr_fix_context(State(state), Path(conversation_id.to_string()))
                .await
                .expect("PR fix context should load");

        assert_eq!(response.issue_comment_evidence.len(), 1);
        let evidence = &response.issue_comment_evidence[0];
        assert_eq!(evidence.comment_id, "comment-long");
        assert!(evidence.has_more);
        assert!(evidence.full_body_available);
        assert!(evidence.is_untrusted);
        assert_eq!(evidence.read_tool, "read_agent_workspace_pr_comment");
        assert_eq!(evidence.body_length_chars, long_body.chars().count());
        assert!(evidence.body_excerpt.chars().count() <= 480);
        assert!(
            response
                .health
                .as_ref()
                .expect("health should be present")
                .issue_comments[0]
                .body
                .chars()
                .count()
                <= 480
        );
        let stored = app_state
            .agent_conversation_workspace_repo
            .get_pr_comment_evidence(&conversation_id, 267, "comment-long")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.body, long_body);
        assert!(stored.last_included_at.is_some());
        assert_eq!(github.state().fetch_pr_health_calls, 1);
    }

    #[tokio::test]
    async fn pr_fix_context_uses_linked_plan_branch_pr_target() {
        let repo = tempfile::TempDir::new().expect("repo tempdir");
        let worktrees = tempfile::TempDir::new().expect("worktree tempdir");
        git(repo.path(), &["init", "-b", "main"]);
        git(repo.path(), &["config", "user.email", "test@example.com"]);
        git(repo.path(), &["config", "user.name", "RalphX Test"]);
        std::fs::write(repo.path().join("README.md"), "base\n").expect("write base");
        git(repo.path(), &["add", "README.md"]);
        git(repo.path(), &["commit", "-m", "base"]);
        let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);

        let github = Arc::new(MockGithubService::new());
        github.state().fetch_pr_health_result = Some(Ok(PrHealth {
            sync_state: PrSyncState {
                status: PrStatus::Open,
                merge_state_status: None,
                mergeable: None,
                is_draft: false,
                head_ref_name: "ralphx/test/plan-pr-context".to_string(),
                base_ref_name: "main".to_string(),
                head_ref_oid: Some("plan-context-head".to_string()),
                base_ref_oid: None,
            },
            review_decision: None,
            checks: Vec::new(),
            issue_comments: Vec::new(),
            auto_merge_request: None,
        }));
        let mut app_state = AppState::new_test();
        app_state.github_service = Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>);
        let app_state = Arc::new(app_state);

        let mut project = Project::new(
            "Plan PR Context".to_string(),
            repo.path().to_string_lossy().to_string(),
        );
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktrees.path().to_string_lossy().to_string());
        app_state
            .project_repo
            .create(project.clone())
            .await
            .expect("seed project");

        let conversation_id = ChatConversationId::from_string("conversation-plan-pr-context");
        let session_id = IdeationSessionId::from_string("session-plan-pr-context");
        let plan_branch_id = PlanBranchId::from_string("plan-branch-pr-context");
        let mut conversation = ChatConversation::new_project(project.id.clone());
        conversation.id = conversation_id.clone();
        conversation.context_type = ChatContextType::Project;
        conversation.context_id = project.id.as_str().to_string();
        app_state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("seed conversation");

        let branch_name = "ralphx/test/plan-pr-context";
        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string("artifact-plan-pr-context"),
            session_id.clone(),
            project.id.clone(),
            branch_name.to_string(),
            "main".to_string(),
        );
        plan_branch.id = plan_branch_id.clone();
        plan_branch.pr_eligible = true;
        plan_branch.merge_task_id = Some(TaskId::from_string(
            "merge-task-plan-pr-context".to_string(),
        ));
        plan_branch.pr_number = Some(602);
        plan_branch.pr_url = Some("https://github.com/owner/repo/pull/602".to_string());
        plan_branch.pr_status = Some(PlanPrStatus::Open);
        plan_branch.pr_push_status = PlanPrPushStatus::Pushed;
        let plan_worktree = resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch)
            .expect("plan worktree path");
        git(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                branch_name,
                plan_worktree.to_str().unwrap(),
                "main",
            ],
        );
        app_state
            .plan_branch_repo
            .create(plan_branch)
            .await
            .expect("seed plan branch");

        let mut workspace = AgentConversationWorkspace::new(
            conversation_id.clone(),
            project.id.clone(),
            AgentConversationWorkspaceMode::Ideation,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some(base_sha),
            branch_name.to_string(),
            plan_worktree.to_string_lossy().to_string(),
        );
        workspace.linked_ideation_session_id = Some(session_id);
        workspace.linked_plan_branch_id = Some(plan_branch_id);
        workspace.publication_pr_number = None;
        workspace.pr_supervision_status = Some("fixing".to_string());
        workspace.pr_autofix_enabled = true;
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("seed workspace");

        let state = test_http_state(Arc::clone(&app_state));
        let Json(response) =
            get_agent_workspace_pr_fix_context(State(state), Path(conversation_id.to_string()))
                .await
                .expect("PR fix context should load");

        assert_eq!(response.target_kind.as_deref(), Some("ideation_plan_pr"));
        assert_eq!(response.pr_number, Some(602));
        assert_eq!(
            response.pr_url.as_deref(),
            Some("https://github.com/owner/repo/pull/602")
        );
        assert_eq!(response.target_branch.as_deref(), Some(branch_name));
        assert_eq!(response.target_base_branch.as_deref(), Some("main"));
        assert_eq!(response.workspace.publication_pr_number, Some(602));
        let stored = app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .expect("workspace row should exist");
        assert_eq!(stored.publication_pr_number, None);
    }

    #[test]
    fn readiness_treats_base_ahead_as_recommended_action_not_blocker() {
        let freshness = test_freshness(true, true, Some(1), "valid");

        assert!(publish_readiness_blockers(&freshness, None).is_empty());
        assert_eq!(
            publish_readiness_recommended_actions(&freshness),
            vec!["update_from_base".to_string()]
        );
    }

    #[test]
    fn readiness_blocks_missing_changes_and_blocked_base() {
        let no_changes = test_freshness(false, false, Some(0), "valid");
        assert_eq!(
            publish_readiness_blockers(&no_changes, None),
            vec!["No committed or uncommitted workspace changes to publish".to_string()]
        );

        let blocked = test_freshness(true, true, Some(1), "blocked");
        assert_eq!(
            publish_readiness_blockers(&blocked, None),
            vec!["Workspace base is blocked".to_string()]
        );
        assert!(publish_readiness_recommended_actions(&blocked).is_empty());
    }

    #[test]
    fn readiness_includes_workspace_review_gate_blocker() {
        let freshness = test_freshness(true, true, Some(1), "valid");

        assert_eq!(
            publish_readiness_blockers(
                &freshness,
                Some("Workspace Review is required before publishing".to_string()),
            ),
            vec!["Workspace Review is required before publishing".to_string()]
        );
    }

    #[tokio::test]
    async fn submit_agent_workspace_pr_description_saves_valid_body() {
        let app_state = Arc::new(AppState::new_test());
        let conversation_id = ChatConversationId::new();
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(test_workspace(conversation_id.clone()))
            .await
            .unwrap();
        let state = test_http_state(Arc::clone(&app_state));

        let Json(response) = submit_agent_workspace_pr_description(
            State(state),
            Path(conversation_id.to_string()),
            Json(SubmitAgentWorkspacePrDescriptionRequest {
                title: Some("Better PR title".to_string()),
                body_markdown: "## Summary\n\nGenerated body".to_string(),
            }),
        )
        .await
        .unwrap();

        assert!(response.success);
        let saved = app_state
            .agent_conversation_workspace_repo
            .get_pr_description(&conversation_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.title.as_deref(), Some("Better PR title"));
        assert_eq!(saved.body_markdown, "## Summary\n\nGenerated body");
    }

    #[tokio::test]
    async fn submit_agent_workspace_pr_description_rejects_empty_body() {
        let state = test_http_state(Arc::new(AppState::new_test()));

        let (status, Json(body)) = submit_agent_workspace_pr_description(
            State(state),
            Path(ChatConversationId::new().to_string()),
            Json(SubmitAgentWorkspacePrDescriptionRequest {
                title: None,
                body_markdown: "   ".to_string(),
            }),
        )
        .await
        .unwrap_err();

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["error"].as_str().unwrap().contains("cannot be empty"));
    }

    #[tokio::test]
    async fn submit_agent_workspace_pr_description_requires_workspace() {
        let state = test_http_state(Arc::new(AppState::new_test()));

        let (status, Json(body)) = submit_agent_workspace_pr_description(
            State(state),
            Path(ChatConversationId::new().to_string()),
            Json(SubmitAgentWorkspacePrDescriptionRequest {
                title: None,
                body_markdown: "## Summary\n\nGenerated body".to_string(),
            }),
        )
        .await
        .unwrap_err();

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], "Agent workspace not found");
    }

    // =========================================================================
    // Extension A/B — Diff HTTP handler tests
    // =========================================================================

    async fn create_diff_workspace() -> (
        tempfile::TempDir,
        Arc<AppState>,
        ChatConversationId,
        std::path::PathBuf,
    ) {
        use crate::application::agent_conversation_workspace::{
            prepare_agent_conversation_workspace, AgentConversationWorkspaceBaseSelection,
        };
        use crate::domain::entities::{
            AgentConversationWorkspaceMode, IdeationAnalysisBaseRefKind, Project,
        };

        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).expect("create repo");
        git(repo.as_path(), &["init", "-b", "main"]);
        git(
            repo.as_path(),
            &["config", "user.email", "test@example.com"],
        );
        git(repo.as_path(), &["config", "user.name", "Test"]);
        std::fs::write(repo.join("base.txt"), "base\n").unwrap();
        git(repo.as_path(), &["add", "."]);
        git(repo.as_path(), &["commit", "-m", "Initial"]);

        let mut project = Project::new("Diff Test".to_string(), repo.display().to_string());
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory =
            Some(tmp.path().join("worktrees").display().to_string());

        let conversation_id = ChatConversationId::new();
        let workspace = prepare_agent_conversation_workspace(
            &project,
            &conversation_id,
            AgentConversationWorkspaceMode::Edit,
            AgentConversationWorkspaceBaseSelection {
                kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
                branch_mode: None,
                base_ref: Some("main".to_string()),
                display_name: None,
                source_pull_request: None,
            },
        )
        .await
        .expect("workspace prepared");

        let worktree_path = std::path::PathBuf::from(&workspace.worktree_path);

        let app_state = Arc::new(AppState::new_test());
        app_state
            .project_repo
            .create(project)
            .await
            .expect("seed project");
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("seed workspace");

        (tmp, app_state, conversation_id, worktree_path)
    }

    #[tokio::test]
    async fn get_staged_changes_handler_returns_staged_files() {
        let (_tmp, app_state, conversation_id, worktree_path) = create_diff_workspace().await;

        std::fs::write(worktree_path.join("staged.txt"), "staged\n").unwrap();
        git(worktree_path.as_path(), &["add", "staged.txt"]);

        let state = test_http_state(app_state);
        let Json(changes) = get_agent_workspace_staged_file_changes(
            State(state),
            Path(conversation_id.to_string()),
        )
        .await
        .expect("staged changes should load");

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].path, "staged.txt");
    }

    #[tokio::test]
    async fn get_unstaged_changes_handler_returns_unstaged_files() {
        let (_tmp, app_state, conversation_id, worktree_path) = create_diff_workspace().await;

        // Modify committed file without staging
        std::fs::write(worktree_path.join("base.txt"), "base\nmodified\n").unwrap();

        let state = test_http_state(app_state);
        let Json(changes) = get_agent_workspace_unstaged_file_changes(
            State(state),
            Path(conversation_id.to_string()),
        )
        .await
        .expect("unstaged changes should load");

        assert!(changes.iter().any(|c| c.path == "base.txt"));
    }

    #[tokio::test]
    async fn get_staged_diff_handler_returns_head_vs_index_content() {
        let (_tmp, app_state, conversation_id, worktree_path) = create_diff_workspace().await;

        std::fs::write(worktree_path.join("base.txt"), "base\nnew\n").unwrap();
        git(worktree_path.as_path(), &["add", "base.txt"]);
        // Further unstaged change — should NOT appear
        std::fs::write(worktree_path.join("base.txt"), "base\nnew\nextra\n").unwrap();

        let state = test_http_state(app_state);
        let Json(diff) = get_agent_workspace_staged_file_diff(
            State(state),
            Path((conversation_id.to_string(), "base.txt".to_string())),
        )
        .await
        .expect("staged diff should load");

        // Hunk-based: staged diff HEAD→index; "new" line appears as an addition
        assert!(
            diff.hunks
                .iter()
                .flat_map(|h| h.lines.iter())
                .any(|l| l.content.contains("new")),
            "staged diff hunks should contain the staged addition"
        );
        assert_eq!(diff.old_total_lines, 1, "HEAD has 1 line");
        assert_eq!(diff.new_total_lines, 2, "index has 2 lines");
    }

    #[tokio::test]
    async fn get_cumulative_changes_handler_shows_all_committed_changes() {
        let (_tmp, app_state, conversation_id, worktree_path) = create_diff_workspace().await;

        // Commit a change in the worktree
        std::fs::write(worktree_path.join("committed.txt"), "committed\n").unwrap();
        git(worktree_path.as_path(), &["add", "committed.txt"]);
        git(
            worktree_path.as_path(),
            &["commit", "-m", "Add committed file"],
        );

        let state = test_http_state(app_state);
        let Json(changes) = get_agent_workspace_cumulative_file_changes(
            State(state),
            Path(conversation_id.to_string()),
        )
        .await
        .expect("cumulative changes should load");

        assert!(changes.iter().any(|c| c.path == "committed.txt"));
    }

    #[tokio::test]
    async fn get_cumulative_diff_handler_shows_base_to_head_file_content() {
        let (_tmp, app_state, conversation_id, worktree_path) = create_diff_workspace().await;

        // Commit a new file in the worktree
        std::fs::write(worktree_path.join("new.rs"), "pub fn hello() {}\n").unwrap();
        git(worktree_path.as_path(), &["add", "new.rs"]);
        git(worktree_path.as_path(), &["commit", "-m", "Add new.rs"]);

        let state = test_http_state(app_state);
        let Json(diff) = get_agent_workspace_cumulative_file_diff(
            State(state),
            Path((conversation_id.to_string(), "new.rs".to_string())),
        )
        .await
        .expect("cumulative diff should load");

        assert_eq!(diff.file_path, "new.rs");
        // Hunk-based: cumulative diff base→HEAD; "hello" fn appears as additions
        assert!(
            diff.hunks
                .iter()
                .flat_map(|h| h.lines.iter())
                .any(|l| l.content.contains("hello")),
            "cumulative diff hunks should contain the committed function"
        );
        // File did not exist at base, so old_total_lines = 0
        assert_eq!(diff.old_total_lines, 0, "File did not exist in base");
    }

    #[tokio::test]
    async fn staged_and_cumulative_handlers_return_404_for_unknown_workspace() {
        let state = test_http_state(Arc::new(AppState::new_test()));

        let (status, _) = get_agent_workspace_staged_file_changes(
            State(state.clone()),
            Path(ChatConversationId::new().to_string()),
        )
        .await
        .unwrap_err();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);

        let (status, _) = get_agent_workspace_cumulative_file_changes(
            State(state),
            Path(ChatConversationId::new().to_string()),
        )
        .await
        .unwrap_err();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }
}
