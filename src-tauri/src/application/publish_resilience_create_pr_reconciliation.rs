//! Postcondition reconciliation for an orphaned in-flight `create_pr` repair effect.
//!
//! Unlike `publish_resilience_repair_effects`, which is durable-receipts-only, this reconciler
//! must read GitHub: a durable row cannot record whether a `gh pr create` subprocess landed
//! before its owning process died. GitHub reporting zero pull requests for the head branch in
//! *any* state is the proof that authorizes a replay; every other shape, including a failed
//! read, leaves the fence intact.

use chrono::Utc;

use crate::application::agent_conversation_workspace::resolve_effective_agent_conversation_workspace_path;
use crate::application::agent_workspace_publish_repair_state::NEEDS_HUMAN_REPAIR_REASON;
use crate::application::publish_resilience::{
    fail_agent_workspace_repair_effect_for_phase,
    observe_agent_workspace_repair_pr_handoff_effect_for_phase,
    release_agent_workspace_repair_lease_after_pr_handoff,
};
use crate::application::publish_resilience_repair_effects::{
    resolve_repair_effect_identity, RepairEffectIdentity,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspacePublicationEvent, AgentWorkspaceRepairAttempt,
    AgentWorkspaceRepairEffectKind, AgentWorkspaceRepairEffectStatus, AgentWorkspaceRepairOutcome,
    AgentWorkspaceRepairPhase, NewNotification, NotificationCategory, NotificationSeverity,
    NotificationTarget, NotificationTargetKind,
};
use crate::domain::repositories::{
    AgentWorkspaceRepairCompatibilityProjection, SettleAgentWorkspaceRepairAttempt,
    SettleAgentWorkspaceRepairAttemptOutcome,
};
use crate::domain::services::github_service::PrStatus;
use crate::error::AppResult;

/// Publication step recorded when GitHub proved the orphaned creation never landed.
pub(crate) const REPAIR_CREATE_PR_EFFECT_NOT_APPLIED_STEP: &str =
    "repair_create_pr_effect_not_applied";
/// Publication step recorded when a pull request exists but is not proven current.
pub(crate) const REPAIR_CREATE_PR_AMBIGUOUS_STEP: &str = "repair_create_pr_ambiguous";
/// Publication step recorded when an existing pull request is adopted as this attempt's outcome.
pub(crate) const REPAIR_CREATE_PR_EFFECT_ADOPTED_STEP: &str = "repair_create_pr_effect_adopted";

/// Durable reason stored on the terminated effect. It states the exact proof that authorized the
/// termination so the effect history explains why the fence cleared.
const CREATE_PR_NEVER_APPLIED_REASON: &str =
    "terminated an orphaned in-flight pull-request creation on a blocked repair; GitHub reports \
     no pull request for this head branch in any state, proving the creation never landed";

/// Outcome of evaluating an orphaned in-flight `create_pr` effect against GitHub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockedCreatePrEffectReconciliation {
    /// GitHub proved no pull request exists; the effect was terminated and the fence is clear.
    NotApplied,
    /// The pull request exists at the repair head; the effect was observed and the attempt settled.
    Adopted,
    /// A pull request exists but is not proven current; no write was made.
    AmbiguousPrExists,
    /// Nothing was proven (wrong shape, lost authority, or a failed read); no write was made.
    Pending,
}

fn ambiguous_notification_dedupe_key(attempt: &AgentWorkspaceRepairAttempt) -> String {
    format!(
        "repair_create_pr_ambiguous:{}:{}",
        attempt.conversation_id, attempt.id
    )
}

fn open_effect_notification_dedupe_key(attempt: &AgentWorkspaceRepairAttempt) -> String {
    format!(
        "repair_open_effect:{}:{}",
        attempt.conversation_id, attempt.id
    )
}

