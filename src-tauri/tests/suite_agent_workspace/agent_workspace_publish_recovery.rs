use std::sync::Arc;

use ralphx_lib::application::agent_workspace_publish_recovery::{
    recover_stale_agent_workspace_publish_repairs, recover_stale_publish_repair_for_workspace,
};
use ralphx_lib::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentRun, ChatConversationId,
    IdeationAnalysisBaseRefKind, ProjectId,
};
use ralphx_lib::domain::repositories::{AgentConversationWorkspaceRepository, AgentRunRepository};
use ralphx_lib::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryAgentRunRepository,
};

fn needs_agent_workspace(conversation_id: ChatConversationId) -> AgentConversationWorkspace {
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        ProjectId::from_string("project-1".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-1".to_string()),
        "ralphx/test/agent-workspace".to_string(),
        "/tmp/ralphx-agent-workspace".to_string(),
    );
    workspace.publication_pr_number = Some(42);
    workspace.publication_pr_status = Some("failed".to_string());
    workspace.publication_push_status = Some("needs_agent".to_string());
    workspace
}

#[tokio::test]
async fn recovers_needs_agent_workspace_when_no_agent_run_is_active() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let conversation_id = ChatConversationId::from_string("11111111-1111-1111-1111-111111111111");
    let workspace = needs_agent_workspace(conversation_id);
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");

    let run = agent_run_repo
        .create(AgentRun::new(conversation_id))
        .await
        .expect("seed run");
    agent_run_repo
        .fail(&run.id, "repair agent exited")
        .await
        .expect("mark run failed");

    let recovered = recover_stale_agent_workspace_publish_repairs(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
    )
    .await
    .expect("recover stale repair");

    assert_eq!(recovered, 1);
    let refreshed = workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    assert_eq!(refreshed.publication_push_status.as_deref(), Some("failed"));

    let events = workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await
        .expect("list events");
    assert!(events.iter().any(|event| {
        event.step == "stale_repair_recovered"
            && event.status == "succeeded"
            && event.classification.as_deref() == Some("stale_needs_agent")
    }));
}

#[tokio::test]
async fn keeps_needs_agent_workspace_locked_while_agent_run_is_active() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let conversation_id = ChatConversationId::from_string("22222222-2222-2222-2222-222222222222");
    let workspace = needs_agent_workspace(conversation_id);
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    agent_run_repo
        .create(AgentRun::new(conversation_id))
        .await
        .expect("seed active run");

    let recovered = recover_stale_publish_repair_for_workspace(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
        workspace,
    )
    .await
    .expect("check repair state");

    assert!(!recovered);
    let refreshed = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    assert_eq!(
        refreshed.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    assert!(
        workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("list events")
            .is_empty(),
        "active repairs must not be downgraded"
    );
}
