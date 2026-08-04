use std::sync::Arc;

use ralphx_domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatConversationId,
    IdeationAnalysisBaseRefKind, ProjectId,
};
use ralphx_domain::repositories::{
    AgentConversationWorkspaceRepository, AgentWorkspaceRepairRepository,
};

use crate::application::AppState;
use crate::commands::remote_transcript_commands::get_remote_agent_conversation_workspace_for_app_state;
use crate::infrastructure::memory::MemoryAgentConversationWorkspaceRepository;

const CONVERSATION_ID: &str = "11111111-1111-4111-8111-111111111111";
const HOST_PATH: &str = "/Users/host/secret/worktree";

async fn seeded_state() -> (AppState, Arc<MemoryAgentConversationWorkspaceRepository>) {
    let mut state = AppState::new_test();
    let repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    state.agent_conversation_workspace_repo =
        Arc::clone(&repo) as Arc<dyn AgentConversationWorkspaceRepository>;
    state.agent_workspace_repair_repo =
        Arc::clone(&repo) as Arc<dyn AgentWorkspaceRepairRepository>;
    repo.create_or_update(AgentConversationWorkspace::new(
        ChatConversationId::from_string(CONVERSATION_ID),
        ProjectId::from_string("project-1".to_string()),
        AgentConversationWorkspaceMode::Plan,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        "agent/remote-workspace".to_string(),
        HOST_PATH.to_string(),
    ))
    .await
    .unwrap();
    (state, repo)
}

#[tokio::test]
async fn remote_workspace_twin_returns_seeded_workspace_without_host_paths() {
    let (state, _) = seeded_state().await;

    let workspace = get_remote_agent_conversation_workspace_for_app_state(CONVERSATION_ID, &state)
        .await
        .unwrap()
        .expect("seeded workspace");
    let serialized = serde_json::to_string(&workspace).unwrap();

    assert_eq!(workspace.conversation_id, CONVERSATION_ID);
    assert_eq!(workspace.worktree_path, "");
    assert!(!serialized.contains(HOST_PATH));
    assert!(!serialized.contains("/Users/"));
}

#[tokio::test]
async fn remote_workspace_twin_propagates_projection_repository_errors() {
    let (state, repo) = seeded_state().await;
    repo.fail_next_current_repair_attempt_read("repair projection unavailable");

    let error = get_remote_agent_conversation_workspace_for_app_state(CONVERSATION_ID, &state)
        .await
        .expect_err("repository failure must not become absence");

    assert!(error.contains("repair projection unavailable"));
}
