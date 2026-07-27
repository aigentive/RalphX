use std::sync::Arc;

use chrono::Utc;

use super::StalePublishRepairRecoveryOutcome;
use crate::application::agent_conversation_workspace::resolve_effective_agent_conversation_workspace_path;
use crate::application::agent_workspace_publish_repair_state::{
    agent_workspace_repair_dispatch_is_due, block_agent_workspace_repair_completion,
    classify_agent_workspace_repair_delivery, continue_agent_workspace_repair_at_boundary,
    inspect_agent_workspace_repair_completion, record_agent_workspace_repair_validation,
    release_and_clear_agent_workspace_repair_target_lease,
    reserve_agent_workspace_repair_completion_validation, reserve_agent_workspace_repair_dispatch,
    resume_current_agent_workspace_repair_publish, settle_agent_workspace_repair_dispatch_outcome,
    transition_agent_workspace_repair_attempt, validate_agent_workspace_repair_target_lease,
    AgentWorkspaceRepairDispatchOutcome, AgentWorkspaceRepairDispatchSettlement,
    AgentWorkspaceRepairPublishResumeOutcome, AgentWorkspaceRepairTransitionOutcome,
};
use crate::application::chat_service::{ChatService, SendMessageOptions, SendQueuePolicy};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspacePublicationEvent, AgentRunId,
    AgentWorkspaceRepairAttempt, AgentWorkspaceRepairAttemptId, AgentWorkspaceRepairContinuation,
    AgentWorkspaceRepairPhase, AgentWorkspaceRepairSource, ChatContextType, GitTargetLeaseOwner,
};
use crate::domain::repositories::{
    AgentRunRepository, AgentWorkspaceRepairCompatibilityProjection,
    ImportLegacyAgentWorkspaceRepairAttempt, ImportLegacyAgentWorkspaceRepairAttemptOutcome,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::agent_names::AGENT_WORKSPACE_REPAIR;

const LEGACY_REPAIR_IMPORT_BLOCKED_STEP: &str = "legacy_repair_import_blocked";
const LEGACY_REPAIR_IMPORT_BLOCKED_CLASSIFICATION: &str = "legacy_repair_import_ambiguous";
const LEGACY_REPAIR_IMPORTED_STEP: &str = "legacy_repair_imported";
const LEGACY_REPAIR_IMPORTED_CLASSIFICATION: &str = "legacy_repair_import_exact";
const LEGACY_REPAIR_RUN_CLASSIFICATION_PREFIX: &str = "agent_fixable:run:";

pub(crate) async fn recover_stale_publish_repair_for_workspace_in_state_result(
    state: &AppState,
    workspace: AgentConversationWorkspace,
) -> AppResult<(
    AgentConversationWorkspace,
    StalePublishRepairRecoveryOutcome,
)> {
    let attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&workspace.conversation_id)
        .await?;
    let outcome = match attempt {
        Some(attempt) => reconcile_agent_workspace_repair_attempt(state, attempt).await?,
        None => {
            // A legacy projection is migration input, not a fallback authority. Once any
            // generation has existed, even a settled one, the projection is terminally ignored.
            if state
                .agent_workspace_repair_repo
                .get_latest_repair_attempt_for_conversation(&workspace.conversation_id)
                .await?
                .is_some()
            {
                DurableRepairRecoveryOutcome::Noop
            } else if is_legacy_repair_projection(&workspace) {
                import_or_block_legacy_repair_attempt(state, &workspace).await?
            } else {
                DurableRepairRecoveryOutcome::Noop
            }
        }
    };
    let refreshed = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await?
        .unwrap_or(workspace);
    Ok((refreshed, outcome.into_stale_outcome()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DurableRepairRecoveryOutcome {
    Noop,
    Active,
    Continued,
    Blocked,
    Stale,
}

impl DurableRepairRecoveryOutcome {
    fn was_recovered(self) -> bool {
        matches!(self, Self::Continued | Self::Blocked)
    }

    fn into_stale_outcome(self) -> StalePublishRepairRecoveryOutcome {
        match self {
            Self::Noop | Self::Stale => StalePublishRepairRecoveryOutcome::Noop,
            Self::Active => StalePublishRepairRecoveryOutcome::ActiveRepairReconciled,
            Self::Continued => StalePublishRepairRecoveryOutcome::RetryEligible,
            Self::Blocked => StalePublishRepairRecoveryOutcome::Manual,
        }
    }
}

/// Reconcile every durable repair generation through one attempt-first path. A legacy workspace
/// projection is considered only when no durable attempt exists, and then only by the isolated
/// import adapter below.
pub(crate) async fn recover_agent_workspace_repair_attempts_for_state(
    state: &AppState,
) -> AppResult<u32> {
    let attempts = state
        .agent_workspace_repair_repo
        .list_recoverable_repair_attempts()
        .await?;
    let mut recovered = 0;
    for attempt in attempts {
        if reconcile_agent_workspace_repair_attempt(state, attempt)
            .await?
            .was_recovered()
        {
            recovered += 1;
        }
    }
    Ok(recovered)
}

/// Terminal notifications are hints, never authority. The run must still be the exact durable
/// reservation before recovery can inspect or mutate the attempt.
#[cfg(any(test, feature = "test-utils"))]
pub async fn recover_agent_workspace_repair_after_terminal_run(
    state: &AppState,
    conversation_id: &crate::domain::entities::ChatConversationId,
    run_id: &AgentRunId,
) -> AppResult<bool> {
    recover_agent_workspace_repair_after_terminal_run_in_state(state, conversation_id, run_id).await
}

#[cfg(not(any(test, feature = "test-utils")))]
pub(crate) async fn recover_agent_workspace_repair_after_terminal_run(
    state: &AppState,
    conversation_id: &crate::domain::entities::ChatConversationId,
    run_id: &AgentRunId,
) -> AppResult<bool> {
    recover_agent_workspace_repair_after_terminal_run_in_state(state, conversation_id, run_id).await
}

async fn recover_agent_workspace_repair_after_terminal_run_in_state(
    state: &AppState,
    conversation_id: &crate::domain::entities::ChatConversationId,
    run_id: &AgentRunId,
) -> AppResult<bool> {
    let Some(attempt) = state
        .agent_workspace_repair_repo
        .get_repair_attempt_for_run(conversation_id, run_id)
        .await?
    else {
        return Ok(false);
    };
    if let Some(run) = state.agent_run_repo.get_by_id(run_id).await? {
        if run.conversation_id != *conversation_id || !run.status.is_terminal() {
            return Ok(false);
        }
    }
    Ok(reconcile_agent_workspace_repair_attempt(state, attempt)
        .await?
        .was_recovered())
}

async fn reconcile_agent_workspace_repair_attempt(
    state: &AppState,
    attempt: AgentWorkspaceRepairAttempt,
) -> AppResult<DurableRepairRecoveryOutcome> {
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&attempt.conversation_id)
        .await?;
    let Some(current) = current else {
        return Ok(DurableRepairRecoveryOutcome::Stale);
    };
    if current.id != attempt.id
        || current.generation != attempt.generation
        || current.updated_at != attempt.updated_at
    {
        return Ok(DurableRepairRecoveryOutcome::Stale);
    }

    match current.phase {
        AgentWorkspaceRepairPhase::Dispatching => {
            let active = match current.reserved_agent_run_id.as_ref() {
                Some(run_id) => state
                    .agent_run_repo
                    .get_by_id(run_id)
                    .await?
                    .is_some_and(|run| {
                        run.conversation_id == current.conversation_id && run.status.is_active()
                    }),
                None => false,
            };
            if active {
                Ok(DurableRepairRecoveryOutcome::Active)
            } else {
                schedule_interrupted_dispatch_retry(state, current).await
            }
        }
        AgentWorkspaceRepairPhase::Repairing => {
            let active = match current.reserved_agent_run_id.as_ref() {
                Some(run_id) => state
                    .agent_run_repo
                    .get_by_id(run_id)
                    .await?
                    .is_some_and(|run| {
                        run.conversation_id == current.conversation_id && run.status.is_active()
                    }),
                None => false,
            };
            if active {
                Ok(DurableRepairRecoveryOutcome::Active)
            } else {
                recover_clean_interrupted_repair(state, current).await
            }
        }
        AgentWorkspaceRepairPhase::Requested => {
            if current.next_dispatch_at.is_none() {
                return Ok(DurableRepairRecoveryOutcome::Noop);
            }
            if !agent_workspace_repair_dispatch_is_due(&current, Utc::now()) {
                return Ok(DurableRepairRecoveryOutcome::Noop);
            }
            redeliver_due_repair_dispatch(state, current).await
        }
        AgentWorkspaceRepairPhase::Validating => {
            block_recovery_attempt(
                state,
                current,
                "Workspace repair recovery cannot prove the interrupted dispatch or validation result. Retry the blocked operation.",
            )
            .await
        }
        AgentWorkspaceRepairPhase::ContinuationPending | AgentWorkspaceRepairPhase::Continuing => {
            match crate::application::publish_resilience::continue_agent_workspace_repair_publish(
                state,
                current.clone(),
            )
            .await
            {
                Ok(Some(
                    crate::application::publish_resilience::AgentWorkspaceRepairPushOutcome::Busy,
                )) => Ok(DurableRepairRecoveryOutcome::Active),
                Ok(Some(
                    crate::application::publish_resilience::AgentWorkspaceRepairPushOutcome::Stale,
                )) => Ok(DurableRepairRecoveryOutcome::Stale),
                Ok(Some(_)) => Ok(DurableRepairRecoveryOutcome::Continued),
                Ok(None) => {
                    block_recovery_attempt(
                        state,
                        current,
                        "Workspace repair continuation could not prove a publish runtime. Retry the blocked operation.",
                    )
                    .await
                }
                Err(error) => {
                    record_continuation_recovery_failure(state, current, &error).await
                }
            }
        }
        AgentWorkspaceRepairPhase::AwaitingReview => {
            match resume_current_agent_workspace_repair_publish(
                state,
                &current.conversation_id,
                "Resuming the durable workspace repair continuation after Workspace Review.",
                false,
            )
            .await?
            {
                AgentWorkspaceRepairPublishResumeOutcome::Continue(next) => {
                    Box::pin(reconcile_agent_workspace_repair_attempt(state, next)).await
                }
                AgentWorkspaceRepairPublishResumeOutcome::AwaitingReview
                | AgentWorkspaceRepairPublishResumeOutcome::Ready
                | AgentWorkspaceRepairPublishResumeOutcome::Blocked => {
                    Ok(DurableRepairRecoveryOutcome::Noop)
                }
                AgentWorkspaceRepairPublishResumeOutcome::NoAttempt
                | AgentWorkspaceRepairPublishResumeOutcome::Busy
                | AgentWorkspaceRepairPublishResumeOutcome::Stale => {
                    Ok(DurableRepairRecoveryOutcome::Stale)
                }
            }
        }
        AgentWorkspaceRepairPhase::Ready | AgentWorkspaceRepairPhase::Blocked => {
            release_repair_lease_if_settled_boundary(state, &current).await?;
            Ok(DurableRepairRecoveryOutcome::Noop)
        }
    }
}

/// A delivery that failed before a trusted repair worker ran is recoverable. The exact
/// `Dispatching` snapshot still owns the persisted canonical lease, so scheduling the next due
/// retry cannot race a successor or turn an unknown delivery into a second agent run.
async fn schedule_interrupted_dispatch_retry(
    state: &AppState,
    attempt: AgentWorkspaceRepairAttempt,
) -> AppResult<DurableRepairRecoveryOutcome> {
    match settle_agent_workspace_repair_dispatch_outcome(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        attempt,
        AgentWorkspaceRepairDispatchSettlement::RetryableFailure,
        "Workspace repair delivery was interrupted before its reserved worker became active.",
        None,
    )
    .await?
    {
        AgentWorkspaceRepairTransitionOutcome::Applied(attempt) => {
            if attempt.phase == AgentWorkspaceRepairPhase::Blocked {
                release_repair_lease_if_settled_boundary(state, &attempt).await?;
                Ok(DurableRepairRecoveryOutcome::Blocked)
            } else {
                Ok(DurableRepairRecoveryOutcome::Continued)
            }
        }
        AgentWorkspaceRepairTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairTransitionOutcome::Missing => Ok(DurableRepairRecoveryOutcome::Stale),
    }
}

fn due_repair_dispatch_message(
    attempt: &AgentWorkspaceRepairAttempt,
    workspace: &AgentConversationWorkspace,
) -> String {
    let continuation = match attempt.continuation {
        AgentWorkspaceRepairContinuation::UpdateOnly | AgentWorkspaceRepairContinuation::Manual => {
            "Resolve the workspace/base integration problem and commit the repaired workspace."
        }
        AgentWorkspaceRepairContinuation::Publish
        | AgentWorkspaceRepairContinuation::ResumePrSupervision => {
            "Resolve the workspace publish problem and commit the repaired workspace so the durable publish continuation can resume."
        }
    };
    let reason = attempt
        .pending_reasons
        .last()
        .map(String::as_str)
        .unwrap_or("The current durable workspace repair still needs attention.");
    format!(
        "{continuation}\n\nInspect the current workspace state before changing files. When the repair is committed, use the available repair-completion tool.\n\nContext: {reason}\nWorkspace branch: {}\nBase ref: {}",
        workspace.branch_name, attempt.target_base_ref
    )
}

async fn redeliver_due_repair_dispatch(
    state: &AppState,
    attempt: AgentWorkspaceRepairAttempt,
) -> AppResult<DurableRepairRecoveryOutcome> {
    if state
        .agent_workspace_repair_repo
        .get_open_repair_effect(&attempt.id)
        .await?
        .is_some()
    {
        return Ok(DurableRepairRecoveryOutcome::Noop);
    }
    let target_identity = match validate_agent_workspace_repair_target_lease(
        state.branch_update_repo.as_ref(),
        &attempt,
    )
    .await
    {
        Ok(identity) => identity,
        Err(AppError::Conflict(_)) => return Ok(DurableRepairRecoveryOutcome::Stale),
        Err(error) => return Err(error),
    };
    let owner = GitTargetLeaseOwner::agent_workspace_repair(attempt.id.as_str());
    if state
        .branch_update_repo
        .list_in_flight_mutations()
        .await?
        .into_iter()
        .any(|claim| {
            claim.identity == target_identity
                && claim.owner == owner
                && claim.fencing_epoch == attempt.target_lease_epoch.unwrap_or_default()
        })
    {
        return Ok(DurableRepairRecoveryOutcome::Noop);
    }
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&attempt.conversation_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "workspace {} for retry delivery",
                attempt.conversation_id
            ))
        })?;
    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await?
        .ok_or_else(|| AppError::ProjectNotFound(workspace.project_id.to_string()))?;
    let resolved = resolve_effective_agent_conversation_workspace_path(
        &project,
        &workspace,
        state.plan_branch_repo.as_ref(),
    )
    .await?;
    let run_id = AgentRunId::new();
    let reserved = match reserve_agent_workspace_repair_dispatch(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        target_identity,
        attempt,
        run_id.clone(),
        "Retrying the durable workspace repair delivery.",
        workspace.pr_auto_merge_current,
    )
    .await?
    {
        AgentWorkspaceRepairDispatchOutcome::Reserved(attempt) => attempt,
        AgentWorkspaceRepairDispatchOutcome::Stale(_)
        | AgentWorkspaceRepairDispatchOutcome::Missing => {
            return Ok(DurableRepairRecoveryOutcome::Stale);
        }
    };
    let service = state.build_chat_service();
    let delivery = service
        .send_message(
            ChatContextType::Project,
            workspace.project_id.as_str(),
            &due_repair_dispatch_message(&reserved, &workspace),
            SendMessageOptions {
                preallocated_agent_run_id: Some(run_id.clone()),
                queue_policy: SendQueuePolicy::RequireImmediateStart,
                conversation_id_override: Some(workspace.conversation_id.clone()),
                agent_name_override: Some(AGENT_WORKSPACE_REPAIR.to_string()),
                working_directory_override: Some(resolved.path),
                force_new_provider_session: true,
                preserve_conversation_provider_session_ref: true,
                ..Default::default()
            },
        )
        .await;
    let settlement = classify_agent_workspace_repair_delivery(
        delivery.as_ref(),
        &workspace.conversation_id,
        &run_id,
    );
    match settle_agent_workspace_repair_dispatch_outcome(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        reserved,
        settlement,
        "Durable workspace repair delivery retry completed.",
        workspace.pr_auto_merge_current,
    )
    .await?
    {
        AgentWorkspaceRepairTransitionOutcome::Applied(attempt) => {
            if attempt.phase == AgentWorkspaceRepairPhase::Blocked {
                release_repair_lease_if_settled_boundary(state, &attempt).await?;
                Ok(DurableRepairRecoveryOutcome::Blocked)
            } else {
                Ok(DurableRepairRecoveryOutcome::Continued)
            }
        }
        AgentWorkspaceRepairTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairTransitionOutcome::Missing => Ok(DurableRepairRecoveryOutcome::Stale),
    }
}

