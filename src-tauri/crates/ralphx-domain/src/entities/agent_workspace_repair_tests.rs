use chrono::Utc;
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::*;

fn conversation_id() -> ChatConversationId {
    ChatConversationId::from_string("5b46a460-1699-47e6-a687-71305f4e5674")
}

fn assert_serde_wire_contract<T>(value: T, wire: &str)
where
    T: Copy + DeserializeOwned + PartialEq + Serialize + std::fmt::Debug + std::str::FromStr,
    T::Err: std::fmt::Debug,
{
    assert_eq!(
        serde_json::to_value(value).expect("serialize repair enum"),
        serde_json::json!(wire)
    );
    assert_eq!(
        serde_json::from_value::<T>(serde_json::json!(wire)).expect("deserialize repair enum"),
        value
    );
    assert_eq!(
        wire.parse::<T>().expect("parse repair enum wire value"),
        value
    );
}

macro_rules! repair_enum_wire_contract_test {
    ($test_name:ident, [$($variant:path => $wire:literal),+ $(,)?]) => {
        #[test]
        fn $test_name() {
            $(assert_serde_wire_contract($variant, $wire);)+
        }
    };
}

repair_enum_wire_contract_test!(repair_source_serde_matches_wire_contract, [
    AgentWorkspaceRepairSource::BaseUpdate => "base_update",
    AgentWorkspaceRepairSource::Publish => "publish",
    AgentWorkspaceRepairSource::PrConflict => "pr_conflict",
    AgentWorkspaceRepairSource::PrAutofix => "pr_autofix",
    AgentWorkspaceRepairSource::Legacy => "legacy",
]);

repair_enum_wire_contract_test!(repair_phase_serde_matches_wire_contract, [
    AgentWorkspaceRepairPhase::Requested => "requested",
    AgentWorkspaceRepairPhase::Dispatching => "dispatching",
    AgentWorkspaceRepairPhase::Repairing => "repairing",
    AgentWorkspaceRepairPhase::Validating => "validating",
    AgentWorkspaceRepairPhase::AwaitingReview => "awaiting_review",
    AgentWorkspaceRepairPhase::ContinuationPending => "continuation_pending",
    AgentWorkspaceRepairPhase::Continuing => "continuing",
    AgentWorkspaceRepairPhase::Ready => "ready",
    AgentWorkspaceRepairPhase::Blocked => "blocked",
]);

repair_enum_wire_contract_test!(repair_continuation_serde_matches_wire_contract, [
    AgentWorkspaceRepairContinuation::UpdateOnly => "update_only",
    AgentWorkspaceRepairContinuation::Publish => "publish",
    AgentWorkspaceRepairContinuation::ResumePrSupervision => "resume_pr_supervision",
    AgentWorkspaceRepairContinuation::Manual => "manual",
]);

repair_enum_wire_contract_test!(repair_outcome_serde_matches_wire_contract, [
    AgentWorkspaceRepairOutcome::Succeeded => "succeeded",
    AgentWorkspaceRepairOutcome::Superseded => "superseded",
    AgentWorkspaceRepairOutcome::Failed => "failed",
    AgentWorkspaceRepairOutcome::Cancelled => "cancelled",
]);

repair_enum_wire_contract_test!(repair_effect_kind_serde_matches_wire_contract, [
    AgentWorkspaceRepairEffectKind::PushBranch => "push_branch",
    AgentWorkspaceRepairEffectKind::CreatePr => "create_pr",
    AgentWorkspaceRepairEffectKind::UpdatePr => "update_pr",
    AgentWorkspaceRepairEffectKind::RestoreAutoMerge => "restore_auto_merge",
]);

repair_enum_wire_contract_test!(repair_effect_status_serde_matches_wire_contract, [
    AgentWorkspaceRepairEffectStatus::Pending => "pending",
    AgentWorkspaceRepairEffectStatus::InFlight => "in_flight",
    AgentWorkspaceRepairEffectStatus::Observed => "observed",
    AgentWorkspaceRepairEffectStatus::Failed => "failed",
]);

repair_enum_wire_contract_test!(repair_operation_stage_serde_matches_wire_contract, [
    AgentWorkspaceRepairOperationStage::UpdatingBase => "updating_base",
    AgentWorkspaceRepairOperationStage::Repairing => "repairing",
    AgentWorkspaceRepairOperationStage::Validating => "validating",
    AgentWorkspaceRepairOperationStage::Reviewing => "reviewing",
    AgentWorkspaceRepairOperationStage::Publishing => "publishing",
    AgentWorkspaceRepairOperationStage::Ready => "ready",
    AgentWorkspaceRepairOperationStage::Blocked => "blocked",
    AgentWorkspaceRepairOperationStage::Held => "held",
]);

