use ralphx_lib::application::chat_service::{AppChatService, ChatService, SendMessageOptions};
use ralphx_lib::application::AppState;
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::ideation::IdeationSessionBuilder;
use ralphx_lib::domain::entities::{ChatContextType, InternalStatus, Project, ProjectId, Task};
use ralphx_lib::domain::execution::ExecutionSettings;
use ralphx_lib::domain::services::RunningAgentKey;
use ralphx_lib::http_server::types::HttpServerState;
use std::sync::Arc;

async fn setup_test_state() -> HttpServerState {
    HttpServerState {
        app_state: Arc::new(AppState::new_test()),
        execution_state: Arc::new(ExecutionState::new()),
        delegation_service: Default::default(),
        external_mcp_supervisor: None,
    }
}

fn build_chat_service(state: &HttpServerState) -> AppChatService {
    let app = &state.app_state;
    app.build_chat_service_with_execution_state(Arc::clone(&state.execution_state))
}

async fn create_task(state: &HttpServerState, status: InternalStatus) -> String {
    let project = Project::new(
        "Pause Flow Project".to_string(),
        "/tmp/pause-flow".to_string(),
    );
    let project_id = project.id.clone();
    state.app_state.project_repo.create(project).await.unwrap();

    let mut task = Task::new(project_id, "Pause Flow Task".to_string());
    task.internal_status = status;
    let task = state.app_state.task_repo.create(task).await.unwrap();
    task.id.as_str().to_string()
}

async fn create_task_in_project(
    state: &HttpServerState,
    project_id: ProjectId,
    status: InternalStatus,
) -> String {
    let mut task = Task::new(project_id, "Pause Flow Task".to_string());
    task.internal_status = status;
    let task = state.app_state.task_repo.create(task).await.unwrap();
    task.id.as_str().to_string()
}

async fn create_ideation_session_in_project(
    state: &HttpServerState,
    project_id: ProjectId,
) -> String {
    let session = IdeationSessionBuilder::new().project_id(project_id).build();
    let session_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();
    session_id
}

async fn create_project(state: &HttpServerState, name: &str, path: &str) -> Project {
    let project = Project::new(name.to_string(), path.to_string());
    state
        .app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    project
}

#[tokio::test]
async fn test_paused_task_execution_send_is_queued_not_spawned() {
    let state = setup_test_state().await;
    let task_id = create_task(&state, InternalStatus::Executing).await;
    state.execution_state.pause();

    let result = build_chat_service(&state)
        .send_message(
            ChatContextType::TaskExecution,
            &task_id,
            "Queue worker follow-up during pause",
            SendMessageOptions::default(),
        )
        .await
        .expect("paused task execution send should queue");

    assert!(result.was_queued);
    assert_eq!(
        state
            .app_state
            .message_queue
            .get_queued(ChatContextType::TaskExecution, &task_id)
            .len(),
        1
    );
    let key = RunningAgentKey::new("task_execution", &task_id);
    assert!(
        !state
            .app_state
            .running_agent_registry
            .is_running(&key)
            .await
    );
}

#[tokio::test]
async fn test_paused_review_send_is_queued_not_spawned() {
    let state = setup_test_state().await;
    let task_id = create_task(&state, InternalStatus::Reviewing).await;
    state.execution_state.pause();

    let result = build_chat_service(&state)
        .send_message(
            ChatContextType::Review,
            &task_id,
            "Queue review follow-up during pause",
            SendMessageOptions::default(),
        )
        .await
        .expect("paused review send should queue");

    assert!(result.was_queued);
    assert_eq!(
        state
            .app_state
            .message_queue
            .get_queued(ChatContextType::Review, &task_id)
            .len(),
        1
    );
    let key = RunningAgentKey::new("review", &task_id);
    assert!(
        !state
            .app_state
            .running_agent_registry
            .is_running(&key)
            .await
    );
}

#[tokio::test]
async fn test_paused_merge_send_is_queued_not_spawned() {
    let state = setup_test_state().await;
    let task_id = create_task(&state, InternalStatus::Merging).await;
    state.execution_state.pause();

    let result = build_chat_service(&state)
        .send_message(
            ChatContextType::Merge,
            &task_id,
            "Queue merge follow-up during pause",
            SendMessageOptions::default(),
        )
        .await
        .expect("paused merge send should queue");

    assert!(result.was_queued);
    assert_eq!(
        state
            .app_state
            .message_queue
            .get_queued(ChatContextType::Merge, &task_id)
            .len(),
        1
    );
    let key = RunningAgentKey::new("merge", &task_id);
    assert!(
        !state
            .app_state
            .running_agent_registry
            .is_running(&key)
            .await
    );
}

#[tokio::test]
async fn test_paused_task_chat_send_is_queued_not_spawned() {
    let state = setup_test_state().await;
    let task_id = create_task(&state, InternalStatus::Ready).await;
    state.execution_state.pause();

    let result = build_chat_service(&state)
        .send_message(
            ChatContextType::Task,
            &task_id,
            "Queue task chat during pause",
            SendMessageOptions::default(),
        )
        .await
        .expect("paused task chat send should queue");

    assert!(result.was_queued);
    assert_eq!(
        state
            .app_state
            .message_queue
            .get_queued(ChatContextType::Task, &task_id)
            .len(),
        1
    );
    let key = RunningAgentKey::new("task", &task_id);
    assert!(
        !state
            .app_state
            .running_agent_registry
            .is_running(&key)
            .await
    );
}

