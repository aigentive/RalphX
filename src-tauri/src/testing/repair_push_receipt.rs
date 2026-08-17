//! Seeds the authoritative `PushBranch` receipt that proves a workspace repair already reached the
//! remote branch. Tests use it to build continuation-stage blocked attempts, which are the only
//! blocked attempts allowed to stop fencing new base-freshness work.

use crate::domain::entities::{
    AgentWorkspaceRepairAttempt, AgentWorkspaceRepairEffect, AgentWorkspaceRepairEffectKind,
    AgentWorkspaceRepairEffectStatus,
};
use crate::domain::repositories::{
    AgentWorkspaceRepairRepository, CompleteAgentWorkspaceRepairEffect,
    CompleteAgentWorkspaceRepairEffectOutcome, CreateAgentWorkspaceRepairEffect,
    CreateAgentWorkspaceRepairEffectOutcome,
};

/// Records an Observed `PushBranch` effect for `attempt` whose remote OID is `remote_oid`.
///
/// The attempt must still be the current unsettled generation at exactly `attempt.phase` /
/// `attempt.updated_at`, because both effect writes go through the repository CAS.
///
/// # Panics
///
/// Panics when the repository rejects the effect creation or completion, which always means the
/// caller's fixture is stale rather than that the behavior under test failed.
pub async fn record_observed_agent_workspace_repair_push_receipt(
    repo: &dyn AgentWorkspaceRepairRepository,
    attempt: &AgentWorkspaceRepairAttempt,
    remote_oid: &str,
) {
    let idempotency_key = format!(
        "agent_workspace_repair:{}:{}:{}",
        attempt.id,
        attempt.generation,
        AgentWorkspaceRepairEffectKind::PushBranch
    );
    let now = chrono::Utc::now();
    let mut effect = AgentWorkspaceRepairEffect::new(
        attempt.id.clone(),
        AgentWorkspaceRepairEffectKind::PushBranch,
        idempotency_key,
        now,
    );
    effect.intended_head_oid = Some(remote_oid.to_string());
    let effect = match repo
        .create_repair_effect(CreateAgentWorkspaceRepairEffect {
            attempt_id: attempt.id.clone(),
            generation: attempt.generation,
            expected_phase: attempt.phase,
            expected_attempt_updated_at: attempt.updated_at,
            effect,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("repair push effect should persist")
    {
        CreateAgentWorkspaceRepairEffectOutcome::Created(effect) => effect,
        outcome => panic!("repair push effect must be created, got {outcome:?}"),
    };

    let mut observed = effect.clone();
    observed.status = AgentWorkspaceRepairEffectStatus::Observed;
    observed.receipt_json = Some(format!(
        "{{\"remote_ref\":\"refs/heads/repair\",\"remote_oid\":\"{remote_oid}\"}}"
    ));
    observed.completed_at = Some(now + chrono::Duration::seconds(1));
    observed.updated_at = observed.completed_at.expect("completion timestamp");
    match repo
        .complete_repair_effect(CompleteAgentWorkspaceRepairEffect {
            attempt_id: attempt.id.clone(),
            generation: attempt.generation,
            expected_phase: attempt.phase,
            expected_attempt_updated_at: attempt.updated_at,
            expected_effect_updated_at: effect.updated_at,
            expected_effect_status: AgentWorkspaceRepairEffectStatus::Pending,
            effect: observed,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("repair push receipt should settle")
    {
        CompleteAgentWorkspaceRepairEffectOutcome::Applied(_) => {}
        outcome => panic!("repair push receipt must apply, got {outcome:?}"),
    }
}
