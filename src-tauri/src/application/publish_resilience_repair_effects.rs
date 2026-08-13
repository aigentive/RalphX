//! Durable repair-effect identity resolution and orphaned-effect termination.
//!
//! A repair attempt may hold at most one open effect at a time, so a durable effect that its
//! owning process abandoned fences every recovery exit for that attempt. These helpers resolve
//! which effect currently represents a given effect kind (base key or an ordinal retry key), and
//! terminate the one orphaned shape that is provably safe to replay.

use crate::application::publish_resilience::{
    fail_agent_workspace_repair_effect_for_phase, observed_workspace_repair_push_outcome,
    AgentWorkspaceRepairPushOutcome,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspacePublicationEvent, AgentWorkspaceRepairAttempt,
    AgentWorkspaceRepairEffect, AgentWorkspaceRepairEffectKind, AgentWorkspaceRepairEffectStatus,
    AgentWorkspaceRepairPhase,
};
use crate::domain::repositories::AgentWorkspaceRepairRepository;
use crate::error::AppResult;

/// Publication step recorded when an orphaned in-flight PR-update handoff is terminated.
pub(crate) const REPAIR_PR_HANDOFF_EFFECT_TERMINATED_STEP: &str =
    "repair_pr_handoff_effect_terminated";

/// Durable reason stored on the terminated effect. It states the exact proof that authorized the
/// termination so the effect history explains why the fence cleared.
const ORPHANED_PR_HANDOFF_TERMINATION_REASON: &str =
    "terminated an orphaned in-flight PR-update handoff on a blocked repair; the durable push \
     receipt proves the branch head already reached the remote, so the handoff is safely replayable";

/// Bounds the ordinal retry identity space of a single effect kind on one attempt.
const MAX_RETRY_ORDINAL: u32 = 50;

/// Which durable identity currently represents an effect kind on an attempt.
#[derive(Debug)]
pub(crate) enum RepairEffectIdentity {
    /// A row that may still complete: observed, in flight, pending, or a pre-backfill failed row
    /// that was never closed.
    Live(Box<AgentWorkspaceRepairEffect>),
    /// Every row for this kind is terminated (`Failed` with `completed_at`), so a replay needs a
    /// fresh ordinal identity.
    Terminated,
    /// No row exists yet for this kind.
    Absent,
}

/// The deterministic first idempotency key for an effect kind on an attempt generation.
pub(crate) fn repair_effect_base_idempotency_key(
    attempt: &AgentWorkspaceRepairAttempt,
    kind: AgentWorkspaceRepairEffectKind,
) -> String {
    format!(
        "agent_workspace_repair:{}:{}:{}",
        attempt.id, attempt.generation, kind
    )
}

/// True when the effect is closed as failed, which permanently retires its identity: the SQLite
/// writer only matches rows with `completed_at IS NULL`, so such a row can never be completed.
fn is_terminated_repair_effect(effect: &AgentWorkspaceRepairEffect) -> bool {
    effect.status == AgentWorkspaceRepairEffectStatus::Failed && effect.completed_at.is_some()
}

/// Resolves the effect that currently owns `kind` for this attempt, following the base key and its
/// contiguous ordinal retry keys. Ordinal keys are only ever minted for the next free ordinal, so
/// stopping at the first absent key is a complete walk, not a truncation.
pub(crate) async fn resolve_repair_effect_identity(
    repair_repo: &dyn AgentWorkspaceRepairRepository,
    attempt: &AgentWorkspaceRepairAttempt,
    kind: AgentWorkspaceRepairEffectKind,
) -> AppResult<RepairEffectIdentity> {
    let base_idempotency_key = repair_effect_base_idempotency_key(attempt, kind);
    let mut seen_any = false;
    for ordinal in 1..=MAX_RETRY_ORDINAL {
        let candidate = if ordinal == 1 {
            base_idempotency_key.clone()
        } else {
            format!("{base_idempotency_key}#{ordinal}")
        };
        let Some(effect) = repair_repo
            .get_repair_effect_by_idempotency_key(&candidate)
            .await?
        else {
            break;
        };
        seen_any = true;
        if !is_terminated_repair_effect(&effect) {
            return Ok(RepairEffectIdentity::Live(Box::new(effect)));
        }
    }
    if seen_any {
        Ok(RepairEffectIdentity::Terminated)
    } else {
        Ok(RepairEffectIdentity::Absent)
    }
}

/// Finds the next never-used ordinal-suffixed idempotency key for a repeat effect after the base
/// key's effect terminated as `Failed`. `idempotency_key` is unique table-wide, so a retry cannot
/// reuse the base key; this is a bounded read (one lookup per ordinal, capped) rather than an
/// unbounded scan.
pub(crate) async fn next_agent_workspace_repair_retry_idempotency_key(
    repair_repo: &dyn AgentWorkspaceRepairRepository,
    base_idempotency_key: &str,
) -> AppResult<String> {
    for ordinal in 2..=MAX_RETRY_ORDINAL {
        let candidate = format!("{base_idempotency_key}#{ordinal}");
        if repair_repo
            .get_repair_effect_by_idempotency_key(&candidate)
            .await?
            .is_none()
        {
            return Ok(candidate);
        }
    }
    Err(crate::error::AppError::Conflict(
        "workspace repair effect retry identity space exhausted".to_string(),
    ))
}