/// Replays only the completion-owned half of an exact interrupted repair. Reserving `Validating`
/// before every Git read fences duplicate startup/terminal recovery and reuses the normal review
/// and publish continuation rather than redispatching a repair agent.
async fn recover_clean_interrupted_repair(
    state: &AppState,
    attempt: AgentWorkspaceRepairAttempt,
) -> AppResult<DurableRepairRecoveryOutcome> {
    let Some(workspace) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&attempt.conversation_id)
        .await?
    else {
        return block_recovery_attempt(
            state,
            attempt,
            "Workspace repair recovery cannot find its canonical workspace. Start a new repair attempt before retrying.",
        )
        .await;
    };
    let reserved = match reserve_agent_workspace_repair_completion_validation(
        Arc::clone(&state.agent_workspace_repair_repo),
        attempt,
        workspace.pr_auto_merge_current,
    )
    .await?
    {
        AgentWorkspaceRepairTransitionOutcome::Applied(attempt) => attempt,
        AgentWorkspaceRepairTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairTransitionOutcome::Missing => {
            return Ok(DurableRepairRecoveryOutcome::Stale);
        }
    };
    if let Err(error) =
        validate_agent_workspace_repair_target_lease(state.branch_update_repo.as_ref(), &reserved)
            .await
    {
        return block_recovery_attempt(
            state,
            reserved,
            &format!(
                "Workspace repair recovery lost canonical Git target authority before validation: {error}"
            ),
        )
        .await;
    }
    let Some(target_base_commit) = reserved
        .target_base_commit
        .as_deref()
        .filter(|commit| !commit.trim().is_empty())
    else {
        return block_recovery_attempt(
            state,
            reserved,
            "Workspace repair recovery has no exact durable target base commit. Start a new repair attempt before retrying.",
        )
        .await;
    };
    let validation = match inspect_agent_workspace_repair_completion(
        state,
        &workspace,
        &reserved.target_base_ref,
        Some(target_base_commit),
    )
    .await
    {
        Ok(validation) => validation,
        Err(error) => {
            return block_recovery_attempt(
                state,
                reserved,
                &format!(
                    "Workspace repair recovery could not prove a clean committed repair: {error}"
                ),
            )
            .await;
        }
    };
    if reserved
        .repair_head_commit
        .as_deref()
        .is_some_and(|head| head != validation.repair_head_commit)
    {
        return block_recovery_attempt(
            state,
            reserved,
            "Workspace repair recovery found a repair head that disagrees with its durable generation. Start a new repair attempt before retrying.",
        )
        .await;
    }
    let conversation_id = reserved.conversation_id.clone();
    let validated = match record_agent_workspace_repair_validation(
        Arc::clone(&state.agent_workspace_repair_repo),
        reserved,
        &validation.base_ref,
        &validation.base_commit,
        &validation.repair_head_commit,
        "Recovered a clean committed workspace repair after its owning run stopped.",
        validation.auto_merge_current,
    )
    .await?
    {
        AgentWorkspaceRepairTransitionOutcome::Applied(attempt) => attempt,
        AgentWorkspaceRepairTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairTransitionOutcome::Missing => {
            return Ok(DurableRepairRecoveryOutcome::Stale);
        }
    };
    let continuation = match continue_agent_workspace_repair_at_boundary(
        state,
        validated,
        AgentWorkspaceRepairPhase::Validating,
        "Continuing the durable workspace repair after recovery validation.",
        false,
    )
    .await?
    {
        AgentWorkspaceRepairTransitionOutcome::Applied(attempt) => attempt,
        AgentWorkspaceRepairTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairTransitionOutcome::Missing => {
            return Ok(DurableRepairRecoveryOutcome::Stale);
        }
    };
    if continuation.phase == AgentWorkspaceRepairPhase::Blocked {
        return Ok(DurableRepairRecoveryOutcome::Blocked);
    }
    match crate::application::publish_resilience::continue_agent_workspace_repair_publish(
        state,
        continuation.clone(),
    )
    .await
    {
        Ok(Some(crate::application::publish_resilience::AgentWorkspaceRepairPushOutcome::Busy)) => {
            Ok(DurableRepairRecoveryOutcome::Active)
        }
        Ok(Some(
            crate::application::publish_resilience::AgentWorkspaceRepairPushOutcome::Stale,
        )) => Ok(DurableRepairRecoveryOutcome::Stale),
        Ok(Some(_)) => Ok(DurableRepairRecoveryOutcome::Continued),
        Ok(None) => {
            let error = AppError::Conflict(
                "workspace repair continuation could not prove a publish runtime".to_string(),
            );
            record_continuation_recovery_failure(state, continuation, &error).await
        }
        Err(error) => {
            tracing::warn!(
                conversation_id = conversation_id.as_str(),
                error = %error,
                "Clean workspace repair recovery left its durable continuation pending"
            );
            record_continuation_recovery_failure(state, continuation, &error).await
        }
    }
}

