use chrono::{Duration, Utc};
use rusqlite::OptionalExtension;

use super::startup_background::dispatch_one_remote_plan_approval;
use super::AppState;
use crate::application::remote_plan_approval_intent::{
    request_remote_plan_approval_for_state, RequestRemotePlanApprovalInput,
    REMOTE_PLAN_APPROVAL_AUTHORITY_CHANGED, REMOTE_PLAN_APPROVAL_PLAN_CHANGED,
};
use crate::domain::entities::{
    Artifact, ArtifactType, IdeationSession, IdeationSessionFlow, IdeationSessionStatus, ProjectId,
    RemotePlanApprovalRequestStatus,
};

async fn seeded_plan(state: &AppState) -> (IdeationSession, Artifact) {
    state
        .db
        .run(|conn| {
            conn.execute(
                "UPDATE ideation_settings SET tasks_enabled = 0 WHERE id = 1",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("disable assessor side effect");
    let mut session =
        IdeationSession::new(ProjectId::from_string("remote-plan-project".to_string()));
    session.session_flow = IdeationSessionFlow::Planning;
    session.plan_contract_version = 1;
    let session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("seed planning session");
    let artifact = state
        .artifact_repo
        .create(Artifact::new_inline(
            "Remote plan",
            ArtifactType::Specification,
            "Plan body",
            "test",
        ))
        .await
        .expect("seed plan artifact");
    state
        .ideation_session_repo
        .update_plan_artifact_id(&session.id, Some(artifact.id.as_str().to_string()))
        .await
        .expect("link current plan");
    (session, artifact)
}

async fn request(state: &AppState, session: &IdeationSession, artifact_id: &str) -> String {
    request_remote_plan_approval_for_state(
        state,
        RequestRemotePlanApprovalInput {
            session_id: session.id.as_str().to_string(),
            artifact_id: artifact_id.to_string(),
            blueprint_artifact_id: None,
            blueprint_artifact_version: None,
        },
    )
    .await
    .expect("request plan approval")
    .request_id
}

async fn approval_actor(state: &AppState, session: &IdeationSession) -> Option<String> {
    let session_id = session.id.as_str().to_string();
    state
        .db
        .run(move |conn| {
            conn.query_row(
                "SELECT approved_by FROM plan_artifact_approvals WHERE session_id = ?1",
                [session_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
        })
        .await
        .expect("read approval row")
}

#[tokio::test]
async fn remote_plan_dispatch_approves_once_and_persists_terminal_result() {
    let state = AppState::new_sqlite_test();
    let (session, artifact) = seeded_plan(&state).await;
    let request_id = request(&state, &session, artifact.id.as_str()).await;

    dispatch_one_remote_plan_approval(&state)
        .await
        .expect("first dispatch");
    let completed = state
        .remote_plan_approval_request_repo
        .get(&request_id)
        .await
        .expect("read request")
        .expect("request row");
    assert_eq!(completed.status, RemotePlanApprovalRequestStatus::Completed);
    assert!(
        completed.result.is_some(),
        "completed request must retain result JSON"
    );
    assert_eq!(
        approval_actor(&state, &session).await.as_deref(),
        Some("user")
    );

    dispatch_one_remote_plan_approval(&state)
        .await
        .expect("re-dispatch is idle");
    let after_reentry = state
        .remote_plan_approval_request_repo
        .get(&request_id)
        .await
        .expect("read after re-entry")
        .expect("request row");
    assert_eq!(
        after_reentry, completed,
        "settled request must not be changed"
    );
}

#[tokio::test]
async fn remote_plan_dispatch_rejects_changed_plan_without_approval() {
    let state = AppState::new_sqlite_test();
    let (session, stale_artifact) = seeded_plan(&state).await;
    let request_id = request(&state, &session, stale_artifact.id.as_str()).await;
    let current_artifact = state
        .artifact_repo
        .create(Artifact::new_inline(
            "Changed remote plan",
            ArtifactType::Specification,
            "Changed plan body",
            "test",
        ))
        .await
        .expect("seed changed plan");
    state
        .ideation_session_repo
        .update_plan_artifact_id(&session.id, Some(current_artifact.id.as_str().to_string()))
        .await
        .expect("move current plan");

    dispatch_one_remote_plan_approval(&state)
        .await
        .expect("dispatch changed plan");
    let failed = state
        .remote_plan_approval_request_repo
        .get(&request_id)
        .await
        .expect("read request")
        .expect("request row");
    assert_eq!(failed.status, RemotePlanApprovalRequestStatus::Failed);
    assert_eq!(
        failed.error_code.as_deref(),
        Some(REMOTE_PLAN_APPROVAL_PLAN_CHANGED)
    );
    assert!(approval_actor(&state, &session).await.is_none());
}

#[tokio::test]
async fn remote_plan_dispatch_revalidates_authority_before_host_seam() {
    let state = AppState::new_sqlite_test();
    let (session, artifact) = seeded_plan(&state).await;
    let request_id = request(&state, &session, artifact.id.as_str()).await;
    state
        .ideation_session_repo
        .update_status(&session.id, IdeationSessionStatus::Archived)
        .await
        .expect("archive after request");

    dispatch_one_remote_plan_approval(&state)
        .await
        .expect("dispatch archived session");
    let failed = state
        .remote_plan_approval_request_repo
        .get(&request_id)
        .await
        .expect("read request")
        .expect("request row");
    assert_eq!(failed.status, RemotePlanApprovalRequestStatus::Failed);
    assert_eq!(
        failed.error_code.as_deref(),
        Some(REMOTE_PLAN_APPROVAL_AUTHORITY_CHANGED)
    );
    assert!(approval_actor(&state, &session).await.is_none());
}

#[tokio::test]
async fn stale_remote_plan_claim_is_swept_and_never_dispatched() {
    let state = AppState::new_sqlite_test();
    let (session, artifact) = seeded_plan(&state).await;
    let request_id = request(&state, &session, artifact.id.as_str()).await;
    state
        .remote_plan_approval_request_repo
        .claim_pending(Utc::now() - Duration::minutes(10))
        .await
        .expect("claim old request")
        .expect("pending row");
    assert_eq!(
        state
            .remote_plan_approval_request_repo
            .fail_stale(Utc::now() - Duration::minutes(5), Utc::now())
            .await
            .expect("startup stale sweep"),
        1
    );

    dispatch_one_remote_plan_approval(&state)
        .await
        .expect("settled stale request is idle");
    let stale = state
        .remote_plan_approval_request_repo
        .get(&request_id)
        .await
        .expect("read request")
        .expect("request row");
    assert_eq!(stale.status, RemotePlanApprovalRequestStatus::FailedStale);
    assert!(
        stale.result.is_none(),
        "stale request must not gain a result"
    );
    assert!(approval_actor(&state, &session).await.is_none());
}
