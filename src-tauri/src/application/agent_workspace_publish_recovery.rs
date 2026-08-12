use std::sync::Arc;

use ralphx_events::emit_serialized;

#[cfg(any(test, feature = "test-utils"))]
use crate::application::agent_workspace_pr_autofix_attempt::{
    load_latest_exact_pr_autofix_run_for_pr, load_pr_autofix_attempt_decision,
    PrAutofixAttemptDecision,
};
use crate::application::agent_workspace_publish_lease::publish_operation_lease_is_live;
#[cfg(any(test, feature = "test-utils"))]
use crate::application::agent_workspace_publish_repair_state::{
    abort_agent_workspace_pr_fix_review_handoff, claim_agent_workspace_repair,
    reconcile_active_agent_workspace_repair, restore_refreshed_agent_workspace_pr_fix_claim,
    settle_terminal_agent_workspace_repair, terminal_run_authorizes_repair_recovery,
    AgentWorkspaceRepairClaim,
};
#[cfg(any(test, feature = "test-utils"))]
use crate::application::agent_workspace_review::{
    resolve_review_target, AgentWorkspaceReviewTarget,
};
#[cfg(any(test, feature = "test-utils"))]
use crate::application::agent_workspace_review_publish_handoff::{
    has_open_pr_fix_workspace_review_publish_handoff,
    has_pending_pr_fix_workspace_review_publish_handoff,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspacePublicationEvent, AgentRunId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentWorkspacePublicationGuard,
    AgentWorkspacePublicationUpdate,
};
#[cfg(any(test, feature = "test-utils"))]
use crate::domain::repositories::{AgentRunRepository, ProjectRepository};
#[cfg(any(test, feature = "test-utils"))]
use crate::error::AppError;
use crate::error::AppResult;

#[cfg(any(test, feature = "test-utils"))]
pub(super) const STALE_REPAIR_RECOVERED_STEP: &str = "stale_repair_recovered";
#[cfg(any(test, feature = "test-utils"))]
pub(super) const STALE_NEEDS_AGENT_CLASSIFICATION: &str = "stale_needs_agent";
#[cfg(any(test, feature = "test-utils"))]
pub(super) const STALE_REPAIR_BLOCKED_SUMMARY: &str =
    "Recovered stale workspace repair state; no active repair run is running.";
pub(super) const STALE_TRANSIENT_RECOVERED_STEP: &str = "stale_transient_recovered";
pub(super) const STALE_TRANSIENT_CLASSIFICATION: &str = "stale_transient_status";
pub(crate) const AGENT_WORKSPACE_PUBLISH_REDRIVE_REQUESTED: &str =
    "agent-workspace:publish-redrive-requested";
pub(crate) const AGENT_WORKSPACE_PUBLISH_REDRIVE_PENDING_STATUS: &str = "redrive_pending";
pub(crate) const AGENT_WORKSPACE_PUBLISH_REDRIVE_DELIVERING_STATUS: &str = "redrive_delivering";
mod durable_attempt_recovery;
mod pr_autofix_redelivery;

pub(crate) use durable_attempt_recovery::agent_workspace_repair_owns_unpublished_publish_continuation;
pub(crate) use durable_attempt_recovery::blocked_repair_fences_new_base_work;

