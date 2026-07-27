use super::*;
use crate::domain::entities::ideation::{SessionOrigin, VerificationError};
use crate::domain::entities::{
    ArtifactId, IdeationSession, IdeationSessionId, ProjectId, VerificationStatus,
};
use crate::domain::ideation::config::{
    ExternalIdeationOverrides, IdeationPlanMode, IdeationSettings,
};

fn settings(required: bool) -> IdeationSettings {
    IdeationSettings {
        tasks_enabled: false,
        tasks_feature_state: Default::default(),
        plan_mode: IdeationPlanMode::Optional,
        require_plan_approval: false,
        suggest_plans_for_complex: false,
        auto_link_proposals: false,
        auto_verify_plans: false,
        auto_verify_draft_plans: true,
        require_verification_for_accept: required,
        require_verification_for_proposals: false,
        require_accept_for_finalize: false,
        external_overrides: ExternalIdeationOverrides::default(),
    }
}

fn session(current: &str, verified: Option<&str>) -> IdeationSession {
    let mut session = IdeationSession::builder()
        .id(IdeationSessionId::from_string(
            "verification-gate-session".to_string(),
        ))
        .project_id(ProjectId::from_string(
            "verification-gate-project".to_string(),
        ))
        .plan_artifact_id(ArtifactId::from_string(current.to_string()))
        .plan_blueprint_artifact_id(ArtifactId::from_string("blueprint-v2".to_string()))
        .build();
    session.verified_plan_artifact_id = verified.map(|id| ArtifactId::from_string(id.to_string()));
    session.verified_plan_blueprint_artifact_id =
        verified.map(|_| ArtifactId::from_string("blueprint-v2".to_string()));
    session
}

#[test]
fn required_gate_accepts_only_exact_current_artifact_proof() {
    let settings = settings(true);
    let policy = resolve_effective_gate_policy(&settings, SessionOrigin::Internal);

    assert!(check_verification_gate(&session("plan-v2", Some("plan-v2")), &policy).is_ok());
    assert!(matches!(
        check_verification_gate(&session("plan-v2", Some("plan-v1")), &policy),
        Err(VerificationError::NotVerified)
    ));
    assert!(matches!(
        check_verification_gate(&session("plan-v2", None), &policy),
        Err(VerificationError::NotVerified)
    ));
}

#[test]
fn advisory_verification_never_blocks_acceptance() {
    let settings = settings(false);
    let policy = resolve_effective_gate_policy(&settings, SessionOrigin::Internal);
    assert!(check_verification_gate(&session("plan-v2", None), &policy).is_ok());

    let mut legacy_stuck = session("plan-v2", None);
    legacy_stuck.verification_status = VerificationStatus::Reviewing;
    legacy_stuck.verification_in_progress = true;
    assert!(
        check_verification_gate(&legacy_stuck, &policy).is_ok(),
        "retired verifier state must not keep an advisory plan permanently frozen"
    );
}

#[test]
fn external_policy_inherits_or_overrides_auto_and_accept_gate_independently() {
    let mut settings = settings(false);
    settings.auto_verify_plans = true;
    settings.external_overrides.auto_verify_plans = Some(false);
    settings.external_overrides.require_verification_for_accept = Some(true);

    let internal = resolve_effective_gate_policy(&settings, SessionOrigin::Internal);
    assert!(internal.auto_verify_plans);
    assert!(!internal.require_verification_for_accept);

    let external = resolve_effective_gate_policy(&settings, SessionOrigin::External);
    assert!(!external.auto_verify_plans);
    assert!(external.require_verification_for_accept);
}