#[tokio::test]
async fn test_paused_project_chat_send_is_queued_not_spawned() {
    let state = setup_test_state().await;
    let project = create_project(&state, "Paused Project Chat", "/tmp/paused-project-chat").await;
    state.execution_state.pause();

    let result = build_chat_service(&state)
        .send_message(
            ChatContextType::Project,
            project.id.as_str(),
            "Queue project chat during pause",
            SendMessageOptions::default(),
        )
        .await
        .expect("paused project chat send should queue");

    assert!(result.was_queued);
    assert_eq!(
        state
            .app_state
            .message_queue
            .get_queued(ChatContextType::Project, project.id.as_str())
            .len(),
        1
    );
    let key = RunningAgentKey::new("project", project.id.as_str());
    assert!(
        !state
            .app_state
            .running_agent_registry
            .is_running(&key)
            .await
    );
}

#[tokio::test]
async fn test_task_execution_send_blocks_when_project_total_cap_is_reached() {
    let state = setup_test_state().await;

    let project = Project::new(
        "Project Capacity".to_string(),
        "/tmp/project-capacity".to_string(),
    );
    state
        .app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    state
        .app_state
        .execution_settings_repo
        .update_settings(
            Some(&project.id),
            &ExecutionSettings {
                max_concurrent_tasks: 1,
                project_ideation_max: 1,
                auto_commit: true,
                pause_on_failure: true,
                ..ExecutionSettings::default()
            },
        )
        .await
        .unwrap();

    let occupied_task_id =
        create_task_in_project(&state, project.id.clone(), InternalStatus::Executing).await;
    let blocked_task_id =
        create_task_in_project(&state, project.id.clone(), InternalStatus::Executing).await;

    state
        .app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("task_execution", &occupied_task_id),
            77777,
            "occupied-conv".to_string(),
            "occupied-run".to_string(),
            None,
            None,
        )
        .await;

    state.execution_state.set_global_max_concurrent(5);

    let result = build_chat_service(&state)
        .send_message(
            ChatContextType::TaskExecution,
            &blocked_task_id,
            "Attempt worker spawn past project cap",
            SendMessageOptions::default(),
        )
        .await;

    let err = result.expect_err("project total cap must block task execution spawn");
    assert!(
        matches!(err, ralphx_lib::application::chat_service::ChatServiceError::SpawnFailed(ref msg) if msg.contains("project execution capacity reached")),
        "unexpected error: {err}"
    );

    let blocked_key = RunningAgentKey::new("task_execution", &blocked_task_id);
    assert!(
        !state
            .app_state
            .running_agent_registry
            .is_running(&blocked_key)
            .await,
        "failed admission must not leave a registered running-agent slot behind"
    );
}

#[tokio::test]
async fn test_review_send_blocks_when_same_project_ideation_consumes_only_slot() {
    let state = setup_test_state().await;

    let project = Project::new(
        "Review Mixed Load".to_string(),
        "/tmp/review-mixed-load".to_string(),
    );
    state
        .app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    state
        .app_state
        .execution_settings_repo
        .update_settings(
            Some(&project.id),
            &ExecutionSettings {
                max_concurrent_tasks: 1,
                project_ideation_max: 1,
                auto_commit: true,
                pause_on_failure: true,
                ..ExecutionSettings::default()
            },
        )
        .await
        .unwrap();

    let review_task_id =
        create_task_in_project(&state, project.id.clone(), InternalStatus::Reviewing).await;
    let ideation_session_id = create_ideation_session_in_project(&state, project.id.clone()).await;

    state.execution_state.set_global_max_concurrent(5);
    state.execution_state.set_global_ideation_max(5);
    state
        .app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("ideation", &ideation_session_id),
            81818,
            "review-mixed-conv".to_string(),
            "review-mixed-run".to_string(),
            None,
            None,
        )
        .await;

    let result = build_chat_service(&state)
        .send_message(
            ChatContextType::Review,
            &review_task_id,
            "Attempt review spawn while ideation holds only slot",
            SendMessageOptions::default(),
        )
        .await;

    let err = result.expect_err("same-project ideation occupancy must block review spawn");
    assert!(
        matches!(err, ralphx_lib::application::chat_service::ChatServiceError::SpawnFailed(ref msg) if msg.contains("project execution capacity reached")),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn test_merge_send_blocks_when_same_project_ideation_consumes_only_slot() {
    let state = setup_test_state().await;

    let project = Project::new(
        "Merge Mixed Load".to_string(),
        "/tmp/merge-mixed-load".to_string(),
    );
    state
        .app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    state
        .app_state
        .execution_settings_repo
        .update_settings(
            Some(&project.id),
            &ExecutionSettings {
                max_concurrent_tasks: 1,
                project_ideation_max: 1,
                auto_commit: true,
                pause_on_failure: true,
                ..ExecutionSettings::default()
            },
        )
        .await
        .unwrap();

    let merge_task_id =
        create_task_in_project(&state, project.id.clone(), InternalStatus::Merging).await;
    let ideation_session_id = create_ideation_session_in_project(&state, project.id.clone()).await;

    state.execution_state.set_global_max_concurrent(5);
    state.execution_state.set_global_ideation_max(5);
    state
        .app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("ideation", &ideation_session_id),
            91919,
            "merge-mixed-conv".to_string(),
            "merge-mixed-run".to_string(),
            None,
            None,
        )
        .await;

    let result = build_chat_service(&state)
        .send_message(
            ChatContextType::Merge,
            &merge_task_id,
            "Attempt merge spawn while ideation holds only slot",
            SendMessageOptions::default(),
        )
        .await;

    let err = result.expect_err("same-project ideation occupancy must block merge spawn");
    assert!(
        matches!(err, ralphx_lib::application::chat_service::ChatServiceError::SpawnFailed(ref msg) if msg.contains("project execution capacity reached")),
        "unexpected error: {err}"
    );
}
