use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};

use crate::application::agent_conversation_workspace::resolve_effective_agent_conversation_workspace_path;
use crate::application::agent_workspace_review::{
    load_agent_workspace_review_context, load_workspace_review_publish_blocker,
    review_gate_publish_blocker, AgentWorkspaceReviewStart,
};
use crate::application::agent_workspace_review_auto_merge::{
    start_guarded_agent_workspace_review, WorkspaceReviewStartOrigin,
};
use crate::application::chat_service::{ChatServiceError, SendResult};
use crate::application::publish_resilience::{
    inspect_publish_branch_freshness_for_source, verify_agent_workspace_repair_completion,
    AgentWorkspaceRepairCompletionCheck,
};
use crate::application::{AppState, GitService};
#[cfg(any(test, feature = "test-utils"))]
use crate::domain::entities::AgentRun;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentRunId, AgentWorkspaceRepairAttempt,
    AgentWorkspaceRepairCompletionAuthority, AgentWorkspaceRepairContinuation,
    AgentWorkspaceRepairPhase, AgentWorkspaceRepairSource, AgentWorkspaceReviewGateStatus,
    ChatConversationId, GitTargetIdentity, GitTargetLeaseOwner,
};
use crate::domain::repositories::{
    AcquireGitTargetLease, AcquireGitTargetLeaseOutcome, AgentConversationWorkspaceRepository,
    AgentWorkspaceRepairAttemptTransition, AgentWorkspaceRepairAttemptTransitionOutcome,
    AgentWorkspaceRepairCompatibilityProjection, AgentWorkspaceRepairRepository,
    BindAgentWorkspaceRepairAttemptRun, BranchUpdateRepository, GitAuthorityCasOutcome,
    StartOrJoinAgentWorkspaceRepairAttempt, StartOrJoinAgentWorkspaceRepairAttemptOutcome,
};
#[cfg(any(test, feature = "test-utils"))]
use crate::domain::repositories::{
    AgentRunRepository, AgentWorkspaceRepairStateGuard, AgentWorkspaceRepairStateTransition,
};
use crate::error::{AppError, AppResult};

pub(crate) const REPAIR_REQUESTED_STEP: &str = "repair_requested";
#[cfg(any(test, feature = "test-utils"))]
pub(crate) const REPAIR_DEFERRED_STEP: &str = "repair_deferred";
pub(crate) const REPAIR_SENT_STEP: &str = "repair_sent";
#[cfg(test)]
pub(crate) const PR_AUTOFIX_COMPLETED_STEP: &str = "pr_autofix_completed";
#[cfg(test)]
pub(crate) const PR_AUTOFIX_BLOCKED_STEP: &str = "pr_autofix_blocked";
#[cfg(test)]
pub(crate) const PR_AUTOFIX_WORKSPACE_REVIEW_STEP: &str = "pr_autofix_workspace_review";
#[cfg(any(test, feature = "test-utils"))]
pub(crate) const PR_AUTOFIX_WORKSPACE_REVIEW_ABORTED_STEP: &str =
    "pr_autofix_workspace_review_aborted";
#[cfg(test)]
pub(crate) const PR_AUTOFIX_WORKSPACE_REVIEW_PASSED_STEP: &str =
    "pr_autofix_workspace_review_passed";
pub(crate) const DEFERRED_REPAIR_WAIT_TIMEOUT_SECS: u64 = 300;
const REPAIR_RUN_CLASSIFICATION_PREFIX: &str = "agent_fixable:run:";
pub(crate) const AGENT_WORKSPACE_REPAIR_TARGET_IDENTITY_VERSION: u64 = 1;
pub(crate) const MAX_AGENT_WORKSPACE_REPAIR_DISPATCH_RETRIES: u32 = 3;
/// A deliberately small cap: transient runner failures must not create an unbounded CI loop.
pub(crate) const MAX_AGENT_WORKSPACE_CI_RERUN_RETRIES: u32 = 3;
pub(crate) const NEEDS_HUMAN_REPAIR_REASON: &str = "pr_autofix_needs_human";
pub(crate) const PRE_EXISTING_ON_BASE_REPAIR_REASON: &str = "pr_autofix_pre_existing_on_base";
/// Held because GitHub still reports the exact failure the previous generation was dispatched for.
/// Distinct from `PRE_EXISTING_ON_BASE_REPAIR_REASON`: RalphX has not proven anything about the
/// base branch, only that spending another agent generation on identical evidence is waste.
pub(crate) const UNCHANGED_HEALTH_REPAIR_REASON: &str = "pr_autofix_unchanged_health";
pub(crate) const REPAIR_FINGERPRINT_HOLD_STEP: &str = "repair_fingerprint_hold";
pub(crate) const ORPHANED_REPAIR_DISPATCH_RESCUE_GRACE_SECS: i64 = 60;
const AGENT_WORKSPACE_REPAIR_DISPATCH_INITIAL_BACKOFF_SECS: i64 = 5;
const AGENT_WORKSPACE_REPAIR_DISPATCH_MAX_BACKOFF_SECS: i64 = 60;
const AGENT_WORKSPACE_REPAIR_DISPATCH_DEFERRED_DELAY_SECS: i64 = 15;

#[cfg(any(test, feature = "test-utils"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentWorkspaceRepairClaim {
    pub conversation_id: ChatConversationId,
    pub guard: AgentWorkspaceRepairStateGuard,
}

/// Backend-owned request to join the one durable repair attempt for a workspace.
///
/// `verified_newer_base` is intentionally supplied only by the caller that has already derived
/// the base target from Git. This coordinator never treats a different SHA string as proof that
/// it is safe to move the target forward.
#[derive(Debug, Clone)]
pub(crate) struct AgentWorkspaceRepairStartRequest {
    pub conversation_id: ChatConversationId,
    pub source: AgentWorkspaceRepairSource,
    pub continuation: AgentWorkspaceRepairContinuation,
    pub target_base_ref: String,
    pub target_base_commit: Option<String>,
    pub verified_newer_base: bool,
    pub reason: String,
    pub summary: String,
    pub auto_merge_current: Option<bool>,
    pub explicit_publish_requested: bool,
    pub retry_blocked: bool,
    /// Backend-observed PR evidence carried onto a successor generation. Without it a successor
    /// starts with no failure identity, and every fingerprint-based suppression downstream
    /// silently disengages.
    pub carryover_pr_autofix_evidence: Option<PrAutofixCarryover>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PublishAuthority {
    UserExplicit,
    VerifiedAutomation,
}

/// Exact PR evidence observed by the backend immediately before starting a successor generation.
/// Never model supplied and never copied blindly from the predecessor: a stale head or fingerprint
/// would make the successor look like it had already been evaluated against current GitHub state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PrAutofixCarryover {
    pub dispatch_head_commit: Option<String>,
    pub health_fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentWorkspaceRepairStartOutcome {
    Started(AgentWorkspaceRepairAttempt),
    Joined(AgentWorkspaceRepairAttempt),
    SuccessorStarted(AgentWorkspaceRepairAttempt),
    BlockedByCurrent(AgentWorkspaceRepairAttempt),
}

impl AgentWorkspaceRepairStartOutcome {
    #[cfg(test)]
    pub(crate) fn into_attempt(self) -> AgentWorkspaceRepairAttempt {
        match self {
            Self::Started(attempt)
            | Self::Joined(attempt)
            | Self::SuccessorStarted(attempt)
            | Self::BlockedByCurrent(attempt) => attempt,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentWorkspaceRepairTransitionOutcome {
    Applied(AgentWorkspaceRepairAttempt),
    Stale(AgentWorkspaceRepairAttempt),
    Missing,
}

/// Outcome of routing an ordinary publish request through the current durable repair attempt.
/// A caller must never fall through to the normal publisher after any result other than
/// `NoAttempt`: the repair attempt remains the sole owner of continuation authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentWorkspaceRepairPublishResumeOutcome {
    NoAttempt,
    Continue(Box<AgentWorkspaceRepairAttempt>),
    AwaitingReview,
    Ready,
    Blocked,
    Busy,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentWorkspaceRepairDispatchOutcome {
    Reserved(AgentWorkspaceRepairAttempt),
    Stale(AgentWorkspaceRepairAttempt),
    Missing,
}

/// The durable settlement of a reserved repair delivery. Retry bookkeeping stays backend-owned;
/// callers only classify whether a failed delivery can safely be attempted again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentWorkspaceRepairDispatchSettlement {
    Delivered,
    /// The chat service accepted the delivery but could not start the reserved repair turn yet.
    /// This is capacity pressure, not a failed repair delivery, so it must not spend retry budget.
    DeferredQueued,
    RetryableFailure,
    NonRetryableFailure,
}

/// Classifies the immediate delivery acknowledgement at the durable repair boundary. A failed
/// acknowledgement must remain retryable unless the typed chat-service contract proves that the
/// same reserved repair can never start without changing configuration or trusted context.
pub(crate) fn classify_agent_workspace_repair_delivery(
    delivery: Result<&SendResult, &ChatServiceError>,
    conversation_id: &ChatConversationId,
    run_id: &AgentRunId,
) -> AgentWorkspaceRepairDispatchSettlement {
    match delivery {
        Ok(result) if result.was_queued || result.queued_as_pending => {
            AgentWorkspaceRepairDispatchSettlement::DeferredQueued
        }
        Ok(result)
            if !result.was_queued
                && !result.queued_as_pending
                && result.conversation_id == conversation_id.as_str()
                && result.agent_run_id == run_id.as_str() =>
        {
            AgentWorkspaceRepairDispatchSettlement::Delivered
        }
        Ok(_) => AgentWorkspaceRepairDispatchSettlement::RetryableFailure,
        Err(
            ChatServiceError::InvalidInput(_)
            | ChatServiceError::AgentNotAvailable(_)
            | ChatServiceError::SpawnValidation { .. }
            | ChatServiceError::ParseError(_)
            | ChatServiceError::ContextNotFound(_)
            | ChatServiceError::ConversationNotFound(_)
            | ChatServiceError::PersonaUnavailable(_),
        ) => AgentWorkspaceRepairDispatchSettlement::NonRetryableFailure,
        Err(ChatServiceError::MessageDeliveredNotPersisted(_)) => {
            AgentWorkspaceRepairDispatchSettlement::Delivered
        }
        Err(ChatServiceError::ImmediateStartRejected(_)) => {
            AgentWorkspaceRepairDispatchSettlement::DeferredQueued
        }
        Err(
            ChatServiceError::SpawnFailed(_)
            | ChatServiceError::CommunicationFailed(_)
            | ChatServiceError::RepositoryError(_)
            | ChatServiceError::AgentRunFailed(_),
        ) => AgentWorkspaceRepairDispatchSettlement::RetryableFailure,
    }
}

pub(crate) fn agent_workspace_repair_dispatch_retry_delay(dispatch_count: u32) -> Duration {
    let multiplier = 1_i64 << dispatch_count.saturating_sub(1).min(3);
    Duration::seconds(
        AGENT_WORKSPACE_REPAIR_DISPATCH_INITIAL_BACKOFF_SECS
            .saturating_mul(multiplier)
            .min(AGENT_WORKSPACE_REPAIR_DISPATCH_MAX_BACKOFF_SECS),
    )
}

pub(crate) fn agent_workspace_repair_dispatch_is_due(
    attempt: &AgentWorkspaceRepairAttempt,
    now: DateTime<Utc>,
) -> bool {
    attempt.next_dispatch_at.is_none_or(|due_at| due_at <= now)
}

impl AgentWorkspaceRepairTransitionOutcome {
    #[cfg(test)]
    pub(crate) fn is_stale(&self) -> bool {
        matches!(self, Self::Stale(_))
    }
}

fn repair_attempt_projection(
    attempt: &AgentWorkspaceRepairAttempt,
    summary: &str,
    auto_merge_current: Option<bool>,
) -> AgentWorkspaceRepairCompatibilityProjection {
    let (publication_push_status, pr_supervision_status) = match attempt.phase {
        AgentWorkspaceRepairPhase::Requested
        | AgentWorkspaceRepairPhase::Dispatching
        | AgentWorkspaceRepairPhase::Repairing
        | AgentWorkspaceRepairPhase::Validating => ("needs_agent", "fixing"),
        AgentWorkspaceRepairPhase::AwaitingReview => ("refreshed", "reviewing"),
        AgentWorkspaceRepairPhase::ContinuationPending | AgentWorkspaceRepairPhase::Continuing => {
            ("refreshed", "publishing")
        }
        AgentWorkspaceRepairPhase::Ready => ("refreshed", "paused"),
        AgentWorkspaceRepairPhase::Blocked => ("failed", "blocked"),
    };
    AgentWorkspaceRepairCompatibilityProjection {
        publication_push_status: Some(publication_push_status.to_string()),
        pr_supervision_status: Some(pr_supervision_status.to_string()),
        pr_supervision_summary: Some(summary.to_string()),
        pr_supervision_updated_at: Some(attempt.updated_at),
        pr_auto_merge_current: auto_merge_current,
        // Compatibility projection must retain the Git-verified repair base. Clearing it while
        // a continuation is still active makes the shared PR publisher fail closed before its
        // create/update handoff can run.
        base_commit: attempt.target_base_commit.clone(),
    }
}

fn start_attempt_from_workspace(
    workspace: &AgentConversationWorkspace,
    request: &AgentWorkspaceRepairStartRequest,
) -> AgentWorkspaceRepairAttempt {
    let now = Utc::now();
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        request.conversation_id.clone(),
        request.source,
        request.continuation,
        request.target_base_ref.clone(),
        false,
        workspace.auto_publish_enabled,
        workspace.pr_auto_merge_desired,
        Some(workspace.pr_auto_merge_method.clone()),
        now,
    );
    attempt.target_base_commit = request.target_base_commit.clone();
    attempt.summary = Some(request.summary.clone());
    attempt.explicit_publish_requested = request.explicit_publish_requested;
    if let Some(carryover) = request.carryover_pr_autofix_evidence.as_ref() {
        attempt.pr_autofix_dispatch_head_commit = carryover.dispatch_head_commit.clone();
        attempt.pr_autofix_health_fingerprint = carryover.health_fingerprint.clone();
    }
    attempt
}

fn repair_attempt_transition_outcome(
    outcome: AgentWorkspaceRepairAttemptTransitionOutcome,
) -> AgentWorkspaceRepairTransitionOutcome {
    match outcome {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => {
            AgentWorkspaceRepairTransitionOutcome::Applied(attempt)
        }
        AgentWorkspaceRepairAttemptTransitionOutcome::Stale(attempt) => {
            AgentWorkspaceRepairTransitionOutcome::Stale(attempt)
        }
        AgentWorkspaceRepairAttemptTransitionOutcome::Missing => {
            AgentWorkspaceRepairTransitionOutcome::Missing
        }
    }
}

/// Starts exactly one repair generation or joins its existing owner. Joining never replaces the
/// reserved run; continuation upgrades, reasons, verified base advancement, and preferences are
/// CAS-updated on that same durable generation.
pub(crate) async fn start_or_join_agent_workspace_repair(
    repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    request: AgentWorkspaceRepairStartRequest,
) -> AppResult<AgentWorkspaceRepairStartOutcome> {
    start_or_join_agent_workspace_repair_with_projection(repair_repo, workspace_repo, request, true)
        .await
}

/// Starts or joins a durable repair before the caller has acquired the Git target lease. The
/// caller must reserve dispatch before it projects compatibility state or invokes external
/// effects, so a foreign target owner cannot make a stale poll look like an active repair.
pub(crate) async fn start_or_join_agent_workspace_repair_without_projection(
    repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    request: AgentWorkspaceRepairStartRequest,
) -> AppResult<AgentWorkspaceRepairStartOutcome> {
    start_or_join_agent_workspace_repair_with_projection(
        repair_repo,
        workspace_repo,
        request,
        false,
    )
    .await
}

async fn start_or_join_agent_workspace_repair_with_projection(
    repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    request: AgentWorkspaceRepairStartRequest,
    project_compatibility_state: bool,
) -> AppResult<AgentWorkspaceRepairStartOutcome> {
    let workspace = workspace_repo
        .get_by_conversation_id(&request.conversation_id)
        .await?
        .ok_or_else(|| {
            crate::error::AppError::NotFound(format!(
                "workspace {} for repair attempt",
                request.conversation_id
            ))
        })?;
    let attempt = start_attempt_from_workspace(&workspace, &request);
    let projection = project_compatibility_state.then(|| {
        repair_attempt_projection(
            &attempt,
            &request.summary,
            request
                .auto_merge_current
                .or(workspace.pr_auto_merge_current),
        )
    });
    let outcome = repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt,
            reason: request.reason.clone(),
            verified_newer_base: request.verified_newer_base,
            compatibility_projection: projection,
            events: Vec::new(),
        })
        .await?;

