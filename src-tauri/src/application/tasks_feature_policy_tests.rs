use crate::application::tasks_feature_policy::{TasksFeaturePolicy, TASKS_DISABLED_ERROR_CODE};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus,
    ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSession, InternalStatus, Project,
    ProjectId, Task,
};
use crate::domain::ideation::IdeationSettings;

async fn attached_pipeline(state: &AppState) -> IdeationSession {
    let project_id = ProjectId::from_string("project-1".to_string());
    let session = IdeationSession::new(project_id.clone());
    state
        .ideation_session_repo
        .create(session.clone())
        .await
        .unwrap();

    let mut workspace = AgentConversationWorkspace::new(
        ChatConversationId::from_string("conversation-1"),
        project_id,
        AgentConversationWorkspaceMode::Tasks,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        None,
        "ralphx/conversation-1".to_string(),
        "/tmp/ralphx-policy-test".to_string(),
    );
    workspace.task_pipeline_session_id = Some(session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();
    session
}

async fn disable_tasks(state: &AppState) {
    state
        .ideation_settings_repo
        .update_settings(&IdeationSettings::default())
        .await
        .unwrap();
}

#[tokio::test]
async fn tasks_policy_defaults_to_denied_for_standalone_work() {
    let state = AppState::new_test();
    disable_tasks(&state).await;

    let error = TasksFeaturePolicy::from_state(&state)
        .authorize_session(None)
        .await
        .expect_err("standalone Tasks must be disabled by default");

    assert!(error.to_string().starts_with(TASKS_DISABLED_ERROR_CODE));
}

#[tokio::test]
async fn tasks_policy_allows_all_work_when_globally_enabled() {
    let state = AppState::new_test();
    state
        .ideation_settings_repo
        .update_settings(&IdeationSettings {
            tasks_enabled: true,
            ..Default::default()
        })
        .await
        .unwrap();

    TasksFeaturePolicy::from_state(&state)
        .authorize_session(None)
        .await
        .expect("enabled Tasks must allow standalone work");
}

#[tokio::test]
async fn tasks_policy_allows_only_active_attached_pipeline_while_off() {
    let state = AppState::new_test();
    let session = attached_pipeline(&state).await;
    disable_tasks(&state).await;
    let policy = TasksFeaturePolicy::from_state(&state);

    policy
        .authorize_session(Some(&session.id))
        .await
        .expect("active attached pipeline must be grandfathered");

    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_task_pipeline_session_id(&session.id)
        .await
        .unwrap()
        .unwrap();
    workspace.status = AgentConversationWorkspaceStatus::Archived;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let error = policy
        .authorize_session(Some(&session.id))
        .await
        .expect_err("archived attachment must not remain entitled");
    assert!(error.to_string().starts_with(TASKS_DISABLED_ERROR_CODE));
}

#[tokio::test]
async fn disabled_policy_rejects_progress_transition_without_changing_task_state() {
    let state = AppState::new_test();
    let project = Project::new("Tasks policy test".to_string(), "/tmp".to_string());
    state.project_repo.create(project.clone()).await.unwrap();
    let task = Task::new(project.id, "Standalone task".to_string());
    let task_id = task.id.clone();
    state.task_repo.create(task).await.unwrap();
    disable_tasks(&state).await;

    let service = state.build_transition_service_for_test_runtime();
    let error = service
        .transition_task(&task_id, InternalStatus::Ready)
        .await
        .expect_err("stale progress must be rejected after Tasks is disabled");
    assert!(error.to_string().starts_with(TASKS_DISABLED_ERROR_CODE));

    let unchanged = state.task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(unchanged.internal_status, InternalStatus::Backlog);
}
