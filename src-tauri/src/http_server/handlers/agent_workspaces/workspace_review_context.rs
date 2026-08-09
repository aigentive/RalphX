//! Workspace Review context HTTP handler.

use std::time::Instant;

use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Json,
};

use super::*;
use crate::application::agent_workspace_review::{
    apply_workspace_review_runtime_authority, load_current_workspace_review_eligible,
    lock_workspace_review_lifecycle,
};
use crate::application::agent_workspace_review_context::{
    load_agent_workspace_review_presentation_context, AgentWorkspaceReviewContextReadMode,
};

/// GET /api/agent-workspaces/{conversation_id}/workspace-review-context
pub async fn get_agent_workspace_review_context(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<AgentWorkspaceReviewContextQuery>,
) -> Result<Json<AgentWorkspaceReviewContextResponse>, JsonError> {
    let started = Instant::now();
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
    let _lifecycle_guard = lock_workspace_review_lifecycle(&conversation_id).await;
    let workspace = load_current_workspace_review_eligible(state.app_state.as_ref(), &workspace)
        .await
        .map_err(workspace_review_action_error)?;
    let workspace_response = agent_workspace_response_with_pr_supervision_for_state(
        state.app_state.as_ref(),
        &state.execution_state,
        workspace.clone(),
    )
    .await
    .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error, None))?;
    let events =
        load_agent_workspace_publication_events(state.app_state.as_ref(), &conversation_id).await?;
    let include_review_packet = query.include_review_packet.unwrap_or(false);
    let read_mode = if include_review_packet {
        AgentWorkspaceReviewContextReadMode::FullPacket
    } else if query.refresh_target.unwrap_or(false) {
        AgentWorkspaceReviewContextReadMode::FullTarget
    } else {
        AgentWorkspaceReviewContextReadMode::StatusSnapshot
    };
    let mut context = load_agent_workspace_review_presentation_context(
        state.app_state.as_ref(),
        &workspace,
        read_mode,
    )
    .await
    .map_err(workspace_review_action_error)?;
    let caller_run_id = workspace_review_runtime_header(&headers, "x-ralphx-agent-run-id");
    let caller_conversation_id =
        workspace_review_runtime_header(&headers, "x-ralphx-conversation-id");
    apply_workspace_review_runtime_authority(
        state.app_state.as_ref(),
        &mut context,
        caller_run_id.as_deref(),
        caller_conversation_id.as_deref(),
    )
    .await
    .map_err(workspace_review_action_error)?;
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
        can_mutate_review_state = context.can_mutate_review_state,
        review_runtime_state = %context.review_runtime_state,
        should_show_tab = context.should_show_tab,
        has_artifact = context.monitor.review_artifact_id.is_some(),
        "Served workspace Review context"
    );

    Ok(Json(AgentWorkspaceReviewContextResponse {
        success: true,
        workspace: workspace_response,
        events,
        target: context.target.map(|target| {
            AgentWorkspaceReviewTargetResponse::from_target(target, include_review_packet)
        }),
        monitor: AgentWorkspaceReviewMonitorResponse::from(context.monitor),
        goal_context: context.goal_context,
        is_current: context.is_current,
        is_outdated: context.is_outdated,
        review_artifact_is_current: context.review_artifact_is_current,
        review_artifact_is_outdated: context.review_artifact_is_outdated,
        can_mutate_review_state: context.can_mutate_review_state,
        review_runtime_state: context.review_runtime_state.to_string(),
        should_show_tab: context.should_show_tab,
    }))
}

pub(super) fn workspace_review_runtime_header(
    headers: &HeaderMap,
    name: &'static str,
) -> Option<String> {
    headers.get(name).map(|value| {
        value
            .to_str()
            .map(str::to_string)
            .unwrap_or_else(|_| "<malformed-runtime-identity>".to_string())
    })
}