/// A failed continuation can have crossed an external-effect boundary before the caller sees its
/// error. Re-read the exact durable generation: a persisted blocker is authoritative, while a
/// current pending/continuing generation keeps its effect receipt and target lease fenced for
/// postcondition reconciliation. Never convert either state into a false `Continued` outcome.
async fn record_continuation_recovery_failure(
    state: &AppState,
    failed_attempt: AgentWorkspaceRepairAttempt,
    error: &AppError,
) -> AppResult<DurableRepairRecoveryOutcome> {
    let Some(current) = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&failed_attempt.conversation_id)
        .await?
    else {
        return Ok(DurableRepairRecoveryOutcome::Stale);
    };
    if current.id != failed_attempt.id || current.generation != failed_attempt.generation {
        return Ok(DurableRepairRecoveryOutcome::Stale);
    }
    if current.phase == AgentWorkspaceRepairPhase::Blocked {
        release_repair_lease_if_settled_boundary(state, &current).await?;
        return Ok(DurableRepairRecoveryOutcome::Blocked);
    }
    if !matches!(
        current.phase,
        AgentWorkspaceRepairPhase::ContinuationPending | AgentWorkspaceRepairPhase::Continuing
    ) {
        return Ok(DurableRepairRecoveryOutcome::Stale);
    }

    let summary = format!(
        "Workspace repair continuation is pending reconciliation after recovery error: {error}"
    );
    match transition_agent_workspace_repair_attempt(
        Arc::clone(&state.agent_workspace_repair_repo),
        current.clone(),
        current.phase,
        &summary,
        None,
    )
    .await?
    {
        AgentWorkspaceRepairTransitionOutcome::Applied(_) => {
            Ok(DurableRepairRecoveryOutcome::Active)
        }
        AgentWorkspaceRepairTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairTransitionOutcome::Missing => Ok(DurableRepairRecoveryOutcome::Stale),
    }
}

