use super::plan_references::{
    normalize_query, plan_reference_status, score_reference, session_can_reference_plan,
    PlanApprovalLookup,
};
use super::AgentComposerPlanReferenceResponse;
use crate::domain::entities::{
    ArtifactId, IdeationSession, IdeationSessionStatus, ProjectId, SessionPurpose,
};
use chrono::{TimeZone, Utc};

fn plan_reference(
    title: Option<&str>,
    session_id: &str,
    artifact_id: &str,
    status: &str,
) -> AgentComposerPlanReferenceResponse {
    AgentComposerPlanReferenceResponse {
        session_id: session_id.to_string(),
        artifact_id: artifact_id.to_string(),
        title: title.map(str::to_string),
        status: status.to_string(),
        artifact_version: 4,
        updated_at: "2026-06-12T00:00:00Z".to_string(),
        approved_at: None,
    }
}

fn referenceable_session() -> IdeationSession {
    IdeationSession::builder()
        .project_id(ProjectId::from_string("project-1".to_string()))
        .plan_artifact_id(ArtifactId::from_string("artifact-1"))
        .updated_at(Utc.with_ymd_and_hms(2026, 6, 12, 0, 0, 0).unwrap())
        .build()
}

#[test]
fn normalize_query_trims_and_lowercases() {
    assert_eq!(normalize_query("  Plan Alpha  "), "plan alpha");
}

#[test]
fn score_reference_ranks_identifier_title_and_status_matches() {
    assert_eq!(
        score_reference(
            &plan_reference(Some("Plan Alpha"), "session-1", "artifact-1", "approved"),
            ""
        ),
        Some(1)
    );
    assert_eq!(
        score_reference(
            &plan_reference(Some("Plan Alpha"), "session-1", "artifact-1", "approved"),
            "artifact-1"
        ),
        Some(100)
    );
    assert_eq!(
        score_reference(
            &plan_reference(Some("Plan Alpha"), "session-1", "artifact-1", "approved"),
            "plan alpha"
        ),
        Some(90)
    );
    assert_eq!(
        score_reference(
            &plan_reference(Some("Plan Alpha"), "session-1", "artifact-1", "approved"),
            "plan"
        ),
        Some(70)
    );
    assert_eq!(
        score_reference(
            &plan_reference(Some("Plan Alpha"), "session-1", "artifact-1", "approved"),
            "alpha"
        ),
        Some(55)
    );
    assert_eq!(
        score_reference(
            &plan_reference(None, "session-1", "artifact-1", "approved"),
            "approve"
        ),
        Some(35)
    );
    assert_eq!(
        score_reference(
            &plan_reference(Some("Plan Alpha"), "session-1", "artifact-1", "approved"),
            "missing"
        ),
        None
    );
}

#[test]
fn session_can_reference_owned_or_inherited_active_non_verification_plan() {
    assert!(session_can_reference_plan(&referenceable_session()));

    let inherited_session = IdeationSession::builder()
        .project_id(ProjectId::from_string("project-1".to_string()))
        .inherited_plan_artifact_id(ArtifactId::from_string("artifact-parent"))
        .build();
    assert!(session_can_reference_plan(&inherited_session));

    let mut archived = referenceable_session();
    archived.status = IdeationSessionStatus::Archived;
    assert!(!session_can_reference_plan(&archived));

    let mut verification = referenceable_session();
    verification.session_purpose = SessionPurpose::Verification;
    assert!(!session_can_reference_plan(&verification));

    let no_plan = IdeationSession::builder()
        .project_id(ProjectId::from_string("project-1".to_string()))
        .build();
    assert!(!session_can_reference_plan(&no_plan));
}

#[test]
fn plan_reference_status_prefers_accepted_then_approved_then_draft() {
    let mut accepted = referenceable_session();
    accepted.status = IdeationSessionStatus::Accepted;
    assert_eq!(plan_reference_status(&accepted, None), "accepted");

    let approval = PlanApprovalLookup {
        approved_artifact_id: "artifact-1".to_string(),
        approved_version: 4,
        approved_at: Some("2026-06-12T00:00:00Z".to_string()),
    };
    let active = referenceable_session();
    assert_eq!(plan_reference_status(&active, Some(&approval)), "approved");
    assert_eq!(plan_reference_status(&active, None), "draft");
}