/// Settles an orphaned in-flight `create_pr` effect on a blocked repair attempt against GitHub.
///
/// A `Blocked` phase proves the publish continuation already returned with an error, so the process
/// that owned the effect is not running and the effect can never complete on its own. Because no
/// durable receipt can prove whether the creation landed, the only admissible evidence is GitHub
/// itself: zero pull requests for the head branch in any state authorizes terminating the effect,
/// and an open pull request whose head OID equals the repair head authorizes adopting it. Every
/// other shape, including a failed read, performs no durable write and leaves the fence intact.
///
/// # Errors
/// Returns an error when a repository write loses attempt authority mid-flight.
pub(crate) async fn reconcile_blocked_agent_workspace_repair_create_pr_effect(
    state: &AppState,
    observed: &AgentWorkspaceRepairAttempt,
) -> AppResult<BlockedCreatePrEffectReconciliation> {
    let Some(current) = state
        .agent_workspace_repair_repo
        .get_repair_attempt(&observed.id)
        .await?
    else {
        return Ok(BlockedCreatePrEffectReconciliation::Pending);
    };
    if current.id != observed.id
        || current.generation != observed.generation
        || current.phase != AgentWorkspaceRepairPhase::Blocked
        || current.updated_at != observed.updated_at
        || current.settled_at.is_some()
    {
        return Ok(BlockedCreatePrEffectReconciliation::Pending);
    }
    // An explicit human hold outranks automatic settlement: the operator may already be resolving
    // this pull request by hand.
    if current
        .pending_reasons
        .iter()
        .any(|reason| reason == NEEDS_HUMAN_REPAIR_REASON)
    {
        return Ok(BlockedCreatePrEffectReconciliation::Pending);
    }
    let Some(repair_head) = current
        .repair_head_commit
        .as_deref()
        .map(str::trim)
        .filter(|head| !head.is_empty())
    else {
        return Ok(BlockedCreatePrEffectReconciliation::Pending);
    };

    // Resolve by identity rather than the open-effect lookup so an ordinal retry key
    // (`…:create_pr#2`) is handled the same way the sibling reconcilers handle it.
    let RepairEffectIdentity::Live(effect) = resolve_repair_effect_identity(
        state.agent_workspace_repair_repo.as_ref(),
        &current,
        AgentWorkspaceRepairEffectKind::CreatePr,
    )
    .await?
    else {
        return Ok(BlockedCreatePrEffectReconciliation::Pending);
    };
    let effect = *effect;
    // `Live` also matches `Pending`, `Observed`, and pre-backfill unclosed `Failed` rows, so
    // narrowing to `InFlight` is required, not defensive. The `intended_head_oid` guard keeps a
    // creation begun against an older head from being adopted against a newer repair head.
    if effect.attempt_id != current.id
        || effect.kind != AgentWorkspaceRepairEffectKind::CreatePr
        || effect.status != AgentWorkspaceRepairEffectStatus::InFlight
        || effect.intended_head_oid.as_deref() != Some(repair_head)
    {
        return Ok(BlockedCreatePrEffectReconciliation::Pending);
    }

    let Some(workspace) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&current.conversation_id)
        .await?
    else {
        return Ok(BlockedCreatePrEffectReconciliation::Pending);
    };
    let Some(project) = state.project_repo.get_by_id(&workspace.project_id).await? else {
        return Ok(BlockedCreatePrEffectReconciliation::Pending);
    };
    // An unresolvable working directory proves nothing about GitHub, so it must not abort the
    // sweep for other workspaces.
    let target = match resolve_effective_agent_conversation_workspace_path(
        &project,
        &workspace,
        state.plan_branch_repo.as_ref(),
    )
    .await
    {
        Ok(target) => target,
        Err(error) => {
            tracing::warn!(
                conversation_id = current.conversation_id.as_str(),
                attempt_id = current.id.as_str(),
                %error,
                "Could not resolve the workspace path for a blocked create_pr reconciliation"
            );
            return Ok(BlockedCreatePrEffectReconciliation::Pending);
        }
    };
    let Some(github) = state.github_service.as_ref() else {
        return Ok(BlockedCreatePrEffectReconciliation::Pending);
    };

    // The head branch must match what `pr_publish_service::create_draft_pr` uses as head
    // (src-tauri/src/domain/services/pr_publish_service.rs:169-177), which is always
    // `workspace.branch_name`. Using `target.branch_name` would diverge for linked-plan
    // workspaces and could report "no pull request" on the plan branch while a PR exists on the
    // workspace branch — the exact false proof that would authorize a duplicate creation.
    let head_branch = workspace.branch_name.as_str();
    let found = match github
        .find_latest_pr_by_head_branch(&target.path, head_branch)
        .await
    {
        Ok(found) => found,
        // A failed read proves nothing. Stay fenced and let the next tick re-evaluate.
        Err(error) => {
            tracing::warn!(
                conversation_id = current.conversation_id.as_str(),
                attempt_id = current.id.as_str(),
                %error,
                "Could not read GitHub for a blocked create_pr reconciliation; leaving the fence intact"
            );
            return Ok(BlockedCreatePrEffectReconciliation::Pending);
        }
    };

    let Some(pr) = found else {
        fail_agent_workspace_repair_effect_for_phase(
            state.agent_workspace_repair_repo.as_ref(),
            &current,
            effect,
            AgentWorkspaceRepairPhase::Blocked,
            CREATE_PR_NEVER_APPLIED_REASON,
        )
        .await?;
        record_create_pr_effect_not_applied(state, &current).await;
        return Ok(BlockedCreatePrEffectReconciliation::NotApplied);
    };

    // GitHub answered a head-branch query with a different head. Nothing about this attempt is
    // proven, so decline exactly as the external-PR adopter does.
    if pr.head_ref_name != head_branch {
        tracing::warn!(
            conversation_id = current.conversation_id.as_str(),
            attempt_id = current.id.as_str(),
            expected_head = head_branch,
            found_head = pr.head_ref_name.as_str(),
            "Blocked create_pr reconciliation found a pull request for a different head branch"
        );
        return Ok(BlockedCreatePrEffectReconciliation::Pending);
    }

    let sync_state = match github.check_pr_sync_state(&target.path, pr.number).await {
        Ok(sync_state) => sync_state,
        Err(error) => {
            tracing::warn!(
                conversation_id = current.conversation_id.as_str(),
                attempt_id = current.id.as_str(),
                pr_number = pr.number,
                %error,
                "Could not verify whether the found pull request is current; leaving the fence intact"
            );
            return Ok(BlockedCreatePrEffectReconciliation::Pending);
        }
    };
    if sync_state.status != PrStatus::Open
        || sync_state.head_ref_name != head_branch
        || sync_state.head_ref_oid.as_deref() != Some(repair_head)
    {
        surface_ambiguous_create_pr_attention(state, &current, pr.number).await?;
        return Ok(BlockedCreatePrEffectReconciliation::AmbiguousPrExists);
    }

    // Link the pull request before settling. A settle failure after linking leaves a linked but
    // unsettled workspace, which the next sweep finishes; the reverse order would leave a settled
    // but unlinked workspace that nothing repairs.
    //
    // The unguarded writer is correct here only because the external-PR adopter skips
    // repair-blocked workspaces and the repair pipeline holds the target lease, so there is
    // provably no competing writer for `publication_pr_number` while this attempt is unsettled. If
    // that exclusion ever changes, this call must move to `update_publication_with_events`.
    state
        .agent_conversation_workspace_repo
        .update_publication(
            &current.conversation_id,
            Some(pr.number),
            Some(&pr.url),
            Some(pr.publication_status()),
            Some("pushed"),
        )
        .await?;

    observe_agent_workspace_repair_pr_handoff_effect_for_phase(
        state.agent_workspace_repair_repo.as_ref(),
        &current,
        effect,
        AgentWorkspaceRepairPhase::Blocked,
        pr.number,
        Some(pr.url.as_str()),
    )
    .await?;
    release_agent_workspace_repair_lease_after_pr_handoff(state, &current).await?;

    let settled_at = Utc::now();
    let summary = format!(
        "Adopted the pull request this repair had already created (#{}).",
        pr.number
    );
    let projection = AgentWorkspaceRepairCompatibilityProjection {
        publication_push_status: Some("pushed".to_string()),
        pr_supervision_status: Some("monitoring".to_string()),
        pr_supervision_summary: Some(summary.clone()),
        pr_supervision_updated_at: Some(settled_at),
        pr_auto_merge_current: workspace.pr_auto_merge_current,
        pr_autofix_enabled: None,
        pr_auto_merge_desired: None,
        base_commit: current.target_base_commit.clone(),
    };
    let event = AgentConversationWorkspacePublicationEvent::new(
        current.conversation_id.clone(),
        REPAIR_CREATE_PR_EFFECT_ADOPTED_STEP,
        "completed",
        summary,
        Some(current.id.to_string()),
    );
    match state
        .agent_workspace_repair_repo
        .settle_repair_attempt(SettleAgentWorkspaceRepairAttempt {
            attempt_id: current.id.clone(),
            generation: current.generation,
            expected_phase: AgentWorkspaceRepairPhase::Blocked,
            expected_updated_at: current.updated_at,
            outcome: AgentWorkspaceRepairOutcome::Succeeded,
            settled_at,
            compatibility_projection: Some(projection),
            events: vec![event],
        })
        .await?
    {
        SettleAgentWorkspaceRepairAttemptOutcome::Applied(_) => {
            resolve_attempt_attention_notifications(state, &current).await;
            Ok(BlockedCreatePrEffectReconciliation::Adopted)
        }
        SettleAgentWorkspaceRepairAttemptOutcome::Stale(_)
        | SettleAgentWorkspaceRepairAttemptOutcome::Missing => {
            Ok(BlockedCreatePrEffectReconciliation::Pending)
        }
    }
}