    match outcome {
        StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(started) => {
            Ok(AgentWorkspaceRepairStartOutcome::Started(started))
        }
        StartOrJoinAgentWorkspaceRepairAttemptOutcome::Joined(current) => {
            if request.retry_blocked && current.phase == AgentWorkspaceRepairPhase::Blocked {
                retry_blocked_agent_workspace_repair(repair_repo, workspace_repo, request, current)
                    .await
            } else {
                Ok(AgentWorkspaceRepairStartOutcome::Joined(current))
            }
        }
        StartOrJoinAgentWorkspaceRepairAttemptOutcome::BlockedByCurrent(current) => {
            // A current attempt with a different target base ref refuses joins, but a blocked
            // generation must still be retryable when the caller explicitly retargets it —
            // otherwise a workspace whose base moved (for example its base PR merged) can never
            // supersede the drifted blocked repair.
            if request.retry_blocked && current.phase == AgentWorkspaceRepairPhase::Blocked {
                retry_blocked_agent_workspace_repair(repair_repo, workspace_repo, request, current)
                    .await
            } else {
                Ok(AgentWorkspaceRepairStartOutcome::BlockedByCurrent(current))
            }
        }
    }
}

async fn retry_blocked_agent_workspace_repair(
    repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    request: AgentWorkspaceRepairStartRequest,
    blocked: AgentWorkspaceRepairAttempt,
) -> AppResult<AgentWorkspaceRepairStartOutcome> {
    let workspace = workspace_repo
        .get_by_conversation_id(&request.conversation_id)
        .await?
        .ok_or_else(|| {
            crate::error::AppError::NotFound(format!(
                "workspace {} for blocked repair retry",
                request.conversation_id
            ))
        })?;
    let successor = start_attempt_from_workspace(&workspace, &request);
    let successor_projection = repair_attempt_projection(
        &successor,
        &request.summary,
        request
            .auto_merge_current
            .or(workspace.pr_auto_merge_current),
    );
    let now = next_transition_at(Some(blocked.updated_at));
    let result = repair_repo
        .settle_and_start_repair_successor(
            crate::domain::repositories::SettleAndStartAgentWorkspaceRepairSuccessor {
                attempt_id: blocked.id.clone(),
                generation: blocked.generation,
                expected_phase: AgentWorkspaceRepairPhase::Blocked,
                expected_updated_at: blocked.updated_at,
                outcome: crate::domain::entities::AgentWorkspaceRepairOutcome::Superseded,
                settled_at: now,
                successor: StartOrJoinAgentWorkspaceRepairAttempt {
                    attempt: successor,
                    reason: request.reason,
                    verified_newer_base: request.verified_newer_base,
                    compatibility_projection: Some(successor_projection),
                    events: Vec::new(),
                },
                compatibility_projection: None,
                events: Vec::new(),
            },
        )
        .await?;
    match result {
        crate::domain::repositories::SettleAndStartAgentWorkspaceRepairSuccessorOutcome::Started(
            successor,
        ) => Ok(AgentWorkspaceRepairStartOutcome::SuccessorStarted(successor)),
        crate::domain::repositories::SettleAndStartAgentWorkspaceRepairSuccessorOutcome::Stale(
            current,
        ) => Ok(AgentWorkspaceRepairStartOutcome::Joined(current)),
        crate::domain::repositories::SettleAndStartAgentWorkspaceRepairSuccessorOutcome::Missing => {
            Ok(AgentWorkspaceRepairStartOutcome::BlockedByCurrent(blocked))
        }
    }
}

/// Applies a phase transition only when the exact durable generation is still in the caller's
/// expected phase. Stale callers get the current attempt and no compatibility/audit side effect.
pub(crate) async fn transition_agent_workspace_repair_attempt(
    repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
    mut attempt: AgentWorkspaceRepairAttempt,
    expected_phase: AgentWorkspaceRepairPhase,
    summary: &str,
    auto_merge_current: Option<bool>,
) -> AppResult<AgentWorkspaceRepairTransitionOutcome> {
    let expected_updated_at = attempt.updated_at;
    attempt.phase = expected_phase;
    attempt.summary = Some(summary.to_string());
    attempt.updated_at = next_transition_at(Some(attempt.updated_at));
    let projection = repair_attempt_projection(&attempt, summary, auto_merge_current);
    repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase,
            expected_updated_at,
            next_phase: expected_phase,
            compatibility_projection: Some(projection),
            events: Vec::new(),
        })
        .await
        .map(repair_attempt_transition_outcome)
}

/// Atomically records an actionable blocker for the exact trusted repair generation. Callers must
/// classify completion authority first; a stale result writes neither compatibility state nor an
/// audit event.
pub(crate) async fn block_agent_workspace_repair_completion(
    repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
    branch_update_repo: Arc<dyn BranchUpdateRepository>,
    mut attempt: AgentWorkspaceRepairAttempt,
    summary: &str,
    blocker: &str,
    auto_merge_current: Option<bool>,
) -> AppResult<AgentWorkspaceRepairTransitionOutcome> {
    let expected_phase = attempt.phase;
    let expected_updated_at = attempt.updated_at;
    attempt.phase = AgentWorkspaceRepairPhase::Blocked;
    attempt.summary = Some(summary.to_string());
    attempt.blocker = Some(blocker.to_string());
    attempt.updated_at = next_transition_at(Some(attempt.updated_at));
    let projection = repair_attempt_projection(&attempt, blocker, auto_merge_current);
    let outcome = repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Blocked,
            compatibility_projection: Some(projection),
            events: Vec::new(),
        })
        .await
        .map(repair_attempt_transition_outcome)?;
    match outcome {
        AgentWorkspaceRepairTransitionOutcome::Applied(attempt) => {
            release_and_clear_agent_workspace_repair_target_lease(
                repair_repo.as_ref(),
                branch_update_repo.as_ref(),
                attempt,
            )
            .await
        }
        outcome => Ok(outcome),
    }
}

/// A typed PR-fixer escalation is terminal for automatic recovery. The marker is persisted on
/// the current generation rather than inferred from agent-authored summary text.
pub(crate) async fn block_agent_workspace_repair_needs_human(
    repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
    branch_update_repo: Arc<dyn BranchUpdateRepository>,
    mut attempt: AgentWorkspaceRepairAttempt,
    summary: &str,
    auto_merge_current: Option<bool>,
) -> AppResult<AgentWorkspaceRepairTransitionOutcome> {
    if !attempt
        .pending_reasons
        .iter()
        .any(|reason| reason == NEEDS_HUMAN_REPAIR_REASON)
    {
        attempt
            .pending_reasons
            .push(NEEDS_HUMAN_REPAIR_REASON.to_string());
    }
    block_agent_workspace_repair_completion(
        repair_repo,
        branch_update_repo,
        attempt,
        summary,
        summary,
        auto_merge_current,
    )
    .await
}

/// True while a PR autofix generation is parked against a backend-derived health fingerprint.
/// Only the poller may end such a hold, and only after GitHub reports different health; no other
/// retry path may consume the hold's budget or settle it on a timer.
pub(crate) fn agent_workspace_repair_is_health_held(attempt: &AgentWorkspaceRepairAttempt) -> bool {
    attempt.pending_reasons.iter().any(|reason| {
        reason == PRE_EXISTING_ON_BASE_REPAIR_REASON || reason == UNCHANGED_HEALTH_REPAIR_REASON
    })
}

/// Holds a PR autofix generation at a backend-derived health fingerprint without pretending the
/// failing state was repaired. The poller settles it only after health changes.
pub(crate) async fn reserve_agent_workspace_pre_existing_on_base(
    repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
    attempt: AgentWorkspaceRepairAttempt,
    summary: &str,
    auto_merge_current: Option<bool>,
) -> AppResult<AgentWorkspaceRepairTransitionOutcome> {
    reserve_agent_workspace_repair_health_hold(
        repair_repo,
        attempt,
        PRE_EXISTING_ON_BASE_REPAIR_REASON,
        summary,
        auto_merge_current,
        Vec::new(),
    )
    .await
}

