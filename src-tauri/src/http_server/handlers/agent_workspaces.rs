//! Agent workspace HTTP handlers.

use std::{str::FromStr, sync::Arc};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use tauri::Emitter;

use super::*;
use crate::application::agent_conversation_workspace::{
    resolve_valid_agent_conversation_workspace_path, AgentConversationWorkspaceBaseSelection,
};
use crate::application::agent_workspace_pr_description::validate_agent_workspace_pr_description_body;
use crate::application::agent_workspace_review::{
    apply_review_artifact_to_monitor, load_agent_workspace_review_context,
    start_agent_workspace_review,
    AgentWorkspaceReviewTarget,
};
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
use crate::domain::entities::plan_branch::PrPushStatus;
use crate::domain::entities::{
    pr_comment_body_excerpt, AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentWorkspacePrCommentEvidence,
    AgentWorkspacePrDescription, AgentWorkspacePrReviewAction, AgentWorkspacePrReviewActionKind,
    AgentWorkspacePrReviewActionStatus, AgentWorkspacePrReviewMonitor,
    AgentWorkspacePrReviewMonitorStatus, AgentWorkspaceReviewMonitor,
    AgentWorkspaceReviewTargetScope, Artifact, ArtifactType, ChatConversationId,
    IdeationAnalysisBaseRefKind,
};
use crate::domain::services::github_service::{
    GithubServiceTrait, PrHealth, PrReviewFeedback, PrReviewSubmissionEvent, PrStatus,
};

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
}

