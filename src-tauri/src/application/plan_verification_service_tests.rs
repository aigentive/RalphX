use crate::application::chat_service::MockChatService;
use crate::application::plan_verification_service::{
    ensure_plan_verification_for_acceptance, request_plan_verification,
    PlanVerificationRequestOutcome, PlanVerificationRequestSource,
};
use crate::application::AppState;
use crate::domain::entities::{
    ArtifactId, IdeationSession, ProjectId, VerificationRunSnapshot, VerificationStatus,
};
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

#[tokio::test]
async fn concurrent_verification_requests_admit_exactly_one_turn() {
    let state = AppState::new_test();
    let session = session_with_plan(&state).await;
    let chat = MockChatService::new();

    let (first, second) = tokio::join!(
        request_plan_verification(
            &state,
            &chat,
            &session.id,
            PlanVerificationRequestSource::Manual,
        ),
        request_plan_verification(
            &state,
            &chat,
            &session.id,
            PlanVerificationRequestSource::Automatic,
        ),
    );

    first.expect("first request should settle");
    second.expect("second request should settle");
    assert_eq!(
        chat.call_count(),
        1,
        "admission must be serialized per plan"
    );
}

#[tokio::test]
async fn verification_request_does_not_queue_when_session_is_already_in_progress() {
    let state = AppState::new_test();
    let session = session_with_plan(&state).await;
    let chat = MockChatService::new();
    state
        .ideation_session_repo
        .update_verification_state(&session.id, VerificationStatus::Reviewing, true)
        .await
        .expect("active verification state should be recorded");

    let outcome = request_plan_verification(
        &state,
        &chat,
        &session.id,
        PlanVerificationRequestSource::Manual,
    )
    .await
    .expect("request should return the current verification state");

    assert_eq!(outcome, PlanVerificationRequestOutcome::AlreadyRunning);
    assert_eq!(chat.call_count(), 0, "active work must not be duplicated");
}

#[tokio::test]
async fn verification_request_does_not_queue_when_current_snapshot_is_in_progress() {
    let state = AppState::new_test();
    let session = session_with_plan(&state).await;
    let chat = MockChatService::new();
    let snapshot = VerificationRunSnapshot {
        generation: session.verification_generation,
        status: VerificationStatus::Reviewing,
        in_progress: true,
        current_round: 1,
        max_rounds: 3,
        best_round_index: None,
        convergence_reason: None,
        current_gaps: vec![],
        rounds: vec![],
    };
    state
        .ideation_session_repo
        .save_verification_run_snapshot(&session.id, &snapshot)
        .await
        .expect("active run snapshot should be recorded");
    state
        .ideation_session_repo
        .update_verification_state(&session.id, VerificationStatus::Unverified, false)
        .await
        .expect("stale session summary should be reset");

    let outcome = request_plan_verification(
        &state,
        &chat,
        &session.id,
        PlanVerificationRequestSource::Automatic,
    )
    .await
    .expect("request should respect the current run snapshot");

    assert_eq!(outcome, PlanVerificationRequestOutcome::AlreadyRunning);
    assert_eq!(
        chat.call_count(),
        0,
        "snapshot-owned work must not be duplicated"
    );
}
