//! Retargets an interrupted-but-clean workspace repair onto a base that moved while it ran.
//!
//! There are deliberately two sources of truth here, and they answer different questions:
//!
//! - [`super::pr_autofix_redelivery::repair_base_advanced`] is the cheap durable predicate. It
//!   compares the generation's `target_base_commit` against `workspace.base_commit` and answers
//!   *should we consider retargeting at all*. It is the same notion of "base advanced" the blocked
//!   redelivery path already uses, so this module reuses it rather than inventing a second one.
//! - The completion classifier answers *onto exactly which commit*, because it read real Git. The
//!   durable `workspace.base_commit` can itself be stale, so the classifier's commit always wins.
//!
//! When the two disagree the classifier still decides and the divergence is logged, because a
//! silent preference between two disagreeing sources is how stale base state becomes invisible.

use std::path::PathBuf;
use std::sync::Arc;

use super::durable_attempt_recovery::{
    reserve_and_deliver_repair_dispatch, DurableRepairRecoveryOutcome,
};
use super::pr_autofix_redelivery::repair_base_advanced;
use crate::application::agent_conversation_workspace::resolve_effective_agent_conversation_workspace_path;
use crate::application::agent_workspace_publish_repair_state::{
    release_and_clear_agent_workspace_repair_target_lease,
    settle_repair_and_start_retargeted_successor, AgentWorkspaceRepairRetargetOutcome,
    AgentWorkspaceRepairTransitionOutcome,
};
use crate::application::{AppState, GitService};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspacePublicationEvent,
    AgentWorkspaceRepairAttempt, GitTargetIdentity,
};
use crate::error::{AppError, AppResult};

/// Publication step recorded when an interrupted repair is aimed at a newer base instead of blocked.
pub(crate) const REPAIR_BASE_ADVANCE_RETARGETED_STEP: &str = "repair_base_advance_retargeted";

const RETARGET_SUMMARY: &str =
    "Base advanced while the repair was interrupted; retargeting the committed repair to the new base.";
const RETARGET_RESERVATION_SUMMARY: &str =
    "Retargeting the durable workspace repair to the advanced base.";
const RETARGET_SETTLEMENT_SUMMARY: &str =
    "Redelivered the durable workspace repair against the advanced base.";

/// Supersedes a reserved `Validating` generation with one aimed at `new_target_base_commit` and
/// delivers it through the normal recovery dispatch lane.
///
/// The caller owns the reservation, so a `Stale` return means another recovery pass won the race
/// and this pass must not write anything further.
///
/// # Errors
///
/// Returns an error when a repository write before the successor exists fails, or when the
/// successor's own delivery fails. Resolution failures after the successor is durable degrade to
/// `Continued` instead of propagating.
pub(super) async fn retarget_reserved_repair_to_advanced_base(
    state: &AppState,
    reserved: AgentWorkspaceRepairAttempt,
    workspace: &AgentConversationWorkspace,
    new_target_base_commit: &str,
) -> AppResult<DurableRepairRecoveryOutcome> {
    if !repair_base_advanced(&reserved, workspace) {
        // The durable workspace base has not caught up yet. The classifier read Git directly, so it
        // is the authority, but the disagreement is worth seeing in logs.
        tracing::info!(
            conversation_id = reserved.conversation_id.as_str(),
            durable_target_base = reserved.target_base_commit.as_deref().unwrap_or("<none>"),
            durable_workspace_base = workspace.base_commit.as_deref().unwrap_or("<none>"),
            observed_target_base = new_target_base_commit,
            "Retargeting an interrupted repair on Git evidence the durable workspace base does not show yet"
        );
    }
    // The canonical target lease is owned per attempt id, so the superseded generation has to let
    // go before its successor can reserve a dispatch against the same target. Releasing first keeps
    // the `Validating` reservation as the only fence during the handover, exactly as the blocked
    // path does before it starts a successor.
    let reserved = match release_and_clear_agent_workspace_repair_target_lease(
        state.agent_workspace_repair_repo.as_ref(),
        state.branch_update_repo.as_ref(),
        reserved,
    )
    .await?
    {
        AgentWorkspaceRepairTransitionOutcome::Applied(attempt) => attempt,
        AgentWorkspaceRepairTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairTransitionOutcome::Missing => {
            return Ok(DurableRepairRecoveryOutcome::Stale);
        }
    };
    let successor = match settle_repair_and_start_retargeted_successor(
        Arc::clone(&state.agent_workspace_repair_repo),
        &reserved,
        workspace,
        new_target_base_commit,
        RETARGET_SUMMARY,
    )
    .await?
    {
        AgentWorkspaceRepairRetargetOutcome::Started(successor) => *successor,
        AgentWorkspaceRepairRetargetOutcome::Stale => {
            return Ok(DurableRepairRecoveryOutcome::Stale);
        }
    };
    append_base_advance_retargeted_event(state, workspace, new_target_base_commit).await;

    // Degrading is safe here for the same reason the event above is best effort: the successor is
    // already durable, so this lineage's recovery has succeeded whatever these reads say. An
    // undelivered successor is exactly the shape `rescue_orphaned_repair_dispatch` owns, and the
    // next sweep delivers it after the spawn grace. Propagating instead would abort the batch
    // sweep in `recover_agent_workspace_repair_attempts_for_state` for every other workspace.
    let dispatch = match resolve_retarget_dispatch_inputs(state, workspace).await {
        Ok(dispatch) => dispatch,
        Err(error) => {
            tracing::warn!(
                conversation_id = workspace.conversation_id.as_str(),
                successor_attempt_id = successor.id.as_str(),
                error = %error,
                "Retargeted workspace repair is durable but its dispatch inputs could not be resolved; leaving delivery to the orphan rescue lane"
            );
            return Ok(DurableRepairRecoveryOutcome::Continued);
        }
    };
    reserve_and_deliver_repair_dispatch(
        state,
        successor,
        dispatch.target_identity,
        workspace.clone(),
        dispatch.workspace_path,
        RETARGET_RESERVATION_SUMMARY,
        RETARGET_SETTLEMENT_SUMMARY,
    )
    .await
}

/// The project, workspace path, and canonical Git target the successor needs before it can be
/// dispatched.
struct RetargetDispatchInputs {
    workspace_path: PathBuf,
    target_identity: GitTargetIdentity,
}

async fn resolve_retarget_dispatch_inputs(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> AppResult<RetargetDispatchInputs> {
    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await?
        .ok_or_else(|| AppError::ProjectNotFound(workspace.project_id.to_string()))?;
    let resolved = resolve_effective_agent_conversation_workspace_path(
        &project,
        workspace,
        state.plan_branch_repo.as_ref(),
    )
    .await?;
    let target_identity =
        GitService::canonical_target_identity(&resolved.path, &workspace.branch_name).await?;
    Ok(RetargetDispatchInputs {
        workspace_path: resolved.path,
        target_identity,
    })
}

/// Best effort by design: the lineage is already correct once the successor exists, so failing to
/// record the event must never turn a successful retarget into a failed recovery pass.
async fn append_base_advance_retargeted_event(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    new_target_base_commit: &str,
) {
    if let Err(error) = state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            workspace.conversation_id.clone(),
            REPAIR_BASE_ADVANCE_RETARGETED_STEP,
            "repairing",
            format!("The base moved to {new_target_base_commit} while the repair was interrupted; RalphX retargeted the committed repair instead of blocking it."),
            None,
        ))
        .await
    {
        tracing::warn!(
            conversation_id = workspace.conversation_id.as_str(),
            error = %error,
            "Failed to record the base-advance retarget publication event"
        );
    }
}