/// Parks the current PR autofix generation because GitHub still reports the exact failure it was
/// dispatched for. Spending another agent generation on unchanged evidence cannot produce new
/// information, so the hold replaces the successor rather than delaying it.
pub(crate) async fn reserve_agent_workspace_unchanged_health_hold(
    repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
    attempt: AgentWorkspaceRepairAttempt,
    summary: &str,
    auto_merge_current: Option<bool>,
) -> AppResult<AgentWorkspaceRepairTransitionOutcome> {
    let event = AgentConversationWorkspacePublicationEvent::new(
        attempt.conversation_id.clone(),
        REPAIR_FINGERPRINT_HOLD_STEP,
        "blocked",
        summary,
        attempt.pr_autofix_health_fingerprint.clone(),
    );
    reserve_agent_workspace_repair_health_hold(
        repair_repo,
        attempt,
        UNCHANGED_HEALTH_REPAIR_REASON,
        summary,
        auto_merge_current,
        vec![event],
    )
    .await
}

async fn reserve_agent_workspace_repair_health_hold(
    repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
    mut attempt: AgentWorkspaceRepairAttempt,
    hold_reason: &str,
    summary: &str,
    auto_merge_current: Option<bool>,
    events: Vec<AgentConversationWorkspacePublicationEvent>,
) -> AppResult<AgentWorkspaceRepairTransitionOutcome> {
    // A hold is only meaningful against an exact observed failure identity. Without one there is
    // nothing for the poller to compare later, so refuse rather than park indefinitely.
    let Some(_) = attempt.pr_autofix_health_fingerprint.as_deref() else {
        return Ok(AgentWorkspaceRepairTransitionOutcome::Stale(attempt));
    };
    let expected_phase = attempt.phase;
    let expected_updated_at = attempt.updated_at;
    if !attempt
        .pending_reasons
        .iter()
        .any(|reason| reason == hold_reason)
    {
        attempt.pending_reasons.push(hold_reason.to_string());
    }
    attempt.phase = AgentWorkspaceRepairPhase::Ready;
    attempt.summary = Some(summary.to_string());
    attempt.blocker = None;
    attempt.updated_at = next_transition_at(Some(expected_updated_at));
    let projection = repair_attempt_projection(&attempt, summary, auto_merge_current);
    repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Ready,
            compatibility_projection: Some(projection),
            events,
        })
        .await
        .map(repair_attempt_transition_outcome)
}

/// CAS-reserve a GitHub Actions rerun after the completion boundary has authenticated the
/// current repair attempt. The caller invokes `gh` only after this write succeeds.
pub(crate) async fn reserve_agent_workspace_ci_rerun(
    repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
    mut attempt: AgentWorkspaceRepairAttempt,
    fingerprint: &str,
    summary: &str,
    auto_merge_current: Option<bool>,
) -> AppResult<AgentWorkspaceRepairTransitionOutcome> {
    if attempt.ci_rerun_count >= MAX_AGENT_WORKSPACE_CI_RERUN_RETRIES {
        return Ok(AgentWorkspaceRepairTransitionOutcome::Stale(attempt));
    }
    let expected_phase = attempt.phase;
    let expected_updated_at = attempt.updated_at;
    attempt.ci_rerun_count += 1;
    attempt.ci_rerun_fingerprint = Some(fingerprint.to_string());
    // Ready is deliberately non-terminal: startup/recovery sees a settled boundary, while the
    // poller owns observation of the next CI conclusion rather than replaying this agent run.
    attempt.phase = AgentWorkspaceRepairPhase::Ready;
    attempt.summary = Some(summary.to_string());
    attempt.blocker = None;
    attempt.updated_at = next_transition_at(Some(expected_updated_at));
    let projection = repair_attempt_projection(&attempt, summary, auto_merge_current);
    repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Ready,
            compatibility_projection: Some(projection),
            events: Vec::new(),
        })
        .await
        .map(repair_attempt_transition_outcome)
}

/// Persists Git facts derived by the backend after the trusted run has passed authority checks.
/// The completion handler reserves the exact attempt into `Validating` before any Git inspection;
/// this transition only records facts against that reservation.
pub(crate) async fn record_agent_workspace_repair_validation(
    repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
    mut attempt: AgentWorkspaceRepairAttempt,
    base_ref: &str,
    base_commit: &str,
    repair_head_commit: &str,
    summary: &str,
    auto_merge_current: Option<bool>,
) -> AppResult<AgentWorkspaceRepairTransitionOutcome> {
    let expected_phase = attempt.phase;
    let expected_updated_at = attempt.updated_at;
    attempt.phase = AgentWorkspaceRepairPhase::Validating;
    attempt.target_base_ref = base_ref.to_string();
    attempt.target_base_commit = Some(base_commit.to_string());
    attempt.repair_head_commit = Some(repair_head_commit.to_string());
    attempt.summary = Some(summary.to_string());
    // A successfully validated completion supersedes any stale blocker left by an earlier
    // blocked generation state (for example a resurrected exact-run completion).
    attempt.blocker = None;
    attempt.updated_at = next_transition_at(Some(attempt.updated_at));
    let projection = repair_attempt_projection(&attempt, summary, auto_merge_current);
    repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Validating,
            compatibility_projection: Some(projection),
            events: Vec::new(),
        })
        .await
        .map(repair_attempt_transition_outcome)
}

/// Backend-derived Git facts for an exact repair-validation reservation. Both trusted completion
/// and crash recovery use this one canonical workspace inspection before continuing the attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentWorkspaceRepairValidationFacts {
    pub(crate) base_ref: String,
    pub(crate) base_commit: String,
    pub(crate) repair_head_commit: String,
    pub(crate) auto_merge_current: Option<bool>,
}

pub(crate) async fn inspect_agent_workspace_repair_completion(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    target_base_ref: &str,
    target_base_commit: Option<&str>,
) -> AppResult<AgentWorkspaceRepairValidationFacts> {
    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Project not found".to_string()))?;
    let linked_ideation_plan_workspace = workspace.mode == AgentConversationWorkspaceMode::Ideation
        && workspace.linked_plan_branch_id.is_some();
    if workspace.is_execution_owned() && !linked_ideation_plan_workspace {
        return Err(AppError::Validation(
            "execution-owned workspaces cannot use direct repair publication".to_string(),
        ));
    }
    if workspace.mode == AgentConversationWorkspaceMode::Ideation
        && workspace.linked_plan_branch_id.is_none()
    {
        return Err(AppError::Validation(
            "ideation workspaces without a linked plan branch cannot use repair publication"
                .to_string(),
        ));
    }
    let resolved = resolve_effective_agent_conversation_workspace_path(
        &project,
        workspace,
        state.plan_branch_repo.as_ref(),
    )
    .await?;
    let checked_out = GitService::get_current_branch(&resolved.path).await?;
    if checked_out != resolved.branch_name {
        return Err(AppError::Validation(format!(
            "workspace repair validation expected branch '{}' but found '{}'",
            resolved.branch_name, checked_out
        )));
    }
    if target_base_ref.trim().is_empty() {
        return Err(AppError::Validation(
            "workspace repair validation has no durable target base ref".to_string(),
        ));
    }

    let freshness = inspect_publish_branch_freshness_for_source(
        &resolved.path,
        target_base_ref,
        &resolved.branch_name,
        target_base_commit,
    )
    .await?;
    let workspace_head_sha =
        GitService::get_branch_sha(&resolved.path, &resolved.branch_name).await?;
    let has_uncommitted_changes = GitService::has_uncommitted_changes(&resolved.path).await?;
    let unfinished_operation = GitService::unfinished_operation_state(&resolved.path)?;
    let has_conflict_markers = GitService::has_conflict_markers(&resolved.path).await?;
    let has_conflict_files = !GitService::get_conflict_files(&resolved.path)
        .await?
        .is_empty();

    if unfinished_operation.is_unfinished() {
        return Err(AppError::Conflict(
            "workspace repair has an unfinished merge, rebase, cherry-pick, or revert".to_string(),
        ));
    }
    verify_agent_workspace_repair_completion(AgentWorkspaceRepairCompletionCheck {
        freshness_status: &freshness,
        workspace_base_ref: target_base_ref,
        resolved_base_ref: target_base_ref,
        resolved_base_commit: &freshness.target_base_commit,
        repair_commit_sha: &workspace_head_sha,
        workspace_head_sha: &workspace_head_sha,
        has_uncommitted_changes,
        is_merge_in_progress: false,
        is_rebase_in_progress: false,
        has_conflict_files,
        has_conflict_markers,
    })
    .map_err(AppError::Conflict)?;

    Ok(AgentWorkspaceRepairValidationFacts {
        base_ref: target_base_ref.to_string(),
        base_commit: freshness.target_base_commit,
        repair_head_commit: workspace_head_sha,
        auto_merge_current: workspace.pr_auto_merge_current,
    })
}

/// Atomically moves an authorized repair generation into the validation-owned phase before the
/// completion handler resolves a target or inspects Git. The `updated_at` guard fences a same
/// generation join as well as a successor, so callers must treat `Stale` and `Missing` as a
/// no-Git return path.
pub(crate) async fn reserve_agent_workspace_repair_completion_validation(
    repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
    mut attempt: AgentWorkspaceRepairAttempt,
    auto_merge_current: Option<bool>,
) -> AppResult<AgentWorkspaceRepairTransitionOutcome> {
    let expected_phase = attempt.phase;
    let expected_updated_at = attempt.updated_at;
    attempt.phase = AgentWorkspaceRepairPhase::Validating;
    attempt.updated_at = next_transition_at(Some(attempt.updated_at));
    let projection_summary = attempt
        .summary
        .clone()
        .unwrap_or_else(|| "Validating workspace repair completion.".to_string());
    let projection = repair_attempt_projection(&attempt, &projection_summary, auto_merge_current);
    repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Validating,
            compatibility_projection: Some(projection),
            events: Vec::new(),
        })
        .await
        .map(repair_attempt_transition_outcome)
}

/// Reopens a validation-owned repair after backend Git inspection rejects the completion. This
/// restores the repair agent's retryable phase instead of leaving an unverified reservation
/// looking like an accepted completion.
pub(crate) async fn reopen_agent_workspace_repair_after_validation_failure(
    repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
    mut attempt: AgentWorkspaceRepairAttempt,
    auto_merge_current: Option<bool>,
) -> AppResult<AgentWorkspaceRepairTransitionOutcome> {
    let expected_updated_at = attempt.updated_at;
    attempt.phase = AgentWorkspaceRepairPhase::Repairing;
    attempt.updated_at = next_transition_at(Some(attempt.updated_at));
    let projection_summary = attempt
        .summary
        .clone()
        .unwrap_or_else(|| "Workspace repair needs another validation attempt.".to_string());
    let projection = repair_attempt_projection(&attempt, &projection_summary, auto_merge_current);
    repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase: AgentWorkspaceRepairPhase::Validating,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Repairing,
            compatibility_projection: Some(projection),
            events: Vec::new(),
        })
        .await
        .map(repair_attempt_transition_outcome)
}

fn repair_requested_event_classification(
    continuation: AgentWorkspaceRepairContinuation,
) -> &'static str {
    match continuation {
        AgentWorkspaceRepairContinuation::Manual | AgentWorkspaceRepairContinuation::UpdateOnly => {
            "agent_fixable:update_only"
        }
        AgentWorkspaceRepairContinuation::Publish
        | AgentWorkspaceRepairContinuation::ResumePrSupervision => "agent_fixable:publish",
    }
}

fn repair_transition_events(
    attempt: &AgentWorkspaceRepairAttempt,
    run_id: &AgentRunId,
    summary: &str,
) -> Vec<AgentConversationWorkspacePublicationEvent> {
    vec![
        AgentConversationWorkspacePublicationEvent::new(
            attempt.conversation_id.clone(),
            REPAIR_REQUESTED_STEP,
            "started",
            summary,
            Some(repair_requested_event_classification(attempt.continuation).to_string()),
        ),
        AgentConversationWorkspacePublicationEvent::new(
            attempt.conversation_id.clone(),
            REPAIR_SENT_STEP,
            "started",
            "Starting the durable workspace repair attempt.",
            Some(repair_run_event_classification(run_id)),
        ),
    ]
}