#[cfg(test)]
pub(crate) use durable_attempt_recovery::due_repair_dispatch_message;
pub(crate) use durable_attempt_recovery::is_blocked_and_not_auto_retryable;
#[cfg(any(test, feature = "test-utils"))]
pub use durable_attempt_recovery::recover_agent_workspace_repair_after_terminal_run;
#[cfg(not(any(test, feature = "test-utils")))]
pub(crate) use durable_attempt_recovery::recover_agent_workspace_repair_after_terminal_run;
pub(crate) use durable_attempt_recovery::recover_agent_workspace_repair_attempts_for_state;
pub(crate) use durable_attempt_recovery::recover_stale_publish_repair_for_workspace_in_state_result;
#[cfg(test)]
pub(crate) use durable_attempt_recovery::CONTINUATION_RECOVERY_BLOCKED_STEP;
#[cfg(test)]
pub(crate) use durable_attempt_recovery::{
    recover_agent_workspace_repair_continuation, DurableRepairRecoveryOutcome,
};
pub(crate) use durable_attempt_recovery::{
    AUTO_RETRY_BLOCKED_REPAIR_REASON_PREFIX, AUTO_RETRY_READY_REPAIR_REASON_PREFIX,
    BLOCKED_STREAK_REARMED_REASON_PREFIX, EXHAUSTED_PUBLISH_REDRIVE_CHECKED_REASON_PREFIX,
};
pub(crate) use durable_attempt_recovery::{
    CONTINUATION_OPEN_EFFECT_ATTENTION_REASON, CONTINUATION_OPEN_EFFECT_EVIDENCE_REASON_PREFIX,
    CONTINUATION_OPEN_EFFECT_REARMED_STEP, CONTINUATION_OPEN_EFFECT_RECOVERY_REASON_PREFIX,
};
#[cfg(test)]
pub(crate) use pr_autofix_redelivery::due_pr_autofix_redispatch_message;
pub(crate) use pr_autofix_redelivery::pr_autofix_fingerprint_spend;
#[cfg(test)]
pub(crate) use pr_autofix_redelivery::{evaluate_pr_autofix_successor, PrAutofixSuccessorDecision};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StalePublishRepairRecoveryOutcome {
    Noop,
    #[cfg(any(test, feature = "test-utils"))]
    ActiveReplacement,
    ActiveRepairReconciled,
    RetryEligible,
    Manual,
    #[cfg(any(test, feature = "test-utils"))]
    HandoffPreserved,
    #[cfg(any(test, feature = "test-utils"))]
    TerminalRecovered,
}

impl StalePublishRepairRecoveryOutcome {
    fn was_recovered(self) -> bool {
        #[cfg(any(test, feature = "test-utils"))]
        if matches!(self, Self::TerminalRecovered) {
            return true;
        }
        matches!(self, Self::RetryEligible | Self::Manual)
    }
}

#[cfg(any(test, feature = "test-utils"))]
pub async fn recover_stale_agent_workspace_publish_repairs_on_startup(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
) {
    match recover_stale_agent_workspace_publish_repairs(workspace_repo, agent_run_repo).await {
        Ok(count) if count > 0 => {
            tracing::info!(
                count,
                "Recovered stale agent workspace publish repair states on startup"
            );
        }
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(
                error = %error,
                "Failed to recover stale agent workspace publish repair states on startup"
            );
        }
    }
}

pub async fn recover_stale_agent_workspace_publish_repairs_on_startup_for_state(state: &AppState) {
    match recover_stale_agent_workspace_publish_repairs_for_state(state).await {
        Ok(count) if count > 0 => {
            tracing::info!(
                count,
                "Recovered stale agent workspace publish repair states on startup"
            );
        }
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(
                error = %error,
                "Failed to recover stale agent workspace publish repair states on startup"
            );
        }
    }
}

#[cfg(any(test, feature = "test-utils"))]
pub async fn recover_stale_agent_workspace_publish_repairs(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
) -> AppResult<u32> {
    let workspaces = workspace_repo.list_active_needs_agent_workspaces().await?;
    let mut recovered = 0u32;

    for workspace in workspaces {
        if recover_stale_publish_repair_for_workspace(
            Arc::clone(&workspace_repo),
            Arc::clone(&agent_run_repo),
            workspace,
        )
        .await?
        {
            recovered += 1;
        }
    }

    Ok(recovered)
}

pub async fn recover_stale_agent_workspace_publish_repairs_for_state(
    state: &AppState,
) -> AppResult<u32> {
    let mut recovered = recover_agent_workspace_repair_attempts_for_state(state).await?;
    let workspaces = state
        .agent_conversation_workspace_repo
        .list_active_needs_agent_workspaces()
        .await?;

    for workspace in workspaces {
        let (_workspace, outcome) =
            recover_stale_publish_repair_for_workspace_in_state_result(state, workspace).await?;
        if outcome.was_recovered() {
            recovered += 1;
        }
    }

    Ok(recovered)
}

pub async fn recover_stale_publish_repair_for_workspace_in_state(
    state: &crate::application::AppState,
    workspace: AgentConversationWorkspace,
) -> AppResult<AgentConversationWorkspace> {
    recover_stale_publish_repair_for_workspace_in_state_result(state, workspace)
        .await
        .map(|(workspace, _)| workspace)
}

