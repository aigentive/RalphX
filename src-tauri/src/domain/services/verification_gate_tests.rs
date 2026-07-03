use super::*;
use crate::domain::entities::ideation::{SessionOrigin, VerificationError, VerificationStatus};
use crate::domain::entities::{ArtifactId, IdeationSession, IdeationSessionId, ProjectId};
use crate::domain::ideation::config::{ExternalIdeationOverrides, IdeationPlanMode, IdeationSettings};

fn make_session(status: VerificationStatus) -> IdeationSession {
    with_verification_status(
        IdeationSession::builder()
        .id(IdeationSessionId::from_string("test-session-id".to_string()))
        .project_id(ProjectId::from_string("test-project-id".to_string()))
        .build(),
        status,
    )
}

// We use a small helper to set verification status since builder doesn't expose it after build.
fn with_verification_status(mut session: IdeationSession, status: VerificationStatus) -> IdeationSession {
    session.verification_status = status;
    session
}

fn settings_with_required(required: bool) -> IdeationSettings {
    IdeationSettings {
        plan_mode: IdeationPlanMode::Optional,
        require_plan_approval: false,
        suggest_plans_for_complex: false,
        auto_link_proposals: false,
        require_verification_for_accept: required,
        require_verification_for_proposals: false,
        require_accept_for_finalize: false,
        external_overrides: Default::default(),
    }
}

/// Resolve the acceptance gate policy for a session using its own origin.
fn accept_policy(session: &IdeationSession, settings: &IdeationSettings) -> EffectiveGatePolicy {
    resolve_effective_gate_policy(settings, session.origin)
}

/// Resolve the proposal gate policy for a session using its own origin.
fn proposal_policy(session: &IdeationSession, settings: &IdeationSettings) -> EffectiveGatePolicy {
    resolve_effective_gate_policy(settings, session.origin)
}

#[test]
fn test_gate_blocks_unverified_when_required() {
    let session = make_session(VerificationStatus::Unverified);
    let settings = settings_with_required(true);
    let result = check_verification_gate(&session, &accept_policy(&session, &settings));
    assert!(matches!(result, Err(VerificationError::NotVerified)));
}

#[test]
fn test_gate_allows_verified() {
    let session = make_session(VerificationStatus::Verified);
    let settings = settings_with_required(true);
    assert!(check_verification_gate(&session, &accept_policy(&session, &settings)).is_ok());
}

#[test]
fn test_gate_allows_skipped() {
    let session = make_session(VerificationStatus::Skipped);
    let settings = settings_with_required(true);
    assert!(check_verification_gate(&session, &accept_policy(&session, &settings)).is_ok());
}

#[test]
fn test_gate_blocks_reviewing() {
    let session = make_session(VerificationStatus::Reviewing);
    let settings = settings_with_required(true);
    let result = check_verification_gate(&session, &accept_policy(&session, &settings));
    assert!(
        matches!(result, Err(VerificationError::InProgress { .. })),
        "reviewing should block with InProgress"
    );
}

#[test]
fn test_gate_blocks_needs_revision() {
    let session = make_session(VerificationStatus::NeedsRevision);
    let settings = settings_with_required(true);
    let result = check_verification_gate(&session, &accept_policy(&session, &settings));
    assert!(
        matches!(result, Err(VerificationError::HasUnresolvedGaps { .. })),
        "needs_revision should block with HasUnresolvedGaps"
    );
}

#[test]
fn test_gate_passes_for_inactive_statuses_when_not_required() {
    let settings = settings_with_required(false);
    for status in [
        VerificationStatus::Unverified,
        VerificationStatus::NeedsRevision,
        VerificationStatus::Verified,
        VerificationStatus::Skipped,
    ] {
        let session = make_session(status);
        let policy = accept_policy(&session, &settings);
        assert!(
            check_verification_gate(&session, &policy).is_ok(),
            "gate should pass for {:?} when require_verification=false",
            status
        );
    }
}

// ============================================================================
// check_proposal_verification_gate() — 9 unit tests (Scenarios 1-9)
// ============================================================================

fn make_session_with_own_plan(status: VerificationStatus) -> IdeationSession {
    let mut session = IdeationSession::builder()
        .id(IdeationSessionId::from_string("own-plan-session".to_string()))
        .project_id(ProjectId::from_string("test-project-id".to_string()))
        .plan_artifact_id(ArtifactId::from_string("test-artifact-id".to_string()))
        .build();
    session.verification_status = status;
    session
}