/// Reserves the trusted agent run and moves a newly-created attempt to dispatching before the
/// caller invokes the chat service. The canonical target lease is acquired and its exact epoch is
/// checkpointed on the durable generation before the run can be bound; a stale or already-reserved
/// generation never reaches send.
pub(crate) async fn reserve_agent_workspace_repair_dispatch(
    repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
    branch_update_repo: Arc<dyn BranchUpdateRepository>,
    target_identity: GitTargetIdentity,
    attempt: AgentWorkspaceRepairAttempt,
    run_id: AgentRunId,
    summary: &str,
    auto_merge_current: Option<bool>,
) -> AppResult<AgentWorkspaceRepairDispatchOutcome> {
    if !agent_workspace_repair_dispatch_is_due(&attempt, Utc::now()) {
        return Ok(AgentWorkspaceRepairDispatchOutcome::Stale(attempt));
    }
    if repair_repo
        .get_open_repair_effect(&attempt.id)
        .await?
        .is_some()
    {
        return Err(AppError::Conflict(
            "workspace repair dispatch cannot replace an active Git effect".to_string(),
        ));
    }
    let owner = GitTargetLeaseOwner::agent_workspace_repair(attempt.id.as_str());
    let (fencing_epoch, lease_acquired_here) = match branch_update_repo
        .acquire_target_lease(AcquireGitTargetLease {
            identity: target_identity.clone(),
            owner: owner.clone(),
        })
        .await?
    {
        AcquireGitTargetLeaseOutcome::Acquired { fencing_epoch } => (fencing_epoch, true),
        AcquireGitTargetLeaseOutcome::AlreadyOwned { fencing_epoch } => (fencing_epoch, false),
        AcquireGitTargetLeaseOutcome::TargetBusy {
            owner: active_owner,
            fencing_epoch,
        } => {
            return Err(AppError::Conflict(format!(
                "workspace repair dispatch target is owned by {:?} at fencing epoch {fencing_epoch}",
                active_owner
            )));
        }
    };

    let checkpointed = checkpoint_agent_workspace_repair_target_lease(
        repair_repo.as_ref(),
        attempt,
        &target_identity,
        fencing_epoch,
        AgentWorkspaceRepairPhase::Requested,
    )
    .await;
    let attempt = match checkpointed {
        Ok(AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt)) => attempt,
        Ok(AgentWorkspaceRepairAttemptTransitionOutcome::Stale(attempt)) => {
            release_new_agent_workspace_repair_dispatch_lease(
                branch_update_repo.as_ref(),
                &target_identity,
                &owner,
                fencing_epoch,
                lease_acquired_here,
            )
            .await?;
            return Ok(AgentWorkspaceRepairDispatchOutcome::Stale(attempt));
        }
        Ok(AgentWorkspaceRepairAttemptTransitionOutcome::Missing) => {
            release_new_agent_workspace_repair_dispatch_lease(
                branch_update_repo.as_ref(),
                &target_identity,
                &owner,
                fencing_epoch,
                lease_acquired_here,
            )
            .await?;
            return Ok(AgentWorkspaceRepairDispatchOutcome::Missing);
        }
        Err(error) => {
            release_new_agent_workspace_repair_dispatch_lease(
                branch_update_repo.as_ref(),
                &target_identity,
                &owner,
                fencing_epoch,
                lease_acquired_here,
            )
            .await?;
            return Err(error);
        }
    };
    let bound = repair_repo
        .bind_repair_attempt_run(BindAgentWorkspaceRepairAttemptRun {
            attempt_id: attempt.id.clone(),
            generation: attempt.generation,
            expected_phase: AgentWorkspaceRepairPhase::Requested,
            expected_updated_at: attempt.updated_at,
            run_id: run_id.clone(),
            updated_at: next_transition_at(Some(attempt.updated_at)),
        })
        .await?;
    let bound = match bound {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        AgentWorkspaceRepairAttemptTransitionOutcome::Stale(attempt) => {
            release_new_agent_workspace_repair_dispatch_lease(
                branch_update_repo.as_ref(),
                &target_identity,
                &owner,
                fencing_epoch,
                lease_acquired_here,
            )
            .await?;
            return Ok(AgentWorkspaceRepairDispatchOutcome::Stale(attempt));
        }
        AgentWorkspaceRepairAttemptTransitionOutcome::Missing => {
            release_new_agent_workspace_repair_dispatch_lease(
                branch_update_repo.as_ref(),
                &target_identity,
                &owner,
                fencing_epoch,
                lease_acquired_here,
            )
            .await?;
            return Ok(AgentWorkspaceRepairDispatchOutcome::Missing);
        }
    };
    let mut dispatching = bound.clone();
    let expected_updated_at = bound.updated_at;
    dispatching.phase = AgentWorkspaceRepairPhase::Dispatching;
    dispatching.summary = Some(summary.to_string());
    dispatching.blocker = None;
    dispatching.next_dispatch_at = None;
    dispatching.updated_at = next_transition_at(Some(bound.updated_at));
    let outcome = repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: dispatching.clone(),
            expected_phase: AgentWorkspaceRepairPhase::Requested,
            expected_updated_at,
            next_phase: AgentWorkspaceRepairPhase::Dispatching,
            compatibility_projection: Some(repair_attempt_projection(
                &dispatching,
                summary,
                auto_merge_current,
            )),
            events: repair_transition_events(&dispatching, &run_id, summary),
        })
        .await;
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(error) => {
            let blocker =
                format!("Workspace repair dispatch audit could not be persisted: {error}");
            let blocked = block_agent_workspace_repair_completion(
                Arc::clone(&repair_repo),
                Arc::clone(&branch_update_repo),
                bound,
                "Workspace repair dispatch was blocked before delivery.",
                &blocker,
                auto_merge_current,
            )
            .await;
            if !matches!(
                blocked,
                Ok(AgentWorkspaceRepairTransitionOutcome::Applied(_))
            ) {
                release_new_agent_workspace_repair_dispatch_lease(
                    branch_update_repo.as_ref(),
                    &target_identity,
                    &owner,
                    fencing_epoch,
                    lease_acquired_here,
                )
                .await?;
            }
            return Err(error);
        }
    };
    match outcome {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => {
            Ok(AgentWorkspaceRepairDispatchOutcome::Reserved(attempt))
        }
        AgentWorkspaceRepairAttemptTransitionOutcome::Stale(attempt) => {
            release_new_agent_workspace_repair_dispatch_lease(
                branch_update_repo.as_ref(),
                &target_identity,
                &owner,
                fencing_epoch,
                lease_acquired_here,
            )
            .await?;
            Ok(AgentWorkspaceRepairDispatchOutcome::Stale(attempt))
        }
        AgentWorkspaceRepairAttemptTransitionOutcome::Missing => {
            release_new_agent_workspace_repair_dispatch_lease(
                branch_update_repo.as_ref(),
                &target_identity,
                &owner,
                fencing_epoch,
                lease_acquired_here,
            )
            .await?;
            Ok(AgentWorkspaceRepairDispatchOutcome::Missing)
        }
    }
}

/// Checkpoints exact canonical target authority on the durable attempt with a same-phase CAS.
/// Dispatch and resumed continuations share this seam so a stale snapshot cannot reuse an old
/// epoch after a wait boundary has released it.
async fn checkpoint_agent_workspace_repair_target_lease(
    repair_repo: &dyn AgentWorkspaceRepairRepository,
    mut attempt: AgentWorkspaceRepairAttempt,
    target_identity: &GitTargetIdentity,
    fencing_epoch: u64,
    expected_phase: AgentWorkspaceRepairPhase,
) -> AppResult<AgentWorkspaceRepairAttemptTransitionOutcome> {
    let common_dir = target_identity
        .git_common_dir()
        .to_string_lossy()
        .to_string();
    let target_ref = target_identity.full_ref().to_string();
    if attempt.git_common_dir.is_some()
        || attempt.target_ref.is_some()
        || attempt.target_identity_version.is_some()
        || attempt.target_lease_epoch.is_some()
    {
        if attempt.git_common_dir.as_deref() == Some(common_dir.as_str())
            && attempt.target_ref.as_deref() == Some(target_ref.as_str())
            && attempt.target_identity_version
                == Some(AGENT_WORKSPACE_REPAIR_TARGET_IDENTITY_VERSION)
            && attempt.target_lease_epoch == Some(fencing_epoch)
        {
            return repair_repo
                .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
                    expected_phase,
                    expected_updated_at: attempt.updated_at,
                    next_phase: expected_phase,
                    attempt,
                    compatibility_projection: None,
                    events: Vec::new(),
                })
                .await;
        }
        return Err(AppError::Conflict(
            "workspace repair dispatch target lease does not match its durable generation"
                .to_string(),
        ));
    }
    let expected_updated_at = attempt.updated_at;
    attempt.git_common_dir = Some(common_dir);
    attempt.target_ref = Some(target_ref);
    attempt.target_identity_version = Some(AGENT_WORKSPACE_REPAIR_TARGET_IDENTITY_VERSION);
    attempt.target_lease_epoch = Some(fencing_epoch);
    attempt.updated_at = next_transition_at(Some(attempt.updated_at));
    repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase,
            expected_updated_at,
            next_phase: expected_phase,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
}

async fn release_new_agent_workspace_repair_dispatch_lease(
    branch_update_repo: &dyn BranchUpdateRepository,
    target_identity: &GitTargetIdentity,
    owner: &GitTargetLeaseOwner,
    fencing_epoch: u64,
    lease_acquired_here: bool,
) -> AppResult<()> {
    if !lease_acquired_here {
        return Ok(());
    }
    match branch_update_repo
        .release_target_lease(target_identity, owner, fencing_epoch)
        .await?
    {
        GitAuthorityCasOutcome::Applied { .. } | GitAuthorityCasOutcome::StaleAuthority => Ok(()),
        outcome => Err(AppError::Conflict(format!(
            "workspace repair dispatch could not release its uncheckpointed Git target lease: {outcome:?}"
        ))),
    }
}

/// Proves that this exact durable generation still owns its dispatch-acquired canonical target
/// lease. Call this before completion validation or any repair-owned Git/GitHub effect.
pub(crate) async fn validate_agent_workspace_repair_target_lease(
    branch_update_repo: &dyn BranchUpdateRepository,
    attempt: &AgentWorkspaceRepairAttempt,
) -> AppResult<GitTargetIdentity> {
    let (Some(common_dir), Some(target_ref), Some(epoch)) = (
        attempt.git_common_dir.as_deref(),
        attempt.target_ref.as_deref(),
        attempt.target_lease_epoch,
    ) else {
        return Err(AppError::Conflict(
            "workspace repair generation has no durable canonical target lease".to_string(),
        ));
    };
    if attempt.target_identity_version != Some(AGENT_WORKSPACE_REPAIR_TARGET_IDENTITY_VERSION) {
        return Err(AppError::Conflict(
            "workspace repair generation has an unsupported canonical target identity".to_string(),
        ));
    }
    let identity = GitTargetIdentity::new(std::path::PathBuf::from(common_dir), target_ref)
        .map_err(|error| {
            AppError::Validation(format!("invalid durable repair lease identity: {error}"))
        })?;
    let owner = GitTargetLeaseOwner::agent_workspace_repair(attempt.id.as_str());
    let lease = branch_update_repo
        .get_target_lease(&identity)
        .await?
        .ok_or_else(|| {
            AppError::Conflict("workspace repair canonical target lease is missing".to_string())
        })?;
    if lease.is_released() || lease.owner() != &owner || lease.fencing_epoch() != epoch {
        return Err(AppError::Conflict(
            "workspace repair canonical target lease is stale or owned by another workflow"
                .to_string(),
        ));
    }
    Ok(identity)
}

