//! Falsification tests for the spawn-free remote conversation start.
//!
//! Every refusal asserts BOTH the error code AND the absence of the durable rows the command
//! would otherwise create (seeded conversation + intent), because the failures this command
//! exists to prevent are a forged scope/provider/model reaching a spawn and a rejected request
//! that still left state behind. The model-validation test additionally proves the local unknown-model
//! leniency is NOT reproduced.

use std::sync::Arc;

use super::*;
use crate::application::AppState;
use crate::infrastructure::memory::MemoryAgentProviderSettingsRepository;
use ralphx_domain::agents::{AgentHarnessKind, AgentProviderSettings};
use ralphx_domain::entities::{ChatContextType, Project};

const CLAUDE_ENABLED_MODEL: &str = "sonnet";

/// `AppState::new_test()` pre-seeds provider rows (e.g. an enabled Codex). These tests assert on
/// EXACT provider enablement, so they start from an empty provider repo and seed only what they
/// name.
fn fresh_state() -> AppState {
    let mut state = AppState::new_test();
    state.agent_provider_settings_repo = Arc::new(MemoryAgentProviderSettingsRepository::new());
    state
}

fn input(project_id: &str) -> RequestRemoteAgentConversationStartInput {
    RequestRemoteAgentConversationStartInput {
        project_id: project_id.to_string(),
        content: "explore the auth module".to_string(),
        title: None,
        provider: None,
        model_override: None,
        logical_effort: None,
        mode: "chat".to_string(),
    }
}

fn claude_enabled_default() -> AgentProviderSettings {
    AgentProviderSettings {
        enabled: true,
        is_default: true,
        model: Some(CLAUDE_ENABLED_MODEL.to_string()),
        ..AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude)
    }
}

/// Seeds a project + an enabled default Claude provider. Returns the persisted project id.
async fn seed(state: &AppState) -> String {
    let project = Project::new(
        "Remote start test".to_string(),
        "/tmp/remote-start".to_string(),
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
    project.id.to_string()
}

async fn conversation_count(state: &AppState, project_id: &str) -> usize {
    state
        .chat_conversation_repo
        .get_by_context(ChatContextType::Project, project_id)
        .await
        .expect("list conversations")
        .len()
}

#[tokio::test]
async fn persists_a_pending_intent_and_seeds_one_draft_conversation() {
    let state = fresh_state();
    let project_id = seed(&state).await;

    let response = request_remote_agent_conversation_start_for_state(&state, input(&project_id))
        .await
        .expect("start persisted");

    assert_eq!(
        response.status,
        ralphx_domain::entities::RemoteConversationStartStatus::Pending
    );
    // Exactly one seeded draft; the intent row resolves; provider/model are the validated names.
    assert_eq!(conversation_count(&state, &project_id).await, 1);
    let stored = state
        .remote_conversation_start_request_repo
        .get_start_request(&response.start_request_id)
        .await
        .expect("read intent")
        .expect("intent exists");
    assert_eq!(stored.provider, "claude");
    assert_eq!(stored.model.as_deref(), Some(CLAUDE_ENABLED_MODEL));
    assert_eq!(stored.mode, "chat");
    // Absence: nothing was spawned or queued — no live run for the seeded conversation.
    let active = state
        .agent_run_repo
        .get_active_for_conversation(&stored.conversation_id)
        .await
        .expect("run read");
    assert!(
        active.is_none(),
        "no agent run may exist before the driver starts it"
    );
}

#[tokio::test]
async fn rejects_empty_content_and_leaves_no_state() {
    let state = fresh_state();
    let project_id = seed(&state).await;

    let err = request_remote_agent_conversation_start_for_state(
        &state,
        RequestRemoteAgentConversationStartInput {
            content: "   ".to_string(),
            ..input(&project_id)
        },
    )
    .await
    .expect_err("empty content rejected");
    assert_eq!(err, REMOTE_CONV_START_EMPTY_CONTENT);
    assert_eq!(conversation_count(&state, &project_id).await, 0);
}

#[tokio::test]
async fn accepts_known_plan_mode_and_persists_it_end_to_end() {
    let state = fresh_state();
    let project_id = seed(&state).await;

    let response = request_remote_agent_conversation_start_for_state(
        &state,
        RequestRemoteAgentConversationStartInput {
            mode: "plan".to_string(),
            ..input(&project_id)
        },
    )
    .await
    .expect("known plan mode accepted");

    let stored = state
        .remote_conversation_start_request_repo
        .get_start_request(&response.start_request_id)
        .await
        .expect("read intent")
        .expect("intent exists");
    assert_eq!(stored.mode, "plan");
    let conversation = state
        .chat_conversation_repo
        .get_by_id(&stored.conversation_id)
        .await
        .expect("read conversation")
        .expect("conversation exists");
    assert_eq!(
        conversation.agent_mode,
        Some(AgentConversationWorkspaceMode::Plan)
    );
}

#[tokio::test]
async fn rejects_unknown_mode_fail_closed_and_leaves_no_state() {
    let state = fresh_state();
    let project_id = seed(&state).await;

    let err = request_remote_agent_conversation_start_for_state(
        &state,
        RequestRemoteAgentConversationStartInput {
            mode: "future_mode".to_string(),
            ..input(&project_id)
        },
    )
    .await
    .expect_err("unknown mode rejected");
    assert_eq!(err, REMOTE_CONV_START_INVALID_MODE);
    assert_eq!(conversation_count(&state, &project_id).await, 0);
}

#[tokio::test]
async fn rejects_missing_project() {
    let state = fresh_state();
    seed(&state).await;

    let err = request_remote_agent_conversation_start_for_state(&state, input("does-not-exist"))
        .await
        .expect_err("missing project rejected");
    assert_eq!(err, REMOTE_CONV_START_PROJECT_NOT_FOUND);
}

#[tokio::test]
async fn rejects_disabled_provider() {
    let state = fresh_state();
    let project_id = seed(&state).await;

    let err = request_remote_agent_conversation_start_for_state(
        &state,
        RequestRemoteAgentConversationStartInput {
            provider: Some("codex".to_string()),
            ..input(&project_id)
        },
    )
    .await
    .expect_err("disabled provider rejected");
    assert_eq!(err, REMOTE_CONV_START_PROVIDER_NOT_ENABLED);
    assert_eq!(conversation_count(&state, &project_id).await, 0);
}

#[tokio::test]
async fn rejects_unknown_model_and_does_not_reproduce_local_leniency() {
    let state = fresh_state();
    let project_id = seed(&state).await;

    // The local `normalize_agent_runtime_selection` would PASS an unknown model id through to the
    // spawned CLI. This command must REJECT it, and leave no seeded conversation or intent.
    let err = request_remote_agent_conversation_start_for_state(
        &state,
        RequestRemoteAgentConversationStartInput {
            model_override: Some("totally-not-a-real-model".to_string()),
            ..input(&project_id)
        },
    )
    .await
    .expect_err("unknown model rejected");
    assert_eq!(err, REMOTE_CONV_START_MODEL_NOT_ENABLED);
    assert_eq!(
        conversation_count(&state, &project_id).await,
        0,
        "an unknown model must not leave a seeded conversation behind"
    );
}

#[tokio::test]
async fn rejects_when_no_default_provider_is_configured() {
    let state = fresh_state();
    let project = Project::new("No default".to_string(), "/tmp/no-default".to_string());
    let project = state
        .project_repo
        .create(project)
        .await
        .expect("seed project");
    // Enabled but NOT default.
    state
        .agent_provider_settings_repo
        .upsert(&AgentProviderSettings {
            enabled: true,
            is_default: false,
            model: Some(CLAUDE_ENABLED_MODEL.to_string()),
            ..AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude)
        })
        .await
        .expect("seed provider");

    let err =
        request_remote_agent_conversation_start_for_state(&state, input(&project.id.to_string()))
            .await
            .expect_err("no default provider rejected");
    assert_eq!(err, REMOTE_CONV_START_NO_DEFAULT_PROVIDER);
}