fn make_no_plan_session() -> IdeationSession {
    IdeationSession::builder()
        .id(IdeationSessionId::from_string("no-plan-session".to_string()))
        .project_id(ProjectId::from_string("test-project-id".to_string()))
        .build()
}

fn make_inherited_plan_session() -> IdeationSession {
    // Child session: inherited_plan_artifact_id set, plan_artifact_id unset
    IdeationSession::builder()
        .id(IdeationSessionId::from_string("child-session".to_string()))
        .project_id(ProjectId::from_string("test-project-id".to_string()))
        .inherited_plan_artifact_id(ArtifactId::from_string("parent-artifact-id".to_string()))
        .build()
}

fn proposal_gate_settings(enabled: bool) -> IdeationSettings {
    IdeationSettings {
        plan_mode: IdeationPlanMode::Optional,
        require_plan_approval: false,
        suggest_plans_for_complex: false,
        auto_link_proposals: false,
        require_verification_for_accept: false,
        require_verification_for_proposals: enabled,
        require_accept_for_finalize: false,
        external_overrides: Default::default(),
    }
}

/// Scenario 1: Config bypass — require_verification_for_proposals=false → all ops pass regardless of status.
#[test]
fn test_proposal_gate_config_bypass_allows_all() {
    let settings = proposal_gate_settings(false);
    for status in [
        VerificationStatus::Unverified,
        VerificationStatus::Reviewing,
        VerificationStatus::NeedsRevision,
        VerificationStatus::Verified,
        VerificationStatus::Skipped,
    ] {
        for op in [
            ProposalOperation::Create,
            ProposalOperation::Update,
            ProposalOperation::Delete,
        ] {
            let session = make_session_with_own_plan(status);
            let policy = proposal_policy(&session, &settings);
            let result = check_proposal_verification_gate(&session, &policy, None, op);
            assert!(
                result.is_ok(),
                "gate=false must bypass for status={:?} op={:?}",
                status,
                op
            );
        }
    }
}

/// Scenario 2: No-plan session passthrough — neither own nor inherited plan → Ok regardless.
#[test]
fn test_proposal_gate_no_plan_passthrough() {
    let settings = proposal_gate_settings(true);
    let session = make_no_plan_session();
    let policy = proposal_policy(&session, &settings);
    for op in [
        ProposalOperation::Create,
        ProposalOperation::Update,
        ProposalOperation::Delete,
    ] {
        assert!(
            check_proposal_verification_gate(&session, &policy, None, op).is_ok(),
            "no-plan session must passthrough for op={:?}",
            op
        );
    }
}

