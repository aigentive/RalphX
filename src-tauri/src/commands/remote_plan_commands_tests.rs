use super::remote_plan_commands::*;
use crate::application::AppState;
use crate::domain::entities::{
    Artifact, ArtifactType, IdeationSession, IdeationSessionFlow, IdeationSessionStatus, ProjectId,
};

async fn planning_session(state: &AppState) -> IdeationSession {
    let mut session =
        IdeationSession::new(ProjectId::from_string("remote-plan-project".to_string()));
    session.session_flow = IdeationSessionFlow::Planning;
    state
        .ideation_session_repo
        .create(session)
        .await
        .expect("seed session")
}

#[tokio::test]
async fn remote_plan_edit_request_rejects_version_drift_and_deduplicates_only_identical_edits() {
    let state = AppState::new_sqlite_test();
    let session = planning_session(&state).await;
    let artifact = state
        .artifact_repo
        .create(Artifact::new_inline(
            "Plan",
            ArtifactType::Specification,
            "old",
            "test",
        ))
        .await
        .expect("artifact");
    state
        .ideation_session_repo
        .update_plan_artifact_id(&session.id, Some(artifact.id.to_string()))
        .await
        .expect("link");
    let make = |content: &str, expected_version| RequestRemotePlanEditInput {
        artifact_id: artifact.id.to_string(),
        content: content.to_string(),
        expected_version,
    };
    assert_eq!(
        request_remote_plan_edit_for_state(&state, make("new", artifact.metadata.version + 1))
            .await
            .expect_err("stale token"),
        REMOTE_PLAN_EDIT_VERSION_CONFLICT
    );
    assert!(
        state
            .remote_plan_edit_request_repo
            .find_unsettled_for_artifact(artifact.id.as_str())
            .await
            .expect("read")
            .is_none(),
        "validation must persist last"
    );
    let first = request_remote_plan_edit_for_state(&state, make("new", artifact.metadata.version))
        .await
        .expect("persist");
    let duplicate =
        request_remote_plan_edit_for_state(&state, make("new", artifact.metadata.version))
            .await
            .expect("dedupe");
    assert_eq!(first.request_id, duplicate.request_id);
    assert!(duplicate.deduplicated);
    assert_eq!(
        request_remote_plan_edit_for_state(&state, make("different", artifact.metadata.version))
            .await
            .expect_err("conflicting content"),
        REMOTE_PLAN_EDIT_AUTHORITY_CHANGED
    );
    assert_eq!(
        get_remote_plan_edit_request_for_state(&state, "missing".into())
            .await
            .expect_err("missing poll"),
        REMOTE_PLAN_EDIT_REQUEST_NOT_FOUND
    );
}
fn input(session: &IdeationSession, artifact: &str) -> RequestRemotePlanApprovalInput {
    RequestRemotePlanApprovalInput {
        session_id: session.id.as_str().to_string(),
        artifact_id: artifact.to_string(),
        blueprint_artifact_id: Some("blueprint-1".into()),
        blueprint_artifact_version: Some(2),
    }
}

#[tokio::test]
async fn remote_plan_request_validates_persists_last_and_deduplicates_exact_triple() {
    let state = AppState::new_test();
    let missing = request_remote_plan_approval_for_state(
        &state,
        RequestRemotePlanApprovalInput {
            session_id: "missing".into(),
            artifact_id: "plan".into(),
            blueprint_artifact_id: None,
            blueprint_artifact_version: None,
        },
    )
    .await
    .expect_err("missing rejects");
    assert_eq!(missing, REMOTE_PLAN_APPROVAL_SESSION_NOT_FOUND);
    let session = planning_session(&state).await;
    let blank = request_remote_plan_approval_for_state(&state, input(&session, " "))
        .await
        .expect_err("blank rejects");
    assert_eq!(blank, REMOTE_PLAN_APPROVAL_ARTIFACT_REQUIRED);
    assert!(state
        .remote_plan_approval_request_repo
        .find_unsettled_for_session(&session.id)
        .await
        .expect("read")
        .is_none());
    let first = request_remote_plan_approval_for_state(&state, input(&session, "plan-1"))
        .await
        .expect("persist");
    let duplicate = request_remote_plan_approval_for_state(&state, input(&session, "plan-1"))
        .await
        .expect("dedupe");
    assert_eq!(first.request_id, duplicate.request_id);
    assert!(duplicate.deduplicated);
    let conflict = request_remote_plan_approval_for_state(&state, input(&session, "plan-2"))
        .await
        .expect_err("different triple rejects");
    assert_eq!(conflict, REMOTE_PLAN_APPROVAL_AUTHORITY_CHANGED);
}

#[tokio::test]
async fn remote_plan_request_requires_active_planning_and_poll_distinguishes_missing() {
    let state = AppState::new_test();
    let mut session = planning_session(&state).await;
    session.status = IdeationSessionStatus::Archived;
    state
        .ideation_session_repo
        .update_status(&session.id, session.status)
        .await
        .expect("archive");
    assert_eq!(
        request_remote_plan_approval_for_state(&state, input(&session, "plan"))
            .await
            .expect_err("inactive rejects"),
        REMOTE_PLAN_APPROVAL_AUTHORITY_CHANGED
    );
    assert!(state
        .remote_plan_approval_request_repo
        .find_unsettled_for_session(&session.id)
        .await
        .expect("read")
        .is_none());
    assert_eq!(
        get_remote_plan_approval_request_for_state(&state, "missing".into())
            .await
            .expect_err("missing poll"),
        REMOTE_PLAN_APPROVAL_REQUEST_NOT_FOUND
    );
}
