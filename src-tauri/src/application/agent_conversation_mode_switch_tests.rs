use super::agent_conversation_mode_switch::{
    automation_run_mode_locked_error_message, is_automation_run_mode_switch_locked,
    system_switch_automation_run_to_edit, system_switch_automation_run_to_ideation,
    AUTOMATION_RUN_MODE_LOCKED_ERROR_CODE,
};
use super::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AutomationRunId, ChatContextType,
    ChatConversation, ChatConversationId, IdeationAnalysisBaseRefKind, ProjectId,
};
use crate::error::AppError;

fn workspace_for(
    conversation: &ChatConversation,
    project_id: ProjectId,
    mode: AgentConversationWorkspaceMode,
) -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        conversation.id.clone(),
        project_id,
        mode,
        IdeationAnalysisBaseRefKind::LocalBranch,
        "main".to_string(),
        Some("main".to_string()),
        Some("abc123".to_string()),
        "run-branch".to_string(),
        "/tmp/run-branch".to_string(),
    )
}

#[test]
fn automation_run_mode_lock_helpers_are_explicit() {
    let project_id = ProjectId::new();
    let mut normal = ChatConversation::new_project(project_id.clone());
    assert!(!is_automation_run_mode_switch_locked(&normal));

    normal.automation_run_id = Some(AutomationRunId::new());
    assert!(is_automation_run_mode_switch_locked(&normal));
    assert!(
        automation_run_mode_locked_error_message().contains(AUTOMATION_RUN_MODE_LOCKED_ERROR_CODE)
    );
}

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
    let workspace = workspace_for(
        &conversation,
        project_id,
        AgentConversationWorkspaceMode::Plan,
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

#[tokio::test]
async fn system_mode_switch_can_deliver_an_automation_plan_to_ideation() {
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
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace_for(
            &conversation,
            project_id,
            AgentConversationWorkspaceMode::Plan,
        ))
        .await
        .expect("create workspace");

    system_switch_automation_run_to_ideation(&conversation.id, &state)
        .await
        .expect("switch to ideation");

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
        Some(AgentConversationWorkspaceMode::Ideation)
    );
    assert_eq!(workspace.mode, AgentConversationWorkspaceMode::Ideation);
}

#[tokio::test]
async fn system_mode_switch_rejects_missing_or_non_project_conversations() {
    let state = AppState::new_test();
    let missing = system_switch_automation_run_to_edit(&ChatConversationId::new(), &state)
        .await
        .expect_err("missing conversation should fail");
    assert!(matches!(missing, AppError::NotFound(_)));

    let mut task_conversation = ChatConversation::new_task(crate::domain::entities::TaskId::new());
    task_conversation.automation_run_id = Some(AutomationRunId::new());
    task_conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Plan));
    let task_conversation = state
        .chat_conversation_repo
        .create(task_conversation)
        .await
        .expect("create task conversation");

    let err = system_switch_automation_run_to_edit(&task_conversation.id, &state)
        .await
        .expect_err("non-project conversation should fail");
    assert!(matches!(err, AppError::Validation(message) if message.contains("Only project")));
    assert_eq!(task_conversation.context_type, ChatContextType::Task);
}

#[tokio::test]
async fn system_mode_switch_requires_workspace_and_rejects_non_plan_edit_modes() {
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

    let missing_workspace = system_switch_automation_run_to_edit(&conversation.id, &state)
        .await
        .expect_err("missing workspace should fail");
    assert!(
        matches!(missing_workspace, AppError::NotFound(message) if message.contains("agent workspace not found"))
    );

    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace_for(
            &conversation,
            project_id,
            AgentConversationWorkspaceMode::ReviewPr,
        ))
        .await
        .expect("create workspace");

    let invalid_mode = system_switch_automation_run_to_edit(&conversation.id, &state)
        .await
        .expect_err("review workspace cannot switch to edit");
    assert!(
        matches!(invalid_mode, AppError::Validation(message) if message.contains("cannot switch from review_pr"))
    );
}

#[tokio::test]
async fn system_mode_switch_repairs_only_the_stale_side() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.automation_run_id = Some(AutomationRunId::new());
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Edit));
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("create conversation");
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace_for(
            &conversation,
            project_id,
            AgentConversationWorkspaceMode::Plan,
        ))
        .await
        .expect("create workspace");

    system_switch_automation_run_to_edit(&conversation.id, &state)
        .await
        .expect("workspace-only repair should succeed");

    let conversation_after = state
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .expect("load conversation")
        .expect("conversation");
    let workspace_after = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .expect("load workspace")
        .expect("workspace");
    assert_eq!(
        conversation_after.agent_mode,
        Some(AgentConversationWorkspaceMode::Edit)
    );
    assert_eq!(workspace_after.mode, AgentConversationWorkspaceMode::Edit);
}