/// Scenario 3: VerificationStatus × ProposalOperation matrix — all 15 combinations.
///
/// Verified          → all ops allowed.
/// Skipped           → Create blocked (ProposalSkippedNotAllowed), Update/Delete allowed.
/// Unverified        → Create blocked (ProposalNotVerified), Update/Delete allowed.
/// Reviewing         → all ops blocked (ProposalReviewInProgress).
/// NeedsRevision     → all ops blocked (ProposalHasUnresolvedGaps).
#[test]
fn test_proposal_gate_status_operation_matrix() {
    let settings = proposal_gate_settings(true);

    // Verified → always allow
    for op in [
        ProposalOperation::Create,
        ProposalOperation::Update,
        ProposalOperation::Delete,
    ] {
        let session = make_session_with_own_plan(VerificationStatus::Verified);
        let policy = proposal_policy(&session, &settings);
        assert!(
            check_proposal_verification_gate(&session, &policy, None, op).is_ok(),
            "status=Verified op={:?} should be Ok",
            op
        );
    }

    // Skipped → Create blocked, Update/Delete allowed
    let session = make_session_with_own_plan(VerificationStatus::Skipped);
    let policy = proposal_policy(&session, &settings);
    assert!(
        matches!(
            check_proposal_verification_gate(&session, &policy, None, ProposalOperation::Create),
            Err(VerificationError::ProposalSkippedNotAllowed)
        ),
        "status=Skipped op=Create should be ProposalSkippedNotAllowed"
    );
    assert!(
        check_proposal_verification_gate(&session, &policy, None, ProposalOperation::Update)
            .is_ok(),
        "status=Skipped op=Update should be Ok"
    );
    assert!(
        check_proposal_verification_gate(&session, &policy, None, ProposalOperation::Delete)
            .is_ok(),
        "status=Skipped op=Delete should be Ok"
    );

    // Unverified → Create blocked, Update/Delete allowed
    let session = make_session_with_own_plan(VerificationStatus::Unverified);
    let policy = proposal_policy(&session, &settings);
    assert!(matches!(
        check_proposal_verification_gate(&session, &policy, None, ProposalOperation::Create),
        Err(VerificationError::ProposalNotVerified)
    ));
    assert!(
        check_proposal_verification_gate(&session, &policy, None, ProposalOperation::Update)
            .is_ok()
    );
    assert!(
        check_proposal_verification_gate(&session, &policy, None, ProposalOperation::Delete)
            .is_ok()
    );

    // Reviewing → all blocked
    let session = make_session_with_own_plan(VerificationStatus::Reviewing);
    let policy = proposal_policy(&session, &settings);
    for op in [
        ProposalOperation::Create,
        ProposalOperation::Update,
        ProposalOperation::Delete,
    ] {
        assert!(
            matches!(
                check_proposal_verification_gate(&session, &policy, None, op),
                Err(VerificationError::ProposalReviewInProgress { .. })
            ),
            "Reviewing should block op={:?}",
            op
        );
    }

    // NeedsRevision → all blocked
    let session = make_session_with_own_plan(VerificationStatus::NeedsRevision);
    let policy = proposal_policy(&session, &settings);
    for op in [
        ProposalOperation::Create,
        ProposalOperation::Update,
        ProposalOperation::Delete,
    ] {
        assert!(
            matches!(
                check_proposal_verification_gate(&session, &policy, None, op),
                Err(VerificationError::ProposalHasUnresolvedGaps { .. })
            ),
            "NeedsRevision should block op={:?}",
            op
        );
    }
}

/// Scenario 7: Update and Delete explicitly allowed on Unverified (editing before review is fine).
#[test]
fn test_proposal_gate_update_delete_allowed_on_unverified() {
    let settings = proposal_gate_settings(true);
    let session = make_session_with_own_plan(VerificationStatus::Unverified);
    let policy = proposal_policy(&session, &settings);
    assert!(
        check_proposal_verification_gate(
            &session,
            &policy,
            None,
            ProposalOperation::Update
        )
        .is_ok(),
        "Update must be allowed when Unverified"
    );
    assert!(
        check_proposal_verification_gate(
            &session,
            &policy,
            None,
            ProposalOperation::Delete
        )
        .is_ok(),
        "Delete must be allowed when Unverified"
    );
}

/// Scenario 4: Child with inherited plan + parent Verified → all ops allowed.
#[test]
fn test_proposal_gate_inherited_plan_parent_verified_allows() {
    let settings = proposal_gate_settings(true);
    let session = make_inherited_plan_session();
    let policy = proposal_policy(&session, &settings);
    let parent_status = Some(VerificationStatus::Verified);
    for op in [
        ProposalOperation::Create,
        ProposalOperation::Update,
        ProposalOperation::Delete,
    ] {
        assert!(
            check_proposal_verification_gate(&session, &policy, parent_status, op).is_ok(),
            "Verified parent should allow op={:?}",
            op
        );
    }
}

/// Scenario 5: Child with inherited plan + parent Unverified → Create blocked, Update/Delete allowed.
#[test]
fn test_proposal_gate_inherited_plan_parent_unverified_blocks_create() {
    let settings = proposal_gate_settings(true);
    let session = make_inherited_plan_session();
    let policy = proposal_policy(&session, &settings);
    let parent_status = Some(VerificationStatus::Unverified);

    assert!(matches!(
        check_proposal_verification_gate(
            &session,
            &policy,
            parent_status,
            ProposalOperation::Create
        ),
        Err(VerificationError::ProposalNotVerified)
    ));
    assert!(
        check_proposal_verification_gate(
            &session,
            &policy,
            parent_status,
            ProposalOperation::Update
        )
        .is_ok()
    );
    assert!(
        check_proposal_verification_gate(
            &session,
            &policy,
            parent_status,
            ProposalOperation::Delete
        )
        .is_ok()
    );
}