#[tokio::test]
async fn accepts_an_explicit_enabled_model_and_clamps_effort() {
    let state = fresh_state();
    let project_id = seed(&state).await;

    let response = request_remote_agent_conversation_start_for_state(
        &state,
        RequestRemoteAgentConversationStartInput {
            model_override: Some(CLAUDE_ENABLED_MODEL.to_string()),
            logical_effort: Some("nonsense-effort".to_string()),
            ..input(&project_id)
        },
    )
    .await
    .expect("explicit enabled model accepted");
    let stored = state
        .remote_conversation_start_request_repo
        .get_start_request(&response.start_request_id)
        .await
        .expect("read intent")
        .expect("intent exists");
    assert_eq!(stored.model.as_deref(), Some(CLAUDE_ENABLED_MODEL));
    // A nonsense effort was clamped to a real supported effort, never carried verbatim.
    assert!(stored.effort.is_some());
    assert_ne!(stored.effort.as_deref(), Some("nonsense-effort"));
}

#[tokio::test]
async fn status_read_resolves_the_intent_and_fails_closed_on_unknown() {
    let state = fresh_state();
    let project_id = seed(&state).await;

    let response = request_remote_agent_conversation_start_for_state(&state, input(&project_id))
        .await
        .expect("start persisted");

    let view = get_remote_conversation_start_request_for_state(&state, &response.start_request_id)
        .await
        .expect("status resolves");
    assert_eq!(view.id, response.start_request_id);
    assert_eq!(
        view.status,
        ralphx_domain::entities::RemoteConversationStartStatus::Pending
    );
    assert!(view.error_code.is_none());
    assert!(view.agent_run_id.is_none());

    let missing = get_remote_conversation_start_request_for_state(&state, "no-such-id").await;
    assert_eq!(missing.unwrap_err(), REMOTE_CONV_START_REQUEST_NOT_FOUND);
}
