use std::sync::Arc;
use std::time::Instant;

use chrono::{Duration, Utc};

use super::base_advance_retarget::retarget_reserved_repair_to_advanced_base;
use super::pr_autofix_redelivery::{
    due_pr_autofix_redispatch_message, evaluate_pr_autofix_successor, pr_autofix_fingerprint_spend,
    remember_blocked_pr_autofix_fingerprint, PrAutofixFingerprintSpend, PrAutofixSuccessorDecision,
};
use super::StalePublishRepairRecoveryOutcome;
use crate::application::agent_conversation_workspace::resolve_effective_agent_conversation_workspace_path;
use crate::application::agent_workspace_pr_autofix_attempt::load_latest_exact_pr_autofix_run_for_pr;
use crate::application::agent_workspace_publish_repair_state::{
    agent_workspace_repair_dispatch_is_due, agent_workspace_repair_hold_reason,
    block_agent_workspace_repair_completion, block_agent_workspace_repair_needs_human,
    classify_agent_workspace_repair_delivery, continue_agent_workspace_repair_at_boundary,
    inspect_agent_workspace_repair_completion_classified, last_human_repair_reason,
    reacquire_agent_workspace_repair_target_lease_for_continuation,
    record_agent_workspace_repair_validation,
    release_and_clear_agent_workspace_repair_target_lease,
    reserve_agent_workspace_repair_completion_validation, reserve_agent_workspace_repair_dispatch,
    reserve_agent_workspace_unchanged_health_hold, resume_current_agent_workspace_repair_publish,
    settle_agent_workspace_repair_dispatch_outcome, start_or_join_agent_workspace_repair,
    transition_agent_workspace_repair_attempt, validate_agent_workspace_repair_target_lease,
    AgentWorkspaceRepairCompletionInspection, AgentWorkspaceRepairDispatchOutcome,
    AgentWorkspaceRepairDispatchSettlement, AgentWorkspaceRepairPublishResumeOutcome,
    AgentWorkspaceRepairStartOutcome, AgentWorkspaceRepairStartRequest,
    AgentWorkspaceRepairTransitionOutcome, PublishAuthority,
    ORPHANED_REPAIR_DISPATCH_RESCUE_GRACE_SECS,
};
use crate::application::chat_service::{ChatService, SendMessageOptions, SendQueuePolicy};
use crate::application::publish_resilience::{
    reconcile_blocked_agent_workspace_repair_create_pr_effect,
    reconcile_blocked_agent_workspace_repair_pr_handoff,
    terminate_orphaned_blocked_repair_pr_handoff_effect,
    terminate_orphaned_blocked_repair_push_effect, BlockedCreatePrEffectReconciliation,
    BlockedRepairPrHandoffReconciliation,
};
use crate::application::{AppState, GitService};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspacePublicationEvent, AgentRunId,
    AgentRunStatus, AgentWorkspaceRepairAttempt, AgentWorkspaceRepairAttemptId,
    AgentWorkspaceRepairContinuation, AgentWorkspaceRepairOperationHoldReason,
    AgentWorkspaceRepairPhase, AgentWorkspaceRepairSource, ChatContextType, GitTargetLeaseOwner,
};
use crate::domain::entities::{
    NewNotification, NotificationCategory, NotificationSeverity, NotificationTarget,
    NotificationTargetKind,
};
use crate::domain::repositories::{
    AgentRunRepository, AgentWorkspaceRepairAttemptTransition,
    AgentWorkspaceRepairAttemptTransitionOutcome, AgentWorkspaceRepairCompatibilityProjection,
    ImportLegacyAgentWorkspaceRepairAttempt, ImportLegacyAgentWorkspaceRepairAttemptOutcome,
    SettleAgentWorkspaceRepairAttempt, SettleAgentWorkspaceRepairAttemptOutcome,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::agent_names::AGENT_WORKSPACE_REPAIR;

/// Publication step recorded when unattended repair stops because its budget is spent.
const REPAIR_BUDGET_EXHAUSTED_STEP: &str = "repair_budget_exhausted";
/// Recorded every time a due auto-retry dispatch actually executes, deliberately with no
/// read-before-append dedupe: each execution is a distinct observable event, so a retry that
/// fires three times over an hour must produce three entries in the publish panel event log.
const REPAIR_AUTO_RETRY_DISPATCHED_STEP: &str = "repair_auto_retry_dispatched";
const REPAIR_PUBLISH_REDRIVE_STEP: &str = "repair_publish_redrive";
pub(crate) const CONTINUATION_RECOVERY_BLOCKED_STEP: &str = "continuation_recovery_blocked";
const CONTINUATION_OPEN_EFFECT_ATTENTION_STEP: &str = "continuation_open_effect_attention_required";
const CONTINUATION_EFFECT_NOT_APPLIED_STEP: &str = "continuation_effect_not_applied";
const LEGACY_REPAIR_IMPORT_BLOCKED_STEP: &str = "legacy_repair_import_blocked";
const LEGACY_REPAIR_IMPORT_BLOCKED_CLASSIFICATION: &str = "legacy_repair_import_ambiguous";
const LEGACY_REPAIR_IMPORTED_STEP: &str = "legacy_repair_imported";
const LEGACY_REPAIR_IMPORTED_CLASSIFICATION: &str = "legacy_repair_import_exact";
const LEGACY_REPAIR_RUN_CLASSIFICATION_PREFIX: &str = "agent_fixable:run:";
pub(crate) const AUTO_RETRY_BLOCKED_REPAIR_REASON_PREFIX: &str = "auto_retry_blocked_repair:";
const AUTO_RETRY_BLOCKED_REPAIR_BASE_DELAY_SECS: i64 = 60;
const AUTO_RETRY_BLOCKED_REPAIR_MAX_DELAY_SECS: i64 = 15 * 60;
pub(crate) const MAX_AUTO_RETRY_BLOCKED_REPAIR_STREAK: u32 = 3;
pub(crate) const AUTO_RETRY_READY_REPAIR_REASON_PREFIX: &str = "auto_retry_ready_repair:";
const AUTO_RETRY_READY_REPAIR_BASE_DELAY_SECS: i64 = 60;
const AUTO_RETRY_READY_REPAIR_MAX_DELAY_SECS: i64 = 15 * 60;
const MAX_AUTO_RETRY_READY_REPAIR_STREAK: u32 = 3;
pub(crate) const EXHAUSTED_PUBLISH_REDRIVE_CHECKED_REASON_PREFIX: &str =
    "exhausted_publish_redrive_checked:";
pub(crate) const PR_AUTOFIX_HEAD_REDRIVE_REASON_PREFIX: &str = "pr_autofix_head_redrive:";
const PR_AUTOFIX_HEAD_REDRIVE_RETRY_REASON_PREFIX: &str = "pr_autofix_head_redrive_retry:";
const CONTINUATION_RECOVERY_FAILURE_REASON_PREFIX: &str = "continuation_recovery_failure:";
pub(crate) const CONTINUATION_OPEN_EFFECT_RECOVERY_REASON_PREFIX: &str =
    "continuation_open_effect_recovery:";
pub(crate) const CONTINUATION_OPEN_EFFECT_ATTENTION_REASON: &str =
    "continuation_open_effect_attention_required";
/// Written by the poller when it observes changed PR evidence against an escalated open-effect
/// continuation. The exact identity suffix is the loop guard: an unchanged identity must never
/// re-arm twice.
pub(crate) const CONTINUATION_OPEN_EFFECT_EVIDENCE_REASON_PREFIX: &str =
    "continuation_open_effect_evidence:";
pub(crate) const CONTINUATION_OPEN_EFFECT_REARMED_STEP: &str = "continuation_open_effect_rearmed";
/// Guards a blocked PR autofix streak re-arm to at most once per distinct failure identity.
pub(crate) const BLOCKED_STREAK_REARMED_REASON_PREFIX: &str = "blocked_streak_rearmed:";
const MAX_CONTINUATION_RECOVERY_FAILURE_STREAK: u32 = 3;

/// Only a marker written after a current health check proves that this repair owns the
/// unpublished-head continuation. A repair head by itself is not enough: it might already have
/// reached the remote, in which case the fingerprint hold remains authoritative.
pub(crate) fn agent_workspace_repair_owns_unpublished_publish_continuation(
    attempt: &AgentWorkspaceRepairAttempt,
) -> bool {
    let Some(head) = attempt.unpublished_local_head() else {
        return false;
    };
    let marker = format!("{PR_AUTOFIX_HEAD_REDRIVE_REASON_PREFIX}{head}");
    attempt
        .pending_reasons
        .iter()
        .any(|reason| reason == &marker)
}

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
            let durable_generation_exists = state
                .agent_workspace_repair_repo
                .get_latest_repair_attempt_for_conversation(&workspace.conversation_id)
                .await?
                .is_some();
            if durable_generation_exists {
                DurableRepairRecoveryOutcome::Noop
            } else {
                #[cfg(any(test, feature = "test-utils"))]
                if is_legacy_pr_fix_review_projection(&workspace) {
                    return super::recover_stale_publish_repair_for_workspace_with_project_repo_outcome(
                        Arc::clone(&state.agent_conversation_workspace_repo),
                        Arc::clone(&state.agent_run_repo),
                        Arc::clone(&state.project_repo),
                        workspace,
                    )
                    .await;
                }
                if is_legacy_repair_projection(&workspace) {
                    if active_exact_pr_autofix_owns_legacy_projection(state, &workspace).await? {
                        DurableRepairRecoveryOutcome::Noop
                    } else {
                        import_or_block_legacy_repair_attempt(state, &workspace).await?
                    }
                } else {
                    DurableRepairRecoveryOutcome::Noop
                }
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

async fn active_exact_pr_autofix_owns_legacy_projection(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> AppResult<bool> {
    let Some(pr_number) = workspace.publication_pr_number else {
        return Ok(false);
    };
    Ok(load_latest_exact_pr_autofix_run_for_pr(
        state.agent_run_repo.as_ref(),
        &workspace.conversation_id,
        pr_number,
    )
    .await?
    .is_some_and(|run| run.status == AgentRunStatus::Running))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DurableRepairRecoveryOutcome {
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
            match reserved_dispatch_liveness(state, &current).await? {
                ReservedDispatchLiveness::Running => Ok(DurableRepairRecoveryOutcome::Active),
                // The reserved worker ran and ended without completing the repair. That is a real
                // interrupted delivery and settles immediately, exactly as before.
                ReservedDispatchLiveness::Ended => {
                    schedule_interrupted_dispatch_retry(state, current).await
                }
                // The reservation is written before the run row exists, so "no run yet" is the
                // normal state for the first moments of a dispatch. Treating it as interruption
                // raced a live spawn by milliseconds in production and queued a duplicate
                // delivery against the same attempt.
                ReservedDispatchLiveness::Unobserved
                    if Utc::now() - current.updated_at
                        < Duration::seconds(ORPHANED_REPAIR_DISPATCH_RESCUE_GRACE_SECS) =>
                {
                    Ok(DurableRepairRecoveryOutcome::Noop)
                }
                ReservedDispatchLiveness::Unobserved => {
                    schedule_interrupted_dispatch_retry(state, current).await
                }
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
                if repair_attempt_has_target_lease(&current) {
                    return redeliver_due_repair_dispatch(state, current).await;
                }
                if Utc::now() - current.updated_at
                    < Duration::seconds(ORPHANED_REPAIR_DISPATCH_RESCUE_GRACE_SECS)
                {
                    return Ok(DurableRepairRecoveryOutcome::Noop);
                }
                return rescue_orphaned_repair_dispatch(state, current).await;
            }
            if !agent_workspace_repair_dispatch_is_due(&current, Utc::now()) {
                return Ok(DurableRepairRecoveryOutcome::Noop);
            }
            let conversation_id = current.conversation_id;
            let outcome = redeliver_due_repair_dispatch(state, current).await?;
            // Only a genuine redelivery (the dispatch was actually reserved and sent) is an
            // auto-retry worth surfacing; `Noop`/`Stale` mean this call lost a race (an open
            // effect, an in-flight mutation, or a stale lease/reservation) and nothing advanced.
            if outcome.was_recovered() {
                append_repair_auto_retry_dispatched_event(state, conversation_id).await;
            }
            Ok(outcome)
        }
        AgentWorkspaceRepairPhase::Validating => {
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
        AgentWorkspaceRepairPhase::ContinuationPending | AgentWorkspaceRepairPhase::Continuing => {
            let continuation = match validate_agent_workspace_repair_target_lease(
                state.branch_update_repo.as_ref(),
                &current,
            )
            .await
            {
                Ok(_) => current.clone(),
                Err(AppError::Conflict(_)) => {
                    let open_effect = match state
                        .agent_workspace_repair_repo
                        .get_open_repair_effect(&current.id)
                        .await
                    {
                        Ok(effect) => effect,
                        Err(error) => {
                            return escalate_or_record_continuation_recovery_failure(
                                state, current, &error,
                            )
                            .await;
                        }
                    };
                    if let Some(effect) = open_effect {
                        match crate::application::publish_resilience::reconcile_open_agent_workspace_repair_push_effect(
                            state,
                            &current,
                            effect,
                        )
                        .await
                        {
                            Ok(crate::application::publish_resilience::AgentWorkspaceRepairOpenPushEffectReconciliation::Observed) => {}
                            Ok(crate::application::publish_resilience::AgentWorkspaceRepairOpenPushEffectReconciliation::NotApplied) => {
                                // The reconciler proved the push never reached the remote and
                                // terminated the effect as Failed, clearing the fence. Return Noop
                                // so the next sweep reacquires the lease without spending an
                                // open-effect recovery credit or re-raising the attention
                                // notification that record_continuation_effect_not_applied just
                                // resolved.
                                record_continuation_effect_not_applied(state, &current).await;
                                return Ok(DurableRepairRecoveryOutcome::Noop);
                            }
                            Ok(crate::application::publish_resilience::AgentWorkspaceRepairOpenPushEffectReconciliation::Pending) => {
                                let error = AppError::Conflict(
                                    "workspace repair continuation lost its canonical target authority while an external effect remains open"
                                        .to_string(),
                                );
                                return escalate_or_record_continuation_recovery_failure(
                                    state, current, &error,
                                )
                                .await;
                            }
                            Err(error) => {
                                return escalate_or_record_continuation_recovery_failure(
                                    state, current, &error,
                                )
                                .await;
                            }
                        }
                    }
                    let workspace = match state
                        .agent_conversation_workspace_repo
                        .get_by_conversation_id(&current.conversation_id)
                        .await
                    {
                        Ok(Some(workspace)) => workspace,
                        Ok(None) => {
                            return block_recovery_attempt(
                                state,
                                current,
                                "Workspace repair continuation recovery cannot find its canonical workspace.",
                            )
                            .await;
                        }
                        Err(error) => {
                            return escalate_or_record_continuation_recovery_failure(
                                state, current, &error,
                            )
                            .await;
                        }
                    };
                    match reacquire_agent_workspace_repair_target_lease_for_continuation(
                        state,
                        &workspace,
                        current.clone(),
                        current.phase,
                    )
                    .await
                    {
                        Ok(AgentWorkspaceRepairTransitionOutcome::Applied(attempt)) => attempt,
                        Ok(AgentWorkspaceRepairTransitionOutcome::Stale(_))
                        | Ok(AgentWorkspaceRepairTransitionOutcome::Missing) => {
                            return Ok(DurableRepairRecoveryOutcome::Stale);
                        }
                        Err(error) => {
                            return escalate_or_record_continuation_recovery_failure(
                                state, current, &error,
                            )
                            .await;
                        }
                    }
                }
                Err(error) => {
                    return escalate_or_record_continuation_recovery_failure(
                        state, current, &error,
                    )
                    .await;
                }
            };
            recover_agent_workspace_repair_continuation(state, continuation, true).await
        }
        AgentWorkspaceRepairPhase::AwaitingReview => {
            match resume_current_agent_workspace_repair_publish(
                state,
                &current.conversation_id,
                "Resuming the durable workspace repair continuation after Workspace Review.",
                false,
                PublishAuthority::VerifiedAutomation,
            )
            .await?
            {
                AgentWorkspaceRepairPublishResumeOutcome::Continue(next) => {
                    Box::pin(reconcile_agent_workspace_repair_attempt(state, *next)).await
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
        AgentWorkspaceRepairPhase::Ready => {
            release_repair_lease_if_settled_boundary(state, &current).await?;
            retry_safe_ready_agent_workspace_repair_publish(state, current).await
        }
        AgentWorkspaceRepairPhase::Blocked => {
            // A failed PR-handoff reconciliation read (gh outage, expired auth, unreadable
            // workspace) must not abort the recovery sweep for other workspaces. Declining
            // here performs no recovery write, so the next sweep re-evaluates the same
            // evidence from scratch.
            match reconcile_blocked_agent_workspace_repair_pr_handoff(state, &current).await {
                Ok(BlockedRepairPrHandoffReconciliation::Recovered) => {
                    return Ok(DurableRepairRecoveryOutcome::Continued);
                }
                Ok(BlockedRepairPrHandoffReconciliation::Stale) => {
                    return Ok(DurableRepairRecoveryOutcome::Stale);
                }
                Ok(BlockedRepairPrHandoffReconciliation::NotRecoverable) => {
                    // The handoff was not a no-op, so the reconciler cannot finish it. If its
                    // durable effect is merely orphaned, clearing that fence here is what lets the
                    // retry evaluation below re-run the real publish in this same pass.
                    if let Err(error) =
                        terminate_orphaned_blocked_repair_pr_handoff_effect(state, &current).await
                    {
                        tracing::warn!(
                            conversation_id = current.conversation_id.as_str(),
                            attempt_id = current.id.as_str(),
                            %error,
                            "Orphaned blocked PR-handoff effect could not be terminated; leaving the attempt unsettled"
                        );
                        return Ok(DurableRepairRecoveryOutcome::Noop);
                    }
                    // At most one of the two hatches can fire: each matches a different effect kind
                    // and an attempt holds at most one open effect. A blocked repair whose branch
                    // push was abandoned in flight is fenced by that effect alone, so clearing it
                    // is what lets the retry evaluation below re-drive the real publish.
                    if let Err(error) =
                        terminate_orphaned_blocked_repair_push_effect(state, &current).await
                    {
                        tracing::warn!(
                            conversation_id = current.conversation_id.as_str(),
                            attempt_id = current.id.as_str(),
                            %error,
                            "Orphaned blocked branch-push effect could not be terminated; leaving the attempt unsettled"
                        );
                        return Ok(DurableRepairRecoveryOutcome::Noop);
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        conversation_id = current.conversation_id.as_str(),
                        attempt_id = current.id.as_str(),
                        %error,
                        "Blocked PR-handoff reconciliation could not be evaluated; leaving the attempt unsettled"
                    );
                    return Ok(DurableRepairRecoveryOutcome::Noop);
                }
            }
            // Both arms above decline every `create_pr` shape by design: no durable receipt can
            // prove whether a PR creation landed. That question is only answerable against GitHub,
            // so evaluate it here. Like the hatch above, a decline performs no write and the next
            // sweep re-evaluates the same evidence. `current` stays valid across this call because
            // the reconciler's terminating arm writes the effect row, not the attempt row.
            match reconcile_blocked_agent_workspace_repair_create_pr_effect(state, &current).await {
                Ok(BlockedCreatePrEffectReconciliation::Adopted) => {
                    return Ok(DurableRepairRecoveryOutcome::Continued);
                }
                Ok(BlockedCreatePrEffectReconciliation::NotApplied) => {
                    // The fence is clear. Fall through to the retry evaluation exactly as the
                    // PR-update hatch does; whether the replay happens in this pass or a later one
                    // is owned by the continuation, streak, and backoff gates below.
                }
                Ok(BlockedCreatePrEffectReconciliation::AmbiguousPrExists)
                | Ok(BlockedCreatePrEffectReconciliation::Pending) => {}
                Err(error) => {
                    // An error here already means no durable write happened, so falling through is
                    // safe: the retry evaluation finds the still-open effect and returns `Noop`.
                    tracing::warn!(
                        conversation_id = current.conversation_id.as_str(),
                        attempt_id = current.id.as_str(),
                        %error,
                        "Blocked create_pr effect reconciliation failed; leaving the attempt fenced"
                    );
                }
            }
            release_repair_lease_if_settled_boundary(state, &current).await?;
            retry_safe_blocked_agent_workspace_repair(state, current).await
        }
    }
}

fn automatic_ready_repair_streak(attempt: &AgentWorkspaceRepairAttempt) -> u32 {
    attempt
        .pending_reasons
        .iter()
        .filter_map(|reason| reason.strip_prefix(AUTO_RETRY_READY_REPAIR_REASON_PREFIX))
        .filter_map(|streak| streak.parse::<u32>().ok())
        .max()
        .unwrap_or_default()
}

fn automatic_held_head_redrive_streak(
    attempt: &AgentWorkspaceRepairAttempt,
    repair_head: &str,
) -> u32 {
    let prefix = format!("{PR_AUTOFIX_HEAD_REDRIVE_RETRY_REASON_PREFIX}{repair_head}:");
    attempt
        .pending_reasons
        .iter()
        .filter_map(|reason| reason.strip_prefix(&prefix))
        .filter_map(|streak| streak.parse::<u32>().ok())
        .max()
        .unwrap_or_else(|| {
            u32::from(agent_workspace_repair_owns_unpublished_publish_continuation(attempt))
        })
}

fn automatic_ready_repair_retry_delay(streak: u32) -> Duration {
    let multiplier = 1_i64 << streak.min(4);
    Duration::seconds(
        AUTO_RETRY_READY_REPAIR_BASE_DELAY_SECS
            .saturating_mul(multiplier)
            .min(AUTO_RETRY_READY_REPAIR_MAX_DELAY_SECS),
    )
}

async fn retry_safe_ready_agent_workspace_repair_publish(
    state: &AppState,
    current: AgentWorkspaceRepairAttempt,
) -> AppResult<DurableRepairRecoveryOutcome> {
    if state
        .agent_workspace_repair_repo
        .get_open_repair_effect(&current.id)
        .await?
        .is_some()
    {
        return Ok(DurableRepairRecoveryOutcome::Noop);
    }
    let (held_head_redrive_authorized, held_head_redrive_marker, held_repair_head) =
        match agent_workspace_repair_hold_reason(&current) {
            Some(
                AgentWorkspaceRepairOperationHoldReason::UnchangedHealth
                | AgentWorkspaceRepairOperationHoldReason::PreExistingOnBase,
            ) => {
                // A health hold normally survives recovery. The one exception is a concrete repair head
                // GitHub has not seen: without publishing it, the health evidence can never change. The
                // successor evaluator resolves the workspace/project/path/GitHub evidence fail-closed.
                let workspace = match state
                    .agent_conversation_workspace_repo
                    .get_by_conversation_id(&current.conversation_id)
                    .await
                {
                    Ok(Some(workspace)) => workspace,
                    Ok(None) => return Ok(DurableRepairRecoveryOutcome::Noop),
                    Err(error) => {
                        tracing::warn!(
                            conversation_id = current.conversation_id.as_str(),
                            attempt_id = current.id.as_str(),
                            %error,
                            "Could not read workspace evidence for held repair publish re-drive"
                        );
                        return Ok(DurableRepairRecoveryOutcome::Noop);
                    }
                };
                if !matches!(
                    evaluate_pr_autofix_successor(state, &current, &workspace).await,
                    PrAutofixSuccessorDecision::RedrivePublish
                ) {
                    return Ok(DurableRepairRecoveryOutcome::Noop);
                }
                let Some(head) = current.unpublished_local_head() else {
                    return Ok(DurableRepairRecoveryOutcome::Noop);
                };
                let marker = format!("{PR_AUTOFIX_HEAD_REDRIVE_REASON_PREFIX}{head}");
                (
                    true,
                    (!agent_workspace_repair_owns_unpublished_publish_continuation(&current))
                        .then_some(marker),
                    Some(head.to_string()),
                )
            }
            Some(_) => return Ok(DurableRepairRecoveryOutcome::Noop),
            None => (false, None, None),
        };

    let redrive_authorized = match current.continuation {
        AgentWorkspaceRepairContinuation::ResumePrSupervision => true,
        AgentWorkspaceRepairContinuation::Publish => {
            current.explicit_publish_requested
                || state
                    .agent_conversation_workspace_repo
                    .get_by_conversation_id(&current.conversation_id)
                    .await?
                    .is_some_and(|workspace| workspace.auto_publish_enabled)
        }
        AgentWorkspaceRepairContinuation::Manual | AgentWorkspaceRepairContinuation::UpdateOnly => {
            false
        }
    };
    if !redrive_authorized {
        return Ok(DurableRepairRecoveryOutcome::Noop);
    }

    let streak = if let Some(head) = held_repair_head.as_deref() {
        automatic_held_head_redrive_streak(&current, head)
    } else {
        automatic_ready_repair_streak(&current)
    };
    if held_head_redrive_authorized {
        if streak >= MAX_AUTO_RETRY_READY_REPAIR_STREAK {
            // The unchanged-health hold remains authoritative after this head exhausts its
            // publish retries. Settling it would permit a fresh fixer generation on identical
            // evidence. A different repair head gets an independent counter and re-arms here.
            return Ok(DurableRepairRecoveryOutcome::Noop);
        }
        if held_head_redrive_marker.is_none()
            && Utc::now() - current.updated_at < automatic_ready_repair_retry_delay(streak)
        {
            return Ok(DurableRepairRecoveryOutcome::Noop);
        }
    } else {
        if streak >= MAX_AUTO_RETRY_READY_REPAIR_STREAK {
            return settle_exhausted_ready_agent_workspace_repair(state, current).await;
        }
        if Utc::now() - current.updated_at < automatic_ready_repair_retry_delay(streak) {
            return Ok(DurableRepairRecoveryOutcome::Noop);
        }
    }

    let expected_updated_at = current.updated_at;
    let mut marked = current;
    if let Some(marker) = held_head_redrive_marker {
        marked.pending_reasons.push(marker);
    }
    if let Some(head) = held_repair_head.as_deref() {
        marked.pending_reasons.push(format!(
            "{PR_AUTOFIX_HEAD_REDRIVE_RETRY_REASON_PREFIX}{head}:{}",
            streak + 1
        ));
    } else {
        marked.pending_reasons.push(format!(
            "{AUTO_RETRY_READY_REPAIR_REASON_PREFIX}{}",
            streak + 1
        ));
    }
    marked.updated_at = std::cmp::max(Utc::now(), expected_updated_at + Duration::microseconds(1));
    let marked = match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: marked,
            expected_phase: AgentWorkspaceRepairPhase::Ready,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Ready,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await?
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        AgentWorkspaceRepairAttemptTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairAttemptTransitionOutcome::Missing => {
            return Ok(DurableRepairRecoveryOutcome::Stale);
        }
    };

    // A failed re-drive must not abort the whole recovery sweep for other workspaces. Ordinary
    // Ready retries persist a bounded backoff streak. Authorized unpublished-head retries use a
    // per-head counter so an exhausted head stays held while a different repair head re-arms.
    // The authority gate above proves re-drive authority from durable consent, current Auto
    // Publish policy, or supervision of an already-created PR; it does not grant user consent.
    let resumed = match resume_current_agent_workspace_repair_publish(
        state,
        &marked.conversation_id,
        "Resuming a parked durable workspace repair publish continuation.",
        true,
        PublishAuthority::VerifiedAutomation,
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(error) => {
            tracing::warn!(
                conversation_id = marked.conversation_id.as_str(),
                attempt_id = marked.id.as_str(),
                %error,
                "Re-driving a parked ready workspace repair continuation failed; retrying after backoff"
            );
            return Ok(DurableRepairRecoveryOutcome::Noop);
        }
    };
    match resumed {
        AgentWorkspaceRepairPublishResumeOutcome::Continue(next) => {
            Box::pin(reconcile_agent_workspace_repair_attempt(state, *next)).await
        }
        AgentWorkspaceRepairPublishResumeOutcome::AwaitingReview
        | AgentWorkspaceRepairPublishResumeOutcome::Ready
        | AgentWorkspaceRepairPublishResumeOutcome::Blocked
        | AgentWorkspaceRepairPublishResumeOutcome::Busy => Ok(DurableRepairRecoveryOutcome::Noop),
        AgentWorkspaceRepairPublishResumeOutcome::NoAttempt
        | AgentWorkspaceRepairPublishResumeOutcome::Stale => {
            Ok(DurableRepairRecoveryOutcome::Stale)
        }
    }
}

async fn settle_exhausted_ready_agent_workspace_repair(
    state: &AppState,
    current: AgentWorkspaceRepairAttempt,
) -> AppResult<DurableRepairRecoveryOutcome> {
    match state
        .agent_workspace_repair_repo
        .settle_repair_attempt(SettleAgentWorkspaceRepairAttempt {
            attempt_id: current.id,
            generation: current.generation,
            expected_phase: AgentWorkspaceRepairPhase::Ready,
            expected_updated_at: current.updated_at,
            outcome: crate::domain::entities::AgentWorkspaceRepairOutcome::Failed,
            settled_at: Utc::now(),
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await?
    {
        SettleAgentWorkspaceRepairAttemptOutcome::Applied(_) => {
            Ok(DurableRepairRecoveryOutcome::Continued)
        }
        SettleAgentWorkspaceRepairAttemptOutcome::Stale(_)
        | SettleAgentWorkspaceRepairAttemptOutcome::Missing => {
            Ok(DurableRepairRecoveryOutcome::Stale)
        }
    }
}

fn automatic_blocked_repair_streak(attempt: &AgentWorkspaceRepairAttempt) -> u32 {
    attempt
        .pending_reasons
        .iter()
        .filter_map(|reason| reason.strip_prefix(AUTO_RETRY_BLOCKED_REPAIR_REASON_PREFIX))
        .filter_map(|streak| streak.parse::<u32>().ok())
        .max()
        .unwrap_or_default()
}

fn exhausted_publish_redrive_was_checked(
    attempt: &AgentWorkspaceRepairAttempt,
    repair_head: &str,
) -> bool {
    let checked_reason = format!("{EXHAUSTED_PUBLISH_REDRIVE_CHECKED_REASON_PREFIX}{repair_head}");
    attempt
        .pending_reasons
        .iter()
        .any(|reason| reason == &checked_reason)
}

async fn mark_exhausted_publish_redrive_checked(
    state: &AppState,
    current: AgentWorkspaceRepairAttempt,
    repair_head: &str,
) -> AppResult<DurableRepairRecoveryOutcome> {
    let expected_updated_at = current.updated_at;
    let mut marked = current;
    marked.pending_reasons.push(format!(
        "{EXHAUSTED_PUBLISH_REDRIVE_CHECKED_REASON_PREFIX}{repair_head}"
    ));
    marked.updated_at = std::cmp::max(Utc::now(), expected_updated_at + Duration::microseconds(1));
    match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: marked,
            expected_phase: AgentWorkspaceRepairPhase::Blocked,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Blocked,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await?
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(marked) => {
            remember_blocked_pr_autofix_fingerprint(state, &marked).await;
            Ok(DurableRepairRecoveryOutcome::Noop)
        }
        AgentWorkspaceRepairAttemptTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairAttemptTransitionOutcome::Missing => {
            Ok(DurableRepairRecoveryOutcome::Stale)
        }
    }
}

/// Only the current durable generation may suspend unrelated publish work. A blocked repair is
/// terminal for automatic recovery when its delivery budget has been spent, or its automatic
/// blocked-repair successor budget has been spent; queued deliveries retain a next dispatch and
/// therefore deliberately do not match.
pub(crate) fn is_blocked_and_not_auto_retryable(attempt: &AgentWorkspaceRepairAttempt) -> bool {
    attempt.phase == AgentWorkspaceRepairPhase::Blocked
        && attempt.next_dispatch_at.is_none()
        && (attempt.pending_reasons.iter().any(|reason| reason == crate::application::agent_workspace_publish_repair_state::NEEDS_HUMAN_REPAIR_REASON)
            || attempt.dispatch_count >= crate::application::agent_workspace_publish_repair_state::MAX_AGENT_WORKSPACE_REPAIR_DISPATCH_RETRIES
            || (attempt.continuation.is_automatic()
                && automatic_blocked_repair_streak(attempt) >= MAX_AUTO_RETRY_BLOCKED_REPAIR_STREAK))
}

/// A blocked-exhausted attempt fences new base-freshness work unless its local repair already
/// landed on the remote branch. Such a continuation-stage block failed *after* the push, so the
/// worktree itself is fine and holding new base work hostage only strands the workspace. A
/// human hold, a repair-stage block, or an unreadable effect all keep the fence.
pub(crate) async fn blocked_repair_fences_new_base_work(
    state: &AppState,
    attempt: &AgentWorkspaceRepairAttempt,
) -> bool {
    if !is_blocked_and_not_auto_retryable(attempt) {
        return false;
    }
    if attempt.pending_reasons.iter().any(|reason| {
        reason == crate::application::agent_workspace_publish_repair_state::NEEDS_HUMAN_REPAIR_REASON
    }) {
        return true;
    }
    match crate::application::publish_resilience::has_authoritative_observed_agent_workspace_repair_push(
        state, attempt,
    )
    .await
    {
        Ok(observed_push) => !observed_push,
        Err(error) => {
            tracing::warn!(
                target: "ralphx_lib::application::agent_workspace_publish_recovery",
                operation = "blocked_repair_fences_new_base_work",
                conversation_id = %attempt.conversation_id,
                attempt_id = %attempt.id,
                error = %error,
                "Failed to read repair push effect; keeping the base-freshness fence"
            );
            true
        }
    }
}

fn automatic_blocked_repair_retry_delay(streak: u32) -> Duration {
    let multiplier = 1_i64 << streak.min(4);
    Duration::seconds(
        AUTO_RETRY_BLOCKED_REPAIR_BASE_DELAY_SECS
            .saturating_mul(multiplier)
            .min(AUTO_RETRY_BLOCKED_REPAIR_MAX_DELAY_SECS),
    )
}

/// An exhausted blocked-repair streak is otherwise terminal for automatic recovery. New PR
/// evidence is the only thing allowed to lift that: a changed successor fingerprint, still inside
/// the fingerprint's agent-minutes budget, resets the streak markers exactly once per distinct
/// failure identity so the next reconciliation pass takes the normal successor path instead of
/// staying parked forever.
async fn rearm_blocked_pr_autofix_streak(
    state: &AppState,
    current: &AgentWorkspaceRepairAttempt,
) -> AppResult<Option<DurableRepairRecoveryOutcome>> {
    let Some(workspace) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&current.conversation_id)
        .await?
    else {
        return Ok(None);
    };
    let PrAutofixSuccessorDecision::Proceed(Some(carryover)) =
        evaluate_pr_autofix_successor(state, current, &workspace).await
    else {
        return Ok(None);
    };
    let Some(new_fingerprint) = carryover.health_fingerprint.as_deref() else {
        return Ok(None);
    };
    if current.pr_autofix_health_fingerprint.as_deref() == Some(new_fingerprint) {
        return Ok(None);
    }
    let rearm_marker = format!("{BLOCKED_STREAK_REARMED_REASON_PREFIX}{new_fingerprint}");
    if current
        .pending_reasons
        .iter()
        .any(|reason| reason == &rearm_marker)
    {
        return Ok(None);
    }
    if pr_autofix_fingerprint_spend(state, &current.conversation_id, new_fingerprint)
        .await?
        .is_exhausted()
    {
        return Ok(None);
    }

    let expected_updated_at = current.updated_at;
    let mut marked = current.clone();
    marked
        .pending_reasons
        .retain(|reason| !reason.starts_with(AUTO_RETRY_BLOCKED_REPAIR_REASON_PREFIX));
    marked.pending_reasons.push(rearm_marker);
    marked.updated_at = std::cmp::max(Utc::now(), expected_updated_at + Duration::microseconds(1));
    match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: marked,
            expected_phase: AgentWorkspaceRepairPhase::Blocked,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Blocked,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await?
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_) => {
            Ok(Some(DurableRepairRecoveryOutcome::Noop))
        }
        AgentWorkspaceRepairAttemptTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairAttemptTransitionOutcome::Missing => Ok(None),
    }
}

async fn retry_safe_blocked_agent_workspace_repair(
    state: &AppState,
    current: AgentWorkspaceRepairAttempt,
) -> AppResult<DurableRepairRecoveryOutcome> {
    if current
        .pending_reasons
        .iter()
        .any(|reason| reason == crate::application::agent_workspace_publish_repair_state::NEEDS_HUMAN_REPAIR_REASON)
    {
        return Ok(DurableRepairRecoveryOutcome::Noop);
    }
    if !current.continuation.is_automatic() {
        return Ok(DurableRepairRecoveryOutcome::Noop);
    }
    if state
        .agent_workspace_repair_repo
        .get_open_repair_effect(&current.id)
        .await?
        .is_some()
    {
        return Ok(DurableRepairRecoveryOutcome::Noop);
    }
    // The retry ladder is 60s → 120s → 240s with a cap of three, so all three successors land
    // inside the same hourly GraphQL window. Retrying against a still-exhausted limit spends the
    // whole budget and terminalizes the attempt for a cause that resolves itself. Defer instead:
    // the streak marker is only written when a successor is actually dispatched, so waiting here
    // costs nothing and the cap plus every other guard stays untouched.
    //
    // The signal is the structured shared `RateLimitState`, never the blocker text — see the
    // doctrine below on why free-form agent prose is not evidence.
    if let Some((remaining, reset_at)) = state.pr_poller_registry.rate_limit_snapshot() {
        if remaining == 0 && reset_at > Instant::now() {
            tracing::info!(
                conversation_id = current.conversation_id.as_str(),
                attempt_id = current.id.as_str(),
                "Deferring blocked workspace repair retry until the GitHub rate limit resets"
            );
            return Ok(DurableRepairRecoveryOutcome::Noop);
        }
    }
    let streak = automatic_blocked_repair_streak(&current);
    if streak >= MAX_AUTO_RETRY_BLOCKED_REPAIR_STREAK {
        // A completed local repair whose head is still absent from the PR is a publish-only gap.
        // It may not take the ordinary exhausted-successor path: that would preserve the
        // publish-vs-health hold livelock. Evaluate only the durable-head shape here so the
        // pre-existing cap behavior remains unchanged for ordinary successors and holds.
        if current.source == AgentWorkspaceRepairSource::PrAutofix {
            let repair_head = current.unpublished_local_head().map(str::to_string);
            if let Some(repair_head) =
                repair_head.filter(|head| !exhausted_publish_redrive_was_checked(&current, head))
            {
                match state
                    .agent_conversation_workspace_repo
                    .get_by_conversation_id(&current.conversation_id)
                    .await
                {
                    Ok(Some(workspace))
                        if matches!(
                            evaluate_pr_autofix_successor(state, &current, &workspace).await,
                            PrAutofixSuccessorDecision::RedrivePublish
                        ) =>
                    {
                        return redrive_blocked_repair_publish(state, current, &workspace).await;
                    }
                    Ok(_) => {
                        return mark_exhausted_publish_redrive_checked(
                            state,
                            current,
                            &repair_head,
                        )
                        .await;
                    }
                    Err(error) => {
                        tracing::warn!(
                            conversation_id = current.conversation_id.as_str(),
                            attempt_id = current.id.as_str(),
                            %error,
                            "Could not evaluate whether an exhausted PR autofix repair needs a publish re-drive"
                        );
                    }
                }
            }
        }
        // A repair head still awaiting publication is a bounded, already-owned gap: the redrive
        // check above owns it for exactly one GitHub read per head. Re-arming here as well would
        // add a second, unbounded health read for the same generation on every later pass.
        // Must stay keyed on the same accessor as the redrive check above, or an attempt carrying
        // only base-update evidence would both skip the redrive and re-arm the streak.
        let has_unpublished_repair_head = current.unpublished_local_head().is_some();
        if current.source == AgentWorkspaceRepairSource::PrAutofix && !has_unpublished_repair_head {
            if let Some(outcome) = rearm_blocked_pr_autofix_streak(state, &current).await? {
                return Ok(outcome);
            }
        }
        // This streak is finished. Repair attempts are per-streak, so unless the workspace itself
        // remembers what this streak died against, the next poll starts a brand new streak with
        // no memory and re-spends the same agents on the same failing check.
        remember_blocked_pr_autofix_fingerprint(state, &current).await;
        return Ok(DurableRepairRecoveryOutcome::Noop);
    }
    if Utc::now() - current.updated_at < automatic_blocked_repair_retry_delay(streak) {
        return Ok(DurableRepairRecoveryOutcome::Noop);
    }
    let Some(workspace) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&current.conversation_id)
        .await?
    else {
        return Ok(DurableRepairRecoveryOutcome::Noop);
    };
    // Spawning a successor is the most expensive thing this pass can do, so a PR autofix
    // generation must earn it against live PR health rather than against the streak counter alone.
    // Blocker text is deliberately not consulted: it is free-form agent prose, not evidence.
    // Cost, not attempt count, is what actually needs bounding: three cheap generations and three
    // hour-long Opus generations look identical to the streak counter above.
    if current.source == AgentWorkspaceRepairSource::PrAutofix {
        if let Some(fingerprint) = current.pr_autofix_health_fingerprint.as_deref() {
            let spend =
                pr_autofix_fingerprint_spend(state, &current.conversation_id, fingerprint).await?;
            if spend.is_exhausted() {
                return park_exhausted_pr_autofix_budget(state, current, &workspace, spend).await;
            }
        }
    }
    let carryover_pr_autofix_evidence = if current.source == AgentWorkspaceRepairSource::PrAutofix {
        match evaluate_pr_autofix_successor(state, &current, &workspace).await {
            PrAutofixSuccessorDecision::Proceed(carryover) => carryover,
            PrAutofixSuccessorDecision::RedrivePublish => {
                return redrive_blocked_repair_publish(state, current, &workspace).await;
            }
            PrAutofixSuccessorDecision::HoldUnchanged => {
                return hold_unchanged_pr_autofix_health(state, current, &workspace).await;
            }
            PrAutofixSuccessorDecision::Withhold(reason) => {
                tracing::info!(
                    conversation_id = current.conversation_id.as_str(),
                    attempt_id = current.id.as_str(),
                    reason,
                    "Withholding a blocked PR autofix successor; no current failure identity authorizes one"
                );
                return Ok(DurableRepairRecoveryOutcome::Noop);
            }
        }
    } else {
        None
    };
    let marker = format!("{AUTO_RETRY_BLOCKED_REPAIR_REASON_PREFIX}{}", streak + 1);
    let start = start_or_join_agent_workspace_repair(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.agent_conversation_workspace_repo),
        AgentWorkspaceRepairStartRequest {
            conversation_id: current.conversation_id.clone(),
            source: current.source,
            continuation: current.continuation,
            target_base_ref: workspace.base_ref,
            target_base_commit: workspace.base_commit,
            verified_newer_base: false,
            reason: marker,
            summary: "Automatically retrying the blocked workspace repair.".to_string(),
            auto_merge_current: workspace.pr_auto_merge_current,
            explicit_publish_requested: current.explicit_publish_requested,
            retry_blocked: true,
            carryover_pr_autofix_evidence,
        },
    )
    .await?;
    match start {
        AgentWorkspaceRepairStartOutcome::SuccessorStarted(successor) => {
            rescue_orphaned_repair_dispatch(state, successor).await
        }
        AgentWorkspaceRepairStartOutcome::Started(_)
        | AgentWorkspaceRepairStartOutcome::Joined(_)
        | AgentWorkspaceRepairStartOutcome::BlockedByCurrent(_) => {
            Ok(DurableRepairRecoveryOutcome::Stale)
        }
    }
}

/// Stops unattended repair on one failure identity once it has consumed its agent-minutes budget,
/// and tells the user why. Budget exhaustion is a handover, never a silent skip: the generation is
/// marked needs-human so no automatic path may revive it, the spend is recorded on the publication
/// timeline, and an Inbox notification carries it to the user.
async fn park_exhausted_pr_autofix_budget(
    state: &AppState,
    current: AgentWorkspaceRepairAttempt,
    workspace: &AgentConversationWorkspace,
    spend: PrAutofixFingerprintSpend,
) -> AppResult<DurableRepairRecoveryOutcome> {
    let conversation_id = current.conversation_id.clone();
    // Captured before `current` is consumed below. The dedupe key must be the identity this
    // generation actually exhausted, so a later, different failure still reaches the user.
    let fingerprint = current.pr_autofix_health_fingerprint.clone();
    let summary = if spend.budget_minutes == 0 {
        format!(
            "RalphX re-drove publication of this completed repair {} times without reaching the \
             PR, which is the configured limit. Automatic repair has stopped so it does not keep \
             retrying a publish it cannot complete.",
            spend.generations
        )
    } else {
        format!(
            "RalphX has spent {} minutes across {} repair generations on this same PR failure without \
             resolving it, which is the configured limit. Automatic repair has stopped so it does not \
             keep spending on a failure it cannot fix.",
            spend.minutes, spend.generations
        )
    };

    remember_blocked_pr_autofix_fingerprint(state, &current).await;
    state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            REPAIR_BUDGET_EXHAUSTED_STEP,
            "blocked",
            &summary,
            current.pr_autofix_health_fingerprint.clone(),
        ))
        .await?;

    let what_happened = current.what_happened.clone();
    let what_i_did = current.what_i_did.clone();
    let outcome = block_agent_workspace_repair_needs_human(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        current,
        &summary,
        workspace.pr_auto_merge_current,
        what_happened.as_deref(),
        what_i_did.as_deref(),
    )
    .await?;

    state
        .notification_service()
        .record(NewNotification {
            project_id: Some(workspace.project_id.to_string()),
            category: NotificationCategory::TaskBlocked,
            severity: NotificationSeverity::ActionRequired,
            title: match workspace.publication_pr_number {
                Some(pr_number) => format!("PR #{pr_number} repair needs you"),
                None => "Workspace repair needs you".to_string(),
            },
            body: Some(summary),
            target: NotificationTarget {
                kind: NotificationTargetKind::AgentConversation,
                project_id: Some(workspace.project_id.to_string()),
                task_id: None,
                conversation_id: Some(conversation_id.to_string()),
                setup_conversation_id: None,
                automation_id: None,
                run_id: None,
            },
            dedupe_key: Some(format!(
                "repair_budget:{}:{}",
                conversation_id.as_str(),
                fingerprint.as_deref().unwrap_or("unknown")
            )),
        })
        .await;

    match outcome {
        AgentWorkspaceRepairTransitionOutcome::Applied(_) => {
            Ok(DurableRepairRecoveryOutcome::Blocked)
        }
        AgentWorkspaceRepairTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairTransitionOutcome::Missing => Ok(DurableRepairRecoveryOutcome::Stale),
    }
}