/// Scenario 6: Child with own plan uses own verification status; parent status is irrelevant.
#[test]
fn test_proposal_gate_own_plan_uses_own_status_ignores_parent() {
    let settings = proposal_gate_settings(true);

    // Own plan Verified, parent Unverified → Create allowed (own status wins)
    let session = make_session_with_own_plan(VerificationStatus::Verified);
    let policy = proposal_policy(&session, &settings);
    let parent_unverified = Some(VerificationStatus::Unverified);
    assert!(
        check_proposal_verification_gate(
            &session,
            &policy,
            parent_unverified,
            ProposalOperation::Create
        )
        .is_ok(),
        "Own Verified plan should allow Create even if parent is Unverified"
    );

    // Own plan Unverified, parent Verified → Create blocked (own status wins)
    let session = make_session_with_own_plan(VerificationStatus::Unverified);
    let policy = proposal_policy(&session, &settings);
    let parent_verified = Some(VerificationStatus::Verified);
    assert!(
        matches!(
            check_proposal_verification_gate(
                &session,
                &policy,
                parent_verified,
                ProposalOperation::Create
            ),
            Err(VerificationError::ProposalNotVerified)
        ),
        "Own Unverified plan should block Create even if parent is Verified"
    );
}

/// Scenario 8: Child with inherited plan + deleted parent (None status) → graceful degradation → all ops allowed.
#[test]
fn test_proposal_gate_inherited_plan_deleted_parent_allows() {
    let settings = proposal_gate_settings(true);
    let session = make_inherited_plan_session();
    let policy = proposal_policy(&session, &settings);
    // None = parent was deleted (FK ON DELETE SET NULL → parent lookup returns None)
    for op in [
        ProposalOperation::Create,
        ProposalOperation::Update,
        ProposalOperation::Delete,
    ] {
        assert!(
            check_proposal_verification_gate(&session, &policy, None, op).is_ok(),
            "Deleted parent (None) should allow op={:?} via graceful degradation",
            op
        );
    }
}

/// Scenario 9: Child with inherited plan + parent NeedsRevision → all ops blocked (archived parent scenario).
#[test]
fn test_proposal_gate_inherited_plan_parent_needs_revision_blocks() {
    let settings = proposal_gate_settings(true);
    let session = make_inherited_plan_session();
    let policy = proposal_policy(&session, &settings);
    let parent_needs_revision = Some(VerificationStatus::NeedsRevision);
    for op in [
        ProposalOperation::Create,
        ProposalOperation::Update,
        ProposalOperation::Delete,
    ] {
        assert!(
            matches!(
                check_proposal_verification_gate(
                    &session,
                    &policy,
                    parent_needs_revision,
                    op
                ),
                Err(VerificationError::ProposalHasUnresolvedGaps { .. })
            ),
            "Parent NeedsRevision should block op={:?}",
            op
        );
    }
}

// ============================================================================
// check_verification_gate() — in_progress flag tests
// ============================================================================

/// Test: session with in_progress=true blocks with InProgress regardless of status field being Reviewing
#[test]
fn test_verification_gate_in_progress_blocks_regardless_of_status() {
    let settings = settings_with_required(true);
    let mut session = make_session(VerificationStatus::Reviewing);
    session.verification_in_progress = true;
    let policy = accept_policy(&session, &settings);
    let result = check_verification_gate(&session, &policy);
    assert!(
        matches!(result, Err(VerificationError::InProgress { .. })),
        "in_progress=true with Reviewing status should return InProgress"
    );
}

/// Test: session with in_progress=true but status=Unverified still blocks with InProgress.
/// This is an inconsistent state (shouldn't happen in practice) but guards are ordered
/// to check in_progress first.
#[test]
fn test_verification_gate_in_progress_before_status_check() {
    let settings = settings_with_required(true);
    let mut session = make_session(VerificationStatus::Unverified);
    session.verification_in_progress = true;
    let policy = accept_policy(&session, &settings);
    let result = check_verification_gate(&session, &policy);
    // in_progress=true should take priority over status=Unverified (which would normally → NotVerified)
    assert!(
        matches!(result, Err(VerificationError::InProgress { .. })),
        "in_progress=true must be checked before status, returning InProgress not NotVerified"
    );
}

