//! Falsification tests for the spawn-free remote conversation MODE SWITCH.
//!
//! Every refusal asserts BOTH the error code AND the absence of a persisted intent row. The
//! failures this command exists to prevent are (a) a switch persisted against a conversation the
//! host cannot resolve — accepted-looking and undrainable, and (b) a switch that races a live
//! run into a worktree the running agent is using.

use super::*;
use crate::application::AppState;
use ralphx_domain::entities::{AgentRun, ChatConversation, Project};

/// Seeds a project and one IDLE project conversation. Conversations start with `agent_mode: None`
/// so the resolver's `unwrap_or(Chat)` fallback puts them in Chat, exactly like a remote
/// conversation seeded by the start intent (which host-pins `mode` to "chat").
async fn seed(state: &AppState) -> (String, ChatConversation) {
    let project = Project::new(
        "Remote mode switch test".to_string(),
        "/tmp/remote-mode-switch".to_string(),
    );
    let project = state
        .project_repo
        .create(project)
        .await
        .expect("seed project");
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .expect("seed conversation");
    (project.id.to_string(), conversation)
}

fn input(
    conversation_id: &str,
    project_id: &str,
    mode: &str,
) -> RequestRemoteAgentConversationModeSwitchInput {
    RequestRemoteAgentConversationModeSwitchInput {
        conversation_id: conversation_id.to_string(),
        project_id: project_id.to_string(),
        mode: mode.to_string(),
    }
}

/// No unsettled intent row may exist for this conversation.
async fn unsettled_absent(state: &AppState, conversation_id: &ChatConversationId) -> bool {
    state
        .remote_conversation_mode_switch_request_repo
        .find_unsettled_mode_switch_request_for_conversation(conversation_id)
        .await
        .expect("dedupe read")
        .is_none()
}

#[tokio::test]
async fn persists_a_pending_intent_for_an_idle_chat_conversation() {
    let state = AppState::new_test();
    let (project_id, conversation) = seed(&state).await;

    let response = request_remote_agent_conversation_mode_switch_for_state(
        &state,
        input(&conversation.id.as_str(), &project_id, "edit"),
    )
    .await
    .expect("intent persisted");

    assert_eq!(response.status, RemoteConversationModeSwitchStatus::Pending);
    assert!(!response.deduplicated);
    assert_eq!(response.conversation_id, conversation.id.as_str());

    let stored = state
        .remote_conversation_mode_switch_request_repo
        .get_mode_switch_request(&response.mode_switch_request_id)
        .await
        .expect("read intent")
        .expect("intent exists");
    assert_eq!(stored.conversation_id, conversation.id);
    assert_eq!(stored.target_mode, AgentConversationWorkspaceMode::Edit);
    assert!(stored.error_code.is_none());
    assert!(
        stored.claimed_at.is_none(),
        "the request path must never claim its own intent"
    );
}

/// Every non-chat mode the picker offers must be reachable. This is the actual bug: a remote
/// conversation could reach chat and nothing else.
#[tokio::test]
async fn accepts_every_valid_workspace_mode() {
    for mode in ["edit", "plan", "ideation", "autopilot", "review_pr"] {
        let state = AppState::new_test();
        let (project_id, conversation) = seed(&state).await;

        let response = request_remote_agent_conversation_mode_switch_for_state(
            &state,
            input(&conversation.id.as_str(), &project_id, mode),
        )
        .await
        .unwrap_or_else(|error| panic!("mode {mode} must be requestable, got {error}"));
        assert_eq!(
            response.status,
            RemoteConversationModeSwitchStatus::Pending,
            "mode {mode}"
        );
    }
}

#[tokio::test]
async fn rejects_an_unknown_mode_and_leaves_no_intent() {
    let state = AppState::new_test();
    let (project_id, conversation) = seed(&state).await;

    let err = request_remote_agent_conversation_mode_switch_for_state(
        &state,
        input(&conversation.id.as_str(), &project_id, "sudo"),
    )
    .await
    .expect_err("unknown mode rejected");
    assert_eq!(err, REMOTE_MODE_SWITCH_INVALID_MODE);
    assert!(unsettled_absent(&state, &conversation.id).await);
}

#[tokio::test]
async fn rejects_empty_conversation_id_and_leaves_no_intent() {
    let state = AppState::new_test();
    let (project_id, conversation) = seed(&state).await;

    let err = request_remote_agent_conversation_mode_switch_for_state(
        &state,
        input("   ", &project_id, "edit"),
    )
    .await
    .expect_err("empty conversation id rejected");
    assert_eq!(err, REMOTE_MODE_SWITCH_EMPTY_CONVERSATION);
    assert!(unsettled_absent(&state, &conversation.id).await);
}