/// Re-enters the existing publish boundary for a completed repair whose durable local head has
/// not reached the PR. This is deliberately not a successor: the fixer already produced output,
/// and the boundary remains authoritative for review and auto-publish gates.
async fn redrive_blocked_repair_publish(
    state: &AppState,
    mut current: AgentWorkspaceRepairAttempt,
    workspace: &AgentConversationWorkspace,
) -> AppResult<DurableRepairRecoveryOutcome> {
    let streak = automatic_blocked_repair_streak(&current);
    if streak >= MAX_AUTO_RETRY_BLOCKED_REPAIR_STREAK {
        return park_exhausted_pr_autofix_budget(
            state,
            current,
            workspace,
            PrAutofixFingerprintSpend {
                generations: streak,
                minutes: 0,
                budget_minutes: 0,
            },
        )
        .await;
    }

    let summary =
        "Re-driving the publish of a completed repair whose output has not reached the PR.";
    current.pending_reasons.push(format!(
        "{AUTO_RETRY_BLOCKED_REPAIR_REASON_PREFIX}{}",
        streak + 1
    ));
    let conversation_id = current.conversation_id.clone();
    let fingerprint = current.pr_autofix_health_fingerprint.clone();
    match continue_agent_workspace_repair_at_boundary(
        state,
        current,
        AgentWorkspaceRepairPhase::Blocked,
        summary,
        false,
        PublishAuthority::VerifiedAutomation,
    )
    .await?
    {
        AgentWorkspaceRepairTransitionOutcome::Applied(_) => {
            state
                .agent_conversation_workspace_repo
                .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                    conversation_id,
                    REPAIR_PUBLISH_REDRIVE_STEP,
                    "active",
                    summary,
                    fingerprint,
                ))
                .await?;
            Ok(DurableRepairRecoveryOutcome::Continued)
        }
        AgentWorkspaceRepairTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairTransitionOutcome::Missing => Ok(DurableRepairRecoveryOutcome::Stale),
    }
}