/// Releases only the exact persisted repair authority at an inactive boundary. A stale release
/// is intentionally harmless: another owner or recovery has already settled that lease.
pub(crate) async fn release_agent_workspace_repair_target_lease(
    branch_update_repo: &dyn BranchUpdateRepository,
    attempt: &AgentWorkspaceRepairAttempt,
) -> AppResult<()> {
    let identity =
        match validate_agent_workspace_repair_target_lease(branch_update_repo, attempt).await {
            Ok(identity) => identity,
            Err(AppError::Conflict(_)) => return Ok(()),
            Err(error) => return Err(error),
        };
    let owner = GitTargetLeaseOwner::agent_workspace_repair(attempt.id.as_str());
    let epoch = attempt
        .target_lease_epoch
        .expect("validated repair lease has an epoch");
    match branch_update_repo
        .release_target_lease(&identity, &owner, epoch)
        .await?
    {
        GitAuthorityCasOutcome::Applied { .. } | GitAuthorityCasOutcome::StaleAuthority => Ok(()),
        outcome => Err(AppError::Conflict(format!(
            "workspace repair lease could not settle at an inactive boundary: {outcome:?}"
        ))),
    }
}

fn has_agent_workspace_repair_target_authority(attempt: &AgentWorkspaceRepairAttempt) -> bool {
    attempt.git_common_dir.is_some()
        || attempt.target_ref.is_some()
        || attempt.target_identity_version.is_some()
        || attempt.target_lease_epoch.is_some()
}

fn clear_agent_workspace_repair_target_authority(attempt: &mut AgentWorkspaceRepairAttempt) {
    attempt.git_common_dir = None;
    attempt.target_ref = None;
    attempt.target_identity_version = None;
    attempt.target_lease_epoch = None;
}

/// An inactive repair boundary may not retain metadata that looks like live Git authority. The
/// exact branch lease is released first, so a concurrent resume sees stale authority and fails
/// closed; the immediately following same-phase CAS removes all persisted identity/epoch fields.
/// A restart encountering the brief released snapshot takes this helper again before it can
/// resume, so it cannot validate or mutate with the old epoch.
pub(crate) async fn release_and_clear_agent_workspace_repair_target_lease(
    repair_repo: &dyn AgentWorkspaceRepairRepository,
    branch_update_repo: &dyn BranchUpdateRepository,
    mut attempt: AgentWorkspaceRepairAttempt,
) -> AppResult<AgentWorkspaceRepairTransitionOutcome> {
    if !has_agent_workspace_repair_target_authority(&attempt) {
        return Ok(AgentWorkspaceRepairTransitionOutcome::Applied(attempt));
    }

    release_agent_workspace_repair_target_lease(branch_update_repo, &attempt).await?;
    let expected_phase = attempt.phase;
    let expected_updated_at = attempt.updated_at;
    clear_agent_workspace_repair_target_authority(&mut attempt);
    attempt.updated_at = next_transition_at(Some(expected_updated_at));
    repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase,
            expected_updated_at,
            next_phase: expected_phase,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .map(repair_attempt_transition_outcome)
}

/// Reacquires the canonical target for a repair that was durably parked at an inactive boundary.
/// The current phase and timestamp are checkpointed with the new fencing epoch before a review
/// pass, manual publish, or downstream publisher can continue.
pub(crate) async fn reacquire_agent_workspace_repair_target_lease_for_continuation(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    attempt: AgentWorkspaceRepairAttempt,
    expected_phase: AgentWorkspaceRepairPhase,
) -> AppResult<AgentWorkspaceRepairTransitionOutcome> {
    let attempt = if has_agent_workspace_repair_target_authority(&attempt) {
        match release_and_clear_agent_workspace_repair_target_lease(
            state.agent_workspace_repair_repo.as_ref(),
            state.branch_update_repo.as_ref(),
            attempt,
        )
        .await?
        {
            AgentWorkspaceRepairTransitionOutcome::Applied(attempt) => attempt,
            outcome => return Ok(outcome),
        }
    } else {
        attempt
    };

    if attempt.phase != expected_phase {
        return Ok(AgentWorkspaceRepairTransitionOutcome::Stale(attempt));
    }
    if state
        .agent_workspace_repair_repo
        .get_open_repair_effect(&attempt.id)
        .await?
        .is_some()
    {
        return Err(AppError::Conflict(
            "workspace repair continuation cannot reacquire its target while an external effect is open"
                .to_string(),
        ));
    }
    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "project {} for workspace repair continuation",
                workspace.project_id
            ))
        })?;
    let target = resolve_effective_agent_conversation_workspace_path(
        &project,
        workspace,
        state.plan_branch_repo.as_ref(),
    )
    .await?;
    let target_identity =
        GitService::canonical_target_identity(&target.path, &target.branch_name).await?;
    let owner = GitTargetLeaseOwner::agent_workspace_repair(attempt.id.as_str());
    let (fencing_epoch, lease_acquired_here) = match state
        .branch_update_repo
        .acquire_target_lease(AcquireGitTargetLease {
            identity: target_identity.clone(),
            owner: owner.clone(),
        })
        .await?
    {
        AcquireGitTargetLeaseOutcome::Acquired { fencing_epoch } => (fencing_epoch, true),
        AcquireGitTargetLeaseOutcome::AlreadyOwned { fencing_epoch } => (fencing_epoch, false),
        AcquireGitTargetLeaseOutcome::TargetBusy {
            owner: active_owner,
            fencing_epoch,
        } => {
            return Err(AppError::Conflict(format!(
                "workspace repair continuation target is owned by {:?} at fencing epoch {fencing_epoch}",
                active_owner
            )));
        }
    };
    match checkpoint_agent_workspace_repair_target_lease(
        state.agent_workspace_repair_repo.as_ref(),
        attempt,
        &target_identity,
        fencing_epoch,
        expected_phase,
    )
    .await
    {
        Ok(outcome @ AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_)) => {
            Ok(repair_attempt_transition_outcome(outcome))
        }
        Ok(outcome @ AgentWorkspaceRepairAttemptTransitionOutcome::Stale(_)) => {
            release_new_agent_workspace_repair_dispatch_lease(
                state.branch_update_repo.as_ref(),
                &target_identity,
                &owner,
                fencing_epoch,
                lease_acquired_here,
            )
            .await?;
            Ok(repair_attempt_transition_outcome(outcome))
        }
        Ok(AgentWorkspaceRepairAttemptTransitionOutcome::Missing) => {
            release_new_agent_workspace_repair_dispatch_lease(
                state.branch_update_repo.as_ref(),
                &target_identity,
                &owner,
                fencing_epoch,
                lease_acquired_here,
            )
            .await?;
            Ok(AgentWorkspaceRepairTransitionOutcome::Missing)
        }
        Err(error) => {
            release_new_agent_workspace_repair_dispatch_lease(
                state.branch_update_repo.as_ref(),
                &target_identity,
                &owner,
                fencing_epoch,
                lease_acquired_here,
            )
            .await?;
            Err(error)
        }
    }
}

/// Records a classified durable dispatch outcome for every repair delivery path.
pub(crate) async fn settle_agent_workspace_repair_dispatch_outcome(
    repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
    branch_update_repo: Arc<dyn BranchUpdateRepository>,
    mut attempt: AgentWorkspaceRepairAttempt,
    settlement: AgentWorkspaceRepairDispatchSettlement,
    summary: &str,
    auto_merge_current: Option<bool>,
) -> AppResult<AgentWorkspaceRepairTransitionOutcome> {
    validate_agent_workspace_repair_target_lease(branch_update_repo.as_ref(), &attempt).await?;
    let retryable = matches!(
        settlement,
        AgentWorkspaceRepairDispatchSettlement::RetryableFailure
    );
    let exhausted =
        retryable && attempt.dispatch_count >= MAX_AGENT_WORKSPACE_REPAIR_DISPATCH_RETRIES;
    let next_phase = match settlement {
        AgentWorkspaceRepairDispatchSettlement::Delivered => AgentWorkspaceRepairPhase::Repairing,
        AgentWorkspaceRepairDispatchSettlement::DeferredQueued => {
            AgentWorkspaceRepairPhase::Requested
        }
        AgentWorkspaceRepairDispatchSettlement::RetryableFailure if !exhausted => {
            AgentWorkspaceRepairPhase::Requested
        }
        AgentWorkspaceRepairDispatchSettlement::RetryableFailure
        | AgentWorkspaceRepairDispatchSettlement::NonRetryableFailure => {
            AgentWorkspaceRepairPhase::Blocked
        }
    };
    let expected_phase = AgentWorkspaceRepairPhase::Dispatching;
    let expected_updated_at = attempt.updated_at;
    attempt.phase = next_phase;
    attempt.summary = Some(match settlement {
        AgentWorkspaceRepairDispatchSettlement::DeferredQueued => {
            format!("{summary} Waiting for the conversation to become available.")
        }
        AgentWorkspaceRepairDispatchSettlement::RetryableFailure if !exhausted => format!(
            "{summary} Retrying delivery {}/{} automatically.",
            attempt.dispatch_count + 1,
            MAX_AGENT_WORKSPACE_REPAIR_DISPATCH_RETRIES,
        ),
        AgentWorkspaceRepairDispatchSettlement::RetryableFailure => {
            format!("{summary} Automatic repair delivery retries are exhausted.",)
        }
        _ => summary.to_string(),
    });
    match settlement {
        AgentWorkspaceRepairDispatchSettlement::Delivered => {
            attempt.next_dispatch_at = None;
            attempt.blocker = None;
        }
        AgentWorkspaceRepairDispatchSettlement::DeferredQueued => {
            attempt.next_dispatch_at = Some(
                Utc::now() + Duration::seconds(AGENT_WORKSPACE_REPAIR_DISPATCH_DEFERRED_DELAY_SECS),
            );
            attempt.reserved_agent_run_id = None;
            attempt.blocker = None;
        }
        AgentWorkspaceRepairDispatchSettlement::RetryableFailure if !exhausted => {
            let next_count = attempt.dispatch_count.checked_add(1).ok_or_else(|| {
                AppError::Validation("workspace repair dispatch retry count overflow".to_string())
            })?;
            attempt.dispatch_count = next_count;
            attempt.next_dispatch_at =
                Some(Utc::now() + agent_workspace_repair_dispatch_retry_delay(next_count));
            attempt.reserved_agent_run_id = None;
            attempt.blocker = None;
        }
        AgentWorkspaceRepairDispatchSettlement::RetryableFailure
        | AgentWorkspaceRepairDispatchSettlement::NonRetryableFailure => {
            attempt.next_dispatch_at = None;
            attempt.reserved_agent_run_id = None;
            attempt.blocker = attempt.summary.clone();
        }
    }
    attempt.updated_at = next_transition_at(Some(attempt.updated_at));
    let run_classification = attempt
        .reserved_agent_run_id
        .as_ref()
        .map(repair_run_event_classification);
    let event = AgentConversationWorkspacePublicationEvent::new(
        attempt.conversation_id.clone(),
        REPAIR_SENT_STEP,
        match settlement {
            AgentWorkspaceRepairDispatchSettlement::Delivered => "succeeded",
            AgentWorkspaceRepairDispatchSettlement::DeferredQueued => "deferred",
            AgentWorkspaceRepairDispatchSettlement::RetryableFailure if !exhausted => "retrying",
            AgentWorkspaceRepairDispatchSettlement::RetryableFailure
            | AgentWorkspaceRepairDispatchSettlement::NonRetryableFailure => "failed",
        },
        attempt.summary.as_deref().unwrap_or(summary),
        run_classification,
    );
    let projection_summary = attempt
        .summary
        .clone()
        .unwrap_or_else(|| summary.to_string());
    let transition = repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: attempt.clone(),
            expected_phase,
            expected_updated_at,
            next_phase,
            compatibility_projection: Some(repair_attempt_projection(
                &attempt,
                &projection_summary,
                auto_merge_current,
            )),
            events: vec![event],
        })
        .await;
    let outcome = match transition {
        Ok(outcome) => repair_attempt_transition_outcome(outcome),
        Err(error) if settlement == AgentWorkspaceRepairDispatchSettlement::Delivered => {
            tracing::warn!(
                conversation_id = attempt.conversation_id.as_str(),
                error = %error,
                "Workspace repair delivery succeeded but its success audit event could not be persisted; retrying the authoritative phase transition without the audit event"
            );
            repair_repo
                .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
                    attempt: attempt.clone(),
                    expected_phase,
                    expected_updated_at,
                    next_phase,
                    compatibility_projection: Some(repair_attempt_projection(
                        &attempt,
                        &projection_summary,
                        auto_merge_current,
                    )),
                    events: Vec::new(),
                })
                .await
                .map(repair_attempt_transition_outcome)?
        }
        Err(error) => return Err(error),
    };
    if next_phase != AgentWorkspaceRepairPhase::Blocked {
        return Ok(outcome);
    }
    match outcome {
        AgentWorkspaceRepairTransitionOutcome::Applied(attempt) => {
            release_and_clear_agent_workspace_repair_target_lease(
                repair_repo.as_ref(),
                branch_update_repo.as_ref(),
                attempt,
            )
            .await
        }
        outcome => Ok(outcome),
    }
}