/// The termination itself is the recovery write. A timeline append or notification resolve that
/// fails afterwards must not undo it, so both are best-effort and only logged.
async fn record_create_pr_effect_not_applied(
    state: &AppState,
    attempt: &AgentWorkspaceRepairAttempt,
) {
    let summary = "Workspace repair pull-request creation was not applied: GitHub reports no pull \
                   request for this branch in any state, proving the creation never landed. The \
                   effect fence is now clear.";
    if let Err(error) = state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            attempt.conversation_id.clone(),
            REPAIR_CREATE_PR_EFFECT_NOT_APPLIED_STEP,
            "active",
            summary,
            Some(attempt.id.to_string()),
        ))
        .await
    {
        tracing::warn!(
            conversation_id = attempt.conversation_id.as_str(),
            attempt_id = attempt.id.as_str(),
            %error,
            "Terminated an orphaned repair pull-request creation but could not append its publication event"
        );
    }
    resolve_attempt_attention_notifications(state, attempt).await;
}

/// Both attention keys clear together whenever this attempt reaches a settled shape. An ambiguous
/// pull request that is later deleted or retargeted flips the attempt out of the ambiguous arm,
/// and that notification uses its own key, so the terminating and adopting arms are the only
/// places it can ever be resolved.
async fn resolve_attempt_attention_notifications(
    state: &AppState,
    attempt: &AgentWorkspaceRepairAttempt,
) {
    let notifications = state.notification_service();
    for dedupe_key in [
        open_effect_notification_dedupe_key(attempt),
        ambiguous_notification_dedupe_key(attempt),
    ] {
        notifications
            .resolve_workflow_notification(&dedupe_key)
            .await;
    }
}