/// Test: optional verification still cannot be finalized while a verifier is running.
#[test]
fn test_verification_gate_in_progress_blocks_when_not_required() {
    let settings = settings_with_required(false);
    let mut session = make_session(VerificationStatus::Unverified);
    session.verification_in_progress = true;
    let policy = accept_policy(&session, &settings);
    let result = check_verification_gate(&session, &policy);
    assert!(
        matches!(result, Err(VerificationError::InProgress { .. })),
        "in_progress=true must block even when require_verification=false"
    );
}

/// Test: ImportedVerified sessions pass the acceptance gate (they are pre-verified by import).
#[test]
fn test_gate_allows_imported_verified() {
    let session = make_session(VerificationStatus::ImportedVerified);
    let settings = settings_with_required(true);
    assert!(
        check_verification_gate(&session, &accept_policy(&session, &settings)).is_ok(),
        "ImportedVerified should pass the acceptance gate"
    );
}

/// Test: ImportedVerified passes all three proposal operations.
#[test]
fn test_proposal_gate_imported_verified_allows_all_operations() {
    let settings = proposal_gate_settings(true);
    let session = make_session_with_own_plan(VerificationStatus::ImportedVerified);
    let policy = proposal_policy(&session, &settings);
    for op in [
        ProposalOperation::Create,
        ProposalOperation::Update,
        ProposalOperation::Delete,
    ] {
        assert!(
            check_proposal_verification_gate(&session, &policy, None, op).is_ok(),
            "ImportedVerified should allow op={:?}",
            op
        );
    }
}

/// Test: Child session with inherited ImportedVerified parent allows all operations.
#[test]
fn test_proposal_gate_inherited_plan_parent_imported_verified_allows() {
    let settings = proposal_gate_settings(true);
    let session = make_inherited_plan_session();
    let policy = proposal_policy(&session, &settings);
    let parent_status = Some(VerificationStatus::ImportedVerified);
    for op in [
        ProposalOperation::Create,
        ProposalOperation::Update,
        ProposalOperation::Delete,
    ] {
        assert!(
            check_proposal_verification_gate(&session, &policy, parent_status, op).is_ok(),
            "ImportedVerified parent should allow op={:?}",
            op
        );
    }
}

/// Test: gate passes for ImportedVerified when gate is disabled (config bypass).
#[test]
fn test_gate_passes_imported_verified_when_not_required() {
    let settings = settings_with_required(false);
    let session = make_session(VerificationStatus::ImportedVerified);
    assert!(
        check_verification_gate(&session, &accept_policy(&session, &settings)).is_ok(),
        "ImportedVerified must pass when require_verification=false"
    );
}

/// Test: session with in_progress=false and status=Reviewing falls through to status match.
#[test]
fn test_verification_gate_reviewing_status_without_in_progress() {
    let settings = settings_with_required(true);
    let mut session = make_session(VerificationStatus::Reviewing);
    session.verification_in_progress = false;
    let policy = accept_policy(&session, &settings);
    let result = check_verification_gate(&session, &policy);
    // Should still be InProgress from the Reviewing match arm (defense-in-depth)
    assert!(
        matches!(result, Err(VerificationError::InProgress { .. })),
        "Reviewing status without in_progress flag should still return InProgress"
    );
}

/// Test: optional verification still treats Reviewing as an active verification state.
#[test]
fn test_verification_gate_reviewing_status_blocks_when_not_required() {
    let settings = settings_with_required(false);
    let mut session = make_session(VerificationStatus::Reviewing);
    session.verification_in_progress = false;
    let policy = accept_policy(&session, &settings);
    let result = check_verification_gate(&session, &policy);
    assert!(
        matches!(result, Err(VerificationError::InProgress { .. })),
        "Reviewing status must block even when require_verification=false"
    );
}

// ============================================================================
// Origin-aware gate tests
// ============================================================================

/// Helper: build a session with a specific origin and verification status.
fn make_session_with_own_plan_and_origin(
    status: VerificationStatus,
    origin: SessionOrigin,
) -> IdeationSession {
    let mut session = IdeationSession::builder()
        .id(IdeationSessionId::from_string("origin-test-session".to_string()))
        .project_id(ProjectId::from_string("test-project-id".to_string()))
        .plan_artifact_id(ArtifactId::from_string("test-artifact-id".to_string()))
        .origin(origin)
        .build();
    session.verification_status = status;
    session
}