/// Re-reads the live publish and review gates before moving a repaired attempt out of validation.
/// A repository/read failure is returned to the caller, so it cannot be mistaken for a clear
/// review gate or disabled Auto Publish preference.
pub(crate) type DurableRepairWorkspaceReviewStartFuture<'a> =
    Pin<Box<dyn Future<Output = AppResult<AgentWorkspaceReviewStart>> + Send + 'a>>;

pub(crate) trait DurableRepairWorkspaceReviewStarter {
    fn start<'a>(
        &'a self,
        state: Arc<AppState>,
        workspace: &'a AgentConversationWorkspace,
        force: bool,
    ) -> DurableRepairWorkspaceReviewStartFuture<'a>;
}

struct DefaultDurableRepairWorkspaceReviewStarter;

impl DurableRepairWorkspaceReviewStarter for DefaultDurableRepairWorkspaceReviewStarter {
    fn start<'a>(
        &'a self,
        state: Arc<AppState>,
        workspace: &'a AgentConversationWorkspace,
        force: bool,
    ) -> DurableRepairWorkspaceReviewStartFuture<'a> {
        Box::pin(start_guarded_agent_workspace_review(
            state,
            workspace,
            force,
            WorkspaceReviewStartOrigin::Automated,
            None,
        ))
    }
}

pub(crate) async fn continue_agent_workspace_repair_at_boundary(
    state: &AppState,
    attempt: AgentWorkspaceRepairAttempt,
    expected_phase: AgentWorkspaceRepairPhase,
    summary: &str,
    explicit_publish: bool,
    publish_authority: PublishAuthority,
) -> AppResult<AgentWorkspaceRepairTransitionOutcome> {
    continue_agent_workspace_repair_at_boundary_with_review_starter(
        state,
        attempt,
        expected_phase,
        summary,
        explicit_publish,
        publish_authority,
        &DefaultDurableRepairWorkspaceReviewStarter,
    )
    .await
}

pub(crate) async fn continue_agent_workspace_repair_at_boundary_with_review_starter<S>(
    state: &AppState,
    attempt: AgentWorkspaceRepairAttempt,
    expected_phase: AgentWorkspaceRepairPhase,
    summary: &str,
    explicit_publish: bool,
    publish_authority: PublishAuthority,
    review_starter: &S,
) -> AppResult<AgentWorkspaceRepairTransitionOutcome>
where
    S: DurableRepairWorkspaceReviewStarter + ?Sized,
{
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&attempt.conversation_id)
        .await?
        .ok_or_else(|| {
            crate::error::AppError::NotFound(format!(
                "workspace {} for repair continuation",
                attempt.conversation_id
            ))
        })?;
    let mut attempt = if matches!(
        expected_phase,
        AgentWorkspaceRepairPhase::AwaitingReview
            | AgentWorkspaceRepairPhase::Ready
            | AgentWorkspaceRepairPhase::Blocked
    ) {
        match reacquire_agent_workspace_repair_target_lease_for_continuation(
            state,
            &workspace,
            attempt,
            expected_phase,
        )
        .await?
        {
            AgentWorkspaceRepairTransitionOutcome::Applied(attempt) => attempt,
            outcome => return Ok(outcome),
        }
    } else {
        attempt
    };
    let review_settings = state
        .review_settings_repo
        .get_settings()
        .await
        .map_err(|error| {
            AppError::Infrastructure(format!("Failed to load review settings: {error}"))
        })?;
    let review_blocker = load_workspace_review_publish_blocker(state, &workspace).await?;
    let expected_updated_at = attempt.updated_at;

    attempt.review_required = attempt.review_required || review_settings.require_workspace_review;
    attempt.auto_publish_enabled = workspace.auto_publish_enabled;
    attempt.auto_merge_desired = workspace.pr_auto_merge_desired;
    attempt.auto_merge_method = Some(workspace.pr_auto_merge_method.clone());
    if explicit_publish
        && attempt.continuation.priority() < AgentWorkspaceRepairContinuation::Publish.priority()
    {
        attempt.continuation = AgentWorkspaceRepairContinuation::Publish;
    }
    if explicit_publish
        && publish_authority == PublishAuthority::UserExplicit
        && matches!(
            attempt.continuation,
            AgentWorkspaceRepairContinuation::Publish
                | AgentWorkspaceRepairContinuation::ResumePrSupervision
        )
    {
        attempt.explicit_publish_requested = true;
    }
    let next_phase = if review_blocker.is_some() {
        AgentWorkspaceRepairPhase::AwaitingReview
    } else if matches!(
        attempt.continuation,
        AgentWorkspaceRepairContinuation::Manual | AgentWorkspaceRepairContinuation::UpdateOnly
    ) || (!workspace.auto_publish_enabled
        && !explicit_publish
        && !attempt.explicit_publish_requested)
    {
        AgentWorkspaceRepairPhase::Ready
    } else {
        AgentWorkspaceRepairPhase::ContinuationPending
    };
    attempt.phase = next_phase;
    attempt.summary = Some(review_blocker.unwrap_or_else(|| summary.to_string()));
    attempt.updated_at = next_transition_at(Some(attempt.updated_at));
    let projection = repair_attempt_projection(
        &attempt,
        attempt.summary.as_deref().unwrap_or(summary),
        workspace.pr_auto_merge_current,
    );
    let transition = state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt,
            expected_phase,
            expected_updated_at,
            next_phase,
            compatibility_projection: Some(projection),
            events: Vec::new(),
        })
        .await
        .map(repair_attempt_transition_outcome)?;
    match transition {
        AgentWorkspaceRepairTransitionOutcome::Applied(attempt)
            if attempt.phase == AgentWorkspaceRepairPhase::AwaitingReview =>
        {
            match release_and_clear_agent_workspace_repair_target_lease(
                state.agent_workspace_repair_repo.as_ref(),
                state.branch_update_repo.as_ref(),
                attempt,
            )
            .await?
            {
                AgentWorkspaceRepairTransitionOutcome::Applied(attempt) => {
                    continue_agent_workspace_repair_workspace_review_handoff(
                        state,
                        workspace,
                        attempt,
                        summary,
                        explicit_publish,
                        publish_authority,
                        review_starter,
                    )
                    .await
                }
                outcome => Ok(outcome),
            }
        }
        AgentWorkspaceRepairTransitionOutcome::Applied(attempt) => {
            if matches!(
                attempt.phase,
                AgentWorkspaceRepairPhase::Ready | AgentWorkspaceRepairPhase::Blocked
            ) {
                return release_and_clear_agent_workspace_repair_target_lease(
                    state.agent_workspace_repair_repo.as_ref(),
                    state.branch_update_repo.as_ref(),
                    attempt,
                )
                .await;
            }
            Ok(AgentWorkspaceRepairTransitionOutcome::Applied(attempt))
        }
        outcome => Ok(outcome),
    }
}

/// Starts or resumes the existing Workspace Review monitor only after this exact repair
/// generation has durably reached `AwaitingReview`. The review monitor owns reviewer identity and
/// is idempotent for an already-running target; the repair attempt remains the sole authority for
/// resuming its continuation after a pass.
async fn continue_agent_workspace_repair_workspace_review_handoff<S>(
    state: &AppState,
    workspace: AgentConversationWorkspace,
    attempt: AgentWorkspaceRepairAttempt,
    summary: &str,
    explicit_publish: bool,
    publish_authority: PublishAuthority,
    review_starter: &S,
) -> AppResult<AgentWorkspaceRepairTransitionOutcome>
where
    S: DurableRepairWorkspaceReviewStarter + ?Sized,
{
    let current = match state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&attempt.conversation_id)
        .await?
    {
        Some(current) => current,
        None => return Ok(AgentWorkspaceRepairTransitionOutcome::Missing),
    };
    if current.id != attempt.id
        || current.generation != attempt.generation
        || current.updated_at != attempt.updated_at
        || current.phase != AgentWorkspaceRepairPhase::AwaitingReview
    {
        return Ok(AgentWorkspaceRepairTransitionOutcome::Stale(current));
    }

    let review_settings = state
        .review_settings_repo
        .get_settings()
        .await
        .map_err(|error| {
            AppError::Infrastructure(format!("Failed to load review settings: {error}"))
        })?;
    if !review_settings.require_workspace_review {
        return Box::pin(
            continue_agent_workspace_repair_at_boundary_with_review_starter(
                state,
                current,
                AgentWorkspaceRepairPhase::AwaitingReview,
                summary,
                explicit_publish,
                publish_authority,
                review_starter,
            ),
        )
        .await;
    }

    let review_context = load_agent_workspace_review_context(state, &workspace).await?;
    match review_context.monitor.review_gate_status {
        AgentWorkspaceReviewGateStatus::NotRequired | AgentWorkspaceReviewGateStatus::Passed => {
            Box::pin(
                continue_agent_workspace_repair_at_boundary_with_review_starter(
                    state,
                    current,
                    AgentWorkspaceRepairPhase::AwaitingReview,
                    summary,
                    explicit_publish,
                    publish_authority,
                    review_starter,
                ),
            )
            .await
        }
        AgentWorkspaceReviewGateStatus::Reviewing => {
            Ok(AgentWorkspaceRepairTransitionOutcome::Applied(current))
        }
        AgentWorkspaceReviewGateStatus::Blocking | AgentWorkspaceReviewGateStatus::Failed => {
            let blocker = review_gate_publish_blocker(&review_context).unwrap_or_else(|| {
                "Workspace Review blocks the durable repair continuation".to_string()
            });
            block_agent_workspace_repair_completion(
                Arc::clone(&state.agent_workspace_repair_repo),
                Arc::clone(&state.branch_update_repo),
                current,
                "Workspace Review blocked the durable repair continuation.",
                &blocker,
                workspace.pr_auto_merge_current,
            )
            .await
        }
        AgentWorkspaceReviewGateStatus::Required => {
            let started = review_starter
                .start(Arc::new(state.clone()), &workspace, false)
                .await;
            let started = match started {
                Ok(started) => started,
                Err(error) => {
                    let blocker = format!(
                        "Workspace Review could not start for the durable repair continuation: {error}"
                    );
                    return block_agent_workspace_repair_completion(
                        Arc::clone(&state.agent_workspace_repair_repo),
                        Arc::clone(&state.branch_update_repo),
                        current,
                        "Workspace Review could not start the durable repair continuation.",
                        &blocker,
                        workspace.pr_auto_merge_current,
                    )
                    .await;
                }
            };
            let current = match state
                .agent_workspace_repair_repo
                .get_current_repair_attempt(&attempt.conversation_id)
                .await?
            {
                Some(current) => current,
                None => return Ok(AgentWorkspaceRepairTransitionOutcome::Missing),
            };
            if current.id != attempt.id
                || current.generation != attempt.generation
                || current.phase != AgentWorkspaceRepairPhase::AwaitingReview
            {
                return Ok(AgentWorkspaceRepairTransitionOutcome::Stale(current));
            }
            match started.context.monitor.review_gate_status {
                AgentWorkspaceReviewGateStatus::NotRequired
                | AgentWorkspaceReviewGateStatus::Passed => {
                    Box::pin(
                        continue_agent_workspace_repair_at_boundary_with_review_starter(
                            state,
                            current,
                            AgentWorkspaceRepairPhase::AwaitingReview,
                            summary,
                            explicit_publish,
                            publish_authority,
                            review_starter,
                        ),
                    )
                    .await
                }
                AgentWorkspaceReviewGateStatus::Reviewing => {
                    Ok(AgentWorkspaceRepairTransitionOutcome::Applied(current))
                }
                AgentWorkspaceReviewGateStatus::Blocking
                | AgentWorkspaceReviewGateStatus::Failed => {
                    let blocker =
                        review_gate_publish_blocker(&started.context).unwrap_or_else(|| {
                            "Workspace Review blocks the durable repair continuation".to_string()
                        });
                    block_agent_workspace_repair_completion(
                        Arc::clone(&state.agent_workspace_repair_repo),
                        Arc::clone(&state.branch_update_repo),
                        current,
                        "Workspace Review blocked the durable repair continuation.",
                        &blocker,
                        workspace.pr_auto_merge_current,
                    )
                    .await
                }
                AgentWorkspaceReviewGateStatus::Required => {
                    let blocker = "Workspace Review did not reserve a current reviewer for the durable repair continuation.";
                    block_agent_workspace_repair_completion(
                        Arc::clone(&state.agent_workspace_repair_repo),
                        Arc::clone(&state.branch_update_repo),
                        current,
                        "Workspace Review could not start the durable repair continuation.",
                        blocker,
                        workspace.pr_auto_merge_current,
                    )
                    .await
                }
            }
        }
    }
}

