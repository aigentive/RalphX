use std::sync::Arc;

use super::session_namer_agent::{build_session_namer_agent_spawn, SessionNamerTarget};
use super::AppState;
use crate::domain::agents::{
    AgentHarnessKind, AgentLane, AgentLaneSettings, AgenticClient, LogicalEffort,
};
use crate::domain::entities::{ChatConversation, IdeationSession, Project};
use crate::infrastructure::agents::claude::agent_names;
use crate::infrastructure::MockAgenticClient;

#[tokio::test]
async fn session_namer_conversation_spawn_uses_active_project_cwd_and_conversation_harness() {
    let default_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let codex_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let state = AppState::new_test()
        .with_agent_client(default_client)
        .with_harness_agent_client(AgentHarnessKind::Codex, codex_client.clone());

    let project_dir = tempfile::tempdir().unwrap();
    let project = Project::new(
        "Codex Conversation Project".to_string(),
        project_dir.path().display().to_string(),
    );
    state.project_repo.create(project.clone()).await.unwrap();

    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.provider_harness = Some(AgentHarnessKind::Codex);
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();

    let spawn = build_session_namer_agent_spawn(
        &state,
        SessionNamerTarget::ConversationInitial {
            conversation_id: conversation.id.as_str(),
            user_message: "Name this Codex conversation".to_string(),
        },
    )
    .await
    .unwrap();

    assert!(Arc::ptr_eq(&spawn.client, &codex_client));
    assert_eq!(spawn.config.harness, Some(AgentHarnessKind::Codex));
    assert_eq!(spawn.config.model, None);
    assert_eq!(spawn.config.working_directory, project_dir.path());
    assert_eq!(
        spawn.config.agent.as_deref(),
        Some(agent_names::AGENT_SESSION_NAMER)
    );
    assert!(spawn.config.prompt.contains("Name this Codex conversation"));
}

#[tokio::test]
async fn session_namer_session_spawn_uses_active_project_cwd_and_project_ideation_harness() {
    let default_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let codex_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let state = AppState::new_test()
        .with_agent_client(default_client)
        .with_harness_agent_client(AgentHarnessKind::Codex, codex_client.clone());

    let project_dir = tempfile::tempdir().unwrap();
    let project = Project::new(
        "Codex Session Project".to_string(),
        project_dir.path().display().to_string(),
    );
    state.project_repo.create(project.clone()).await.unwrap();
    let mut codex_lane = AgentLaneSettings::new(AgentHarnessKind::Codex);
    codex_lane.model = Some("gpt-5.4".to_string());
    codex_lane.effort = Some(LogicalEffort::XHigh);
    state
        .agent_lane_settings_repo
        .upsert_for_project(project.id.as_str(), AgentLane::IdeationPrimary, &codex_lane)
        .await
        .unwrap();

    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();

    let spawn = build_session_namer_agent_spawn(
        &state,
        SessionNamerTarget::SessionInitial {
            session_id: session.id.as_str().to_string(),
            user_message: "Build the settings analyzer".to_string(),
        },
    )
    .await
    .unwrap();

    assert!(Arc::ptr_eq(&spawn.client, &codex_client));
    assert_eq!(spawn.config.harness, Some(AgentHarnessKind::Codex));
    assert_eq!(spawn.config.model.as_deref(), Some("gpt-5.4"));
    assert_eq!(spawn.config.logical_effort, Some(LogicalEffort::XHigh));
    assert_eq!(spawn.config.working_directory, project_dir.path());
    assert_eq!(
        spawn.config.agent.as_deref(),
        Some(agent_names::AGENT_SESSION_NAMER)
    );
    assert!(spawn.config.prompt.contains("Build the settings analyzer"));
}
