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

    for reason in [
        AgentWorkspaceRepairHoldReason::UnchangedHealth,
        AgentWorkspaceRepairHoldReason::PreExistingOnBase,
        AgentWorkspaceRepairHoldReason::CiRerunPending,
    ] {
        assert_eq!(
            reason
                .as_str()
                .parse::<AgentWorkspaceRepairHoldReason>()
                .unwrap(),
            reason
        );
    }

    assert!("unknown".parse::<AgentWorkspaceRepairSource>().is_err());
    assert!("settled".parse::<AgentWorkspaceRepairPhase>().is_err());
    assert!("pr_autofix_unknown"
        .parse::<AgentWorkspaceRepairHoldReason>()
        .is_err());
}

#[test]
fn repair_ids_and_continuation_priorities_are_stable_domain_contracts() {
    let attempt_id = AgentWorkspaceRepairAttemptId::default();
    let effect_id = AgentWorkspaceRepairEffectId::default();

    assert!(!attempt_id.as_str().is_empty());
    assert!(!effect_id.as_str().is_empty());
    assert_ne!(attempt_id.as_str(), effect_id.as_str());
    assert_eq!(AgentWorkspaceRepairContinuation::Manual.priority(), 0);
    assert_eq!(AgentWorkspaceRepairContinuation::UpdateOnly.priority(), 1);
    assert_eq!(AgentWorkspaceRepairContinuation::Publish.priority(), 2);
    assert_eq!(
        AgentWorkspaceRepairContinuation::ResumePrSupervision.priority(),
        3
    );
    assert!(!AgentWorkspaceRepairContinuation::Manual.is_automatic());
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
fn repair_operation_snapshots_project_every_terminal_and_active_stage() {
    let now = Utc::now();
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        conversation_id(),
        AgentWorkspaceRepairSource::Publish,
        AgentWorkspaceRepairContinuation::Publish,
        "origin/main",
        false,
        true,
        false,
        None,
        now,
    );

    for (phase, stage, status, automatic_continuation) in [
        (
            AgentWorkspaceRepairPhase::Repairing,
            AgentWorkspaceRepairOperationStage::Repairing,
            AgentWorkspaceRepairOperationStatus::Active,
            true,
        ),
        (
            AgentWorkspaceRepairPhase::Validating,
            AgentWorkspaceRepairOperationStage::Validating,
            AgentWorkspaceRepairOperationStatus::Active,
            true,
        ),
        (
            AgentWorkspaceRepairPhase::AwaitingReview,
            AgentWorkspaceRepairOperationStage::Reviewing,
            AgentWorkspaceRepairOperationStatus::Active,
            true,
        ),
        (
            AgentWorkspaceRepairPhase::ContinuationPending,
            AgentWorkspaceRepairOperationStage::Publishing,
            AgentWorkspaceRepairOperationStatus::Active,
            true,
        ),
        (
            AgentWorkspaceRepairPhase::Continuing,
            AgentWorkspaceRepairOperationStage::Publishing,
            AgentWorkspaceRepairOperationStatus::Active,
            true,
        ),
        (
            AgentWorkspaceRepairPhase::Ready,
            AgentWorkspaceRepairOperationStage::Ready,
            AgentWorkspaceRepairOperationStatus::Ready,
            false,
        ),
        (
            AgentWorkspaceRepairPhase::Blocked,
            AgentWorkspaceRepairOperationStage::Blocked,
            AgentWorkspaceRepairOperationStatus::Blocked,
            false,
        ),
    ] {
        attempt.phase = phase;
        let snapshot = attempt.operation_snapshot();
        assert_eq!(snapshot.stage, stage, "{phase}");
        assert_eq!(snapshot.status, status, "{phase}");
        assert_eq!(
            snapshot.automatic_continuation, automatic_continuation,
            "{phase}"
        );
    }
}