#[cfg(test)]
pub(crate) async fn recover_stale_publish_repair_for_workspace_with_project_repo(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    workspace: AgentConversationWorkspace,
) -> AppResult<(AgentConversationWorkspace, bool)> {
    recover_stale_publish_repair_for_workspace_with_project_repo_outcome(
        workspace_repo,
        agent_run_repo,
        project_repo,
        workspace,
    )
    .await
    .map(|(workspace, outcome)| (workspace, outcome.was_recovered()))
}

#[cfg(any(test, feature = "test-utils"))]
pub(crate) async fn recover_stale_publish_repair_for_workspace_with_project_repo_outcome(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    workspace: AgentConversationWorkspace,
) -> AppResult<(
    AgentConversationWorkspace,
    StalePublishRepairRecoveryOutcome,
)> {
    if agent_run_repo
        .get_active_for_conversation(&workspace.conversation_id)
        .await?
        .is_none()
    {
        let publication_events = workspace_repo
            .list_publication_events(&workspace.conversation_id)
            .await?;
        if has_pending_pr_fix_workspace_review_publish_handoff(&publication_events) {
            let latest_run = agent_run_repo
                .get_latest_for_conversation(&workspace.conversation_id)
                .await?;
            let latest_run = latest_run.filter(|run| {
                run.status.is_terminal()
                    && terminal_run_authorizes_repair_recovery(&workspace, &publication_events, run)
            });
            let Some(latest_run) = latest_run else {
                return abort_invalid_pr_fix_review_handoff(
                    Arc::clone(&workspace_repo),
                    workspace,
                    "Recovered a PR fix Review handoff without current terminal repair evidence. Start a fresh repair attempt.",
                )
                .await;
            };
            let current_review_target =
                current_pr_fix_review_handoff_target(project_repo.as_ref(), &workspace).await;
            let current_review_target = match current_review_target {
                Ok(target) => target,
                Err(AppError::WorkspaceReviewUnfinishedGitOperation) => {
                    return abort_invalid_pr_fix_review_handoff(
                        Arc::clone(&workspace_repo),
                        workspace,
                        "Recovered a PR fix Review handoff with an unfinished Git operation. Resolve conflicts and start a fresh repair attempt.",
                    )
                    .await;
                }
                Err(error) => return Err(error),
            };
            let review_monitor = workspace_repo
                .get_workspace_review_monitor(&workspace.conversation_id)
                .await?;
            if !has_open_pr_fix_workspace_review_publish_handoff(
                &publication_events,
                review_monitor.as_ref(),
                current_review_target.as_ref(),
            ) {
                return abort_invalid_pr_fix_review_handoff(
                    Arc::clone(&workspace_repo),
                    workspace,
                    "Recovered a PR fix Review handoff without a current Review monitor. Start a fresh repair attempt.",
                )
                .await;
            }
            tracing::info!(
                conversation_id = workspace.conversation_id.as_str(),
                agent_run_id = %latest_run.id,
                agent_run_status = %latest_run.status,
                "Preserved terminal PR fix state while its current Workspace Review handoff remains open"
            );
            return Ok((
                workspace,
                StalePublishRepairRecoveryOutcome::HandoffPreserved,
            ));
        }
    }
    recover_stale_publish_repair_for_workspace_and_reload_with_review_target(
        workspace_repo,
        agent_run_repo,
        workspace,
        None,
    )
    .await
}

#[cfg(any(test, feature = "test-utils"))]
async fn abort_invalid_pr_fix_review_handoff(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    workspace: AgentConversationWorkspace,
    summary: &str,
) -> AppResult<(
    AgentConversationWorkspace,
    StalePublishRepairRecoveryOutcome,
)> {
    let conversation_id = workspace.conversation_id.clone();
    let claim = AgentWorkspaceRepairClaim {
        conversation_id: conversation_id.clone(),
        guard: crate::domain::repositories::AgentWorkspaceRepairStateGuard::from_workspace(
            &workspace,
        ),
    };
    let recovered =
        abort_agent_workspace_pr_fix_review_handoff(Arc::clone(&workspace_repo), &claim, summary)
            .await?
            .is_some();
    if !recovered {
        return Ok((workspace, StalePublishRepairRecoveryOutcome::Noop));
    }
    Ok((
        workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await?
            .unwrap_or(workspace),
        StalePublishRepairRecoveryOutcome::Manual,
    ))
}