/// Parks a blocked PR autofix generation whose failure identity has not moved. The hold is a
/// visible state, not a silent skip: it appends a publication event and leaves the generation
/// where the poller can settle it once GitHub reports different health.
async fn hold_unchanged_pr_autofix_health(
    state: &AppState,
    current: AgentWorkspaceRepairAttempt,
    workspace: &AgentConversationWorkspace,
) -> AppResult<DurableRepairRecoveryOutcome> {
    let summary = "GitHub still reports the same failing PR health this repair was dispatched for. RalphX is holding the repair instead of running another fixer generation on identical evidence.";
    match reserve_agent_workspace_unchanged_health_hold(
        Arc::clone(&state.agent_workspace_repair_repo),
        current,
        summary,
        workspace.pr_auto_merge_current,
    )
    .await?
    {
        AgentWorkspaceRepairTransitionOutcome::Applied(attempt) => {
            remember_blocked_pr_autofix_fingerprint(state, &attempt).await;
            release_repair_lease_if_settled_boundary(state, &attempt).await?;
            Ok(DurableRepairRecoveryOutcome::Noop)
        }
        AgentWorkspaceRepairTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairTransitionOutcome::Missing => Ok(DurableRepairRecoveryOutcome::Stale),
    }
}

fn repair_attempt_has_target_lease(attempt: &AgentWorkspaceRepairAttempt) -> bool {
    attempt.git_common_dir.is_some()
        || attempt.target_ref.is_some()
        || attempt.target_identity_version.is_some()
        || attempt.target_lease_epoch.is_some()
}