/// A pull request exists but is not proven current, so RalphX will neither adopt it nor create a
/// second one. Surface that once per attempt and leave the decision to a human.
async fn surface_ambiguous_create_pr_attention(
    state: &AppState,
    attempt: &AgentWorkspaceRepairAttempt,
    pr_number: i64,
) -> AppResult<()> {
    let summary = format!(
        "A pull request (#{pr_number}) already exists for this branch but does not point at the \
         repaired commit, so RalphX cannot prove the repair reached it. RalphX will not create a \
         second pull request; review it and retry the publish."
    );
    let classification = attempt.id.to_string();
    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&attempt.conversation_id)
        .await?;
    if !events.iter().any(|event| {
        event.step == REPAIR_CREATE_PR_AMBIGUOUS_STEP
            && event.classification.as_deref() == Some(classification.as_str())
    }) {
        state
            .agent_conversation_workspace_repo
            .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                attempt.conversation_id.clone(),
                REPAIR_CREATE_PR_AMBIGUOUS_STEP,
                "attention_required",
                summary.clone(),
                Some(classification),
            ))
            .await?;
    }

    let project_id = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&attempt.conversation_id)
        .await?
        .map(|workspace| workspace.project_id.to_string());
    state
        .notification_service()
        .record(NewNotification {
            project_id: project_id.clone(),
            category: NotificationCategory::TaskBlocked,
            severity: NotificationSeverity::ActionRequired,
            title: "Workspace repair found an unverified pull request".to_string(),
            body: Some(summary),
            target: NotificationTarget {
                kind: NotificationTargetKind::AgentConversation,
                project_id,
                task_id: None,
                conversation_id: Some(attempt.conversation_id.to_string()),
                setup_conversation_id: None,
                automation_id: None,
                run_id: None,
            },
            dedupe_key: Some(ambiguous_notification_dedupe_key(attempt)),
        })
        .await;
    Ok(())
}