/// Test: Skipped+Create on any session returns ProposalSkippedNotAllowed.
/// Focused test verifying the error variant and that the error message is non-empty.
#[test]
fn test_proposal_gate_blocks_create_on_skipped() {
    let settings = proposal_gate_settings(true);
    let session = make_session_with_own_plan(VerificationStatus::Skipped);
    let policy = proposal_policy(&session, &settings);
    let result =
        check_proposal_verification_gate(&session, &policy, None, ProposalOperation::Create);
    assert!(
        matches!(result, Err(VerificationError::ProposalSkippedNotAllowed)),
        "Skipped+Create must return ProposalSkippedNotAllowed, got: {:?}",
        result
    );
    // Verify the error message is non-empty and actionable
    let err = result.unwrap_err();
    let msg = err.to_string();
    assert!(!msg.is_empty(), "error message must not be empty");
    assert!(
        msg.contains("skipped"),
        "error message should mention 'skipped', got: {}",
        msg
    );
}

/// Test: external session with Skipped status is blocked by the acceptance gate.
/// Defense-in-depth: external sessions should never reach Skipped (handler blocks it),
/// but the gate catches it if they do.
#[test]
fn test_accept_gate_blocks_skipped_external() {
    let settings = settings_with_required(true);
    let session =
        make_session_with_own_plan_and_origin(VerificationStatus::Skipped, SessionOrigin::External);
    let policy = accept_policy(&session, &settings);
    let result = check_verification_gate(&session, &policy);
    assert!(
        matches!(result, Err(VerificationError::ExternalCannotSkip)),
        "External+Skipped must return ExternalCannotSkip, got: {:?}",
        result
    );
}

/// Test: internal session with Skipped status passes the acceptance gate.
/// Internal users who skip verification can still accept proposals.
#[test]
fn test_accept_gate_allows_skipped_internal() {
    let settings = settings_with_required(true);
    let session =
        make_session_with_own_plan_and_origin(VerificationStatus::Skipped, SessionOrigin::Internal);
    let policy = accept_policy(&session, &settings);
    assert!(
        check_verification_gate(&session, &policy).is_ok(),
        "Internal+Skipped must pass the acceptance gate"
    );
}

// ============================================================================
// resolve_effective_gate_policy() — 9 unit tests
// ============================================================================

fn settings_full(
    base_accept: bool,
    base_proposals: bool,
    base_finalize: bool,
    ext_accept: Option<bool>,
    ext_proposals: Option<bool>,
    ext_finalize: Option<bool>,
) -> IdeationSettings {
    IdeationSettings {
        plan_mode: IdeationPlanMode::Optional,
        require_plan_approval: false,
        suggest_plans_for_complex: false,
        auto_link_proposals: false,
        require_verification_for_accept: base_accept,
        require_verification_for_proposals: base_proposals,
        require_accept_for_finalize: base_finalize,
        external_overrides: ExternalIdeationOverrides {
            require_verification_for_accept: ext_accept,
            require_verification_for_proposals: ext_proposals,
            require_accept_for_finalize: ext_finalize,
        },
    }
}

/// Test 1: Internal origin always uses base settings regardless of external_overrides values.
#[test]
fn test_resolve_policy_internal_uses_base() {
    let settings = settings_full(true, false, true, Some(false), Some(true), Some(false));
    let policy = resolve_effective_gate_policy(&settings, SessionOrigin::Internal);
    assert!(policy.require_verification_for_accept);
    assert!(!policy.require_verification_for_proposals);
    assert!(policy.require_accept_for_finalize);
}

/// Test 2: Non-External origin (Internal) ignores external_overrides even when all are Some.
#[test]
fn test_resolve_policy_internal_ignores_overrides() {
    let settings = settings_full(false, false, false, Some(true), Some(true), Some(true));
    let policy = resolve_effective_gate_policy(&settings, SessionOrigin::Internal);
    assert!(!policy.require_verification_for_accept, "Internal must use base=false, not override=true");
    assert!(!policy.require_verification_for_proposals, "Internal must use base=false, not override=true");
    assert!(!policy.require_accept_for_finalize, "Internal must use base=false, not override=true");
}