async fn block_recovery_attempt(
    state: &AppState,
    attempt: AgentWorkspaceRepairAttempt,
    blocker: &str,
) -> AppResult<DurableRepairRecoveryOutcome> {
    let auto_merge_current = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&attempt.conversation_id)
        .await?
        .map(|workspace| workspace.pr_auto_merge_current);
    match block_agent_workspace_repair_completion(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        attempt,
        "Workspace repair recovery is blocked.",
        blocker,
        auto_merge_current.flatten(),
    )
    .await?
    {
        AgentWorkspaceRepairTransitionOutcome::Applied(attempt) => {
            release_repair_lease_if_settled_boundary(state, &attempt).await?;
            Ok(DurableRepairRecoveryOutcome::Blocked)
        }
        AgentWorkspaceRepairTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairTransitionOutcome::Missing => Ok(DurableRepairRecoveryOutcome::Stale),
    }
}

/// A phase parked at Ready or Blocked has no recoverable external effect. Release only when the
/// durable effect table confirms that invariant; an open receipt keeps the exact lease fenced.
async fn release_repair_lease_if_settled_boundary(
    state: &AppState,
    attempt: &AgentWorkspaceRepairAttempt,
) -> AppResult<()> {
    if state
        .agent_workspace_repair_repo
        .get_open_repair_effect(&attempt.id)
        .await?
        .is_some()
    {
        return Ok(());
    }
    let _ = release_and_clear_agent_workspace_repair_target_lease(
        state.agent_workspace_repair_repo.as_ref(),
        state.branch_update_repo.as_ref(),
        attempt.clone(),
    )
    .await?;
    Ok(())
}