/// A user-selected Commit & Publish resumes the same ready generation. It never creates an
/// unrelated repair attempt, and a newly-required Review still wins over the explicit action.
pub(crate) async fn resume_ready_agent_workspace_repair_for_publish(
    state: &AppState,
    attempt: AgentWorkspaceRepairAttempt,
    summary: &str,
    publish_authority: PublishAuthority,
) -> AppResult<AgentWorkspaceRepairTransitionOutcome> {
    continue_agent_workspace_repair_at_boundary(
        state,
        attempt,
        AgentWorkspaceRepairPhase::Ready,
        summary,
        true,
        publish_authority,
    )
    .await
}

/// Re-enters the exact active repair generation before an ordinary publish caller can invoke the
/// normal publisher. A current repair attempt is authoritative even when its compatibility
/// projection makes the workspace appear publishable. Ready requires a gate override with a
/// separately typed origin; review resumption revalidates the persisted Workspace Review gate at
/// the same CAS boundary.
pub(crate) async fn resume_current_agent_workspace_repair_publish(
    state: &AppState,
    conversation_id: &ChatConversationId,
    summary: &str,
    explicit_publish: bool,
    publish_authority: PublishAuthority,
) -> AppResult<AgentWorkspaceRepairPublishResumeOutcome> {
    let Some(attempt) = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(conversation_id)
        .await?
    else {
        return Ok(AgentWorkspaceRepairPublishResumeOutcome::NoAttempt);
    };

    let transition = match attempt.phase {
        AgentWorkspaceRepairPhase::Ready => {
            if !explicit_publish {
                return Ok(AgentWorkspaceRepairPublishResumeOutcome::Ready);
            }
            resume_ready_agent_workspace_repair_for_publish(
                state,
                attempt,
                summary,
                publish_authority,
            )
            .await?
        }
        AgentWorkspaceRepairPhase::AwaitingReview => {
            let workspace = state
                .agent_conversation_workspace_repo
                .get_by_conversation_id(conversation_id)
                .await?
                .ok_or_else(|| {
                    AppError::NotFound(format!(
                        "workspace {} for repair review continuation",
                        conversation_id
                    ))
                })?;
            continue_agent_workspace_repair_workspace_review_handoff(
                state,
                workspace,
                attempt,
                summary,
                explicit_publish,
                publish_authority,
                &DefaultDurableRepairWorkspaceReviewStarter,
            )
            .await?
        }
        // These phases are owned by the live completion/startup recovery publisher. An ordinary
        // publish request may not join that side-effecting path: it would race its durable
        // handoff receipt and could replay the downstream PR publisher.
        AgentWorkspaceRepairPhase::ContinuationPending | AgentWorkspaceRepairPhase::Continuing => {
            return Ok(AgentWorkspaceRepairPublishResumeOutcome::Busy);
        }
        AgentWorkspaceRepairPhase::Blocked => {
            return Ok(AgentWorkspaceRepairPublishResumeOutcome::Blocked);
        }
        AgentWorkspaceRepairPhase::Requested
        | AgentWorkspaceRepairPhase::Dispatching
        | AgentWorkspaceRepairPhase::Repairing
        | AgentWorkspaceRepairPhase::Validating => {
            return Ok(AgentWorkspaceRepairPublishResumeOutcome::Busy);
        }
    };

    match transition {
        AgentWorkspaceRepairTransitionOutcome::Applied(next) => match next.phase {
            AgentWorkspaceRepairPhase::ContinuationPending
            | AgentWorkspaceRepairPhase::Continuing => Ok(
                AgentWorkspaceRepairPublishResumeOutcome::Continue(Box::new(next)),
            ),
            AgentWorkspaceRepairPhase::AwaitingReview => {
                Ok(AgentWorkspaceRepairPublishResumeOutcome::AwaitingReview)
            }
            AgentWorkspaceRepairPhase::Ready => Ok(AgentWorkspaceRepairPublishResumeOutcome::Ready),
            AgentWorkspaceRepairPhase::Blocked => {
                Ok(AgentWorkspaceRepairPublishResumeOutcome::Blocked)
            }
            AgentWorkspaceRepairPhase::Requested
            | AgentWorkspaceRepairPhase::Dispatching
            | AgentWorkspaceRepairPhase::Repairing
            | AgentWorkspaceRepairPhase::Validating => {
                Ok(AgentWorkspaceRepairPublishResumeOutcome::Busy)
            }
        },
        AgentWorkspaceRepairTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairTransitionOutcome::Missing => {
            Ok(AgentWorkspaceRepairPublishResumeOutcome::Stale)
        }
    }
}

/// Classifies trusted run completion against the persisted attempt. This primitive is deliberately
/// side-effect free so the HTTP completion handler can reject stale work before Git inspection.
pub(crate) async fn classify_agent_workspace_repair_completion_authority(
    repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
    conversation_id: &ChatConversationId,
    run_id: &AgentRunId,
) -> AppResult<AgentWorkspaceRepairCompletionAuthority> {
    let exact = repair_repo
        .get_repair_attempt_for_run(conversation_id, run_id)
        .await?;
    let current = repair_repo
        .get_current_repair_attempt(conversation_id)
        .await?;
    let Some(exact) = exact else {
        return Ok(if current.is_some() {
            AgentWorkspaceRepairCompletionAuthority::Superseded
        } else {
            AgentWorkspaceRepairCompletionAuthority::Invalid
        });
    };
    if current
        .as_ref()
        .is_some_and(|attempt| attempt.id != exact.id)
    {
        return Ok(AgentWorkspaceRepairCompletionAuthority::Superseded);
    }
    if exact.settled_at.is_some() {
        return Ok(
            if matches!(
                exact.outcome,
                Some(crate::domain::entities::AgentWorkspaceRepairOutcome::Succeeded)
            ) {
                AgentWorkspaceRepairCompletionAuthority::AlreadyCompleted
            } else {
                AgentWorkspaceRepairCompletionAuthority::Superseded
            },
        );
    }
    match exact.phase {
        AgentWorkspaceRepairPhase::Blocked => {
            Ok(AgentWorkspaceRepairCompletionAuthority::AlreadyBlocked)
        }
        // `Validating` is a durable completion reservation. Re-entering with the same run must
        // not repeat Git inspection while the first completion owns validation.
        AgentWorkspaceRepairPhase::Validating
        | AgentWorkspaceRepairPhase::AwaitingReview
        | AgentWorkspaceRepairPhase::ContinuationPending
        | AgentWorkspaceRepairPhase::Continuing
        | AgentWorkspaceRepairPhase::Ready => {
            Ok(AgentWorkspaceRepairCompletionAuthority::AlreadyCompleted)
        }
        AgentWorkspaceRepairPhase::Requested
        | AgentWorkspaceRepairPhase::Dispatching
        | AgentWorkspaceRepairPhase::Repairing => Ok(
            AgentWorkspaceRepairCompletionAuthority::Current(Box::new(exact)),
        ),
    }
}

pub(crate) fn repair_run_event_classification(run_id: &AgentRunId) -> String {
    format!("{REPAIR_RUN_CLASSIFICATION_PREFIX}{}", run_id.as_str())
}

#[cfg(any(test, feature = "test-utils"))]
fn repair_event_run_id(event: &AgentConversationWorkspacePublicationEvent) -> Option<&str> {
    event
        .classification
        .as_deref()?
        .strip_prefix(REPAIR_RUN_CLASSIFICATION_PREFIX)
}

fn next_transition_at(previous: Option<DateTime<Utc>>) -> DateTime<Utc> {
    let now = Utc::now();
    match previous {
        Some(previous) if now <= previous => previous + Duration::nanoseconds(1),
        _ => now,
    }
}

#[cfg(any(test, feature = "test-utils"))]
fn transition_guard(
    transition: &AgentWorkspaceRepairStateTransition,
) -> AgentWorkspaceRepairStateGuard {
    AgentWorkspaceRepairStateGuard {
        publication_push_status: transition.publication_push_status.clone(),
        pr_supervision_status: transition.pr_supervision_status.clone(),
        pr_supervision_updated_at: Some(transition.pr_supervision_updated_at),
    }
}

#[cfg(any(test, feature = "test-utils"))]
pub(crate) async fn claim_agent_workspace_repair(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    conversation_id: &ChatConversationId,
    summary: &str,
    auto_merge_current: Option<bool>,
) -> AppResult<Option<AgentWorkspaceRepairClaim>> {
    let Some(workspace) = workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
    else {
        return Ok(None);
    };
    if workspace.publication_push_status.as_deref() == Some("needs_agent")
        && workspace.pr_supervision_status.as_deref() == Some("fixing")
    {
        return Ok(None);
    }

    let expected = AgentWorkspaceRepairStateGuard::from_workspace(&workspace);
    let transition = AgentWorkspaceRepairStateTransition {
        publication_push_status: Some("needs_agent".to_string()),
        pr_supervision_status: Some("fixing".to_string()),
        pr_supervision_summary: Some(summary.to_string()),
        pr_supervision_updated_at: next_transition_at(workspace.pr_supervision_updated_at),
        pr_auto_merge_current: auto_merge_current,
        base_commit: None,
    };
    if !workspace_repo
        .compare_and_set_repair_state(conversation_id, &expected, &transition)
        .await?
    {
        return Ok(None);
    }

    Ok(Some(AgentWorkspaceRepairClaim {
        conversation_id: conversation_id.clone(),
        guard: transition_guard(&transition),
    }))
}

#[cfg(any(test, feature = "test-utils"))]
pub(crate) async fn restore_refreshed_agent_workspace_pr_fix_claim(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    workspace: &AgentConversationWorkspace,
) -> AppResult<Option<AgentWorkspaceRepairClaim>> {
    if workspace.publication_push_status.as_deref() != Some("refreshed")
        || workspace.pr_supervision_status.as_deref() != Some("fixing")
    {
        return Ok(None);
    }
    let transition = AgentWorkspaceRepairStateTransition {
        publication_push_status: Some("needs_agent".to_string()),
        pr_supervision_status: Some("fixing".to_string()),
        pr_supervision_summary: workspace.pr_supervision_summary.clone(),
        pr_supervision_updated_at: next_transition_at(workspace.pr_supervision_updated_at),
        pr_auto_merge_current: None,
        base_commit: None,
    };
    if !workspace_repo
        .compare_and_set_repair_state(
            &workspace.conversation_id,
            &AgentWorkspaceRepairStateGuard::from_workspace(workspace),
            &transition,
        )
        .await?
    {
        return Ok(None);
    }
    Ok(Some(AgentWorkspaceRepairClaim {
        conversation_id: workspace.conversation_id.clone(),
        guard: transition_guard(&transition),
    }))
}

#[cfg(test)]
pub(crate) async fn settle_agent_workspace_repair_failure(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    claim: &AgentWorkspaceRepairClaim,
    summary: &str,
) -> AppResult<bool> {
    let transition = AgentWorkspaceRepairStateTransition {
        publication_push_status: Some("needs_agent".to_string()),
        pr_supervision_status: Some("blocked".to_string()),
        pr_supervision_summary: Some(summary.to_string()),
        pr_supervision_updated_at: next_transition_at(claim.guard.pr_supervision_updated_at),
        pr_auto_merge_current: None,
        base_commit: None,
    };
    workspace_repo
        .compare_and_set_repair_state(&claim.conversation_id, &claim.guard, &transition)
        .await
}

#[cfg(any(test, feature = "test-utils"))]
fn latest_repair_event(
    events: &[AgentConversationWorkspacePublicationEvent],
) -> Option<&AgentConversationWorkspacePublicationEvent> {
    events
        .iter()
        .filter(|event| {
            matches!(
                event.step.as_str(),
                REPAIR_REQUESTED_STEP | REPAIR_DEFERRED_STEP | REPAIR_SENT_STEP
            )
        })
        .max_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        })
}