#[tokio::test]
async fn rejects_unknown_conversation_and_leaves_no_intent() {
    let state = AppState::new_test();
    let (project_id, _conversation) = seed(&state).await;

    let err = request_remote_agent_conversation_mode_switch_for_state(
        &state,
        input("does-not-exist", &project_id, "edit"),
    )
    .await
    .expect_err("unknown conversation rejected");
    assert_eq!(err, REMOTE_MODE_SWITCH_CONVERSATION_NOT_FOUND);
    assert!(
        unsettled_absent(
            &state,
            &ChatConversationId::from_string("does-not-exist".to_string())
        )
        .await,
        "a refused switch must not leave an undrainable intent behind"
    );
}

/// Scope comes off the PERSISTED conversation, never off the request: naming another project
/// must not re-target the switch.
#[tokio::test]
async fn rejects_a_project_the_conversation_does_not_belong_to() {
    let state = AppState::new_test();
    let (_project_id, conversation) = seed(&state).await;

    let err = request_remote_agent_conversation_mode_switch_for_state(
        &state,
        input(&conversation.id.as_str(), "some-other-project", "edit"),
    )
    .await
    .expect_err("project mismatch rejected");
    assert_eq!(err, REMOTE_MODE_SWITCH_PROJECT_MISMATCH);
    assert!(unsettled_absent(&state, &conversation.id).await);
}

/// The live-run rule. Mirrors `ModeSwitchRunningAgentPolicy::Reject`: the intent REFUSES rather
/// than implicitly stopping the agent, so a dropdown change can never terminate a live run.
#[tokio::test]
async fn refuses_when_a_run_is_live_and_leaves_no_intent() {
    let state = AppState::new_test();
    let (project_id, conversation) = seed(&state).await;
    state
        .agent_run_repo
        .create(AgentRun::new(conversation.id.clone()))
        .await
        .expect("live run");

    let err = request_remote_agent_conversation_mode_switch_for_state(
        &state,
        input(&conversation.id.as_str(), &project_id, "edit"),
    )
    .await
    .expect_err("live run refused");
    assert_eq!(err, REMOTE_MODE_SWITCH_RUN_ALREADY_LIVE);
    assert!(
        unsettled_absent(&state, &conversation.id).await,
        "a refused switch must not leave an intent the dispatcher would later apply"
    );
}

/// Already in the requested mode: a BENIGN terminal, persisted so the client's poll target
/// exists and settles on its first read. It must NOT be `Pending` — that would make the
/// dispatcher prepare a worktree for a mode the conversation is already in.
#[tokio::test]
async fn requesting_the_current_mode_settles_immediately_as_already_in_mode() {
    let state = AppState::new_test();
    let (project_id, conversation) = seed(&state).await;

    let response = request_remote_agent_conversation_mode_switch_for_state(
        &state,
        input(&conversation.id.as_str(), &project_id, "chat"),
    )
    .await
    .expect("already-in-mode is not an error");

    assert_eq!(
        response.status,
        RemoteConversationModeSwitchStatus::AlreadyInMode
    );
    assert!(response.status.is_terminal());

    let view = get_remote_conversation_mode_switch_request_for_state(
        &state,
        &response.mode_switch_request_id,
    )
    .await
    .expect("status resolves");
    assert_eq!(
        view.status,
        RemoteConversationModeSwitchStatus::AlreadyInMode
    );
    assert!(
        view.error_code.is_none(),
        "a benign terminal must not carry an error code or the client shows a failure"
    );
    assert!(
        state
            .remote_conversation_mode_switch_request_repo
            .claim_pending_mode_switch_request(chrono::Utc::now())
            .await
            .expect("claim")
            .is_none(),
        "an already-in-mode row must never be claimable by the dispatcher"
    );
}

/// A repeat pick of the SAME mode joins the in-flight switch rather than stacking another.
#[tokio::test]
async fn a_second_switch_to_the_same_mode_dedupes() {
    let state = AppState::new_test();
    let (project_id, conversation) = seed(&state).await;

    let first = request_remote_agent_conversation_mode_switch_for_state(
        &state,
        input(&conversation.id.as_str(), &project_id, "edit"),
    )
    .await
    .expect("first switch persisted");
    let second = request_remote_agent_conversation_mode_switch_for_state(
        &state,
        input(&conversation.id.as_str(), &project_id, "edit"),
    )
    .await
    .expect("second switch joins the first");

    assert_eq!(second.mode_switch_request_id, first.mode_switch_request_id);
    assert!(second.deduplicated);
    assert!(!first.deduplicated);
}