#[cfg(any(test, feature = "test-utils"))]
pub async fn recover_stale_publish_repair_for_workspace_and_reload(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    workspace: AgentConversationWorkspace,
) -> AppResult<AgentConversationWorkspace> {
    let (workspace, _outcome) =
        recover_stale_publish_repair_for_workspace_and_reload_with_review_target(
            workspace_repo,
            agent_run_repo,
            workspace,
            None,
        )
        .await?;
    Ok(workspace)
}

#[cfg(any(test, feature = "test-utils"))]
pub(crate) async fn recover_stale_publish_repair_for_workspace_and_reload_with_review_target(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    workspace: AgentConversationWorkspace,
    current_review_target: Option<&AgentWorkspaceReviewTarget>,
) -> AppResult<(
    AgentConversationWorkspace,
    StalePublishRepairRecoveryOutcome,
)> {
    let conversation_id = workspace.conversation_id;
    let recovered = recover_stale_publish_repair_for_workspace_with_review_target(
        Arc::clone(&workspace_repo),
        agent_run_repo,
        workspace.clone(),
        current_review_target,
    )
    .await?;
    if recovered.was_recovered()
        || matches!(
            recovered,
            StalePublishRepairRecoveryOutcome::ActiveReplacement
                | StalePublishRepairRecoveryOutcome::ActiveRepairReconciled
        )
    {
        return Ok((
            workspace_repo
                .get_by_conversation_id(&conversation_id)
                .await?
                .unwrap_or(workspace),
            recovered,
        ));
    }

    Ok((workspace, recovered))
}

#[cfg(any(test, feature = "test-utils"))]
async fn current_pr_fix_review_handoff_target(
    project_repo: &dyn ProjectRepository,
    workspace: &AgentConversationWorkspace,
) -> AppResult<Option<AgentWorkspaceReviewTarget>> {
    let Some(project) = project_repo.get_by_id(&workspace.project_id).await? else {
        return Ok(None);
    };
    resolve_review_target(workspace, &project).await
}

#[cfg(any(test, feature = "test-utils"))]
pub async fn recover_stale_publish_repair_for_workspace(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    workspace: AgentConversationWorkspace,
) -> AppResult<bool> {
    recover_stale_publish_repair_for_workspace_with_review_target(
        workspace_repo,
        agent_run_repo,
        workspace,
        None,
    )
    .await
    .map(StalePublishRepairRecoveryOutcome::was_recovered)
}

