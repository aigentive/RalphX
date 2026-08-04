//! Transient GitHub Actions rerun handling for repair completion.

use std::sync::Arc;

use axum::{http::StatusCode, Json};

use super::repair_completion::{completion_response, stale_completion_transition_response};
use super::*;
use crate::application::agent_workspace_ci_rerun::{
    transient_ci_rerun_plan, CiHoldIdentity, TransientCiPlan,
};
use crate::application::agent_workspace_publish_repair_state::{
    block_agent_workspace_repair_completion, reserve_agent_workspace_ci_await,
    reserve_agent_workspace_ci_rerun, AgentWorkspaceRepairTransitionOutcome,
    MAX_AGENT_WORKSPACE_CI_RERUN_RETRIES,
};
use crate::domain::entities::{AgentRunId, AgentWorkspaceRepairAttempt, ChatConversationId};
use crate::error::AppError;

pub(super) async fn request_transient_ci_rerun(
    state: &HttpServerState,
    conversation_id: &ChatConversationId,
    owning_conversation_id: &ChatConversationId,
    run_id: &AgentRunId,
    attempt: AgentWorkspaceRepairAttempt,
    workspace: &AgentConversationWorkspace,
    summary: &str,
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
    let health = github
        .fetch_pr_health(&target.working_dir, target.pr_number)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::BAD_GATEWAY,
                format!("Failed to reload PR health before CI rerun: {error}"),
                None,
            )
        })?;
    match transient_ci_rerun_plan(&health) {
        TransientCiPlan::MissingHead => Ok(reject_transient_ci_claim(
            "RalphX could not read the current PR head from fresh GitHub health, so it cannot verify a transient CI classification. Re-check PR health and either fix the failure or report a blocker.",
        )),
        TransientCiPlan::NoObservedFailure => block_transient_ci_rerun(
            state,
            attempt,
            workspace,
            summary,
            "RalphX could not identify a failed GitHub Actions run from fresh PR health.",
        )
        .await,
        TransientCiPlan::DeterministicFailures(checks) => Ok(reject_transient_ci_claim(format!(
            "Rejected `transient_ci`: PR health still reports real failing checks at the current head: {}. Rerunning cannot clear them. Fix the failure and complete with `fixed`, or classify `pre_existing_on_base` / `needs_human`.",
            checks.join(", ")
        ))),
        TransientCiPlan::AwaitRuns(hold) => {
            await_transient_ci_runs(
                state,
                conversation_id,
                owning_conversation_id,
                run_id,
                attempt,
                workspace,
                summary,
                &hold,
                "RalphX is waiting for the in-progress GitHub Actions run to finish before rerunning it.",
            )
            .await
        }
        TransientCiPlan::Rerun { run_ids, hold } => {
            if attempt.ci_rerun_count >= MAX_AGENT_WORKSPACE_CI_RERUN_RETRIES {
                return block_transient_ci_rerun(
                    state,
                    attempt,
                    workspace,
                    summary,
                    "The transient CI rerun budget is exhausted.",
                )
                .await;
            }
            let reserved = match reserve_agent_workspace_ci_rerun(
                Arc::clone(&state.app_state.agent_workspace_repair_repo),
                attempt,
                &hold.to_fingerprint(),
                "Transient CI failure accepted; RalphX is rerunning failed GitHub Actions jobs.",
                workspace.pr_auto_merge_current,
            )
            .await
            .map_err(|error| {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
            })? {
                AgentWorkspaceRepairTransitionOutcome::Applied(attempt) => attempt,
                AgentWorkspaceRepairTransitionOutcome::Stale(_)
                | AgentWorkspaceRepairTransitionOutcome::Missing => {
                    return stale_completion_transition_response(state, conversation_id, owning_conversation_id, run_id)
                        .await;
                }
            };

            let mut rerun_errors = Vec::new();
            for workflow_run_id in run_ids {
                if let Err(error) = github
                    .rerun_failed_workflow(&target.working_dir, workflow_run_id)
                    .await
                {
                    rerun_errors.push((github_rerun_error_is_transient(&error), error.to_string()));
                }
            }
            if rerun_errors.is_empty() {
                return Ok(completion_response(
                    "rerun_pending",
                    "RalphX accepted the transient CI classification and reran the failed GitHub Actions jobs.",
                ));
            }
            if let Some((_, error)) = rerun_errors.iter().find(|(transient, _)| !transient) {
                return block_transient_ci_rerun(
                    state,
                    reserved,
                    workspace,
                    summary,
                    &format!("GitHub Actions rerun failed: {error}"),
                )
                .await;
            }
            let errors = rerun_errors
                .into_iter()
                .map(|(_, error)| error)
                .collect::<Vec<_>>()
                .join("; ");
            await_transient_ci_runs(
                state,
                conversation_id,
                owning_conversation_id,
                run_id,
                reserved,
                workspace,
                summary,
                &hold,
                &format!("GitHub Actions rerun is temporarily unavailable: {errors}"),
            )
            .await
        }
    }
}