fn is_legacy_repair_projection(workspace: &AgentConversationWorkspace) -> bool {
    workspace.publication_push_status.as_deref() == Some("needs_agent")
        && matches!(
            workspace.pr_supervision_status.as_deref(),
            Some("fixing") | Some("blocked")
        )
}

async fn import_or_block_legacy_repair_attempt(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> AppResult<DurableRepairRecoveryOutcome> {
    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await?;
    let exact =
        exact_legacy_repair_import(state.agent_run_repo.as_ref(), workspace, &events).await?;
    match exact {
        Some(mut attempt) => {
            let summary = "Imported one exact legacy workspace repair provenance into the durable attempt workflow.";
            let blocked = attempt.phase == AgentWorkspaceRepairPhase::Blocked;
            attempt.updated_at = Utc::now();
            let projection = legacy_projection(&attempt, summary);
            match state
                .agent_workspace_repair_repo
                .import_legacy_repair_attempt(ImportLegacyAgentWorkspaceRepairAttempt {
                    attempt,
                    compatibility_projection: Some(projection),
                    events: vec![AgentConversationWorkspacePublicationEvent::new(
                        workspace.conversation_id.clone(),
                        LEGACY_REPAIR_IMPORTED_STEP,
                        "succeeded",
                        summary,
                        Some(LEGACY_REPAIR_IMPORTED_CLASSIFICATION.to_string()),
                    )],
                })
                .await?
            {
                ImportLegacyAgentWorkspaceRepairAttemptOutcome::Imported(_) => Ok(if blocked {
                    DurableRepairRecoveryOutcome::Blocked
                } else {
                    DurableRepairRecoveryOutcome::Active
                }),
                // A concurrent start/import won the transaction. It owns projection, events,
                // and continuation; legacy recovery only joins that durable authority.
                ImportLegacyAgentWorkspaceRepairAttemptOutcome::ExistingDurable(attempt) => {
                    if attempt.settled_at.is_some() {
                        Ok(DurableRepairRecoveryOutcome::Noop)
                    } else {
                        reconcile_agent_workspace_repair_attempt(state, attempt).await
                    }
                }
            }
        }
        None => block_ambiguous_legacy_repair_attempt(state, workspace).await,
    }
}

async fn exact_legacy_repair_import(
    agent_runs: &dyn AgentRunRepository,
    workspace: &AgentConversationWorkspace,
    events: &[AgentConversationWorkspacePublicationEvent],
) -> AppResult<Option<AgentWorkspaceRepairAttempt>> {
    let run_ids = events
        .iter()
        .filter(|event| event.step == "repair_sent")
        .filter_map(|event| event.classification.as_deref())
        .filter_map(|classification| {
            classification.strip_prefix(LEGACY_REPAIR_RUN_CLASSIFICATION_PREFIX)
        })
        .filter_map(|run_id| run_id.parse::<AgentRunId>().ok())
        .collect::<Vec<_>>();
    if run_ids.len() != 1 {
        return Ok(None);
    }
    let requested = events
        .iter()
        .filter(|event| event.step == "repair_requested")
        .filter_map(|event| event.classification.as_deref())
        .collect::<Vec<_>>();
    let continuation = if requested.len() == 1 && requested[0] == "agent_fixable:update_only" {
        AgentWorkspaceRepairContinuation::UpdateOnly
    } else if requested.len() == 1 && requested[0] == "agent_fixable:publish" {
        AgentWorkspaceRepairContinuation::Publish
    } else {
        return Ok(None);
    };
    let Some(base_commit) = workspace
        .base_commit
        .clone()
        .filter(|base| !base.trim().is_empty())
    else {
        return Ok(None);
    };
    let run_id = run_ids.into_iter().next().expect("one legacy run id");
    let Some(run) = agent_runs.get_by_id(&run_id).await? else {
        return Ok(None);
    };
    if run.conversation_id != workspace.conversation_id
        || (!run.status.is_active() && !run.status.is_terminal())
    {
        return Ok(None);
    }
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        workspace.conversation_id.clone(),
        AgentWorkspaceRepairSource::Legacy,
        continuation,
        workspace.base_ref.clone(),
        false,
        workspace.auto_publish_enabled,
        workspace.pr_auto_merge_desired,
        Some(workspace.pr_auto_merge_method.clone()),
        Utc::now(),
    );
    attempt.id = AgentWorkspaceRepairAttemptId::from_string(run_id.as_str());
    attempt.generation = 1;
    attempt.reserved_agent_run_id = Some(run_id);
    attempt.target_base_commit = Some(base_commit);
    attempt.phase = if run.status.is_active() {
        AgentWorkspaceRepairPhase::Repairing
    } else {
        AgentWorkspaceRepairPhase::Blocked
    };
    if attempt.phase == AgentWorkspaceRepairPhase::Blocked {
        attempt.blocker = Some(
            "The exact legacy repair run ended without a durable completion receipt. Retry the repair."
                .to_string(),
        );
    }
    Ok(Some(attempt))
}