#[cfg(any(test, feature = "test-utils"))]
async fn recover_stale_publish_repair_for_workspace_with_review_target(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    mut workspace: AgentConversationWorkspace,
    current_review_target: Option<&AgentWorkspaceReviewTarget>,
) -> AppResult<StalePublishRepairRecoveryOutcome> {
    let refreshed_fixing = workspace.publication_push_status.as_deref() == Some("refreshed")
        && workspace.pr_supervision_status.as_deref() == Some("fixing");
    if workspace.publication_push_status.as_deref() != Some("needs_agent") && !refreshed_fixing {
        return Ok(StalePublishRepairRecoveryOutcome::Noop);
    }

    let latest_exact_autofix_run = match workspace.publication_pr_number {
        Some(pr_number) => {
            load_latest_exact_pr_autofix_run_for_pr(
                agent_run_repo.as_ref(),
                &workspace.conversation_id,
                pr_number,
            )
            .await?
        }
        None => None,
    };
    if refreshed_fixing {
        if latest_exact_autofix_run.is_none()
            || restore_refreshed_agent_workspace_pr_fix_claim(
                Arc::clone(&workspace_repo),
                &workspace,
            )
            .await?
            .is_none()
        {
            return Ok(StalePublishRepairRecoveryOutcome::Noop);
        }
        workspace = workspace_repo
            .get_by_conversation_id(&workspace.conversation_id)
            .await?
            .unwrap_or(workspace);
    }
    let publication_events = workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await?;
    let exact_autofix = latest_exact_autofix_run
        .as_ref()
        .and_then(|run| {
            workspace
                .publication_pr_number
                .zip(run.action_target_id.clone())
        })
        .or_else(|| legacy_pr_autofix_tuple(&workspace, &publication_events));
    let agent_run_tuple_is_authoritative = latest_exact_autofix_run.is_some();
    if latest_exact_autofix_run
        .as_ref()
        .is_some_and(|run| run.status.is_active())
    {
        if workspace.pr_supervision_status.as_deref() == Some("blocked") {
            claim_agent_workspace_repair(
                Arc::clone(&workspace_repo),
                &workspace.conversation_id,
                "Matching PR autofix replacement is in progress.",
                workspace.pr_auto_merge_current,
            )
            .await?;
        }
        return Ok(StalePublishRepairRecoveryOutcome::ActiveReplacement);
    }
    if agent_run_repo
        .get_active_for_conversation(&workspace.conversation_id)
        .await?
        .is_some()
        && reconcile_active_agent_workspace_repair(
            Arc::clone(&workspace_repo),
            Arc::clone(&agent_run_repo),
            &workspace,
        )
        .await?
    {
        return Ok(StalePublishRepairRecoveryOutcome::ActiveRepairReconciled);
    }

    let review_monitor = workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await?;
    if has_open_pr_fix_workspace_review_publish_handoff(
        &publication_events,
        review_monitor.as_ref(),
        current_review_target,
    ) {
        tracing::info!(
            conversation_id = workspace.conversation_id.as_str(),
            "Skipped stale publish repair recovery while PR fix waits on current Workspace Review"
        );
        return Ok(StalePublishRepairRecoveryOutcome::HandoffPreserved);
    }

    let (summary, latest_run, outcome) = if let Some((pr_number, fingerprint)) = exact_autofix {
        let decision = load_pr_autofix_attempt_decision(
            agent_run_repo.as_ref(),
            &workspace.conversation_id,
            pr_number,
            &fingerprint,
            !agent_run_tuple_is_authoritative,
        )
        .await?;
        if decision == PrAutofixAttemptDecision::Active {
            return Ok(StalePublishRepairRecoveryOutcome::Noop);
        }
        let summary = decision.manual_summary().unwrap_or(
            "The exact PR autofix attempt failed; supervision remains blocked while the single retry is eligible.",
        );
        let outcome = if decision.allows_start() {
            StalePublishRepairRecoveryOutcome::RetryEligible
        } else {
            StalePublishRepairRecoveryOutcome::Manual
        };
        (summary.to_string(), None, outcome)
    } else {
        let latest_run = agent_run_repo
            .get_by_conversation(&workspace.conversation_id)
            .await?
            .into_iter()
            .filter(|run| run.status.is_terminal())
            .max_by_key(|run| run.started_at);
        let Some(latest_run) = latest_run else {
            return Ok(StalePublishRepairRecoveryOutcome::Noop);
        };
        if !latest_run.status.is_terminal() {
            return Ok(StalePublishRepairRecoveryOutcome::Noop);
        }
        (
            STALE_REPAIR_BLOCKED_SUMMARY.to_string(),
            Some(latest_run),
            StalePublishRepairRecoveryOutcome::TerminalRecovered,
        )
    };

    if let Some(latest_run) = latest_run.as_ref() {
        if !terminal_run_authorizes_repair_recovery(&workspace, &publication_events, latest_run) {
            return Ok(StalePublishRepairRecoveryOutcome::Noop);
        }
    }

    if !settle_terminal_agent_workspace_repair(Arc::clone(&workspace_repo), &workspace, &summary)
        .await?
    {
        return Ok(StalePublishRepairRecoveryOutcome::Noop);
    }
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            workspace.conversation_id,
            STALE_REPAIR_RECOVERED_STEP,
            "succeeded",
            "Recovered stale publish repair state; no active workspace agent repair is running.",
            Some(STALE_NEEDS_AGENT_CLASSIFICATION.to_string()),
        ))
        .await?;

    if let Some(latest_run) = latest_run {
        tracing::info!(
            conversation_id = latest_run.conversation_id.as_str(),
            agent_run_id = %latest_run.id,
            agent_run_status = %latest_run.status,
            "Recovered stale agent workspace publish repair state"
        );
    }

    Ok(outcome)
}

