use super::plan_references::{normalize_query, score_reference, session_can_reference_plan};
use super::types::SearchAgentComposerPlanReferencesInput;
use super::AgentComposerPlanReferenceResponse;
use crate::application::agent_plan_context::{
    lookup_plan_approval, plan_reference_status, PlanApprovalLookup,
};
use crate::application::AppState;
use crate::domain::entities::{
    Artifact, ArtifactId, ArtifactType, IdeationSession, IdeationSessionStatus, Project, ProjectId,
    SessionPurpose,
};
use crate::domain::repositories::ArtifactRepository;
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
        approved_at: Some("2026-06-12T00:00:00Z".to_string()),
        approved_by: "user".to_string(),
    };
    let active = referenceable_session();
    assert_eq!(plan_reference_status(&active, Some(&approval)), "approved");
    assert_eq!(plan_reference_status(&active, None), "draft");
}

#[tokio::test]
async fn lookup_plan_approval_rejects_stale_blueprint_pair() {
    let state = AppState::new_sqlite_test();
    let overview = state
        .artifact_repo
        .create(Artifact::new_inline(
            "Overview",
            ArtifactType::Specification,
            "# Overview",
            "planner",
        ))
        .await
        .unwrap();
    let current_blueprint = state
        .artifact_repo
        .create(Artifact::new_inline(
            "Blueprint v2",
            ArtifactType::Specification,
            "# Current Blueprint",
            "planner",
        ))
        .await
        .unwrap();
    let stale_blueprint = state
        .artifact_repo
        .create(Artifact::new_inline(
            "Blueprint v1",
            ArtifactType::Specification,
            "# Stale Blueprint",
            "planner",
        ))
        .await
        .unwrap();
    let project = state
        .project_repo
        .create(Project::new(
            "Composer approval test".to_string(),
            "/tmp/ralphx-composer-approval-test".to_string(),
        ))
        .await
        .unwrap();
    let session = state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(project.id)
                .plan_artifact_id(overview.id.clone())
                .plan_blueprint_artifact_id(current_blueprint.id.clone())
                .plan_contract_version(2)
                .build(),
        )
        .await
        .unwrap();
    let session_id = session.id;
    let session_id_sql = session_id.to_string();
    let overview_id = overview.id.to_string();
    let stale_blueprint_id = stale_blueprint.id.to_string();
    state
        .db
        .run(move |conn| {
            conn.execute(
                "INSERT INTO plan_artifact_approvals (
                    session_id, artifact_id, artifact_version,
                    blueprint_artifact_id, blueprint_artifact_version,
                    status, approved_at, approved_by
                 ) VALUES (?1, ?2, 1, ?3, 1, 'approved', ?4, 'user')",
                rusqlite::params![
                    session_id_sql,
                    overview_id,
                    stale_blueprint_id,
                    Utc::now().to_rfc3339()
                ],
            )?;
            Ok(())
        })
        .await
        .unwrap();

    let approval = lookup_plan_approval(&state, &session_id, &overview, Some(&current_blueprint))
        .await
        .unwrap();

    assert!(
        approval.is_none(),
        "composer references must not project a stale Blueprint approval"
    );
}

/// A failed latest-version resolve must refuse, not silently drop the session.
///
/// The fail-open here was two-stage: the resolver error fell back to the pre-resolution SEED
/// id, the following `get_by_id` then missed on that stale id and `continue`d, and `truncated`
/// is derived from `limit` alone — so a resolver outage returned a short list that reads as
/// complete. Silent omission is worse than an error in a picker: the user cannot tell that the
/// plan they are looking for was dropped rather than absent.
#[tokio::test]
async fn plan_reference_search_propagates_a_failed_latest_artifact_resolve() {
    let mut state = AppState::new_sqlite_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Composer resolver failure".to_string(),
            "/tmp/ralphx-composer-resolver-failure".to_string(),
        ))
        .await
        .unwrap();
    let seed = crate::infrastructure::memory::MemoryArtifactRepository::new();
    let artifact = seed
        .create(Artifact::new_inline(
            "Overview",
            ArtifactType::Specification,
            "# Overview",
            "planner",
        ))
        .await
        .unwrap();
    state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(project.id.clone())
                .plan_artifact_id(artifact.id.clone())
                .build(),
        )
        .await
        .unwrap();

    seed.fail_resolve_latest_artifact_id("artifact store is unavailable")
        .await;
    state.artifact_repo = std::sync::Arc::new(seed);

    let result = super::plan_references::search_agent_composer_plan_references_for_app_state(
        &state,
        SearchAgentComposerPlanReferencesInput {
            project_id: project.id.as_str().to_string(),
            query: String::new(),
            limit: None,
        },
    )
    .await;

    let error = result.expect_err("a failed resolve must not silently drop the session");
    assert!(
        error.contains("artifact store is unavailable"),
        "the refusal must carry the underlying cause, got: {error}"
    );
}