async fn block_ambiguous_legacy_repair_attempt(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> AppResult<DurableRepairRecoveryOutcome> {
    let now = Utc::now();
    let attempt = AgentWorkspaceRepairAttempt::new(
        workspace.conversation_id.clone(),
        AgentWorkspaceRepairSource::Legacy,
        AgentWorkspaceRepairContinuation::Manual,
        workspace.base_ref.clone(),
        false,
        workspace.auto_publish_enabled,
        workspace.pr_auto_merge_desired,
        Some(workspace.pr_auto_merge_method.clone()),
        now,
    );
    let projection = legacy_projection(
        &attempt,
        "Legacy repair provenance is incomplete or ambiguous.",
    );
    let started = state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(crate::domain::repositories::StartOrJoinAgentWorkspaceRepairAttempt {
            attempt,
            reason: "legacy_repair_import_ambiguous".to_string(),
            verified_newer_base: false,
            compatibility_projection: Some(projection),
            events: vec![AgentConversationWorkspacePublicationEvent::new(
                workspace.conversation_id.clone(),
                LEGACY_REPAIR_IMPORT_BLOCKED_STEP,
                "blocked",
                "Legacy repair provenance is incomplete or ambiguous; RalphX did not guess a repair owner.",
                Some(LEGACY_REPAIR_IMPORT_BLOCKED_CLASSIFICATION.to_string()),
            )],
        })
        .await?;
    let attempt = match started {
        crate::domain::repositories::StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(attempt)
        | crate::domain::repositories::StartOrJoinAgentWorkspaceRepairAttemptOutcome::Joined(attempt)
        | crate::domain::repositories::StartOrJoinAgentWorkspaceRepairAttemptOutcome::BlockedByCurrent(attempt) => attempt,
    };
    block_recovery_attempt(
        state,
        attempt,
        "Legacy repair provenance is incomplete or ambiguous. Start a fresh repair from the blocked operation.",
    )
    .await
}

fn legacy_projection(
    attempt: &AgentWorkspaceRepairAttempt,
    summary: &str,
) -> AgentWorkspaceRepairCompatibilityProjection {
    let (push_status, supervision_status) = match attempt.phase {
        AgentWorkspaceRepairPhase::Blocked => ("failed", "blocked"),
        AgentWorkspaceRepairPhase::AwaitingReview => ("refreshed", "reviewing"),
        AgentWorkspaceRepairPhase::ContinuationPending | AgentWorkspaceRepairPhase::Continuing => {
            ("refreshed", "publishing")
        }
        AgentWorkspaceRepairPhase::Ready => ("refreshed", "paused"),
        AgentWorkspaceRepairPhase::Requested
        | AgentWorkspaceRepairPhase::Dispatching
        | AgentWorkspaceRepairPhase::Repairing
        | AgentWorkspaceRepairPhase::Validating => ("needs_agent", "fixing"),
    };
    AgentWorkspaceRepairCompatibilityProjection {
        publication_push_status: Some(push_status.to_string()),
        pr_supervision_status: Some(supervision_status.to_string()),
        pr_supervision_summary: Some(summary.to_string()),
        pr_supervision_updated_at: Some(attempt.updated_at),
        pr_auto_merge_current: None,
        base_commit: attempt.target_base_commit.clone(),
    }
}