#[cfg(any(test, feature = "test-utils"))]
fn legacy_pr_autofix_tuple(
    workspace: &AgentConversationWorkspace,
    publication_events: &[AgentConversationWorkspacePublicationEvent],
) -> Option<(i64, String)> {
    let pr_number = workspace.publication_pr_number?;
    publication_events
        .iter()
        .filter(|event| event.step == "pr_autofix")
        .filter(|event| {
            event
                .classification
                .as_deref()
                .is_some_and(|value| value.starts_with("github_pr_autofix:"))
        })
        .max_by_key(|event| event.created_at)
        .and_then(|event| event.classification.as_deref())
        .map(|fingerprint| (pr_number, fingerprint.to_string()))
}

pub async fn recover_stale_transient_publish_statuses(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    stale_older_than_secs: u64,
) -> AppResult<u32> {
    let workspaces = workspace_repo
        .list_active_transient_publish_status_workspaces(stale_older_than_secs)
        .await?;
    let mut recovered = 0u32;

    for workspace in workspaces {
        // This compatibility entry point has no run-liveness authority. It may recover only
        // pre-lease rows; owned rows are handled by the AppState-aware live/startup path below.
        if workspace.publish_lease_owner_run_id.is_some() || workspace.publish_lease_token.is_some()
        {
            continue;
        }
        recover_stale_transient_workspace(workspace_repo.as_ref(), &workspace).await?;
        append_stale_transient_recovery_event(
            workspace_repo.as_ref(),
            &workspace,
            workspace
                .publication_push_status
                .as_deref()
                .unwrap_or_default(),
            true,
        )
        .await?;
        recovered += 1;
    }

    Ok(recovered)
}

pub async fn recover_stale_transient_publish_statuses_for_state(
    state: &AppState,
    stale_older_than_secs: u64,
) -> AppResult<u32> {
    let events = Arc::clone(&state.events);
    recover_stale_transient_publish_statuses_for_state_with_redrive_emitter(
        state,
        stale_older_than_secs,
        &move |conversation_id| {
            emit_serialized(
                events.as_ref(),
                AGENT_WORKSPACE_PUBLISH_REDRIVE_REQUESTED,
                &conversation_id.as_str(),
            )
            .map_err(|error| error.to_string())
        },
    )
    .await
}

pub(crate) async fn recover_stale_transient_publish_statuses_for_state_with_redrive_emitter<F>(
    state: &AppState,
    stale_older_than_secs: u64,
    emit_redrive: &F,
) -> AppResult<u32>
where
    F: Fn(&crate::domain::entities::ChatConversationId) -> Result<(), String> + Sync,
{
    recover_stale_transient_publish_statuses_for_state_with_emitter(
        state,
        stale_older_than_secs,
        emit_redrive,
    )
    .await
}

