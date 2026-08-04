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
fn stationary_ready_repair_holds_project_distinct_typed_identities() {
    let now = Utc::now();
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        conversation_id(),
        AgentWorkspaceRepairSource::PrAutofix,
        AgentWorkspaceRepairContinuation::ResumePrSupervision,
        "origin/main",
        false,
        true,
        false,
        None,
        now,
    );
    attempt.phase = AgentWorkspaceRepairPhase::Ready;
    attempt
        .pending_reasons
        .push("pr_autofix_unchanged_health".to_string());

    let unchanged = attempt.operation_snapshot();
    assert_eq!(unchanged.stage, AgentWorkspaceRepairOperationStage::Held);
    assert_eq!(unchanged.status, AgentWorkspaceRepairOperationStatus::Held);
    assert_eq!(
        unchanged.hold_reason,
        Some(AgentWorkspaceRepairOperationHoldReason::UnchangedHealth)
    );
    assert!(!unchanged.automatic_continuation);

    attempt.pending_reasons = vec!["pr_autofix_pre_existing_on_base".to_string()];
    let pre_existing = attempt.operation_snapshot();
    assert_eq!(pre_existing.stage, AgentWorkspaceRepairOperationStage::Held);
    assert_eq!(
        pre_existing.status,
        AgentWorkspaceRepairOperationStatus::Held
    );
    assert_eq!(
        pre_existing.hold_reason,
        Some(AgentWorkspaceRepairOperationHoldReason::PreExistingOnBase)
    );

    attempt.pending_reasons.clear();
    attempt.ci_rerun_count = 1;
    attempt.ci_rerun_fingerprint = Some("ci-rerun:123".to_string());
    let ci_rerun = attempt.operation_snapshot();
    assert_eq!(ci_rerun.stage, AgentWorkspaceRepairOperationStage::Held);
    assert_eq!(ci_rerun.status, AgentWorkspaceRepairOperationStatus::Held);
    assert_eq!(
        ci_rerun.hold_reason,
        Some(AgentWorkspaceRepairOperationHoldReason::CiRerunPending)
    );

    for reason in [
        AgentWorkspaceRepairOperationHoldReason::UnchangedHealth,
        AgentWorkspaceRepairOperationHoldReason::PreExistingOnBase,
        AgentWorkspaceRepairOperationHoldReason::CiRerunPending,
    ] {
        assert_eq!(reason.as_str().parse(), Ok(reason));
    }
}

#[test]
fn ready_blocked_and_publish_redrive_keep_their_non_hold_identity() {
    let now = Utc::now();
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        conversation_id(),
        AgentWorkspaceRepairSource::PrAutofix,
        AgentWorkspaceRepairContinuation::ResumePrSupervision,
        "origin/main",
        false,
        true,
        false,
        None,
        now,
    );
    attempt.phase = AgentWorkspaceRepairPhase::Ready;

    let ready = attempt.operation_snapshot();
    assert_eq!(ready.stage, AgentWorkspaceRepairOperationStage::Ready);
    assert_eq!(ready.status, AgentWorkspaceRepairOperationStatus::Ready);
    assert_eq!(ready.hold_reason, None);

    attempt.pending_reasons = vec!["pr_autofix_head_redrive:local-head".to_string()];
    let redrive = attempt.operation_snapshot();
    assert_eq!(
        redrive.stage,
        AgentWorkspaceRepairOperationStage::Publishing
    );
    assert_eq!(redrive.status, AgentWorkspaceRepairOperationStatus::Active);
    assert_eq!(redrive.hold_reason, None);
    assert!(redrive.automatic_continuation);

    attempt.phase = AgentWorkspaceRepairPhase::Blocked;
    attempt.pending_reasons = vec!["pr_autofix_unchanged_health".to_string()];
    let blocked = attempt.operation_snapshot();
    assert_eq!(blocked.stage, AgentWorkspaceRepairOperationStage::Blocked);
    assert_eq!(blocked.status, AgentWorkspaceRepairOperationStatus::Blocked);
    assert_eq!(blocked.hold_reason, None);
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