repair_enum_wire_contract_test!(repair_operation_status_serde_matches_wire_contract, [
    AgentWorkspaceRepairOperationStatus::Active => "active",
    AgentWorkspaceRepairOperationStatus::Ready => "ready",
    AgentWorkspaceRepairOperationStatus::Blocked => "blocked",
    AgentWorkspaceRepairOperationStatus::Held => "held",
]);

repair_enum_wire_contract_test!(repair_hold_reason_serde_matches_wire_contract, [
    AgentWorkspaceRepairOperationHoldReason::UnchangedHealth => "pr_autofix_unchanged_health",
    AgentWorkspaceRepairOperationHoldReason::PreExistingOnBase => "pr_autofix_pre_existing_on_base",
    AgentWorkspaceRepairOperationHoldReason::CiRerunPending => "pr_autofix_ci_rerun_pending",
    AgentWorkspaceRepairOperationHoldReason::BaseStale => "base_stale",
    AgentWorkspaceRepairOperationHoldReason::HealthEvidence => "health_evidence",
    AgentWorkspaceRepairOperationHoldReason::PublishRedrive => "publish_redrive",
    AgentWorkspaceRepairOperationHoldReason::PublicationEffectAttention => "publication_effect_attention",
]);

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
    assert_eq!(
        snapshot.recovery_action,
        AgentWorkspaceRepairOperationRecoveryAction::None
    );
    assert!(snapshot.automatic_continuation);
    assert_eq!(snapshot.what_happened, None);
    assert_eq!(snapshot.what_i_did, None);
}

#[test]
fn repair_operation_snapshot_carries_attempt_narrative_when_present() {
    let now = Utc::now();
    let mut attempt = AgentWorkspaceRepairAttempt::new(
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
    attempt.what_happened = Some("the base branch moved ahead".to_string());
    attempt.what_i_did = Some("rebased the workspace onto the new base".to_string());

    let snapshot = attempt.operation_snapshot();
    assert_eq!(
        snapshot.what_happened,
        Some("the base branch moved ahead".to_string())
    );
    assert_eq!(
        snapshot.what_i_did,
        Some("rebased the workspace onto the new base".to_string())
    );
}

#[test]
fn repair_operation_snapshot_with_absent_narrative_round_trips_as_null() {
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

    let snapshot = attempt.operation_snapshot();
    let serialized = serde_json::to_value(&snapshot).expect("serialize operation snapshot");
    assert_eq!(serialized["what_happened"], serde_json::Value::Null);
    assert_eq!(serialized["what_i_did"], serde_json::Value::Null);

    let mut legacy = serialized;
    legacy
        .as_object_mut()
        .expect("operation snapshot serializes as an object")
        .remove("what_happened");
    legacy
        .as_object_mut()
        .expect("operation snapshot serializes as an object")
        .remove("what_i_did");

    let decoded: AgentWorkspaceRepairOperationSnapshot =
        serde_json::from_value(legacy).expect("legacy operation snapshot remains readable");
    assert_eq!(decoded.what_happened, None);
    assert_eq!(decoded.what_i_did, None);
}

#[test]
fn repair_operation_snapshot_projects_typed_recovery_action_with_legacy_default() {
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        conversation_id(),
        AgentWorkspaceRepairSource::Publish,
        AgentWorkspaceRepairContinuation::Publish,
        "origin/main",
        false,
        true,
        false,
        None,
        Utc::now(),
    );
    attempt.phase = AgentWorkspaceRepairPhase::Blocked;
    let snapshot = attempt.operation_snapshot_with_recovery_action(
        AgentWorkspaceRepairOperationRecoveryAction::RetryRepair,
    );
    assert_eq!(
        snapshot.recovery_action,
        AgentWorkspaceRepairOperationRecoveryAction::RetryRepair
    );

    let mut legacy = serde_json::to_value(snapshot).expect("serialize operation snapshot");
    legacy
        .as_object_mut()
        .expect("snapshot serializes as an object")
        .remove("recovery_action");
    let decoded: AgentWorkspaceRepairOperationSnapshot =
        serde_json::from_value(legacy).expect("legacy snapshot remains readable");
    assert_eq!(
        decoded.recovery_action,
        AgentWorkspaceRepairOperationRecoveryAction::None
    );
}