/// What the durable reservation's agent run proves about an in-flight dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReservedDispatchLiveness {
    /// The exact reserved run exists on this conversation and is still running.
    Running,
    /// The exact reserved run exists on this conversation and has already terminated.
    Ended,
    /// No run row proves anything yet: the reservation has no run id, the row has not been
    /// written, or the id resolves to a different conversation.
    Unobserved,
}

async fn reserved_dispatch_liveness(
    state: &AppState,
    attempt: &AgentWorkspaceRepairAttempt,
) -> AppResult<ReservedDispatchLiveness> {
    let Some(run_id) = attempt.reserved_agent_run_id.as_ref() else {
        return Ok(ReservedDispatchLiveness::Unobserved);
    };
    let Some(run) = state.agent_run_repo.get_by_id(run_id).await? else {
        return Ok(ReservedDispatchLiveness::Unobserved);
    };
    if run.conversation_id != attempt.conversation_id {
        return Ok(ReservedDispatchLiveness::Unobserved);
    }
    Ok(if run.status.is_active() {
        ReservedDispatchLiveness::Running
    } else {
        ReservedDispatchLiveness::Ended
    })
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

pub(super) const DEFAULT_REPAIR_DISPATCH_CONTEXT: &str =
    "The current durable workspace repair still needs attention.";

/// Delegates marker filtering to the durable repair-state seam so every dispatcher shares the
/// same definition of human-authored context.
pub(super) fn human_repair_dispatch_context(attempt: &AgentWorkspaceRepairAttempt) -> Option<&str> {
    last_human_repair_reason(attempt)
}

pub(crate) fn due_repair_dispatch_message(
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
    let reason = human_repair_dispatch_context(attempt).unwrap_or(DEFAULT_REPAIR_DISPATCH_CONTEXT);
    format!(
        // Naming the exact tool is safe now that redelivery is source-aware: this message is only
        // ever addressed to the workspace repairer, which is the agent granted that tool.
        "{continuation}\n\nInspect the current workspace state before changing files. When the repair is committed, call `complete_agent_workspace_repair` with a summary, adding a blocker if the repair cannot be completed safely.\n\nContext: {reason}\nWorkspace branch: {}\nBase ref: {}",
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
    reserve_and_deliver_repair_dispatch(
        state,
        attempt,
        target_identity,
        workspace,
        resolved.path,
        "Retrying the durable workspace repair delivery.",
        "Durable workspace repair delivery retry completed.",
    )
    .await
}

/// Records that a due auto-retry dispatch executed. Deliberately per-execution with no
/// read-before-append dedupe (unlike lifecycle-classification events elsewhere in this module) so
/// the publish panel shows every actual redelivery, not just the first. Emission is non-fatal.
async fn append_repair_auto_retry_dispatched_event(
    state: &AppState,
    conversation_id: crate::domain::entities::ChatConversationId,
) {
    if let Err(error) = state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            REPAIR_AUTO_RETRY_DISPATCHED_STEP,
            "retrying",
            "RalphX automatically redelivered a due workspace repair retry.",
            None,
        ))
        .await
    {
        tracing::warn!(
            conversation_id = conversation_id.as_str(),
            error = %error,
            "Failed to record repair auto-retry dispatched publication event"
        );
    }
}