/// Durable proof that the attempt's repair head already reached the remote: an observed
/// `push_branch` receipt, under the base key or any of its ordinal retry keys, whose intended head
/// and recorded remote OID both equal `repair_head`. Resolving across ordinals matters because a
/// previously terminated push leaves the base key `Failed`, which would otherwise retire the proof.
pub(crate) async fn observed_repair_push_receipt_for_head(
    repair_repo: &dyn AgentWorkspaceRepairRepository,
    attempt: &AgentWorkspaceRepairAttempt,
    repair_head: &str,
) -> AppResult<Option<String>> {
    let base_idempotency_key =
        repair_effect_base_idempotency_key(attempt, AgentWorkspaceRepairEffectKind::PushBranch);
    for ordinal in 1..=MAX_RETRY_ORDINAL {
        let candidate = if ordinal == 1 {
            base_idempotency_key.clone()
        } else {
            format!("{base_idempotency_key}#{ordinal}")
        };
        let Some(effect) = repair_repo
            .get_repair_effect_by_idempotency_key(&candidate)
            .await?
        else {
            break;
        };
        if effect.attempt_id != attempt.id
            || effect.kind != AgentWorkspaceRepairEffectKind::PushBranch
            || effect.status != AgentWorkspaceRepairEffectStatus::Observed
            || effect.intended_head_oid.as_deref() != Some(repair_head)
        {
            continue;
        }
        let AgentWorkspaceRepairPushOutcome::Observed { remote_oid, .. } =
            observed_workspace_repair_push_outcome(effect)?
        else {
            continue;
        };
        if remote_oid == repair_head {
            return Ok(Some(remote_oid));
        }
    }
    Ok(None)
}

/// Terminates an orphaned in-flight PR-update handoff on a blocked repair attempt.
///
/// A `Blocked` phase proves the publish continuation already returned with an error, so the
/// process that owned the effect is not running and the effect can never complete on its own. The
/// observed push receipt proves the only non-idempotent half already landed, which makes replaying
/// the PR-metadata handoff idempotent. Terminating the effect closes it, releasing the attempt's
/// one-open-effect slot so both the automatic blocked-retry sweep and the explicit retry admission
/// stop being fenced.
///
/// The decision reads durable receipts only — never GitHub — so a `gh` outage cannot re-deadlock
/// this escape hatch. `create_pr` effects are out of scope here for the same reason: no durable
/// receipt can prove whether a PR creation landed, so replaying one from receipts alone risks a
/// duplicate pull request. Their settlement is owned by
/// `publish_resilience_create_pr_reconciliation`, which proves absence against GitHub first.
///
/// Returns true when the fence was cleared.
///
/// # Errors
/// Returns an error when a repository read fails or the effect loses attempt authority mid-write.
pub(crate) async fn terminate_orphaned_blocked_repair_pr_handoff_effect(
    state: &AppState,
    observed: &AgentWorkspaceRepairAttempt,
) -> AppResult<bool> {
    let Some(current) = state
        .agent_workspace_repair_repo
        .get_repair_attempt(&observed.id)
        .await?
    else {
        return Ok(false);
    };
    if current.id != observed.id
        || current.generation != observed.generation
        || current.phase != AgentWorkspaceRepairPhase::Blocked
        || current.updated_at != observed.updated_at
        || current.settled_at.is_some()
    {
        return Ok(false);
    }
    let Some(repair_head) = current
        .repair_head_commit
        .as_deref()
        .filter(|head| !head.trim().is_empty())
    else {
        return Ok(false);
    };
    let Some(effect) = state
        .agent_workspace_repair_repo
        .get_open_repair_effect(&current.id)
        .await?
    else {
        return Ok(false);
    };
    if effect.attempt_id != current.id
        || effect.kind != AgentWorkspaceRepairEffectKind::UpdatePr
        || effect.status != AgentWorkspaceRepairEffectStatus::InFlight
        || effect.intended_head_oid.as_deref() != Some(repair_head)
    {
        return Ok(false);
    }
    if observed_repair_push_receipt_for_head(
        state.agent_workspace_repair_repo.as_ref(),
        &current,
        repair_head,
    )
    .await?
    .is_none()
    {
        return Ok(false);
    }

    fail_agent_workspace_repair_effect_for_phase(
        state.agent_workspace_repair_repo.as_ref(),
        &current,
        effect,
        AgentWorkspaceRepairPhase::Blocked,
        ORPHANED_PR_HANDOFF_TERMINATION_REASON,
    )
    .await?;

    // The termination itself is the recovery write. A timeline append that fails afterwards must
    // not undo it, so the event is best-effort and only logged.
    if let Err(error) = state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            current.conversation_id.clone(),
            REPAIR_PR_HANDOFF_EFFECT_TERMINATED_STEP,
            "completed",
            "Cleared an abandoned pull-request update from the blocked repair so it can be retried.",
            Some(repair_head.to_string()),
        ))
        .await
    {
        tracing::warn!(
            conversation_id = current.conversation_id.as_str(),
            attempt_id = current.id.as_str(),
            %error,
            "Terminated an orphaned repair PR-update handoff but could not append its publication event"
        );
    }
    Ok(true)
}
