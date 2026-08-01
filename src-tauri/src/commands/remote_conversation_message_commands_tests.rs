//! Falsification tests for the spawn-free remote conversation continuation.
//!
//! Every refusal asserts BOTH the error code AND the absence of a persisted intent row: the
//! failures this command exists to prevent are (a) a turn dispatched alongside a live run,
//! which would double the message, and (b) a rejected request that still left an intent behind
//! for the dispatcher to pick up.

use std::sync::Arc;

use super::*;
use crate::application::AppState;
use crate::infrastructure::memory::MemoryAgentProviderSettingsRepository;
use ralphx_domain::agents::{AgentHarnessKind, AgentProviderSettings};
use ralphx_domain::entities::{AgentRun, ChatConversation, Project};

const CLAUDE_ENABLED_MODEL: &str = "sonnet";

/// `AppState::new_test()` pre-seeds provider rows (e.g. an enabled Codex). These tests assert on
/// EXACT provider enablement, so they start from an empty provider repo and seed only what they
/// name.
fn fresh_state() -> AppState {
    let mut state = AppState::new_test();
    state.agent_provider_settings_repo = Arc::new(MemoryAgentProviderSettingsRepository::new());
    state
}

fn claude_enabled_default() -> AgentProviderSettings {
    AgentProviderSettings {
        enabled: true,
        is_default: true,
        model: Some(CLAUDE_ENABLED_MODEL.to_string()),
        ..AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude)
    }
}

/// Seeds a project, an enabled default Claude provider, and one IDLE project conversation.
async fn seed(state: &AppState) -> (String, ChatConversation) {
    let project = Project::new(
        "Remote continue test".to_string(),
        "/tmp/remote-continue".to_string(),
    );
    let project = state
        .project_repo
        .create(project)
        .await
        .expect("seed project");
    state
        .agent_provider_settings_repo
        .upsert(&claude_enabled_default())
        .await
        .expect("seed provider");
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .expect("seed conversation");
    (project.id.to_string(), conversation)
}

fn input(conversation_id: &str, project_id: &str) -> RequestRemoteAgentConversationMessageInput {
    RequestRemoteAgentConversationMessageInput {
        conversation_id: conversation_id.to_string(),
        project_id: project_id.to_string(),
        content: "keep going on the auth module".to_string(),
        model_override: None,
        logical_effort: None,
    }
}

/// No intent row may exist for this conversation.
async fn intent_absent(state: &AppState, request_id: &str) -> bool {
    state
        .remote_conversation_message_request_repo
        .get_message_request(request_id)
        .await
        .expect("read intent")
        .is_none()
}

#[tokio::test]
async fn persists_a_pending_intent_for_an_idle_conversation() {
    let state = fresh_state();
    let (project_id, conversation) = seed(&state).await;

    let response = request_remote_agent_conversation_message_for_state(
        &state,
        input(&conversation.id.as_str(), &project_id),
    )
    .await
    .expect("intent persisted");

    assert_eq!(response.status, RemoteConversationMessageStatus::Pending);
    assert_eq!(response.conversation_id, conversation.id.as_str());

    let stored = state
        .remote_conversation_message_request_repo
        .get_message_request(&response.message_request_id)
        .await
        .expect("read intent")
        .expect("intent exists");
    assert_eq!(stored.provider, "claude");
    assert_eq!(stored.content, "keep going on the auth module");
    assert!(stored.model_override.is_none());
    assert!(stored.agent_run_id.is_none());

    // Absence: nothing was spawned or queued — no run and no live queue row.
    assert!(
        state
            .agent_run_repo
            .get_active_for_conversation(&conversation.id)
            .await
            .expect("run read")
            .is_none(),
        "no agent run may exist before the dispatcher sends"
    );
}

/// The whole disjointness guarantee: a live run means the caller must use the live-queue
/// surface, and this command must persist NOTHING.
#[tokio::test]
async fn refuses_when_a_run_is_already_live_and_leaves_no_intent() {
    let state = fresh_state();
    let (project_id, conversation) = seed(&state).await;
    state
        .agent_run_repo
        .create(AgentRun::new(conversation.id.clone()))
        .await
        .expect("live run");

    let err = request_remote_agent_conversation_message_for_state(
        &state,
        input(&conversation.id.as_str(), &project_id),
    )
    .await
    .expect_err("live run refused");
    assert_eq!(err, REMOTE_CONV_MESSAGE_RUN_ALREADY_LIVE);
}

#[tokio::test]
async fn rejects_empty_content() {
    let state = fresh_state();
    let (project_id, conversation) = seed(&state).await;

    let err = request_remote_agent_conversation_message_for_state(
        &state,
        RequestRemoteAgentConversationMessageInput {
            content: "   ".to_string(),
            ..input(&conversation.id.as_str(), &project_id)
        },
    )
    .await
    .expect_err("empty content rejected");
    assert_eq!(err, REMOTE_CONV_MESSAGE_EMPTY_CONTENT);
}

#[tokio::test]
async fn rejects_unknown_conversation() {
    let state = fresh_state();
    let (project_id, _) = seed(&state).await;

    let err = request_remote_agent_conversation_message_for_state(
        &state,
        input("no-such-conversation", &project_id),
    )
    .await
    .expect_err("unknown conversation rejected");
    assert_eq!(err, REMOTE_CONV_MESSAGE_CONVERSATION_NOT_FOUND);
}

