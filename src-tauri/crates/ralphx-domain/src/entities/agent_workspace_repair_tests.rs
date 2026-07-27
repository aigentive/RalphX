use chrono::Utc;

use super::*;

fn conversation_id() -> ChatConversationId {
    ChatConversationId::from_string("5b46a460-1699-47e6-a687-71305f4e5674")
}

#[test]
fn repair_enums_are_closed_snake_case_wire_contracts() {
    for source in [
        AgentWorkspaceRepairSource::BaseUpdate,
        AgentWorkspaceRepairSource::Publish,
        AgentWorkspaceRepairSource::PrConflict,
        AgentWorkspaceRepairSource::PrAutofix,
        AgentWorkspaceRepairSource::Legacy,
    ] {
        assert_eq!(
            source
                .as_str()
                .parse::<AgentWorkspaceRepairSource>()
                .unwrap(),
            source
        );
    }

    for phase in [
        AgentWorkspaceRepairPhase::Requested,
        AgentWorkspaceRepairPhase::Dispatching,
        AgentWorkspaceRepairPhase::Repairing,
        AgentWorkspaceRepairPhase::Validating,
        AgentWorkspaceRepairPhase::AwaitingReview,
        AgentWorkspaceRepairPhase::ContinuationPending,
        AgentWorkspaceRepairPhase::Continuing,
        AgentWorkspaceRepairPhase::Ready,
        AgentWorkspaceRepairPhase::Blocked,
    ] {
        assert_eq!(
            phase.as_str().parse::<AgentWorkspaceRepairPhase>().unwrap(),
            phase
        );
    }

    assert!("unknown".parse::<AgentWorkspaceRepairSource>().is_err());
    assert!("settled".parse::<AgentWorkspaceRepairPhase>().is_err());
}

#[test]
fn new_repair_attempt_is_unsettled_and_projects_only_response_safe_fields() {
    let now = Utc::now();
    let attempt = AgentWorkspaceRepairAttempt::new(
        conversation_id(),
        AgentWorkspaceRepairSource::BaseUpdate,
        AgentWorkspaceRepairContinuation::Publish,
        "origin/main",
        true,
        true,
        true,
        Some("squash".to_string()),
        now,
    );

    assert_eq!(attempt.generation, 0);
    assert_eq!(attempt.phase, AgentWorkspaceRepairPhase::Requested);
    assert_eq!(attempt.pending_reasons, Vec::<String>::new());
    assert_eq!(attempt.outcome, None);
    assert_eq!(attempt.settled_at, None);

    let snapshot = attempt.operation_snapshot();
    assert_eq!(snapshot.operation_id, attempt.id.to_string());
    assert_eq!(snapshot.generation, 0);
    assert_eq!(
        snapshot.stage,
        AgentWorkspaceRepairOperationStage::UpdatingBase
    );
    assert_eq!(snapshot.status, AgentWorkspaceRepairOperationStatus::Active);
    assert!(snapshot.automatic_continuation);
}

#[test]
fn repair_effect_completion_requires_an_observed_receipt() {
    let now = Utc::now();
    let effect = AgentWorkspaceRepairEffect::new(
        AgentWorkspaceRepairAttemptId::from_string("repair-attempt-1"),
        AgentWorkspaceRepairEffectKind::PushBranch,
        "push:repair-attempt-1",
        now,
    );

    assert_eq!(effect.status, AgentWorkspaceRepairEffectStatus::Pending);
    assert_eq!(effect.completed_at, None);
    assert!(effect.can_complete_observed(None, now).is_err());
    assert!(effect
        .can_complete_observed(Some("{\"remote_oid\":\"abc\"}"), now)
        .is_ok());
}