#[test]
fn ready_repairs_with_a_durable_hold_reason_project_as_held_without_changing_phase() {
    for reason in [
        AgentWorkspaceRepairHoldReason::UnchangedHealth,
        AgentWorkspaceRepairHoldReason::PreExistingOnBase,
    ] {
        let mut attempt = AgentWorkspaceRepairAttempt::new(
            conversation_id(),
            AgentWorkspaceRepairSource::PrAutofix,
            AgentWorkspaceRepairContinuation::ResumePrSupervision,
            "origin/main",
            false,
            true,
            true,
            None,
            Utc::now(),
        );
        attempt.phase = AgentWorkspaceRepairPhase::Ready;
        attempt.pending_reasons = vec![reason.as_str().to_string()];

        let snapshot = attempt.operation_snapshot();

        assert_eq!(attempt.phase, AgentWorkspaceRepairPhase::Ready);
        assert_eq!(snapshot.stage, AgentWorkspaceRepairOperationStage::Held);
        assert_eq!(snapshot.status, AgentWorkspaceRepairOperationStatus::Held);
        assert_eq!(snapshot.hold_reason, Some(reason));
        assert!(!snapshot.automatic_continuation);
    }
}

#[test]
fn ready_ci_rerun_projects_as_held_from_durable_rerun_evidence() {
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        conversation_id(),
        AgentWorkspaceRepairSource::PrAutofix,
        AgentWorkspaceRepairContinuation::ResumePrSupervision,
        "origin/main",
        false,
        true,
        true,
        None,
        Utc::now(),
    );
    attempt.phase = AgentWorkspaceRepairPhase::Ready;
    attempt.ci_rerun_count = 1;
    attempt.ci_rerun_fingerprint = Some("ci:clippy:failed".to_string());

    let snapshot = attempt.operation_snapshot();

    assert_eq!(attempt.phase, AgentWorkspaceRepairPhase::Ready);
    assert_eq!(snapshot.stage, AgentWorkspaceRepairOperationStage::Held);
    assert_eq!(snapshot.status, AgentWorkspaceRepairOperationStatus::Held);
    assert_eq!(
        snapshot.hold_reason,
        Some(AgentWorkspaceRepairHoldReason::CiRerunPending)
    );
    assert!(!snapshot.automatic_continuation);
}

#[test]
fn blocked_repair_is_not_reclassified_by_a_stale_hold_reason() {
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        conversation_id(),
        AgentWorkspaceRepairSource::PrAutofix,
        AgentWorkspaceRepairContinuation::ResumePrSupervision,
        "origin/main",
        false,
        true,
        true,
        None,
        Utc::now(),
    );
    attempt.phase = AgentWorkspaceRepairPhase::Blocked;
    attempt.pending_reasons = vec![AgentWorkspaceRepairHoldReason::UnchangedHealth
        .as_str()
        .to_string()];

    let snapshot = attempt.operation_snapshot();

    assert_eq!(snapshot.stage, AgentWorkspaceRepairOperationStage::Blocked);
    assert_eq!(
        snapshot.status,
        AgentWorkspaceRepairOperationStatus::Blocked
    );
    assert_eq!(snapshot.hold_reason, None);
}

#[test]
fn ready_repair_without_a_hold_reason_remains_genuinely_ready() {
    let now = Utc::now();
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        conversation_id(),
        AgentWorkspaceRepairSource::BaseUpdate,
        AgentWorkspaceRepairContinuation::Publish,
        "origin/main",
        false,
        true,
        false,
        None,
        now,
    );
    attempt.phase = AgentWorkspaceRepairPhase::Ready;

    let snapshot = attempt.operation_snapshot();

    assert_eq!(snapshot.stage, AgentWorkspaceRepairOperationStage::Ready);
    assert_eq!(snapshot.status, AgentWorkspaceRepairOperationStatus::Ready);
    assert_eq!(snapshot.hold_reason, None);
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
    assert!(effect
        .can_complete_observed(
            Some("{\"remote_oid\":\"abc\"}"),
            now - chrono::Duration::microseconds(1),
        )
        .is_err());
}