async fn rescue_orphaned_repair_dispatch(
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
    let target_identity =
        GitService::canonical_target_identity(&resolved.path, &workspace.branch_name).await?;
    reserve_and_deliver_repair_dispatch(
        state,
        attempt,
        target_identity,
        workspace,
        resolved.path,
        "Rescuing the orphaned durable workspace repair delivery.",
        "Recovered the orphaned durable workspace repair delivery.",
    )
    .await
}

/// Records the remote head a rescued PR autofix generation is fixing against, so the completion
/// gate at `repair_completion.rs` has the "before" side of the comparison it requires. The poller
/// sets this from live PR health; recovery must work without GitHub, so it uses the fetched remote
/// branch head — the same commit the PR reports — and falls back to the local branch head when the
/// remote is unreachable. Never overwrites dispatch evidence an earlier dispatch already proved,
/// and never fails the delivery: a NULL head only reproduces today's behavior.
async fn backfill_pr_autofix_dispatch_head(
    attempt: &mut AgentWorkspaceRepairAttempt,
    workspace: &AgentConversationWorkspace,
    working_directory: &std::path::Path,
) {
    if attempt.source != AgentWorkspaceRepairSource::PrAutofix
        || attempt
            .pr_autofix_dispatch_head_commit
            .as_deref()
            .is_some_and(|head| !head.trim().is_empty())
    {
        return;
    }
    let remote_head =
        match crate::application::git_mutation_recovery::read_repair_origin_branch_oid(
            working_directory,
            &workspace.branch_name,
        )
        .await
        {
            Ok(head) => head,
            Err(error) => {
                tracing::warn!(
                    conversation_id = attempt.conversation_id.as_str(),
                    branch = workspace.branch_name.as_str(),
                    error = %error,
                    "Could not read the remote head for PR autofix dispatch evidence; falling back to the local branch head"
                );
                None
            }
        };
    let resolved = match remote_head {
        Some(head) => Some(head),
        None => match GitService::get_branch_sha(working_directory, &workspace.branch_name).await {
            Ok(head) => Some(head),
            Err(error) => {
                tracing::warn!(
                    conversation_id = attempt.conversation_id.as_str(),
                    branch = workspace.branch_name.as_str(),
                    error = %error,
                    "Could not resolve any dispatch head for a rescued PR autofix generation"
                );
                None
            }
        },
    };
    let Some(head) = resolved.filter(|head| !head.trim().is_empty()) else {
        return;
    };
    tracing::info!(
        conversation_id = attempt.conversation_id.as_str(),
        dispatch_head = head.as_str(),
        "Backfilled PR autofix dispatch head evidence for a recovery-side delivery"
    );
    attempt.pr_autofix_dispatch_head_commit = Some(head);
}