/// Test 3: External origin with all-None overrides falls back to base settings for all fields.
#[test]
fn test_resolve_policy_external_none_overrides_use_base() {
    let settings = settings_full(true, false, true, None, None, None);
    let policy = resolve_effective_gate_policy(&settings, SessionOrigin::External);
    assert!(policy.require_verification_for_accept, "External+None should fall back to base=true");
    assert!(!policy.require_verification_for_proposals, "External+None should fall back to base=false");
    assert!(policy.require_accept_for_finalize, "External+None should fall back to base=true");
}

/// Test 4: External origin with Some(true) overrides uses true even when base is false.
#[test]
fn test_resolve_policy_external_some_true_override_wins_over_false_base() {
    let settings = settings_full(false, false, false, Some(true), Some(true), Some(true));
    let policy = resolve_effective_gate_policy(&settings, SessionOrigin::External);
    assert!(policy.require_verification_for_accept, "External+Some(true) must override base=false");
    assert!(policy.require_verification_for_proposals, "External+Some(true) must override base=false");
    assert!(policy.require_accept_for_finalize, "External+Some(true) must override base=false");
}

/// Test 5: External origin with Some(false) overrides uses false even when base is true.
#[test]
fn test_resolve_policy_external_some_false_override_wins_over_true_base() {
    let settings = settings_full(true, true, true, Some(false), Some(false), Some(false));
    let policy = resolve_effective_gate_policy(&settings, SessionOrigin::External);
    assert!(!policy.require_verification_for_accept, "External+Some(false) must override base=true");
    assert!(!policy.require_verification_for_proposals, "External+Some(false) must override base=true");
    assert!(!policy.require_accept_for_finalize, "External+Some(false) must override base=true");
}

/// Test 6: External origin with mixed overrides — each field resolves independently.
#[test]
fn test_resolve_policy_external_mixed_overrides() {
    // accept: override Some(false) wins over base=true
    // proposals: None → falls back to base=true
    // finalize: override Some(true) wins over base=false
    let settings = settings_full(true, true, false, Some(false), None, Some(true));
    let policy = resolve_effective_gate_policy(&settings, SessionOrigin::External);
    assert!(!policy.require_verification_for_accept, "override=Some(false) should beat base=true");
    assert!(policy.require_verification_for_proposals, "None should fall back to base=true");
    assert!(policy.require_accept_for_finalize, "override=Some(true) should beat base=false");
}

/// Test 7: External origin with only accept override set; other fields fall back to base.
#[test]
fn test_resolve_policy_external_only_accept_overridden() {
    let settings = settings_full(false, true, false, Some(true), None, None);
    let policy = resolve_effective_gate_policy(&settings, SessionOrigin::External);
    assert!(policy.require_verification_for_accept, "override=Some(true) beats base=false");
    assert!(policy.require_verification_for_proposals, "None falls back to base=true");
    assert!(!policy.require_accept_for_finalize, "None falls back to base=false");
}

/// Test 8: All base settings false and External has all-None overrides → policy is all-false.
#[test]
fn test_resolve_policy_external_all_base_false_none_overrides() {
    let settings = settings_full(false, false, false, None, None, None);
    let policy = resolve_effective_gate_policy(&settings, SessionOrigin::External);
    assert!(!policy.require_verification_for_accept);
    assert!(!policy.require_verification_for_proposals);
    assert!(!policy.require_accept_for_finalize);
}

/// Test 9: Internal and External produce different results when external_overrides are set.
/// Validates that origin-based branching is the distinguishing factor.
#[test]
fn test_resolve_policy_internal_vs_external_diverge_with_overrides() {
    let settings = settings_full(true, true, true, Some(false), Some(false), Some(false));
    let internal_policy = resolve_effective_gate_policy(&settings, SessionOrigin::Internal);
    let external_policy = resolve_effective_gate_policy(&settings, SessionOrigin::External);

    // Internal uses base values
    assert!(internal_policy.require_verification_for_accept);
    assert!(internal_policy.require_verification_for_proposals);
    assert!(internal_policy.require_accept_for_finalize);

    // External uses override values
    assert!(!external_policy.require_verification_for_accept);
    assert!(!external_policy.require_verification_for_proposals);
    assert!(!external_policy.require_accept_for_finalize);
}