/// A pick of a DIFFERENT mode while one is in flight is REFUSED, not replaced: the dispatcher
/// may already have claimed the row and begun preparing a worktree for the mode it named.
#[tokio::test]
async fn a_conflicting_switch_to_a_different_mode_is_refused() {
    let state = AppState::new_test();
    let (project_id, conversation) = seed(&state).await;

    let first = request_remote_agent_conversation_mode_switch_for_state(
        &state,
        input(&conversation.id.as_str(), &project_id, "edit"),
    )
    .await
    .expect("first switch persisted");

    let err = request_remote_agent_conversation_mode_switch_for_state(
        &state,
        input(&conversation.id.as_str(), &project_id, "plan"),
    )
    .await
    .expect_err("conflicting switch refused");
    assert_eq!(err, REMOTE_MODE_SWITCH_CONFLICTING_PENDING);

    let still_pending = state
        .remote_conversation_mode_switch_request_repo
        .get_mode_switch_request(&first.mode_switch_request_id)
        .await
        .expect("read")
        .expect("exists");
    assert_eq!(
        still_pending.target_mode,
        AgentConversationWorkspaceMode::Edit,
        "the refused conflicting request must not have rewritten the in-flight target"
    );
    assert_eq!(
        still_pending.status,
        RemoteConversationModeSwitchStatus::Pending
    );
}

/// Dedupe is conversation-scoped: an unrelated conversation must still get its own switch.
#[tokio::test]
async fn dedupe_does_not_cross_conversations() {
    let state = AppState::new_test();
    let (project_id, first_conversation) = seed(&state).await;
    let second_conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(ProjectId::from_string(
            project_id.clone(),
        )))
        .await
        .expect("seed second conversation");

    let first = request_remote_agent_conversation_mode_switch_for_state(
        &state,
        input(&first_conversation.id.as_str(), &project_id, "edit"),
    )
    .await
    .expect("first switch persisted");
    let second = request_remote_agent_conversation_mode_switch_for_state(
        &state,
        input(&second_conversation.id.as_str(), &project_id, "plan"),
    )
    .await
    .expect("second conversation gets its own switch");

    assert_ne!(second.mode_switch_request_id, first.mode_switch_request_id);
    assert!(!second.deduplicated);
}

/// Once a switch settles, a NEW one can be requested — dedupe must not become a permanent lock.
#[tokio::test]
async fn a_settled_switch_does_not_block_a_later_one() {
    let state = AppState::new_test();
    let (project_id, conversation) = seed(&state).await;

    let first = request_remote_agent_conversation_mode_switch_for_state(
        &state,
        input(&conversation.id.as_str(), &project_id, "edit"),
    )
    .await
    .expect("first switch persisted");
    let now = chrono::Utc::now();
    state
        .remote_conversation_mode_switch_request_repo
        .claim_pending_mode_switch_request(now)
        .await
        .expect("claim")
        .expect("a pending row is claimable");
    state
        .remote_conversation_mode_switch_request_repo
        .complete_mode_switch_request(&first.mode_switch_request_id, now)
        .await
        .expect("complete");

    let second = request_remote_agent_conversation_mode_switch_for_state(
        &state,
        input(&conversation.id.as_str(), &project_id, "plan"),
    )
    .await
    .expect("a settled switch does not block a new one");
    assert_ne!(second.mode_switch_request_id, first.mode_switch_request_id);
    assert!(!second.deduplicated);
}

#[tokio::test]
async fn status_read_resolves_the_intent_and_fails_closed_on_unknown() {
    let state = AppState::new_test();
    let (project_id, conversation) = seed(&state).await;

    let response = request_remote_agent_conversation_mode_switch_for_state(
        &state,
        input(&conversation.id.as_str(), &project_id, "edit"),
    )
    .await
    .expect("intent persisted");

    let view = get_remote_conversation_mode_switch_request_for_state(
        &state,
        &response.mode_switch_request_id,
    )
    .await
    .expect("status resolves");
    assert_eq!(view.id, response.mode_switch_request_id);
    assert_eq!(view.status, RemoteConversationModeSwitchStatus::Pending);
    assert_eq!(view.target_mode, "edit");
    assert!(view.error_code.is_none());

    let missing = get_remote_conversation_mode_switch_request_for_state(&state, "no-such-id").await;
    assert_eq!(missing.unwrap_err(), REMOTE_MODE_SWITCH_REQUEST_NOT_FOUND);
}