pub(super) async fn reserve_and_deliver_repair_dispatch(
    state: &AppState,
    mut attempt: AgentWorkspaceRepairAttempt,
    target_identity: crate::domain::entities::GitTargetIdentity,
    workspace: AgentConversationWorkspace,
    working_directory: std::path::PathBuf,
    reservation_summary: &str,
    settlement_summary: &str,
) -> AppResult<DurableRepairRecoveryOutcome> {
    let run_id = AgentRunId::new();
    // Runtime continuity is resolved before the reservation so a repository failure cannot strand
    // an attempt in `Dispatching` with no delivery attached to it.
    let pr_autofix_options = if attempt.source == AgentWorkspaceRepairSource::PrAutofix {
        Some(
            crate::application::services::pr_merge_poller::agent_workspace_pr_fixer_send_options(
                &workspace,
                &working_directory,
                Some(&state.agent_run_repo),
            )
            .await?,
        )
    } else {
        None
    };
    // Dispatch evidence must exist before the reservation persists the attempt: the reservation is
    // the only write on this lane that carries `pr_autofix_dispatch_head_commit`.
    backfill_pr_autofix_dispatch_head(&mut attempt, &workspace, &working_directory).await;
    let reserved = match reserve_agent_workspace_repair_dispatch(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        target_identity,
        attempt,
        run_id.clone(),
        reservation_summary,
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
    // Source, not phase, decides who owns the work. A PR autofix generation is always the PR
    // fixer's, so retry, rescue, and blocked-successor redelivery cannot hand PR work to the
    // generic workspace repairer, whose tooling cannot classify a PR failure at all.
    let (message, options) = match pr_autofix_options {
        Some(mut options) => {
            options.preallocated_agent_run_id = Some(run_id.clone());
            options.queue_policy = SendQueuePolicy::RequireImmediateStart;
            (
                due_pr_autofix_redispatch_message(&reserved, &workspace),
                options,
            )
        }
        None => (
            due_repair_dispatch_message(&reserved, &workspace),
            SendMessageOptions {
                preallocated_agent_run_id: Some(run_id.clone()),
                queue_policy: SendQueuePolicy::RequireImmediateStart,
                conversation_id_override: Some(workspace.conversation_id.clone()),
                agent_name_override: Some(AGENT_WORKSPACE_REPAIR.to_string()),
                working_directory_override: Some(working_directory),
                force_new_provider_session: true,
                preserve_conversation_provider_session_ref: true,
                ..Default::default()
            },
        ),
    };
    let service = state.build_chat_service();
    let delivery = service
        .send_message(
            ChatContextType::Project,
            workspace.project_id.as_str(),
            &message,
            options,
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
        settlement_summary,
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

/// The human sentence inside an inspection failure, without the variant label `AppError`'s
/// `Display` prepends. Matched on variants rather than stripping a prefix, so the blocked-repair
/// banner cannot start showing `Conflict:` again if a variant's `Display` changes.
fn workspace_inspection_detail(error: &AppError) -> String {
    match error {
        AppError::Conflict(detail)
        | AppError::Validation(detail)
        | AppError::NotFound(detail)
        | AppError::GitOperation(detail)
        | AppError::Infrastructure(detail) => detail.clone(),
        other => other.to_string(),
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
    let validation = match inspect_agent_workspace_repair_completion_classified(
        state,
        &workspace,
        &reserved.target_base_ref,
        Some(target_base_commit),
    )
    .await
    {
        Ok(AgentWorkspaceRepairCompletionInspection::Proven(validation)) => validation,
        // A settled tree behind a newer base is ordinary new input, not an integrity failure. The
        // blocked-redelivery path has always treated a moved base that way; this is the same rule
        // for the interrupted path.
        Ok(AgentWorkspaceRepairCompletionInspection::BehindNewBase {
            target_ref,
            target_base_commit,
            repair_head_commit,
        }) => {
            tracing::info!(
                conversation_id = reserved.conversation_id.as_str(),
                target_ref = target_ref.as_str(),
                target_base_commit = target_base_commit.as_str(),
                repair_head_commit = repair_head_commit.as_str(),
                "Interrupted workspace repair is settled but behind a newer base; retargeting instead of blocking"
            );
            return retarget_reserved_repair_to_advanced_base(
                state,
                reserved,
                &workspace,
                &target_base_commit,
            )
            .await;
        }
        // `detail` is a human sentence owned by the classifier, so the banner never shows an
        // `AppError` variant name like `Conflict:` to a user.
        Ok(AgentWorkspaceRepairCompletionInspection::Unprovable(detail)) => {
            return block_recovery_attempt(
                state,
                reserved,
                &format!(
                    "Workspace repair recovery could not prove a clean committed repair: {detail}"
                ),
            )
            .await;
        }
        Err(error) => {
            return block_recovery_attempt(
                state,
                reserved,
                &format!(
                    "Workspace repair recovery could not inspect the workspace: {}",
                    workspace_inspection_detail(&error)
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
    let what_happened = reserved.what_happened.clone();
    let what_i_did = reserved.what_i_did.clone();
    let validated = match record_agent_workspace_repair_validation(
        Arc::clone(&state.agent_workspace_repair_repo),
        reserved,
        &validation.base_ref,
        &validation.base_commit,
        &validation.repair_head_commit,
        "Recovered a clean committed workspace repair after its owning run stopped.",
        validation.auto_merge_current,
        what_happened.as_deref(),
        what_i_did.as_deref(),
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
        PublishAuthority::VerifiedAutomation,
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
    let outcome = recover_agent_workspace_repair_continuation(state, continuation, false).await;
    if let Err(error) = &outcome {
        tracing::warn!(
            conversation_id = conversation_id.as_str(),
            error = %error,
            "Clean workspace repair recovery left its durable continuation pending"
        );
    }
    outcome
}

pub(crate) async fn recover_agent_workspace_repair_continuation(
    state: &AppState,
    attempt: AgentWorkspaceRepairAttempt,
    block_when_publish_runtime_is_missing: bool,
) -> AppResult<DurableRepairRecoveryOutcome> {
    let continuation =
        crate::application::publish_resilience::continue_agent_workspace_repair_publish(
            state,
            attempt.clone(),
        )
        .await;
    let continuation = match continuation {
        Err(initial_error) => {
            match retry_agent_workspace_repair_continuation_after_lease_healing(state, &attempt)
                .await?
            {
                Some(retry) => retry,
                None => Err(initial_error),
            }
        }
        continuation => continuation,
    };

    match continuation {
        Ok(Some(crate::application::publish_resilience::AgentWorkspaceRepairPushOutcome::Busy)) => {
            Ok(DurableRepairRecoveryOutcome::Active)
        }
        Ok(Some(crate::application::publish_resilience::AgentWorkspaceRepairPushOutcome::Stale)) => {
            Ok(DurableRepairRecoveryOutcome::Stale)
        }
        Ok(Some(_)) => Ok(DurableRepairRecoveryOutcome::Continued),
        Ok(None) if block_when_publish_runtime_is_missing => {
            block_recovery_attempt(
                state,
                attempt,
                "Workspace repair continuation could not prove a publish runtime. Retry the blocked operation.",
            )
            .await
        }
        Ok(None) => {
            let error = AppError::Conflict(
                "workspace repair continuation could not prove a publish runtime".to_string(),
            );
            escalate_or_record_continuation_recovery_failure(state, attempt, &error).await
        }
        Err(error) => escalate_or_record_continuation_recovery_failure(state, attempt, &error).await,
    }
}

/// A continuation can be fenced by a stale persisted lease after a crash even though its durable
/// generation is still the sole current owner. Heal that exact snapshot once, never while a
/// receipt is open, then let the ordinary publisher revalidate every Git-side invariant.
async fn retry_agent_workspace_repair_continuation_after_lease_healing(
    state: &AppState,
    failed_attempt: &AgentWorkspaceRepairAttempt,
) -> AppResult<
    Option<
        AppResult<Option<crate::application::publish_resilience::AgentWorkspaceRepairPushOutcome>>,
    >,
> {
    let Some(current) = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&failed_attempt.conversation_id)
        .await?
    else {
        return Ok(None);
    };
    if current.id != failed_attempt.id
        || current.generation != failed_attempt.generation
        || current.updated_at != failed_attempt.updated_at
        || current.phase != failed_attempt.phase
        || state
            .agent_workspace_repair_repo
            .get_open_repair_effect(&current.id)
            .await?
            .is_some()
    {
        return Ok(None);
    }
    match validate_agent_workspace_repair_target_lease(state.branch_update_repo.as_ref(), &current)
        .await
    {
        Err(AppError::Conflict(_)) => {}
        Ok(_) | Err(_) => return Ok(None),
    }
    let Some(workspace) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&current.conversation_id)
        .await?
    else {
        return Ok(None);
    };
    let healed = match reacquire_agent_workspace_repair_target_lease_for_continuation(
        state,
        &workspace,
        current.clone(),
        current.phase,
    )
    .await
    {
        Ok(healed) => healed,
        Err(error) => return Ok(Some(Err(error))),
    };
    match healed {
        AgentWorkspaceRepairTransitionOutcome::Applied(healed) => Ok(Some(
            crate::application::publish_resilience::continue_agent_workspace_repair_publish(
                state, healed,
            )
            .await,
        )),
        AgentWorkspaceRepairTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairTransitionOutcome::Missing => Ok(Some(Ok(Some(
            crate::application::publish_resilience::AgentWorkspaceRepairPushOutcome::Stale,
        )))),
    }
}

/// A failed continuation can have crossed an external-effect boundary before the caller sees its
/// error. Re-read the exact durable generation: a persisted blocker is authoritative, while a
/// current pending/continuing generation keeps its effect receipt and target lease fenced for
/// postcondition reconciliation. Never convert either state into a false `Continued` outcome.
async fn escalate_or_record_continuation_recovery_failure(
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
        // A blocked attempt still holding an open effect is fenced twice over: automatic retry
        // declines on the open effect, and the user's Retry action is withheld until the attention
        // reason exists. Recording it here is the manual backstop behind the automatic hatch, so no
        // recovery depends solely on automation firing. `CreatePr` stays fenced by the admission
        // projection itself, which never re-admits an unproven pull-request creation.
        record_blocked_open_effect_attention(state, current.clone()).await?;
        release_repair_lease_if_settled_boundary(state, &current).await?;
        return Ok(DurableRepairRecoveryOutcome::Blocked);
    }
    if !matches!(
        current.phase,
        AgentWorkspaceRepairPhase::ContinuationPending | AgentWorkspaceRepairPhase::Continuing
    ) {
        return Ok(DurableRepairRecoveryOutcome::Stale);
    }

    let has_open_effect = state
        .agent_workspace_repair_repo
        .get_open_repair_effect(&current.id)
        .await?
        .is_some();
    if has_open_effect {
        return record_open_effect_continuation_recovery_failure(state, current, error).await;
    }
    let next_streak = continuation_recovery_failure_streak(&current).saturating_add(1);
    if next_streak >= MAX_CONTINUATION_RECOVERY_FAILURE_STREAK {
        let conversation_id = current.conversation_id.clone();
        let fingerprint = current.pr_autofix_health_fingerprint.clone();
        let outcome = block_recovery_attempt(
            state,
            current,
            &format!(
                "Workspace repair continuation recovery failed {next_streak} times without settling: {error}"
            ),
        )
        .await?;
        if outcome == DurableRepairRecoveryOutcome::Blocked {
            state
                .agent_conversation_workspace_repo
                .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                    conversation_id,
                    CONTINUATION_RECOVERY_BLOCKED_STEP,
                    "blocked",
                    format!(
                        "Workspace repair publication was blocked after {next_streak} failed recovery attempts."
                    ),
                    fingerprint,
                ))
                .await?;
        }
        return Ok(outcome);
    }

    let mut marked = current;
    marked.pending_reasons.push(format!(
        "{CONTINUATION_RECOVERY_FAILURE_REASON_PREFIX}{next_streak}"
    ));
    let summary = format!(
        "Workspace repair continuation is pending reconciliation after recovery error: {error}"
    );
    let expected_phase = marked.phase;
    match transition_agent_workspace_repair_attempt(
        Arc::clone(&state.agent_workspace_repair_repo),
        marked,
        expected_phase,
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

/// Records the open-effect attention reason on an attempt that is already blocked, which is the
/// one shape `record_open_effect_continuation_recovery_failure` can never reach: it only runs for
/// `ContinuationPending`/`Continuing`.
///
/// Without this the admission projection at
/// `agent_workspace_publish_repair_state::load_agent_workspace_repair_operation_recovery_action`
/// withholds the user's Retry action forever, because it requires the reason to be present before
/// re-admitting an idempotent push/update replay.
///
/// Additive and idempotent: it appends one reason and keeps the attempt blocked. A write race just
/// means the next sweep records it instead.
async fn record_blocked_open_effect_attention(
    state: &AppState,
    current: AgentWorkspaceRepairAttempt,
) -> AppResult<()> {
    if current
        .pending_reasons
        .iter()
        .any(|reason| reason == CONTINUATION_OPEN_EFFECT_ATTENTION_REASON)
    {
        return Ok(());
    }
    if state
        .agent_workspace_repair_repo
        .get_open_repair_effect(&current.id)
        .await?
        .is_none()
    {
        return Ok(());
    }
    let summary = current.summary.clone().unwrap_or_else(|| {
        "Workspace repair publication is blocked behind an open external effect.".to_string()
    });
    let mut marked = current;
    marked
        .pending_reasons
        .push(CONTINUATION_OPEN_EFFECT_ATTENTION_REASON.to_string());
    let expected_phase = marked.phase;
    match transition_agent_workspace_repair_attempt(
        Arc::clone(&state.agent_workspace_repair_repo),
        marked,
        expected_phase,
        &summary,
        None,
    )
    .await?
    {
        AgentWorkspaceRepairTransitionOutcome::Applied(marked) => {
            surface_open_effect_continuation_attention(state, &marked, &summary).await
        }
        AgentWorkspaceRepairTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairTransitionOutcome::Missing => Ok(()),
    }
}

async fn record_open_effect_continuation_recovery_failure(
    state: &AppState,
    current: AgentWorkspaceRepairAttempt,
    error: &AppError,
) -> AppResult<DurableRepairRecoveryOutcome> {
    let already_escalated = current
        .pending_reasons
        .iter()
        .any(|reason| reason == CONTINUATION_OPEN_EFFECT_ATTENTION_REASON);
    let next_streak = continuation_open_effect_recovery_streak(&current).saturating_add(1);
    let summary = if already_escalated || next_streak >= MAX_CONTINUATION_RECOVERY_FAILURE_STREAK {
        format!(
            "Workspace repair publication needs attention because its external effect remains open after {next_streak} recovery checks. RalphX retained the effect fence and did not reacquire or release Git authority: {error}"
        )
    } else {
        format!(
            "Workspace repair continuation is pending reconciliation for an open external effect after recovery error: {error}"
        )
    };

    if already_escalated {
        surface_open_effect_continuation_attention(state, &current, &summary).await?;
        return Ok(DurableRepairRecoveryOutcome::Active);
    }

    let mut marked = current;
    marked.pending_reasons.push(format!(
        "{CONTINUATION_OPEN_EFFECT_RECOVERY_REASON_PREFIX}{next_streak}"
    ));
    if next_streak >= MAX_CONTINUATION_RECOVERY_FAILURE_STREAK {
        marked
            .pending_reasons
            .push(CONTINUATION_OPEN_EFFECT_ATTENTION_REASON.to_string());
    }
    let expected_phase = marked.phase;
    match transition_agent_workspace_repair_attempt(
        Arc::clone(&state.agent_workspace_repair_repo),
        marked,
        expected_phase,
        &summary,
        None,
    )
    .await?
    {
        AgentWorkspaceRepairTransitionOutcome::Applied(marked) => {
            if next_streak >= MAX_CONTINUATION_RECOVERY_FAILURE_STREAK {
                surface_open_effect_continuation_attention(state, &marked, &summary).await?;
            }
            Ok(DurableRepairRecoveryOutcome::Active)
        }
        AgentWorkspaceRepairTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairTransitionOutcome::Missing => Ok(DurableRepairRecoveryOutcome::Stale),
    }
}

async fn surface_open_effect_continuation_attention(
    state: &AppState,
    attempt: &AgentWorkspaceRepairAttempt,
    summary: &str,
) -> AppResult<()> {
    let classification = attempt.id.to_string();
    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&attempt.conversation_id)
        .await?;
    if !events.iter().any(|event| {
        event.step == CONTINUATION_OPEN_EFFECT_ATTENTION_STEP
            && event.classification.as_deref() == Some(classification.as_str())
    }) {
        state
            .agent_conversation_workspace_repo
            .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                attempt.conversation_id.clone(),
                CONTINUATION_OPEN_EFFECT_ATTENTION_STEP,
                "attention_required",
                summary,
                Some(classification),
            ))
            .await?;
    }

    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&attempt.conversation_id)
        .await?;
    let project_id = workspace
        .as_ref()
        .map(|workspace| workspace.project_id.to_string());
    state
        .notification_service()
        .record(NewNotification {
            project_id: project_id.clone(),
            category: NotificationCategory::TaskBlocked,
            severity: NotificationSeverity::ActionRequired,
            title: "Workspace repair effect needs attention".to_string(),
            body: Some(summary.to_string()),
            target: NotificationTarget {
                kind: NotificationTargetKind::AgentConversation,
                project_id,
                task_id: None,
                conversation_id: Some(attempt.conversation_id.to_string()),
                setup_conversation_id: None,
                automation_id: None,
                run_id: None,
            },
            dedupe_key: Some(format!(
                "repair_open_effect:{}:{}",
                attempt.conversation_id, attempt.id
            )),
        })
        .await;
    Ok(())
}

/// The reconciler proved the push never reached the remote and terminated the effect as
/// `Failed`, clearing the fence. Record a timeline event and settle any attention notification
/// raised by prior open-effect recovery failures. Best-effort: neither write may fail the
/// caller, and the caller defers the lease reacquire to the next recovery pass rather than
/// re-driving publication in this pass.
async fn record_continuation_effect_not_applied(
    state: &AppState,
    attempt: &AgentWorkspaceRepairAttempt,
) {
    let summary = "Workspace repair push effect was not applied: the remote still matches the recorded pre-push state, proving the push never reached the remote. The effect fence is now clear.";
    if let Err(error) = state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            attempt.conversation_id.clone(),
            CONTINUATION_EFFECT_NOT_APPLIED_STEP,
            "active",
            summary,
            Some(attempt.id.to_string()),
        ))
        .await
    {
        tracing::warn!(error = %error, attempt_id = %attempt.id, "Failed to append workspace repair effect not-applied event");
    }
    state
        .notification_service()
        .resolve_workflow_notification(&format!(
            "repair_open_effect:{}:{}",
            attempt.conversation_id, attempt.id
        ))
        .await;
}

