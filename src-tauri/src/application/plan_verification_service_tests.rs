use crate::application::chat_service::MockChatService;
use crate::application::plan_verification_service::ensure_plan_verification_for_acceptance;
use crate::application::AppState;
use crate::domain::entities::{ArtifactId, IdeationSession, ProjectId};
use crate::domain::services::EffectiveGatePolicy;

fn policy(auto_verify_plans: bool, require_verification_for_accept: bool) -> EffectiveGatePolicy {
    EffectiveGatePolicy {
        auto_verify_plans,
        require_verification_for_accept,
        require_accept_for_finalize: false,
    }
}

async fn session_with_plan(state: &AppState) -> IdeationSession {
    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(ProjectId::new()))
        .await
        .expect("session should be created");
    state
        .ideation_session_repo
        .update_plan_artifact_id(&session.id, Some("plan-current".to_string()))
        .await
        .expect("plan should be linked");
    state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .expect("session read should succeed")
        .expect("session should exist")
}

#[tokio::test]
async fn acceptance_queues_required_automatic_verification_and_remains_blocked() {
    let state = AppState::new_test();
    let session = session_with_plan(&state).await;
    let chat = MockChatService::new();

    let error =
        ensure_plan_verification_for_acceptance(&state, &chat, &session, &policy(true, true))
            .await
            .expect_err("acceptance must wait for the queued verification turn");

    assert!(
        error.to_string().contains("queued"),
        "the caller should receive an actionable retry-after-verification error: {error}"
    );
    assert_eq!(chat.call_count(), 1, "exactly one turn should be queued");
    let options = chat.get_sent_options().await;
    let metadata = options[0]
        .metadata
        .as_deref()
        .expect("verification action metadata should be present");
    assert!(metadata.contains("\"ralphx_action_kind\":\"verify_plan\""));
    assert!(metadata.contains(session.id.as_str()));
    assert!(metadata.contains("plan-current"));
}

#[tokio::test]
async fn acceptance_does_not_auto_verify_when_verification_is_advisory() {
    let state = AppState::new_test();
    let session = session_with_plan(&state).await;
    let chat = MockChatService::new();

    ensure_plan_verification_for_acceptance(&state, &chat, &session, &policy(true, false))
        .await
        .expect("advisory verification must not delay acceptance");

    assert_eq!(
        chat.call_count(),
        0,
        "no verification turn should be queued"
    );
}

#[tokio::test]
async fn acceptance_requires_manual_verification_when_auto_trigger_is_disabled() {
    let state = AppState::new_test();
    let session = session_with_plan(&state).await;
    let chat = MockChatService::new();

    let error =
        ensure_plan_verification_for_acceptance(&state, &chat, &session, &policy(false, true))
            .await
            .expect_err("required unverified plan must remain blocked");

    assert!(error.to_string().contains("must be verified"));
    assert_eq!(chat.call_count(), 0, "manual mode must not queue a turn");
}

#[tokio::test]
async fn exact_current_proof_allows_acceptance_without_another_turn() {
    let state = AppState::new_test();
    let mut session = session_with_plan(&state).await;
    session.verified_plan_artifact_id = Some(ArtifactId::from_string("plan-current"));
    let chat = MockChatService::new();

    ensure_plan_verification_for_acceptance(&state, &chat, &session, &policy(true, true))
        .await
        .expect("exact proof should open the acceptance gate");

    assert_eq!(
        chat.call_count(),
        0,
        "verified plans must not queue duplicate work"
    );
}