fn reject_transient_ci_claim(
    message: impl Into<String>,
) -> Json<CompleteAgentWorkspaceRepairResponse> {
    completion_response("rejected", message)
}

#[allow(clippy::too_many_arguments)]
async fn await_transient_ci_runs(
    state: &HttpServerState,
    conversation_id: &ChatConversationId,
    owning_conversation_id: &ChatConversationId,
    run_id: &AgentRunId,
    attempt: AgentWorkspaceRepairAttempt,
    workspace: &AgentConversationWorkspace,
    summary: &str,
    hold: &CiHoldIdentity,
    message: &str,
) -> Result<Json<CompleteAgentWorkspaceRepairResponse>, JsonError> {
    match reserve_agent_workspace_ci_await(
        Arc::clone(&state.app_state.agent_workspace_repair_repo),
        attempt,
        &hold.to_fingerprint(),
        summary,
        workspace.pr_auto_merge_current,
    )
    .await
    .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
    {
        AgentWorkspaceRepairTransitionOutcome::Applied(_) => {
            Ok(completion_response("rerun_pending", message))
        }
        AgentWorkspaceRepairTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairTransitionOutcome::Missing => {
            stale_completion_transition_response(state, conversation_id, owning_conversation_id, run_id).await
        }
    }
}

fn github_rerun_error_is_transient(error: &AppError) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    let rate_limited = message.contains("rate limit") || message.contains("secondary rate");
    if message.contains("not found")
        || mentions_http_status(&message, "404")
        || message.contains("no such run")
        || message.contains("authentication")
        || message.contains("unauthorized")
        || mentions_http_status(&message, "401")
        || (message.contains("403 forbidden") && !rate_limited)
        || message.contains("gh auth")
        || message.contains("workflow file")
        || message.contains("cannot be rerun")
    {
        return false;
    }
    rate_limited
        || message.contains("in progress")
        || message.contains("currently in progress")
        || message.contains("timeout")
        || message.contains("timed out")
        || message.contains("connection")
        || message.contains("network")
        || message.contains("temporarily")
        || mentions_http_status(&message, "502")
        || mentions_http_status(&message, "503")
        || mentions_http_status(&message, "504")
        || message.contains("server error")
}

/// True when `code` appears as an HTTP status token rather than as a substring of an
/// unrelated number such as a workflow run id echoed in a `gh` API URL.
fn mentions_http_status(message: &str, code: &str) -> bool {
    message.match_indices(code).any(|(start, _)| {
        let bytes = message.as_bytes();
        let before_is_digit = start
            .checked_sub(1)
            .is_some_and(|index| bytes[index].is_ascii_digit());
        let after_is_digit = bytes
            .get(start + code.len())
            .is_some_and(|byte| byte.is_ascii_digit());
        !before_is_digit && !after_is_digit
    })
}

async fn block_transient_ci_rerun(
    state: &HttpServerState,
    attempt: AgentWorkspaceRepairAttempt,
    workspace: &AgentConversationWorkspace,
    summary: &str,
    blocker: &str,
) -> Result<Json<CompleteAgentWorkspaceRepairResponse>, JsonError> {
    match block_agent_workspace_repair_completion(
        Arc::clone(&state.app_state.agent_workspace_repair_repo),
        Arc::clone(&state.app_state.branch_update_repo),
        attempt,
        summary,
        blocker,
        workspace.pr_auto_merge_current,
    )
    .await
    .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
    {
        AgentWorkspaceRepairTransitionOutcome::Applied(_) => {
            Ok(completion_response("blocked", blocker))
        }
        AgentWorkspaceRepairTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairTransitionOutcome::Missing => Ok(completion_response(
            "blocked",
            "The CI rerun request became stale before it could be settled.",
        )),
    }
}