impl From<crate::application::agent_workspace_review::AgentWorkspaceReviewTarget>
    for AgentWorkspaceReviewTargetResponse
{
    fn from(value: crate::application::agent_workspace_review::AgentWorkspaceReviewTarget) -> Self {
        Self {
            scope: value.scope.to_string(),
            base_ref: value.base_ref,
            base_sha: value.base_sha,
            head_ref: value.head_ref,
            head_sha: value.head_sha,
            diff_fingerprint: value.diff_fingerprint,
            source_pull_request_number: value.source_pull_request_number,
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct AgentWorkspaceReviewMonitorResponse {
    pub conversation_id: String,
    pub project_id: String,
    pub status: String,
    pub current_target_scope: Option<String>,
    pub reviewed_target_scope: Option<String>,
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
            current_target_scope: value.current_target_scope.map(|scope| scope.to_string()),
            reviewed_target_scope: value.reviewed_target_scope.map(|scope| scope.to_string()),
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
    pub is_current: bool,
    pub is_outdated: bool,
    pub should_show_tab: bool,
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
    pub is_current: bool,
    pub is_outdated: bool,
    pub should_show_tab: bool,
    pub started: bool,
    pub skipped_reason: Option<String>,
    pub was_queued: bool,
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
    let blockers = publish_readiness_blockers(&freshness);
    let recommended_actions = publish_readiness_recommended_actions(&freshness);
    Ok(Json(AgentWorkspacePublishReadinessResponse {
        success: true,
        can_publish: blockers.is_empty(),
        workspace,
        freshness,
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
    let workspace =
        load_agent_workspace_response(state.app_state.as_ref(), &conversation_id).await?;
    let events =
        load_agent_workspace_publication_events(state.app_state.as_ref(), &conversation_id).await?;

    let (health, review_feedback) = match (
        state.app_state.github_service.as_ref(),
        workspace.publication_pr_number,
    ) {
        (Some(github), Some(pr_number)) => {
            let working_dir = std::path::Path::new(&workspace.worktree_path);
            let mut health = github.fetch_pr_health(working_dir, pr_number).await.ok();
            if let Some(health) = health.as_ref() {
                import_agent_workspace_pr_comment_evidence(
                    Arc::clone(&state.app_state.agent_conversation_workspace_repo),
                    &conversation_id,
                    pr_number,
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
                .check_pr_review_feedback(working_dir, pr_number)
                .await
                .ok()
                .flatten();
            (health, review_feedback)
        }
        _ => (None, None),
    };

    let pr_number = workspace.publication_pr_number;
    let pr_url = workspace.publication_pr_url.clone();
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
) -> Result<Json<AgentWorkspaceReviewContextResponse>, JsonError> {
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

    Ok(Json(AgentWorkspaceReviewContextResponse {
        success: true,
        workspace: workspace_response,
        events,
        target: context.target.map(AgentWorkspaceReviewTargetResponse::from),
        monitor: AgentWorkspaceReviewMonitorResponse::from(context.monitor),
        is_current: context.is_current,
        is_outdated: context.is_outdated,
        should_show_tab: context.should_show_tab,
    }))
}

/// POST /api/agent-workspaces/{conversation_id}/workspace-review-runs
pub async fn start_agent_workspace_review_run(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
    Json(req): Json<StartAgentWorkspaceReviewRequest>,
) -> Result<Json<StartAgentWorkspaceReviewResponse>, JsonError> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
    let start = start_agent_workspace_review(
        std::sync::Arc::clone(&state.app_state),
        &workspace,
        req.force.unwrap_or(false),
    )
    .await
    .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    Ok(Json(StartAgentWorkspaceReviewResponse {
        success: true,
        target: start
            .context
            .target
            .map(AgentWorkspaceReviewTargetResponse::from),
        monitor: AgentWorkspaceReviewMonitorResponse::from(start.context.monitor),
        is_current: start.context.is_current,
        is_outdated: start.context.is_outdated,
        should_show_tab: start.context.should_show_tab,
        started: start.started,
        skipped_reason: start.skipped_reason,
        was_queued: start.was_queued,
    }))
}

/// POST /api/agent-workspaces/{conversation_id}/workspace-review-artifact
pub async fn write_agent_workspace_review_artifact(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
    Json(req): Json<WriteAgentWorkspaceReviewArtifactRequest>,
) -> Result<Json<WriteAgentWorkspaceReviewArtifactResponse>, JsonError> {
    let content = non_empty_string(
        normalize_workspace_review_artifact_content(req.content),
        "content",
    )?;
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
    let context = load_agent_workspace_review_context(state.app_state.as_ref(), &workspace)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let mut monitor = context.monitor;
    let target_scope = parse_workspace_review_target_scope(req.target_scope.as_deref())
        .or_else(|| context.target.as_ref().map(|target| target.scope))
        .or(monitor.current_target_scope)
        .ok_or_else(|| {
            json_error(
                StatusCode::BAD_REQUEST,
                "workspace review target scope is required",
                None,
            )
        })?;
    let target_head_sha = req.head_sha.or_else(|| {
        context
            .target
            .as_ref()
            .and_then(|target| target.head_sha.clone())
    });
    let target_diff_fingerprint = req
        .diff_fingerprint
        .or_else(|| {
            context
                .target
                .as_ref()
                .map(|target| target.diff_fingerprint.clone())
        })
        .or_else(|| monitor.current_diff_fingerprint.clone())
        .ok_or_else(|| {
            json_error(
                StatusCode::BAD_REQUEST,
                "workspace review diff_fingerprint is required",
                None,
            )
        })?;

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
        previous_artifact.as_ref().map(|artifact| artifact.name.as_str()),
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
        req.created_by_run_id,
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
    if let Some(app_handle) = &state.app_state.app_handle {
        let _ = app_handle.emit(
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
    }

    let mut artifact_response = ArtifactResponse::from(created);
    artifact_response.previous_artifact_id = previous_artifact_id.clone();

    Ok(Json(WriteAgentWorkspaceReviewArtifactResponse {
        success: true,
        monitor: AgentWorkspaceReviewMonitorResponse::from(monitor),
        artifact: artifact_response,
        previous_artifact_id,
    }))
}

/// POST /api/agent-workspaces/{conversation_id}/complete-workspace-review-run
pub async fn complete_agent_workspace_review_run(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
    Json(req): Json<CompleteAgentWorkspaceReviewRunRequest>,
) -> Result<Json<CompleteAgentWorkspaceReviewRunResponse>, JsonError> {
    let _summary = non_empty_string(req.summary, "summary")?;
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
    let monitor = crate::application::agent_workspace_review::complete_agent_workspace_review_run(
        state.app_state.as_ref(),
        &workspace,
        req.outcome,
        req.blocker,
        req.created_by_run_id,
    )
    .await
    .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
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
    if let Some(app_handle) = &state.app_state.app_handle {
        let _ = app_handle.emit(
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
    }

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
            let mut failed_monitor = monitor_for_submission_failure(monitor, error.to_string());
            failed_monitor.last_seen_head_sha = Some(action.head_sha.clone());
            let _ = state
                .app_state
                .agent_conversation_workspace_repo
                .upsert_pr_review_monitor(failed_monitor)
                .await;
            let _ = state
                .app_state
                .agent_conversation_workspace_repo
                .update_pr_review_action_status(
                    &action.id,
                    AgentWorkspacePrReviewActionStatus::Failed,
                    None,
                )
                .await;
            return Err(json_error(
                StatusCode::BAD_GATEWAY,
                "Failed to submit GitHub PR review",
                Some(error.to_string()),
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
    let pr_number = workspace.publication_pr_number.ok_or_else(|| {
        json_error(
            StatusCode::BAD_REQUEST,
            "Agent workspace has no linked pull request",
            None,
        )
    })?;
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

    if let (Some(github), Some(pr_number)) = (
        state.app_state.github_service.as_ref(),
        workspace.publication_pr_number,
    ) {
        match github
            .check_pr_status(std::path::Path::new(&workspace.worktree_path), pr_number)
            .await
        {
            Ok(PrStatus::Merged { .. }) => {
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
                    pr_number,
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
            pr_number: None,
            pr_url: None,
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
        .update_pr_auto_merge_state(
            &conversation_id,
            workspace.pr_auto_merge_current,
            Some("publishing"),
            Some("PR fix completed; publishing updates."),
        )
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
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
        "Write the Review artifact for the current PR head before proposing or submitting a PR review action",
        None,
    ))
}

fn parse_workspace_review_target_scope(
    value: Option<&str>,
) -> Option<AgentWorkspaceReviewTargetScope> {
    value.and_then(|value| AgentWorkspaceReviewTargetScope::from_str(value.trim()).ok())
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

fn monitor_for_submission_failure(
    mut monitor: AgentWorkspacePrReviewMonitor,
    error: String,
) -> AgentWorkspacePrReviewMonitor {
    monitor.status = AgentWorkspacePrReviewMonitorStatus::Blocked;
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
) -> Vec<String> {
    let mut blockers = Vec::new();
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
        .create_or_update(updated_workspace)
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
    use std::process::Command;
    use std::sync::Arc;

    use crate::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path;
    use crate::application::{AppState, TeamService, TeamStateTracker};
    use crate::commands::ExecutionState;
    use crate::domain::entities::{
        AgentConversationWorkspace, AgentConversationWorkspaceMode,
        AgentWorkspacePrCommentEvidenceUpsert, ArtifactId, ChatContextType, ChatConversation,
        IdeationAnalysisBaseRefKind, Project, ProjectId,
    };
    use crate::domain::services::github_service::{
        GithubServiceTrait, PrHealth, PrIssueCommentSummary, PrStatus, PrSyncState,
    };
    use crate::tests::mock_github_service::MockGithubService;

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
        assert!(body["error"]
            .as_str()
            .unwrap()
            .contains("Write the Review artifact"));
        let actions = app_state
            .agent_conversation_workspace_repo
            .list_pr_review_actions(&conversation_id, 10)
            .await
            .unwrap();
        assert!(actions.is_empty());
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
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(AgentConversationWorkspace::new(
                conversation_id.clone(),
                project.id.clone(),
                AgentConversationWorkspaceMode::Edit,
                IdeationAnalysisBaseRefKind::ProjectDefault,
                "main".to_string(),
                Some("Project default (main)".to_string()),
                Some(base_sha),
                branch_name.to_string(),
                workspace_path.to_string_lossy().to_string(),
            ))
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
        assert!(response.can_publish);
        assert!(response.blockers.is_empty());
        assert!(!response.needs_base_update);
        assert!(response.recommended_actions.is_empty());
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

    #[test]
    fn readiness_treats_base_ahead_as_recommended_action_not_blocker() {
        let freshness = test_freshness(true, true, Some(1), "valid");

        assert!(publish_readiness_blockers(&freshness).is_empty());
        assert_eq!(
            publish_readiness_recommended_actions(&freshness),
            vec!["update_from_base".to_string()]
        );
    }

    #[test]
    fn readiness_blocks_missing_changes_and_blocked_base() {
        let no_changes = test_freshness(false, false, Some(0), "valid");
        assert_eq!(
            publish_readiness_blockers(&no_changes),
            vec!["No committed or uncommitted workspace changes to publish".to_string()]
        );

        let blocked = test_freshness(true, true, Some(1), "blocked");
        assert_eq!(
            publish_readiness_blockers(&blocked),
            vec!["Workspace base is blocked".to_string()]
        );
        assert!(publish_readiness_recommended_actions(&blocked).is_empty());
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
