use super::remote_question_commands::{
    resolve_remote_user_question_for_state, ResolveRemoteUserQuestionInput,
    REMOTE_PLAN_MODE_PROPOSAL_REQUIRES_HOST,
};
use crate::application::{AppState, QuestionAnswer};
use crate::commands::question_commands::{
    PLAN_MODE_PROPOSAL_ACCEPT_VALUE, PLAN_MODE_PROPOSAL_KIND,
};
use ralphx_events::RecordingEventSink;
use std::sync::Arc;

fn state_with_events() -> (AppState, RecordingEventSink) {
    let events = RecordingEventSink::new();
    let mut state = AppState::new_test();
    state.events = Arc::new(events.clone());
    (state, events)
}

fn input(request_id: &str, selected_options: &[&str]) -> ResolveRemoteUserQuestionInput {
    ResolveRemoteUserQuestionInput {
        request_id: request_id.to_string(),
        selected_options: selected_options
            .iter()
            .map(|option| (*option).to_string())
            .collect(),
        custom_response: Some("From the phone".to_string()),
        skipped: false,
    }
}

async fn register_question(
    state: &AppState,
    request_id: &str,
    metadata: Option<serde_json::Value>,
) -> tokio::sync::watch::Receiver<Option<QuestionAnswer>> {
    state
        .question_state
        .register_with_metadata(
            request_id.to_string(),
            "session-remote".to_string(),
            "Which path?".to_string(),
            None,
            vec![],
            false,
            true,
            None,
            None,
            metadata,
        )
        .await
}

#[tokio::test]
async fn remote_question_live_answer_wakes_waiter_and_resolves_durably() {
    let (state, events) = state_with_events();
    let receiver = register_question(&state, "remote-live", None).await;

    let response =
        resolve_remote_user_question_for_state(&state, input("remote-live", &["approve"]))
            .await
            .expect("remote answer succeeds");

    assert!(response.success);
    assert!(response.delivered_to_waiting_agent);
    assert!(!response.plan_mode_proposal_handled);
    let delivered = receiver
        .borrow()
        .clone()
        .expect("live waiter receives answer");
    assert_eq!(delivered.selected_options, vec!["approve"]);
    assert_eq!(delivered.text.as_deref(), Some("From the phone"));
    assert!(state.question_state.get_pending_info().await.is_empty());
    assert_eq!(
        state
            .question_state
            .get_resolved_answer("remote-live")
            .await
            .expect("resolved answer lookup")
            .expect("durable answer")
            .selected_options,
        vec!["approve"]
    );
    assert_eq!(
        events.events(),
        vec![ralphx_events::RecordedEvent {
            event: "agent:question_resolved".to_string(),
            payload: serde_json::json!({
                "sessionId": "session-remote",
                "requestId": "remote-live",
            }),
        }]
    );
}

#[tokio::test]
async fn remote_question_refuses_plan_mode_acceptance_without_side_effects() {
    let (state, events) = state_with_events();
    let receiver = register_question(
        &state,
        "remote-plan",
        Some(serde_json::json!({
            "kind": PLAN_MODE_PROPOSAL_KIND,
            "conversation_id": "conversation-plan",
        })),
    )
    .await;

    let error = resolve_remote_user_question_for_state(
        &state,
        input("remote-plan", &[PLAN_MODE_PROPOSAL_ACCEPT_VALUE]),
    )
    .await
    .expect_err("remote Plan-mode acceptance is refused");

    assert_eq!(error, REMOTE_PLAN_MODE_PROPOSAL_REQUIRES_HOST);
    assert!(
        receiver.borrow().is_none(),
        "the waiting agent is not resumed"
    );
    assert!(state
        .question_state
        .get_resolved_answer("remote-plan")
        .await
        .expect("answer lookup")
        .is_none());
    let claim = state
        .question_state
        .claim_pending("remote-plan")
        .await
        .expect("claim lookup")
        .expect("released question remains claimable");
    state.question_state.release_claim(claim).await;
    assert!(state
        .queued_message_repo
        .list_keys()
        .await
        .expect("continuation queue lookup")
        .is_empty());
    assert!(
        events.events().is_empty(),
        "a refused answer emits no event"
    );
}

#[tokio::test]
async fn remote_question_rejects_an_already_resolved_answer_without_double_delivery() {
    let (state, events) = state_with_events();
    let receiver = register_question(&state, "remote-resolved", None).await;
    resolve_remote_user_question_for_state(&state, input("remote-resolved", &["first"]))
        .await
        .expect("first answer succeeds");

    let error =
        resolve_remote_user_question_for_state(&state, input("remote-resolved", &["second"]))
            .await
            .expect_err("second answer is refused");

    assert!(error.contains("already resolved"));
    assert_eq!(
        receiver
            .borrow()
            .as_ref()
            .expect("only answer")
            .selected_options,
        vec!["first"]
    );
    assert_eq!(
        state
            .question_state
            .get_resolved_answer("remote-resolved")
            .await
            .expect("answer lookup")
            .expect("resolved answer")
            .selected_options,
        vec!["first"]
    );
    assert_eq!(events.events().len(), 1, "the retry emits no second event");
}

#[tokio::test]
async fn remote_question_commits_wait_expired_answer_without_live_delivery() {
    let state = AppState::new_test();
    register_question(&state, "remote-late", None).await;
    state.question_state.expire("remote-late").await;

    let response = resolve_remote_user_question_for_state(&state, input("remote-late", &["late"]))
        .await
        .expect("late answer commits");

    assert!(response.success);
    assert!(!response.delivered_to_waiting_agent);
    assert_eq!(
        state
            .question_state
            .get_resolved_answer("remote-late")
            .await
            .expect("answer lookup")
            .expect("late durable answer")
            .selected_options,
        vec!["late"]
    );
}

#[tokio::test]
async fn remote_question_rejects_unknown_request_id() {
    let state = AppState::new_test();

    let error = resolve_remote_user_question_for_state(&state, input("missing", &[]))
        .await
        .expect_err("unknown question is refused");

    assert_eq!(error, "Question request 'missing' not found");
}