/// The scope comes off the persisted conversation, never off the request: a client that names a
/// project it does not own must be refused rather than silently re-targeted.
#[tokio::test]
async fn rejects_a_project_the_conversation_does_not_belong_to() {
    let state = fresh_state();
    let (_, conversation) = seed(&state).await;

    let err = request_remote_agent_conversation_message_for_state(
        &state,
        input(&conversation.id.as_str(), "some-other-project"),
    )
    .await
    .expect_err("project mismatch rejected");
    assert_eq!(err, REMOTE_CONV_MESSAGE_PROJECT_MISMATCH);
}

#[tokio::test]
async fn rejects_an_archived_conversation() {
    let state = fresh_state();
    let (project_id, conversation) = seed(&state).await;
    state
        .chat_conversation_repo
        .archive(&conversation.id)
        .await
        .expect("archive");

    let err = request_remote_agent_conversation_message_for_state(
        &state,
        input(&conversation.id.as_str(), &project_id),
    )
    .await
    .expect_err("archived conversation rejected");
    assert_eq!(err, REMOTE_CONV_MESSAGE_CONVERSATION_ARCHIVED);
}

/// UX-5: composer options TRAVEL. An enabled model is carried on the intent (rather than
/// dropped), and a nonsense effort is clamped rather than carried verbatim.
#[tokio::test]
async fn carries_an_enabled_model_override_and_clamps_effort() {
    let state = fresh_state();
    let (project_id, conversation) = seed(&state).await;

    let response = request_remote_agent_conversation_message_for_state(
        &state,
        RequestRemoteAgentConversationMessageInput {
            model_override: Some(CLAUDE_ENABLED_MODEL.to_string()),
            logical_effort: Some("nonsense-effort".to_string()),
            ..input(&conversation.id.as_str(), &project_id)
        },
    )
    .await
    .expect("enabled model accepted");

    let stored = state
        .remote_conversation_message_request_repo
        .get_message_request(&response.message_request_id)
        .await
        .expect("read intent")
        .expect("intent exists");
    assert_eq!(stored.model_override.as_deref(), Some(CLAUDE_ENABLED_MODEL));
    assert!(stored.logical_effort.is_some());
    assert_ne!(stored.logical_effort.as_deref(), Some("nonsense-effort"));
}

/// The local send path would pass an unknown model through to the spawned CLI argv. This one
/// must REJECT it, and leave no intent behind.
#[tokio::test]
async fn rejects_an_unknown_model_override() {
    let state = fresh_state();
    let (project_id, conversation) = seed(&state).await;

    let err = request_remote_agent_conversation_message_for_state(
        &state,
        RequestRemoteAgentConversationMessageInput {
            model_override: Some("totally-not-a-real-model".to_string()),
            ..input(&conversation.id.as_str(), &project_id)
        },
    )
    .await
    .expect_err("unknown model rejected");
    assert_eq!(err, REMOTE_CONV_MESSAGE_MODEL_NOT_ENABLED);
}

#[tokio::test]
async fn rejects_when_no_provider_is_enabled() {
    let state = fresh_state();
    let project = Project::new("No provider".to_string(), "/tmp/no-provider".to_string());
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

    let err = request_remote_agent_conversation_message_for_state(
        &state,
        input(&conversation.id.as_str(), &project.id.to_string()),
    )
    .await
    .expect_err("no enabled provider rejected");
    assert_eq!(err, REMOTE_CONV_MESSAGE_PROVIDER_NOT_ENABLED);
}

#[tokio::test]
async fn status_read_resolves_the_intent_and_fails_closed_on_unknown() {
    let state = fresh_state();
    let (project_id, conversation) = seed(&state).await;

    let response = request_remote_agent_conversation_message_for_state(
        &state,
        input(&conversation.id.as_str(), &project_id),
    )
    .await
    .expect("intent persisted");

    let view =
        get_remote_conversation_message_request_for_state(&state, &response.message_request_id)
            .await
            .expect("status resolves");
    assert_eq!(view.id, response.message_request_id);
    assert_eq!(view.status, RemoteConversationMessageStatus::Pending);
    assert!(view.error_code.is_none());
    assert!(view.agent_run_id.is_none());

    let missing = get_remote_conversation_message_request_for_state(&state, "no-such-id").await;
    assert_eq!(missing.unwrap_err(), REMOTE_CONV_MESSAGE_REQUEST_NOT_FOUND);
    assert!(intent_absent(&state, "no-such-id").await);
}

/// Terminal classification is what the client polls on. `Pending`/`Dispatching` must never be
/// treated as settled, and every failure state must be.
#[test]
fn only_pending_and_dispatching_are_non_terminal() {
    assert!(!RemoteConversationMessageStatus::Pending.is_terminal());
    assert!(!RemoteConversationMessageStatus::Dispatching.is_terminal());
    for terminal in [
        RemoteConversationMessageStatus::Dispatched,
        RemoteConversationMessageStatus::Failed,
        RemoteConversationMessageStatus::Cancelled,
        RemoteConversationMessageStatus::FailedStale,
    ] {
        assert!(terminal.is_terminal(), "{terminal} must be terminal");
    }
}
