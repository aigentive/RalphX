use super::query::get_task_agent_workspace_for_app_state;
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatConversation,
    IdeationAnalysisBaseRefKind, IdeationSessionId, Project, Task,
};

#[tokio::test]
async fn resolves_a_linked_task_to_its_active_agent_conversation() {
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Task navigation project".into(),
            "/tmp/task-navigation-project".into(),
        ))
        .await
        .expect("project should persist");
    let session_id = IdeationSessionId::from_string("session-task-navigation");
    let mut task = Task::new(project.id.clone(), "Open in Agents".into());
    task.ideation_session_id = Some(session_id.clone());
    let task = state
        .task_repo
        .create(task)
        .await
        .expect("task should persist");

    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.title = Some("Task owner".into());
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation should persist");

    let mut workspace = AgentConversationWorkspace::new(
        conversation.id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".into(),
        Some("main".into()),
        Some("base-sha".into()),
        "ralphx/task-navigation".into(),
        "/tmp/task-navigation".into(),
    );
    workspace.linked_ideation_session_id = Some(session_id);
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let target = get_task_agent_workspace_for_app_state(task.id.as_str().into(), &state)
        .await
        .expect("task target should resolve")
        .expect("linked task should have an Agent target");

    assert_eq!(target.conversation_id, conversation.id.as_str());
    assert_eq!(target.project_id, project.id.as_str());
    assert_eq!(target.title, "Task owner");
}

#[tokio::test]
async fn returns_none_when_a_task_has_no_agent_workspace() {
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Unlinked task project".into(),
            "/tmp/unlinked-task-project".into(),
        ))
        .await
        .expect("project should persist");
    let task = state
        .task_repo
        .create(Task::new(project.id, "Unlinked task".into()))
        .await
        .expect("task should persist");

    let target = get_task_agent_workspace_for_app_state(task.id.as_str().into(), &state)
        .await
        .expect("task target lookup should succeed");

    assert!(target.is_none());
}