async fn recover_stale_transient_publish_statuses_for_state_with_emitter<F>(
    state: &AppState,
    stale_older_than_secs: u64,
    emit_redrive: &F,
) -> AppResult<u32>
where
    F: Fn(&crate::domain::entities::ChatConversationId) -> Result<(), String> + Sync,
{
    // Owned rows are authorized by owner liveness immediately. The configured age remains only
    // the compatibility fallback for pre-lease rows with no owner identity.
    let workspaces = state
        .agent_conversation_workspace_repo
        .list_active_transient_publish_status_workspaces(0)
        .await?;
    let mut recovered = 0u32;
    let legacy_stale_cutoff = chrono::Utc::now()
        - chrono::Duration::seconds(i64::try_from(stale_older_than_secs).unwrap_or(i64::MAX));

    for workspace in workspaces {
        let owner_is_dead = match workspace.publish_lease_owner_run_id.as_deref() {
            Some(owner) if owner.starts_with("publish-operation:") => {
                !publish_operation_lease_is_live(
                    &workspace.conversation_id,
                    workspace.publish_lease_token.as_deref(),
                )
            }
            Some(owner) => match state
                .agent_run_repo
                .get_by_id(&AgentRunId::from_string(owner))
                .await
            {
                Ok(Some(run)) => !run.status.is_active(),
                Ok(None) => true,
                Err(error) => {
                    tracing::warn!(
                        conversation_id = workspace.conversation_id.as_str(),
                        owner_run_id = owner,
                        error = %error,
                        "Refused stale publish lease recovery after an unreadable owner run"
                    );
                    false
                }
            },
            None => {
                stuck_publish_redrive_is_pending(&workspace)
                    || workspace
                        .publish_lease_heartbeat_at
                        .unwrap_or(workspace.updated_at)
                        <= legacy_stale_cutoff
            }
        };
        if !owner_is_dead {
            continue;
        }

        let stuck_status = workspace
            .publication_push_status
            .clone()
            .unwrap_or_default();
        let was_pending = stuck_publish_redrive_is_pending(&workspace);
        let was_delivering = stuck_publish_redrive_is_delivering(&workspace);
        let newly_pending = if was_pending {
            false
        } else if was_delivering {
            restore_pending_redrive_delivery(
                state.agent_conversation_workspace_repo.as_ref(),
                &workspace,
            )
            .await?
        } else if let Some(token) = workspace.publish_lease_token.as_deref() {
            state
                .agent_conversation_workspace_repo
                .release_publish_lease(
                    &workspace.conversation_id,
                    token,
                    Some(AGENT_WORKSPACE_PUBLISH_REDRIVE_PENDING_STATUS),
                    chrono::Utc::now(),
                )
                .await?
        } else {
            mark_redrive_pending(state.agent_conversation_workspace_repo.as_ref(), &workspace)
                .await?
        };
        if !was_pending && !newly_pending {
            continue;
        }
        let mut pending = workspace.clone();
        pending.publication_push_status =
            Some(AGENT_WORKSPACE_PUBLISH_REDRIVE_PENDING_STATUS.to_string());
        let Some(delivering) = claim_pending_redrive_delivery(
            state.agent_conversation_workspace_repo.as_ref(),
            &pending,
        )
        .await?
        else {
            continue;
        };
        let redrive_emitted = match emit_redrive(&workspace.conversation_id) {
            Ok(()) => {
                settle_redrive_delivery(
                    state.agent_conversation_workspace_repo.as_ref(),
                    &delivering,
                )
                .await?
            }
            Err(error) => {
                tracing::warn!(
                    conversation_id = workspace.conversation_id.as_str(),
                    error = %error,
                    "Claimed stale publish re-drive but could not emit it; restoring durable pending state"
                );
                restore_pending_redrive_delivery(
                    state.agent_conversation_workspace_repo.as_ref(),
                    &delivering,
                )
                .await?;
                false
            }
        };
        tracing::info!(
            conversation_id = workspace.conversation_id.as_str(),
            stuck_status = %stuck_status,
            redrive_emitted,
            "Recovered stale transient publish status workspace"
        );
        if redrive_emitted || newly_pending {
            if let Err(error) = append_stale_transient_recovery_event(
                state.agent_conversation_workspace_repo.as_ref(),
                &workspace,
                &stuck_status,
                redrive_emitted,
            )
            .await
            {
                tracing::warn!(
                    conversation_id = workspace.conversation_id.as_str(),
                    error = %error,
                    "Publication re-drive state changed but its stale-lease audit event could not be recorded"
                );
            }
            recovered += 1;
        }
    }

    Ok(recovered)
}

async fn mark_redrive_pending(
    workspace_repo: &dyn AgentConversationWorkspaceRepository,
    workspace: &AgentConversationWorkspace,
) -> AppResult<bool> {
    transition_redrive_delivery_status(
        workspace_repo,
        workspace,
        AGENT_WORKSPACE_PUBLISH_REDRIVE_PENDING_STATUS,
    )
    .await
}

async fn recover_stale_transient_workspace(
    workspace_repo: &dyn AgentConversationWorkspaceRepository,
    workspace: &AgentConversationWorkspace,
) -> AppResult<()> {
    workspace_repo
        .update_publication(
            &workspace.conversation_id,
            workspace.publication_pr_number,
            workspace.publication_pr_url.as_deref(),
            workspace.publication_pr_status.as_deref(),
            Some("refreshed"),
        )
        .await
}

