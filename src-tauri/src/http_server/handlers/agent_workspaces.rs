//! Agent workspace HTTP handlers.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};

use super::*;
use crate::application::agent_conversation_workspace::AgentConversationWorkspaceBaseSelection;
use crate::application::agent_workspace_pr_description::validate_agent_workspace_pr_description_body;
use crate::application::publish_resilience::{
    inspect_publish_branch_freshness_for_source, push_publish_branch,
    verify_agent_workspace_repair_completion, AgentWorkspaceRepairCompletionCheck,
};
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
    AgentConversationWorkspacePublicationEvent, AgentWorkspacePrDescription, ChatConversationId,
    IdeationAnalysisBaseRefKind,
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
    if let Some(response) = publish_action_response_for_existing_workspace_state(workspace) {
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

async fn load_agent_workspace_response(
    state: &AppState,
    conversation_id: &ChatConversationId,
) -> Result<AgentConversationWorkspaceResponse, JsonError> {
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Agent workspace not found", None))?;
    agent_workspace_response_for_state(state, workspace)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error, None))
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

fn needs_agent_repair_response(
    workspace: AgentConversationWorkspaceResponse,
) -> AgentWorkspacePublishActionResponse {
    let pr_number = workspace.publication_pr_number;
    let pr_url = workspace.publication_pr_url.clone();
    AgentWorkspacePublishActionResponse {
        success: true,
        status: "needs_agent_repair".to_string(),
        message: "Workspace needs agent repair before publishing can continue".to_string(),
        repair_queued: true,
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

fn publish_action_response_for_existing_workspace_state(
    workspace: AgentConversationWorkspaceResponse,
) -> Option<AgentWorkspacePublishActionResponse> {
    match workspace.publication_push_status.as_deref() {
        status if is_publish_in_progress(status) => Some(publish_in_progress_response(workspace)),
        Some("needs_agent") => Some(needs_agent_repair_response(workspace)),
        _ => None,
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
        repair_queued: true,
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
    } else if post_repair_action == AgentWorkspacePostRepairAction::UpdateOnly {
        (
            "Agent workspace repair verified".to_string(),
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
    .map_err(|e| json_error(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string(), None))
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
    .map_err(|e| json_error(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string(), None))
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
    .map_err(|e| json_error(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string(), None))
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
    .map_err(|e| json_error(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string(), None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path as StdPath;
    use std::process::Command;
    use std::sync::Arc;

    use crate::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path;
    use crate::application::{AppState, TeamService, TeamStateTracker};
    use crate::commands::ExecutionState;
    use crate::domain::entities::{
        AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatContextType,
        ChatConversation, IdeationAnalysisBaseRefKind, Project, ProjectId,
    };

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
    async fn needs_repair_action_response_preserves_error_payload() {
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
        assert!(response.repair_queued);
        assert!(response.freshness.is_none());
        assert_eq!(response.pr_number, None);
        assert_eq!(response.pr_url, None);
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
        assert!(response.repair_queued);
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

    async fn create_diff_workspace(
    ) -> (
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
        git(repo.as_path(), &["config", "user.email", "test@example.com"]);
        git(repo.as_path(), &["config", "user.name", "Test"]);
        std::fs::write(repo.join("base.txt"), "base\n").unwrap();
        git(repo.as_path(), &["add", "."]);
        git(repo.as_path(), &["commit", "-m", "Initial"]);

        let mut project =
            Project::new("Diff Test".to_string(), repo.display().to_string());
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
            },
        )
        .await
        .expect("workspace prepared");

        let worktree_path = std::path::PathBuf::from(&workspace.worktree_path);

        let app_state = Arc::new(AppState::new_test());
        app_state.project_repo.create(project).await.expect("seed project");
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("seed workspace");

        (tmp, app_state, conversation_id, worktree_path)
    }

    #[tokio::test]
    async fn get_staged_changes_handler_returns_staged_files() {
        let (_tmp, app_state, conversation_id, worktree_path) =
            create_diff_workspace().await;

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
        let (_tmp, app_state, conversation_id, worktree_path) =
            create_diff_workspace().await;

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
        let (_tmp, app_state, conversation_id, worktree_path) =
            create_diff_workspace().await;

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

        assert_eq!(diff.old_content, "base\n");
        assert_eq!(diff.new_content, "base\nnew\n");
    }

    #[tokio::test]
    async fn get_cumulative_changes_handler_shows_all_committed_changes() {
        let (_tmp, app_state, conversation_id, worktree_path) =
            create_diff_workspace().await;

        // Commit a change in the worktree
        std::fs::write(worktree_path.join("committed.txt"), "committed\n").unwrap();
        git(worktree_path.as_path(), &["add", "committed.txt"]);
        git(worktree_path.as_path(), &["commit", "-m", "Add committed file"]);

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
        let (_tmp, app_state, conversation_id, worktree_path) =
            create_diff_workspace().await;

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
        assert!(diff.new_content.contains("hello"));
        assert_eq!(diff.old_content, "");
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