#[cfg(any(test, feature = "test-utils"))]
pub(crate) fn repair_event_authorizes_active_run(
    events: &[AgentConversationWorkspacePublicationEvent],
    active_run: &AgentRun,
) -> bool {
    let Some(event) = latest_repair_event(events) else {
        return false;
    };
    match event.step.as_str() {
        REPAIR_REQUESTED_STEP | REPAIR_DEFERRED_STEP => {
            event.created_at >= active_run.started_at
                && matches!(event.status.as_str(), "started" | "succeeded")
        }
        REPAIR_SENT_STEP => {
            if let Some(run_id) = repair_event_run_id(event) {
                run_id == active_run.id.as_str()
                    && matches!(event.status.as_str(), "started" | "succeeded")
            } else {
                event.status == "succeeded" && event.created_at >= active_run.started_at
            }
        }
        _ => false,
    }
}

#[cfg(any(test, feature = "test-utils"))]
fn repair_sent_event_authorizes_run(
    event: &AgentConversationWorkspacePublicationEvent,
    active_run: &AgentRun,
    claim_started_at: DateTime<Utc>,
) -> bool {
    if event.step != REPAIR_SENT_STEP
        || !matches!(event.status.as_str(), "started" | "succeeded")
        || event.created_at < claim_started_at
    {
        return false;
    }
    if let Some(run_id) = repair_event_run_id(event) {
        return run_id == active_run.id.as_str();
    }

    event.status == "succeeded" && event.created_at >= active_run.started_at
}

#[cfg(test)]
fn successful_send_authorizes_completion(
    events: &[AgentConversationWorkspacePublicationEvent],
    active_run: &AgentRun,
    claim_started_at: DateTime<Utc>,
) -> bool {
    latest_repair_event(events)
        .is_some_and(|event| repair_sent_event_authorizes_run(event, active_run, claim_started_at))
}

#[cfg(any(test, feature = "test-utils"))]
pub(crate) fn terminal_run_authorizes_repair_recovery(
    workspace: &AgentConversationWorkspace,
    events: &[AgentConversationWorkspacePublicationEvent],
    terminal_run: &AgentRun,
) -> bool {
    let claim_started_at = workspace
        .pr_supervision_updated_at
        .unwrap_or(workspace.updated_at);
    let Some(event) = latest_repair_event(events) else {
        return terminal_run.started_at >= claim_started_at;
    };
    if event.created_at < claim_started_at {
        return false;
    }

    match event.step.as_str() {
        REPAIR_SENT_STEP => repair_sent_event_authorizes_run(event, terminal_run, claim_started_at),
        REPAIR_REQUESTED_STEP => terminal_run.started_at >= event.created_at,
        REPAIR_DEFERRED_STEP => {
            Utc::now().signed_duration_since(event.created_at)
                >= Duration::seconds(DEFERRED_REPAIR_WAIT_TIMEOUT_SECS as i64)
                && terminal_run
                    .completed_at
                    .is_some_and(|completed_at| event.created_at <= completed_at)
        }
        _ => false,
    }
}

#[cfg(any(test, feature = "test-utils"))]
pub(crate) async fn settle_terminal_agent_workspace_repair(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    workspace: &AgentConversationWorkspace,
    summary: &str,
) -> AppResult<bool> {
    if workspace.publication_push_status.as_deref() != Some("needs_agent") {
        return Ok(false);
    }
    let transition = AgentWorkspaceRepairStateTransition {
        publication_push_status: Some("failed".to_string()),
        pr_supervision_status: Some("blocked".to_string()),
        pr_supervision_summary: Some(summary.to_string()),
        pr_supervision_updated_at: next_transition_at(workspace.pr_supervision_updated_at),
        pr_auto_merge_current: None,
        base_commit: None,
    };
    workspace_repo
        .compare_and_set_repair_state(
            &workspace.conversation_id,
            &AgentWorkspaceRepairStateGuard::from_workspace(workspace),
            &transition,
        )
        .await
}

#[cfg(any(test, feature = "test-utils"))]
pub(crate) async fn reconcile_active_agent_workspace_repair(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    workspace: &AgentConversationWorkspace,
) -> AppResult<bool> {
    if workspace.publication_push_status.as_deref() != Some("needs_agent")
        || workspace.pr_supervision_status.as_deref() != Some("blocked")
    {
        return Ok(false);
    }
    let Some(active_run) = agent_run_repo
        .get_active_for_conversation(&workspace.conversation_id)
        .await?
    else {
        return Ok(false);
    };
    let events = workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await?;
    if !repair_event_authorizes_active_run(&events, &active_run) {
        return Ok(false);
    }

    let transition = AgentWorkspaceRepairStateTransition {
        publication_push_status: Some("needs_agent".to_string()),
        pr_supervision_status: Some("fixing".to_string()),
        pr_supervision_summary: Some("Agent workspace repair is in progress.".to_string()),
        pr_supervision_updated_at: next_transition_at(workspace.pr_supervision_updated_at),
        pr_auto_merge_current: None,
        base_commit: None,
    };
    workspace_repo
        .compare_and_set_repair_state(
            &workspace.conversation_id,
            &AgentWorkspaceRepairStateGuard::from_workspace(workspace),
            &transition,
        )
        .await
}

#[cfg(test)]
pub(crate) async fn current_agent_workspace_repair_claim_for_completion(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    workspace: &AgentConversationWorkspace,
) -> AppResult<Option<AgentWorkspaceRepairClaim>> {
    if workspace.publication_push_status.as_deref() != Some("needs_agent")
        || workspace.pr_supervision_status.as_deref() != Some("fixing")
    {
        return Ok(None);
    }
    let Some(active_run) = agent_run_repo
        .get_active_for_conversation(&workspace.conversation_id)
        .await?
    else {
        return Ok(None);
    };
    let events = workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await?;
    let Some(claim_started_at) = workspace.pr_supervision_updated_at else {
        return Ok(None);
    };
    if !successful_send_authorizes_completion(&events, &active_run, claim_started_at) {
        return Ok(None);
    }
    Ok(Some(AgentWorkspaceRepairClaim {
        conversation_id: workspace.conversation_id.clone(),
        guard: AgentWorkspaceRepairStateGuard::from_workspace(workspace),
    }))
}

#[cfg(test)]
pub(crate) async fn complete_agent_workspace_repair_claim(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    claim: &AgentWorkspaceRepairClaim,
    base_commit: &str,
    supervision_status: Option<&str>,
    supervision_summary: Option<&str>,
) -> AppResult<bool> {
    let transition = AgentWorkspaceRepairStateTransition {
        publication_push_status: Some("refreshed".to_string()),
        pr_supervision_status: supervision_status.map(str::to_string),
        pr_supervision_summary: supervision_summary.map(str::to_string),
        pr_supervision_updated_at: next_transition_at(claim.guard.pr_supervision_updated_at),
        pr_auto_merge_current: None,
        base_commit: Some(base_commit.to_string()),
    };
    workspace_repo
        .compare_and_set_repair_state(&claim.conversation_id, &claim.guard, &transition)
        .await
}

#[cfg(any(test, feature = "test-utils"))]
async fn transition_agent_workspace_repair_claim_with_events(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    claim: &AgentWorkspaceRepairClaim,
    publication_push_status: &str,
    pr_supervision_status: &str,
    pr_supervision_summary: &str,
    events: Vec<AgentConversationWorkspacePublicationEvent>,
) -> AppResult<Option<AgentWorkspaceRepairClaim>> {
    let transition = AgentWorkspaceRepairStateTransition {
        publication_push_status: Some(publication_push_status.to_string()),
        pr_supervision_status: Some(pr_supervision_status.to_string()),
        pr_supervision_summary: Some(pr_supervision_summary.to_string()),
        pr_supervision_updated_at: next_transition_at(claim.guard.pr_supervision_updated_at),
        pr_auto_merge_current: None,
        base_commit: None,
    };
    if !workspace_repo
        .compare_and_set_repair_state_with_events(
            &claim.conversation_id,
            &claim.guard,
            &transition,
            events,
        )
        .await?
    {
        return Ok(None);
    }
    Ok(Some(AgentWorkspaceRepairClaim {
        conversation_id: claim.conversation_id.clone(),
        guard: transition_guard(&transition),
    }))
}

#[cfg(test)]
pub(crate) async fn complete_agent_workspace_pr_fix_claim(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    claim: &AgentWorkspaceRepairClaim,
    summary: &str,
    workspace_review_required: bool,
    auto_publish_enabled: bool,
) -> AppResult<Option<AgentWorkspaceRepairClaim>> {
    let (supervision_status, supervision_summary) = if workspace_review_required {
        (
            "reviewing",
            "PR fix verified; Workspace Review must finish before publishing resumes.",
        )
    } else if auto_publish_enabled {
        ("publishing", "PR fix verified; publishing updates.")
    } else {
        ("paused", "PR fix verified; Auto Publish is paused.")
    };
    let mut events = vec![AgentConversationWorkspacePublicationEvent::new(
        claim.conversation_id.clone(),
        PR_AUTOFIX_COMPLETED_STEP,
        "succeeded",
        summary,
        Some(PR_AUTOFIX_COMPLETED_STEP.to_string()),
    )];
    if workspace_review_required {
        events.push(AgentConversationWorkspacePublicationEvent::new(
            claim.conversation_id.clone(),
            PR_AUTOFIX_WORKSPACE_REVIEW_STEP,
            "pending",
            format!("PR fix verified; Workspace Review handoff is pending. Fix summary: {summary}"),
            Some("workspace_review_pending".to_string()),
        ));
    } else if !auto_publish_enabled {
        events.push(AgentConversationWorkspacePublicationEvent::new(
            claim.conversation_id.clone(),
            "pr_autofix_publish_skipped",
            "skipped",
            format!("PR fix completed, but Auto Publish is paused. Fix summary: {summary}"),
            Some("auto_publish_paused".to_string()),
        ));
    }
    transition_agent_workspace_repair_claim_with_events(
        workspace_repo,
        claim,
        "refreshed",
        supervision_status,
        supervision_summary,
        events,
    )
    .await
}

#[cfg(test)]
pub(crate) async fn block_agent_workspace_pr_fix_claim(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    claim: &AgentWorkspaceRepairClaim,
    blocker: &str,
) -> AppResult<Option<AgentWorkspaceRepairClaim>> {
    transition_agent_workspace_repair_claim_with_events(
        workspace_repo,
        claim,
        "failed",
        "blocked",
        blocker,
        vec![AgentConversationWorkspacePublicationEvent::new(
            claim.conversation_id.clone(),
            PR_AUTOFIX_BLOCKED_STEP,
            "blocked",
            blocker,
            Some("pr_autofix_blocker".to_string()),
        )],
    )
    .await
}

#[cfg(any(test, feature = "test-utils"))]
pub(crate) async fn abort_agent_workspace_pr_fix_review_handoff(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    claim: &AgentWorkspaceRepairClaim,
    blocker: &str,
) -> AppResult<Option<AgentWorkspaceRepairClaim>> {
    transition_agent_workspace_repair_claim_with_events(
        workspace_repo,
        claim,
        "failed",
        "blocked",
        blocker,
        vec![AgentConversationWorkspacePublicationEvent::new(
            claim.conversation_id.clone(),
            PR_AUTOFIX_WORKSPACE_REVIEW_ABORTED_STEP,
            "failed",
            blocker,
            Some("workspace_review_aborted".to_string()),
        )],
    )
    .await
}

#[cfg(test)]
pub(crate) async fn continue_agent_workspace_pr_fix_after_review_handoff(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    claim: &AgentWorkspaceRepairClaim,
    summary: &str,
) -> AppResult<Option<AgentWorkspaceRepairClaim>> {
    transition_agent_workspace_repair_claim_with_events(
        workspace_repo,
        claim,
        "refreshed",
        "publishing",
        "Workspace Review handoff settled; publishing PR fix updates.",
        vec![AgentConversationWorkspacePublicationEvent::new(
            claim.conversation_id.clone(),
            PR_AUTOFIX_WORKSPACE_REVIEW_PASSED_STEP,
            "publishing",
            summary,
            Some("workspace_review_not_required".to_string()),
        )],
    )
    .await
}