#[test]
fn legacy_repair_attempt_json_defaults_missing_base_update_target() {
    let attempt = AgentWorkspaceRepairAttempt::new(
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
    let mut legacy = serde_json::to_value(attempt).expect("serialize repair attempt");
    legacy
        .as_object_mut()
        .expect("repair attempt serializes as an object")
        .remove("base_update_target_commit");

    let decoded: AgentWorkspaceRepairAttempt =
        serde_json::from_value(legacy).expect("legacy repair attempt remains readable");

    assert_eq!(decoded.base_update_target_commit, None);
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
fn continuation_phase_publication_effect_attention_marker_holds_snapshot() {
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

    for phase in [
        AgentWorkspaceRepairPhase::ContinuationPending,
        AgentWorkspaceRepairPhase::Continuing,
    ] {
        attempt.phase = phase;
        attempt.pending_reasons =
            vec![CONTINUATION_OPEN_EFFECT_ATTENTION_PENDING_REASON.to_string()];

        let snapshot = attempt.operation_snapshot();
        assert_eq!(
            snapshot.stage,
            AgentWorkspaceRepairOperationStage::Held,
            "{phase}"
        );
        assert_eq!(
            snapshot.status,
            AgentWorkspaceRepairOperationStatus::Held,
            "{phase}"
        );
        assert_eq!(
            snapshot.hold_reason,
            Some(AgentWorkspaceRepairOperationHoldReason::PublicationEffectAttention),
            "{phase}"
        );
        assert!(!snapshot.automatic_continuation, "{phase}");
    }

    assert_eq!(
        AgentWorkspaceRepairOperationHoldReason::PublicationEffectAttention
            .as_str()
            .parse(),
        Ok(AgentWorkspaceRepairOperationHoldReason::PublicationEffectAttention)
    );
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
    attempt.pending_reasons = vec![
        "pr_autofix_unchanged_health".to_string(),
        "pr_autofix_base_stale_after_update".to_string(),
    ];
    assert_eq!(
        attempt.operation_snapshot().hold_reason,
        Some(AgentWorkspaceRepairOperationHoldReason::BaseStale),
        "a pending base refresh must outrank stale health evidence"
    );

    attempt.pending_reasons = vec!["pr_autofix_pre_existing_on_base".to_string()];
    let pre_existing = attempt.operation_snapshot();
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
fn repair_operation_snapshot_serializes_hold_reason_with_canonical_wire_value() {
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        conversation_id(),
        AgentWorkspaceRepairSource::PrAutofix,
        AgentWorkspaceRepairContinuation::ResumePrSupervision,
        "origin/main",
        false,
        true,
        false,
        None,
        Utc::now(),
    );
    attempt.phase = AgentWorkspaceRepairPhase::Ready;
    attempt
        .pending_reasons
        .push("pr_autofix_unchanged_health".to_string());

    let serialized = serde_json::to_value(attempt.operation_snapshot())
        .expect("serialize repair operation snapshot");

    assert_eq!(
        serialized.get("hold_reason"),
        Some(&serde_json::json!("pr_autofix_unchanged_health"))
    );
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
fn remote_matches_recorded_precondition_covers_absent_and_matching_oid_shapes() {
    let now = Utc::now();
    let mut effect = AgentWorkspaceRepairEffect::new(
        AgentWorkspaceRepairAttemptId::from_string("repair-attempt-1"),
        AgentWorkspaceRepairEffectKind::PushBranch,
        "push:repair-attempt-1",
        now,
    );

    effect.expected_remote_absent = true;
    effect.expected_remote_oid = None;
    assert!(effect.remote_matches_recorded_precondition(None));
    assert!(!effect.remote_matches_recorded_precondition(Some("abc123")));

    effect.expected_remote_absent = false;
    effect.expected_remote_oid = Some("abc123".to_string());
    assert!(effect.remote_matches_recorded_precondition(Some("abc123")));
    assert!(!effect.remote_matches_recorded_precondition(Some("def456")));
    assert!(!effect.remote_matches_recorded_precondition(None));
}

#[test]
fn has_exact_remote_oid_proof_requires_unambiguous_intended_and_expected_shapes() {
    let now = Utc::now();
    let mut effect = AgentWorkspaceRepairEffect::new(
        AgentWorkspaceRepairAttemptId::from_string("repair-attempt-1"),
        AgentWorkspaceRepairEffectKind::PushBranch,
        "push:repair-attempt-1",
        now,
    );

    assert!(!effect.has_exact_remote_oid_proof(), "no intended OID yet");

    effect.intended_head_oid = Some("head-oid".to_string());
    assert!(
        !effect.has_exact_remote_oid_proof(),
        "neither expected_remote_absent nor expected_remote_oid is set"
    );

    effect.expected_remote_absent = true;
    assert!(effect.has_exact_remote_oid_proof());

    effect.expected_remote_oid = Some("origin-oid".to_string());
    assert!(
        !effect.has_exact_remote_oid_proof(),
        "expected_remote_absent with an expected_remote_oid is ambiguous"
    );

    effect.expected_remote_absent = false;
    assert!(effect.has_exact_remote_oid_proof());

    effect.expected_remote_oid = Some("   ".to_string());
    assert!(
        !effect.has_exact_remote_oid_proof(),
        "whitespace-only expected_remote_oid is treated as missing"
    );

    effect.expected_remote_oid = Some("origin-oid".to_string());
    effect.intended_head_oid = Some("   ".to_string());
    assert!(
        !effect.has_exact_remote_oid_proof(),
        "whitespace-only intended_head_oid is treated as missing"
    );
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
