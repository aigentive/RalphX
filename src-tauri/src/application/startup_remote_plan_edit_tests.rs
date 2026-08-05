use super::{
    remote_plan_edit_intent::{
        request_remote_plan_edit_for_state, RequestRemotePlanEditInput,
        REMOTE_PLAN_EDIT_VERSION_CONFLICT,
    },
    startup_background::dispatch_one_remote_plan_edit,
    AppState,
};
use crate::domain::entities::{
    Artifact, ArtifactContent, ArtifactType, IdeationSession, IdeationSessionFlow, ProjectId,
    RemotePlanEditRequestStatus,
};

async fn seeded(state: &AppState) -> (IdeationSession, Artifact) {
    let mut session = IdeationSession::new(ProjectId::from_string("remote-edit-project".into()));
    session.session_flow = IdeationSessionFlow::Planning;
    session.plan_contract_version = 2;
    let session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("session");
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
    (session, artifact)
}
async fn request(state: &AppState, artifact: &Artifact, content: &str) -> String {
    request_remote_plan_edit_for_state(
        state,
        RequestRemotePlanEditInput {
            artifact_id: artifact.id.to_string(),
            content: content.into(),
            expected_version: artifact.metadata.version,
        },
    )
    .await
    .expect("request")
    .request_id
}
async fn versions(state: &AppState, id: &str) -> i64 {
    let id = id.to_string();
    state
        .db
        .run(move |conn| {
            Ok(conn.query_row(
                "SELECT COUNT(*) FROM artifacts WHERE id=?1 OR previous_version_id=?1",
                [id],
                |r| r.get(0),
            )?)
        })
        .await
        .expect("count")
}
async fn latest(state: &AppState, artifact: &Artifact) -> Artifact {
    let id = state
        .artifact_repo
        .resolve_latest_artifact_id(&artifact.id)
        .await
        .expect("resolve");
    state
        .artifact_repo
        .get_by_id(&id)
        .await
        .expect("get")
        .expect("artifact")
}

#[tokio::test]
async fn remote_plan_edit_dispatch_creates_one_version_and_reentry_is_idle() {
    let state = AppState::new_sqlite_test();
    let (_, artifact) = seeded(&state).await;
    let id = request(&state, &artifact, "new").await;
    dispatch_one_remote_plan_edit(&state)
        .await
        .expect("dispatch");
    let row = state
        .remote_plan_edit_request_repo
        .get(&id)
        .await
        .expect("read")
        .expect("row");
    assert_eq!(row.status, RemotePlanEditRequestStatus::Completed);
    assert_eq!(versions(&state, artifact.id.as_str()).await, 2);
    dispatch_one_remote_plan_edit(&state).await.expect("idle");
    assert_eq!(
        versions(&state, artifact.id.as_str()).await,
        2,
        "re-entry must not create another version"
    );
    let latest = latest(&state, &artifact).await;
    assert!(matches!(latest.content,ArtifactContent::Inline{ref text} if text=="new"));
}

#[tokio::test]
async fn remote_plan_edit_claim_time_version_conflict_preserves_content() {
    let state = AppState::new_sqlite_test();
    let (_, artifact) = seeded(&state).await;
    let id = request(&state, &artifact, "remote").await;
    crate::application::plan_artifact_edit::update_plan_artifact_for_state(
        &state,
        artifact.id.to_string(),
        "local".into(),
        None,
        None,
    )
    .await
    .expect("local edit");
    dispatch_one_remote_plan_edit(&state)
        .await
        .expect("dispatch");
    let row = state
        .remote_plan_edit_request_repo
        .get(&id)
        .await
        .expect("read")
        .expect("row");
    assert_eq!(row.status, RemotePlanEditRequestStatus::Failed);
    assert_eq!(
        row.error_code.as_deref(),
        Some(REMOTE_PLAN_EDIT_VERSION_CONFLICT)
    );
    let latest = latest(&state, &artifact).await;
    assert!(
        matches!(latest.content,ArtifactContent::Inline{ref text} if text=="local"),
        "remote content must not clobber the newer version"
    );
}
