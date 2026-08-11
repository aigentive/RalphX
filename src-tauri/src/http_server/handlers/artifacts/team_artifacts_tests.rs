use axum::http::{HeaderMap, HeaderValue, StatusCode};

use super::team_artifacts::{
    artifact_author, authorized_team_artifact_author, persist_team_artifact,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentRun, Artifact, ArtifactType, ChatConversation, IdeationSessionId,
};

fn canonical_agent_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ralphx-agent-type",
        HeaderValue::from_static("ralphx-ideation-specialist-backend"),
    );
    headers
}

#[test]
fn artifact_author_uses_validated_canonical_transport_identity() {
    let headers = canonical_agent_headers();

    assert_eq!(
        artifact_author(&headers).expect("canonical caller should be accepted"),
        "ralphx-ideation-specialist-backend"
    );
}

#[tokio::test]
async fn canonical_artifact_author_requires_matching_active_runtime_lineage() {
    let app_state = AppState::new_sqlite_test();
    let conversation = ChatConversation::new_ideation(IdeationSessionId::from_string("session-1"));
    let conversation_id = conversation.id;
    app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    let run = app_state
        .agent_run_repo
        .create(AgentRun::new(conversation_id))
        .await
        .unwrap();

    let mut headers = canonical_agent_headers();
    headers.insert(
        "x-ralphx-agent-run-id",
        HeaderValue::from_str(&run.id.as_str()).unwrap(),
    );
    headers.insert(
        "x-ralphx-conversation-id",
        HeaderValue::from_str(&conversation_id.as_str()).unwrap(),
    );

    assert_eq!(
        authorized_team_artifact_author(&app_state, &headers, "session-1")
            .await
            .expect("active run lineage should authorize the canonical specialist"),
        "ralphx-ideation-specialist-backend"
    );
    let (status, message) =
        authorized_team_artifact_author(&app_state, &headers, "different-session")
            .await
            .expect_err("a different session must not inherit the run authority");
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(message.contains("does not match"));
}

#[tokio::test]
async fn canonical_artifact_author_rejects_missing_runtime_authority() {
    let app_state = AppState::new_sqlite_test();
    let (status, message) =
        authorized_team_artifact_author(&app_state, &canonical_agent_headers(), "session-1")
            .await
            .expect_err("a canonical name alone is not mutation authority");

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(message.contains("requires runtime run and conversation authority"));
}

#[tokio::test]
async fn canonical_agent_without_tool_grant_cannot_create_team_artifacts() {
    let app_state = AppState::new_sqlite_test();
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ralphx-agent-type",
        HeaderValue::from_static("ralphx-general-worker"),
    );

    let (status, message) = authorized_team_artifact_author(&app_state, &headers, "session-1")
        .await
        .expect_err("canonical agents still need the explicit tool grant");
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(message.contains("cannot create team artifacts"));
}

#[test]
fn artifact_author_falls_back_to_system_without_transport_identity() {
    assert_eq!(
        artifact_author(&HeaderMap::new()).expect("non-MCP callers use system attribution"),
        "system"
    );
}

#[test]
fn artifact_author_falls_back_to_system_for_missing_identity_placeholder() {
    let mut headers = HeaderMap::new();
    headers.insert("x-ralphx-agent-type", HeaderValue::from_static("unknown"));

    assert_eq!(
        artifact_author(&headers).expect("missing MCP identity uses system attribution"),
        "system"
    );
}

#[test]
fn artifact_author_rejects_unknown_transport_identity() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ralphx-agent-type",
        HeaderValue::from_static("unknown-agent"),
    );

    let (status, message) = artifact_author(&headers).expect_err("unknown caller must fail closed");
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(message.contains("Unknown canonical caller agent"));
}

#[tokio::test]
async fn team_artifact_relation_failure_rolls_back_artifact() {
    let app_state = AppState::new_sqlite_test();
    let artifact = Artifact::new_inline(
        "Atomic team artifact",
        ArtifactType::TeamSummary,
        "content",
        "system",
    );
    let artifact_id = artifact.id.clone();

    let (status, message) = persist_team_artifact(
        &app_state,
        artifact,
        Some("missing-related-artifact".to_string()),
    )
    .await
    .expect_err("a missing relation target must reject the complete write");

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(message.contains("does not exist"));
    assert!(
        app_state
            .artifact_repo
            .get_by_id(&artifact_id)
            .await
            .unwrap()
            .is_none(),
        "the artifact insert must roll back with the failed relation"
    );
}

#[tokio::test]
async fn team_artifact_and_relation_are_persisted_together() {
    let app_state = AppState::new_sqlite_test();
    let related_artifact = Artifact::new_inline(
        "Existing artifact",
        ArtifactType::TeamResearch,
        "research",
        "system",
    );
    let related_artifact_id = related_artifact.id.clone();
    app_state
        .artifact_repo
        .create(related_artifact)
        .await
        .unwrap();

    let artifact = Artifact::new_inline(
        "Related team artifact",
        ArtifactType::TeamSummary,
        "summary",
        "system",
    );
    let artifact_id = artifact.id.clone();

    let created_id =
        persist_team_artifact(&app_state, artifact, Some(related_artifact_id.to_string()))
            .await
            .expect("a valid artifact and relation should commit atomically");

    assert_eq!(created_id, artifact_id);
    assert!(app_state
        .artifact_repo
        .get_by_id(&created_id)
        .await
        .unwrap()
        .is_some());
    let relations = app_state
        .artifact_repo
        .get_relations(&created_id)
        .await
        .unwrap();
    assert_eq!(relations.len(), 1);
    assert_eq!(relations[0].from_artifact_id, created_id);
    assert_eq!(relations[0].to_artifact_id, related_artifact_id);
}
