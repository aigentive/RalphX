use super::query::{
    get_session_task_history_availability, get_session_task_history_availability_for_app_state,
    get_task_agent_workspace_for_app_state, get_task_dependency_graph,
    get_task_dependency_graph_for_app_state,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatConversation,
    IdeationAnalysisBaseRefKind, IdeationSessionId, InternalStatus, Project, Task,
};
use tauri::Manager;

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

#[tokio::test]
async fn session_history_availability_includes_archived_tasks() {
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Archived history project".into(),
            "/tmp/archived-history-project".into(),
        ))
        .await
        .expect("project should persist");
    let session_id = IdeationSessionId::from_string("session-archived-history");
    let mut task = Task::new(project.id.clone(), "Preserved history".into());
    task.ideation_session_id = Some(session_id.clone());
    let task = state
        .task_repo
        .create(task)
        .await
        .expect("task should persist");
    state
        .task_repo
        .archive(&task.id)
        .await
        .expect("task should archive");

    let availability = get_session_task_history_availability_for_app_state(
        project.id.as_str().to_string(),
        session_id.as_str().to_string(),
        &state,
    )
    .await
    .expect("history query should succeed");

    assert!(availability.has_history);
    assert_eq!(availability.task_count, 1);
}

#[tokio::test]
async fn dependency_graph_keeps_session_scoped_branch_update_blocked_tasks() {
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Branch update graph project".into(),
            "/tmp/branch-update-graph-project".into(),
        ))
        .await
        .expect("project should persist");
    let session_id = IdeationSessionId::from_string("session-branch-update-graph");
    let mut task = Task::new(project.id.clone(), "Resolve branch update".into());
    task.ideation_session_id = Some(session_id.clone());
    task.internal_status = InternalStatus::BranchUpdateBlocked;
    let task = state
        .task_repo
        .create(task)
        .await
        .expect("task should persist");

    let graph = get_task_dependency_graph_for_app_state(
        project.id.as_str().to_string(),
        Some(false),
        Some(session_id.as_str().to_string()),
        None,
        &state,
    )
    .await
    .expect("graph query should succeed");

    assert_eq!(graph.nodes.len(), 1);
    assert_eq!(graph.nodes[0].task_id, task.id.as_str());
    assert_eq!(
        graph.nodes[0].internal_status,
        InternalStatus::BranchUpdateBlocked.as_str()
    );
}

#[tokio::test]
async fn task_query_commands_preserve_archived_history_and_blocked_graph_nodes() {
    let app = tauri::test::mock_builder()
        .manage(AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("task query command app should build");
    let state = app.state::<AppState>();
    let project = state
        .project_repo
        .create(Project::new(
            "Task query command project".into(),
            "/tmp/task-query-command-project".into(),
        ))
        .await
        .expect("project should persist");
    let session_id = IdeationSessionId::from_string("session-task-query-command");
    let mut task = Task::new(project.id.clone(), "Blocked history".into());
    task.ideation_session_id = Some(session_id.clone());
    task.internal_status = InternalStatus::BranchUpdateBlocked;
    let task = state
        .task_repo
        .create(task)
        .await
        .expect("task should persist");
    state
        .task_repo
        .archive(&task.id)
        .await
        .expect("task should archive");

    let availability = get_session_task_history_availability(
        project.id.as_str().to_string(),
        session_id.as_str().to_string(),
        app.state::<AppState>(),
    )
    .await
    .expect("history command should include archived rows");
    assert!(availability.has_history);
    assert_eq!(availability.task_count, 1);

    let graph = get_task_dependency_graph(
        project.id.as_str().to_string(),
        Some(true),
        Some(session_id.as_str().to_string()),
        None,
        app.state::<AppState>(),
    )
    .await
    .expect("graph command should include archived session rows");
    assert_eq!(graph.nodes.len(), 1);
    assert_eq!(graph.nodes[0].task_id, task.id.as_str());
    assert_eq!(
        graph.nodes[0].internal_status,
        InternalStatus::BranchUpdateBlocked.as_str()
    );
}