pub(crate) async fn claim_pending_redrive_delivery(
    workspace_repo: &dyn AgentConversationWorkspaceRepository,
    workspace: &AgentConversationWorkspace,
) -> AppResult<Option<AgentConversationWorkspace>> {
    if !stuck_publish_redrive_is_pending(workspace) {
        return Ok(None);
    }
    if transition_redrive_delivery_status(
        workspace_repo,
        workspace,
        AGENT_WORKSPACE_PUBLISH_REDRIVE_DELIVERING_STATUS,
    )
    .await?
    {
        let mut delivering = workspace.clone();
        delivering.publication_push_status =
            Some(AGENT_WORKSPACE_PUBLISH_REDRIVE_DELIVERING_STATUS.to_string());
        Ok(Some(delivering))
    } else {
        Ok(None)
    }
}

pub(crate) async fn settle_redrive_delivery(
    workspace_repo: &dyn AgentConversationWorkspaceRepository,
    workspace: &AgentConversationWorkspace,
) -> AppResult<bool> {
    transition_redrive_delivery_status(workspace_repo, workspace, "refreshed").await
}

async fn restore_pending_redrive_delivery(
    workspace_repo: &dyn AgentConversationWorkspaceRepository,
    workspace: &AgentConversationWorkspace,
) -> AppResult<bool> {
    transition_redrive_delivery_status(
        workspace_repo,
        workspace,
        AGENT_WORKSPACE_PUBLISH_REDRIVE_PENDING_STATUS,
    )
    .await
}

async fn transition_redrive_delivery_status(
    workspace_repo: &dyn AgentConversationWorkspaceRepository,
    workspace: &AgentConversationWorkspace,
    next_status: &str,
) -> AppResult<bool> {
    workspace_repo
        .update_publication_with_events(
            &workspace.conversation_id,
            &AgentWorkspacePublicationGuard::from_workspace(workspace),
            AgentWorkspacePublicationUpdate {
                pr_number: workspace.publication_pr_number,
                pr_url: workspace.publication_pr_url.clone(),
                pr_status: workspace.publication_pr_status.clone(),
                push_status: Some(next_status.to_string()),
            },
            Vec::new(),
        )
        .await
}

async fn append_stale_transient_recovery_event(
    workspace_repo: &dyn AgentConversationWorkspaceRepository,
    workspace: &AgentConversationWorkspace,
    stuck_status: &str,
    redrive_emitted: bool,
) -> AppResult<()> {
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            workspace.conversation_id.clone(),
            STALE_TRANSIENT_RECOVERED_STEP,
            if redrive_emitted { "succeeded" } else { "pending" },
            if redrive_emitted {
                format!(
                    "Recovered stale transient publish status '{stuck_status}'; queued for normal publication re-drive."
                )
            } else {
                format!(
                    "Recovered stale transient publish status '{stuck_status}'; publication re-drive remains durably pending."
                )
            },
            Some(STALE_TRANSIENT_CLASSIFICATION.to_string()),
        ))
        .await
}

fn stuck_publish_redrive_is_pending(workspace: &AgentConversationWorkspace) -> bool {
    workspace.publication_push_status.as_deref()
        == Some(AGENT_WORKSPACE_PUBLISH_REDRIVE_PENDING_STATUS)
}

fn stuck_publish_redrive_is_delivering(workspace: &AgentConversationWorkspace) -> bool {
    workspace.publication_push_status.as_deref()
        == Some(AGENT_WORKSPACE_PUBLISH_REDRIVE_DELIVERING_STATUS)
}

pub async fn run_periodic_workspace_publish_recovery(state: AppState) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(
            crate::infrastructure::agents::claude::git_runtime_config()
                .agent_workspace_publish_recovery_interval_secs,
        ))
        .await;

        if let Err(err) = recover_stale_agent_workspace_publish_repairs_for_state(&state).await {
            tracing::warn!(
                error = %err,
                "Periodic recovery: failed to recover stale needs_agent workspace repairs"
            );
        }

        if let Err(err) = recover_stale_transient_publish_statuses_for_state(
            &state,
            crate::infrastructure::agents::claude::git_runtime_config()
                .agent_workspace_publish_lease_stale_secs,
        )
        .await
        {
            tracing::warn!(
                error = %err,
                "Periodic recovery: failed to recover stale transient publish statuses"
            );
        }

        if let Err(err) = crate::application::agent_workspace_review_auto_merge::
            reconcile_workspace_review_auto_merge_guards(&state)
            .await
        {
            tracing::warn!(
                error = %err,
                "Periodic recovery: failed to reconcile workspace Review auto-merge guards"
            );
        }
    }
}