fn continuation_recovery_failure_streak(attempt: &AgentWorkspaceRepairAttempt) -> u32 {
    attempt
        .pending_reasons
        .iter()
        .filter_map(|reason| reason.strip_prefix(CONTINUATION_RECOVERY_FAILURE_REASON_PREFIX))
        .filter_map(|streak| streak.parse::<u32>().ok())
        .max()
        .unwrap_or_default()
}

fn continuation_open_effect_recovery_streak(attempt: &AgentWorkspaceRepairAttempt) -> u32 {
    attempt
        .pending_reasons
        .iter()
        .filter_map(|reason| reason.strip_prefix(CONTINUATION_OPEN_EFFECT_RECOVERY_REASON_PREFIX))
        .filter_map(|streak| streak.parse::<u32>().ok())
        .max()
        .unwrap_or_default()
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
    let what_happened = attempt.what_happened.clone();
    let what_i_did = attempt.what_i_did.clone();
    match block_agent_workspace_repair_completion(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        attempt,
        "Workspace repair recovery is blocked.",
        blocker,
        auto_merge_current.flatten(),
        what_happened.as_deref(),
        what_i_did.as_deref(),
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

#[cfg(any(test, feature = "test-utils"))]
fn is_legacy_pr_fix_review_projection(workspace: &AgentConversationWorkspace) -> bool {
    workspace.publication_push_status.as_deref() == Some("needs_agent")
        && workspace.pr_supervision_status.as_deref() == Some("reviewing")
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
        pr_autofix_enabled: None,
        pr_auto_merge_desired: None,
        base_commit: attempt.target_base_commit.clone(),
    }
}
