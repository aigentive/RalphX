use super::agent_conversation_mode_switch::system_switch_automation_run_to_edit;
use super::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AutomationRunId, ChatConversation,
    IdeationAnalysisBaseRefKind, ProjectId,
};

#[tokio::test]
async fn system_mode_switch_updates_plan_workspace_and_is_idempotent() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.automation_run_id = Some(AutomationRunId::new());
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Plan));
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("create conversation");
    let workspace = AgentConversationWorkspace::new(
        conversation.id.clone(),
        project_id,
        AgentConversationWorkspaceMode::Plan,
        IdeationAnalysisBaseRefKind::LocalBranch,
        "main".to_string(),
        Some("main".to_string()),
        Some("abc123".to_string()),
        "run-branch".to_string(),
        "/tmp/run-branch".to_string(),
    );
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("create workspace");

    system_switch_automation_run_to_edit(&conversation.id, &state)
        .await
        .expect("switch to edit");
    system_switch_automation_run_to_edit(&conversation.id, &state)
        .await
        .expect("idempotent switch");

    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .expect("load conversation")
        .expect("conversation");
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .expect("load workspace")
        .expect("workspace");
    assert_eq!(
        conversation.agent_mode,
        Some(AgentConversationWorkspaceMode::Edit)
    );
    assert_eq!(workspace.mode, AgentConversationWorkspaceMode::Edit);
}
