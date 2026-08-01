//! Falsification tests for the spawn-free remote agent stop.
//!
//! Every refusal asserts BOTH the error code AND the absence of the durable row the command
//! would otherwise create, because the failure this command exists to prevent is a brake
//! persisted against a conversation the host cannot resolve — a request that looks accepted
//! and can never drain.

use super::*;
use crate::application::AppState;
use crate::domain::entities::{ChatConversation, ProjectId};

async fn seed_conversation(state: &AppState) -> ChatConversationId {
    let conversation =
        ChatConversation::new_project(ProjectId::from_string("project-1".to_string()));
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("seed conversation");
    conversation.id
}

fn input(conversation_id: &str) -> RequestRemoteAgentStopInput {
    RequestRemoteAgentStopInput {
        conversation_id: conversation_id.to_string(),
    }
}

async fn stop_request_count(state: &AppState, conversation_id: &ChatConversationId) -> usize {
    usize::from(
        state
            .remote_agent_stop_request_repo
            .find_unsettled_stop_request_for_conversation(conversation_id)
            .await
            .expect("dedupe read")
            .is_some(),
    )
}

#[tokio::test]
async fn persists_a_pending_stop_intent() {
    let state = AppState::new_test();
    let conversation_id = seed_conversation(&state).await;

    let response = request_remote_agent_stop_for_state(&state, input(&conversation_id.as_str()))
        .await
        .expect("stop persisted");

    assert_eq!(response.status, RemoteAgentStopStatus::Pending);
    assert!(!response.deduplicated);
    assert_eq!(response.conversation_id, conversation_id.as_str());

    let stored = state
        .remote_agent_stop_request_repo
        .get_stop_request(&response.stop_request_id)
        .await
        .expect("read intent")
        .expect("intent exists");
    assert_eq!(stored.conversation_id, conversation_id);
    assert!(stored.error_code.is_none());
    assert!(
        stored.claimed_at.is_none(),
        "the request path must never claim its own intent"
    );
}

#[tokio::test]
async fn rejects_empty_conversation_id_and_leaves_no_state() {
    let state = AppState::new_test();
    let conversation_id = seed_conversation(&state).await;

    let err = request_remote_agent_stop_for_state(&state, input("   "))
        .await
        .expect_err("empty conversation id rejected");
    assert_eq!(err, REMOTE_AGENT_STOP_EMPTY_CONVERSATION);
    assert_eq!(stop_request_count(&state, &conversation_id).await, 0);
}

#[tokio::test]
async fn rejects_unknown_conversation_and_leaves_no_state() {
    let state = AppState::new_test();

    let err = request_remote_agent_stop_for_state(&state, input("does-not-exist"))
        .await
        .expect_err("unknown conversation rejected");
    assert_eq!(err, REMOTE_AGENT_STOP_CONVERSATION_NOT_FOUND);
    assert_eq!(
        stop_request_count(
            &state,
            &ChatConversationId::from_string("does-not-exist".to_string())
        )
        .await,
        0,
        "a refused stop must not leave an undrainable intent behind"
    );
}

/// A second tap while a brake is unsettled JOINS it. Stacking would keep firing kills at a
/// conversation the user may have restarted in the meantime.
#[tokio::test]
async fn a_second_stop_for_the_same_conversation_dedupes_rather_than_stacking() {
    let state = AppState::new_test();
    let conversation_id = seed_conversation(&state).await;

    let first = request_remote_agent_stop_for_state(&state, input(&conversation_id.as_str()))
        .await
        .expect("first stop persisted");
    let second = request_remote_agent_stop_for_state(&state, input(&conversation_id.as_str()))
        .await
        .expect("second stop joins the first");

    assert_eq!(second.stop_request_id, first.stop_request_id);
    assert!(second.deduplicated);
    assert!(!first.deduplicated);
    assert_eq!(second.created_at, first.created_at);
}

/// Dedupe is conversation-scoped: an unrelated conversation must still get its own brake.
#[tokio::test]
async fn dedupe_does_not_cross_conversations() {
    let state = AppState::new_test();
    let first_conversation = seed_conversation(&state).await;
    let second_conversation = seed_conversation(&state).await;

    let first = request_remote_agent_stop_for_state(&state, input(&first_conversation.as_str()))
        .await
        .expect("first stop persisted");
    let second = request_remote_agent_stop_for_state(&state, input(&second_conversation.as_str()))
        .await
        .expect("second conversation gets its own stop");

    assert_ne!(second.stop_request_id, first.stop_request_id);
    assert!(!second.deduplicated);
}

/// Once the brake settles, a NEW stop can be requested — dedupe must not become a permanent
/// lock on a conversation that has since started another run.
#[tokio::test]
async fn a_settled_stop_does_not_block_a_later_one() {
    let state = AppState::new_test();
    let conversation_id = seed_conversation(&state).await;

    let first = request_remote_agent_stop_for_state(&state, input(&conversation_id.as_str()))
        .await
        .expect("first stop persisted");
    let now = chrono::Utc::now();
    state
        .remote_agent_stop_request_repo
        .claim_pending_stop_request(now)
        .await
        .expect("claim")
        .expect("a pending row is claimable");
    state
        .remote_agent_stop_request_repo
        .complete_stop_request(&first.stop_request_id, now)
        .await
        .expect("complete");

    let second = request_remote_agent_stop_for_state(&state, input(&conversation_id.as_str()))
        .await
        .expect("a settled brake does not block a new one");
    assert_ne!(second.stop_request_id, first.stop_request_id);
    assert!(!second.deduplicated);
}

#[tokio::test]
async fn status_read_resolves_the_intent_and_fails_closed_on_unknown() {
    let state = AppState::new_test();
    let conversation_id = seed_conversation(&state).await;

    let response = request_remote_agent_stop_for_state(&state, input(&conversation_id.as_str()))
        .await
        .expect("stop persisted");

    let view = get_remote_agent_stop_request_for_state(&state, &response.stop_request_id)
        .await
        .expect("status resolves");
    assert_eq!(view.id, response.stop_request_id);
    assert_eq!(view.status, RemoteAgentStopStatus::Pending);
    assert!(view.error_code.is_none());

    let missing = get_remote_agent_stop_request_for_state(&state, "no-such-id").await;
    assert_eq!(missing.unwrap_err(), REMOTE_AGENT_STOP_REQUEST_NOT_FOUND);
}

/// `NoLiveRun` is benign: it must be terminal AND must not be spelled with an error code, or
/// the client would show "couldn't stop the agent" for the ordinary finished-in-flight race.
#[tokio::test]
async fn no_live_run_surfaces_as_a_terminal_without_an_error_code() {
    let state = AppState::new_test();
    let conversation_id = seed_conversation(&state).await;
    let response = request_remote_agent_stop_for_state(&state, input(&conversation_id.as_str()))
        .await
        .expect("stop persisted");

    let now = chrono::Utc::now();
    state
        .remote_agent_stop_request_repo
        .claim_pending_stop_request(now)
        .await
        .expect("claim")
        .expect("claimable");
    state
        .remote_agent_stop_request_repo
        .resolve_stop_request_no_live_run(&response.stop_request_id, now)
        .await
        .expect("resolve");

    let view = get_remote_agent_stop_request_for_state(&state, &response.stop_request_id)
        .await
        .expect("status resolves");
    assert_eq!(view.status, RemoteAgentStopStatus::NoLiveRun);
    assert!(view.status.is_terminal());
    assert!(view.error_code.is_none());
}
