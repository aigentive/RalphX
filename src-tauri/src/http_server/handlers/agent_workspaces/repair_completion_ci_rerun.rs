//! Transient GitHub Actions rerun handling for repair completion.
//!
//! This is a thin HTTP adapter: the durable classification/reservation/triage sequence lives in
//! [`crate::application::agent_workspace_ci_rerun::execute_transient_ci_rerun`], shared with the
//! user-initiated rerun command. This module only resolves the AppState-level PR target and maps
//! the typed outcome onto the exact `completion_response` strings this endpoint has always sent.

use std::sync::Arc;

use axum::{http::StatusCode, Json};

use super::repair_completion::{completion_response, stale_completion_transition_response};
use super::*;
use crate::application::agent_workspace_ci_rerun::{
    execute_transient_ci_rerun, TransientCiRerunOutcome,
};
use crate::domain::entities::{AgentRunId, AgentWorkspaceRepairAttempt, ChatConversationId};

#[allow(clippy::too_many_arguments)]
pub(super) async fn request_transient_ci_rerun(
    state: &HttpServerState,
    conversation_id: &ChatConversationId,
    run_id: &AgentRunId,
    attempt: AgentWorkspaceRepairAttempt,
    workspace: &AgentConversationWorkspace,
    summary: &str,
    what_happened: Option<&str>,
    what_i_did: Option<&str>,
) -> Result<Json<CompleteAgentWorkspaceRepairResponse>, JsonError> {
    let target = resolve_agent_workspace_pr_fix_target(state.app_state.as_ref(), workspace)
        .await?
        .ok_or_else(|| {
            json_error(
                StatusCode::BAD_REQUEST,
                "No linked pull request is available for CI rerun.",
                None,
            )
        })?;
    let github = state.app_state.github_service.as_ref().ok_or_else(|| {
        json_error(
            StatusCode::BAD_GATEWAY,
            "GitHub service is unavailable for CI rerun.",
            None,
        )
    })?;

    let outcome = execute_transient_ci_rerun(
        Arc::clone(&state.app_state.agent_workspace_repair_repo),
        Arc::clone(&state.app_state.branch_update_repo),
        Arc::clone(github),
        attempt,
        &target.working_dir,
        target.pr_number,
        summary,
        workspace.pr_auto_merge_current,
        what_happened,
        what_i_did,
    )
    .await
    .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;

    match outcome {
        TransientCiRerunOutcome::HealthFetchFailed(error) => Err(json_error(
            StatusCode::BAD_GATEWAY,
            format!("Failed to reload PR health before CI rerun: {error}"),
            None,
        )),
        TransientCiRerunOutcome::Rejected(message) => Ok(completion_response("rejected", message)),
        TransientCiRerunOutcome::Blocked(message) => Ok(completion_response("blocked", message)),
        TransientCiRerunOutcome::RerunPending(message) => {
            Ok(completion_response("rerun_pending", message))
        }
        TransientCiRerunOutcome::ReservationStale => {
            stale_completion_transition_response(state, conversation_id, run_id).await
        }
    }
}
