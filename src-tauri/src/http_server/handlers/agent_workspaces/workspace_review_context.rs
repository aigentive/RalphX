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
    load_agent_workspace_review_presentation_context,
    load_persisted_workspace_review_snapshot_context, AgentWorkspaceReviewContextReadMode,
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
    let workspace_response =
        agent_workspace_response_for_state(state.app_state.as_ref(), workspace.clone())
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

/// GET /api/agent-workspaces/{conversation_id}/workspace-review-context — the REMOUNTED
/// variant served on the remote listener (:3849).
///
/// A seam split in the shape `remote_transcript_commands` established: the local handler above
/// stays exactly as it is, and this one is its provable read-only subset. Three differences,
/// each of which is the reason a remount of the local handler would be unsafe:
///
/// 1. **No query extractor.** `?refresh_target=true` (and `?include_review_packet=true`) select
///    `FullTarget` / `FullPacket`, which recompute the review target through
///    `resolve_review_target` — a `git` command lane. Recomputation is not a read: it re-derives
///    the target receipt and diff fingerprint the whole review gate keys on, and it spawns. By
///    taking no `Query`, this handler cannot honour the parameter no matter what the client
///    sends; axum discards the unmatched query string.
/// 2. **Persisted snapshot only.** [`load_persisted_workspace_review_snapshot_context`] never
///    falls through to the calculating path, so even a cold monitor cannot trigger git here.
/// 3. **No repair-recovery scheduling.** `agent_workspace_response_for_state` schedules PR
///    supervision recovery, which can fetch, enqueue an agent, or continue publication; the
///    remote path uses `agent_workspace_response_for_remote_snapshot`.
///
/// Runtime authority is pinned closed rather than resolved: `x-ralphx-agent-run-id` /
/// `x-ralphx-conversation-id` are agent-run trust headers a paired device must never be able to
/// assert (`fetch_remount_tests::forged_trust_headers_do_not_reach_a_mounted_route`), so this
/// handler reads no headers and leaves `build_context`'s defaults —
/// `can_mutate_review_state: false`, `MissingRuntimeIdentity`.
///
/// What the client still gets, and it is the whole point of the route: the persisted monitor's
/// `review_artifact_id` / `review_requested_changes_artifact_id`, which unblock the already
/// registered `get_artifact` reads.
pub async fn get_agent_workspace_review_context_remote_snapshot(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
) -> Result<Json<AgentWorkspaceReviewContextResponse>, JsonError> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
    let context =
        load_persisted_workspace_review_snapshot_context(state.app_state.as_ref(), &workspace)
            .await
            .map_err(workspace_review_action_error)?;
    let workspace_response =
        agent_workspace_response_for_remote_snapshot(state.app_state.as_ref(), workspace)
            .await
            .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error, None))?;
    let events =
        load_agent_workspace_publication_events(state.app_state.as_ref(), &conversation_id).await?;

    Ok(Json(AgentWorkspaceReviewContextResponse {
        success: true,
        workspace: workspace_response,
        events,
        target: context
            .target
            .map(|target| AgentWorkspaceReviewTargetResponse::from_target(target, false)),
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
