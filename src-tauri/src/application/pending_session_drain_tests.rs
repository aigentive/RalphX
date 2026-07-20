use std::sync::Arc;

use crate::application::chat_service::{ChatService, MockChatService};
use crate::application::pending_session_drain::PendingSessionDrainService;
use crate::application::AppState;
use crate::commands::ExecutionState;
use crate::domain::entities::{ChatContextType, IdeationSession, InternalStatus, Project, Task};
use crate::domain::execution::ExecutionSettings;
use crate::domain::services::RunningAgentKey;

#[tokio::test]
async fn pending_drain_does_not_borrow_when_workspace_queue_waits() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.set_global_max_concurrent(5);
    execution_state.set_global_ideation_max(1);
    execution_state.set_allow_ideation_borrow_idle_execution(true);

    let project = app_state
        .project_repo
        .create(Project::new(
            "Pending Workspace Pressure".to_string(),
            "/test/pending-workspace-pressure".to_string(),
        ))
        .await
        .unwrap();
    app_state
        .execution_settings_repo
        .update_settings(
            Some(&project.id),
            &ExecutionSettings {
                max_concurrent_tasks: 5,
                project_ideation_max: 5,
                auto_commit: true,
                pause_on_failure: true,
                ..ExecutionSettings::default()
            },
        )
        .await
        .unwrap();

    let occupied = app_state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();
    let pending = app_state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();
    app_state
        .ideation_session_repo
        .set_pending_initial_prompt(pending.id.as_str(), Some("pending ideation".to_string()))
        .await
        .unwrap();
    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("ideation", occupied.id.as_str()),
            78787,
            "occupied-conv".to_string(),
            "occupied-run".to_string(),
            None,
            None,
        )
        .await;
    app_state.message_queue.queue(
        ChatContextType::Project,
        project.id.as_str(),
        "waiting workspace".to_string(),
    );

    let mock = Arc::new(MockChatService::new());
    let drain = PendingSessionDrainService::new(
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.execution_settings_repo),
        Arc::clone(&execution_state),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&mock) as Arc<dyn ChatService>,
    );

    drain
        .try_drain_pending_for_project(project.id.as_str())
        .await;

    assert_eq!(mock.call_count(), 0);
    let fetched = app_state
        .ideation_session_repo
        .get_by_id(&pending.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        fetched.pending_initial_prompt.as_deref(),
        Some("pending ideation")
    );
}

#[tokio::test]
async fn pending_drain_does_not_borrow_when_ready_task_waits() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.set_global_max_concurrent(5);
    execution_state.set_global_ideation_max(1);
    execution_state.set_allow_ideation_borrow_idle_execution(true);

    let project = app_state
        .project_repo
        .create(Project::new(
            "Pending Ready Task Pressure".to_string(),
            "/test/pending-ready-task-pressure".to_string(),
        ))
        .await
        .unwrap();
    app_state
        .execution_settings_repo
        .update_settings(
            Some(&project.id),
            &ExecutionSettings {
                max_concurrent_tasks: 5,
                project_ideation_max: 5,
                auto_commit: true,
                pause_on_failure: true,
                ..ExecutionSettings::default()
            },
        )
        .await
        .unwrap();

    let occupied = app_state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();
    let pending = app_state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();
    app_state
        .ideation_session_repo
        .set_pending_initial_prompt(pending.id.as_str(), Some("pending ready task".to_string()))
        .await
        .unwrap();
    app_state
        .task_repo
        .create(Task {
            internal_status: InternalStatus::Ready,
            ..Task::new(project.id.clone(), "Ready execution pressure".to_string())
        })
        .await
        .unwrap();
    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("ideation", occupied.id.as_str()),
            79797,
            "occupied-ready-conv".to_string(),
            "occupied-ready-run".to_string(),
            None,
            None,
        )
        .await;

    let mock = Arc::new(MockChatService::new());
    let drain = PendingSessionDrainService::new(
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.execution_settings_repo),
        Arc::clone(&execution_state),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&mock) as Arc<dyn ChatService>,
    );

    drain
        .try_drain_pending_for_project(project.id.as_str())
        .await;

    assert_eq!(mock.call_count(), 0);
    let fetched = app_state
        .ideation_session_repo
        .get_by_id(&pending.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        fetched.pending_initial_prompt.as_deref(),
        Some("pending ready task")
    );
}

#[tokio::test]
async fn pending_drain_does_not_borrow_when_task_queue_waits() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.set_global_max_concurrent(5);
    execution_state.set_global_ideation_max(1);
    execution_state.set_allow_ideation_borrow_idle_execution(true);

    let project = app_state
        .project_repo
        .create(Project::new(
            "Pending Task Queue Pressure".to_string(),
            "/test/pending-task-queue-pressure".to_string(),
        ))
        .await
        .unwrap();
    app_state
        .execution_settings_repo
        .update_settings(
            Some(&project.id),
            &ExecutionSettings {
                max_concurrent_tasks: 5,
                project_ideation_max: 5,
                auto_commit: true,
                pause_on_failure: true,
                ..ExecutionSettings::default()
            },
        )
        .await
        .unwrap();

    let occupied = app_state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();
    let pending = app_state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();
    app_state
        .ideation_session_repo
        .set_pending_initial_prompt(pending.id.as_str(), Some("pending task queue".to_string()))
        .await
        .unwrap();
    let queued_task = app_state
        .task_repo
        .create(Task {
            internal_status: InternalStatus::Reviewing,
            ..Task::new(project.id.clone(), "Queued review pressure".to_string())
        })
        .await
        .unwrap();
    app_state.message_queue.queue(
        ChatContextType::Review,
        queued_task.id.as_str(),
        "queued review pressure".to_string(),
    );
    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("ideation", occupied.id.as_str()),
            80808,
            "occupied-task-queue-conv".to_string(),
            "occupied-task-queue-run".to_string(),
            None,
            None,
        )
        .await;

    let mock = Arc::new(MockChatService::new());
    let drain = PendingSessionDrainService::new(
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.execution_settings_repo),
        Arc::clone(&execution_state),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&mock) as Arc<dyn ChatService>,
    );

    drain
        .try_drain_pending_for_project(project.id.as_str())
        .await;

    assert_eq!(mock.call_count(), 0);
    let fetched = app_state
        .ideation_session_repo
        .get_by_id(&pending.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        fetched.pending_initial_prompt.as_deref(),
        Some("pending task queue")
    );
}

#[tokio::test]
async fn pending_drain_launches_oldest_session_when_capacity_is_available() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.set_global_max_concurrent(5);
    execution_state.set_global_ideation_max(5);

    let project = app_state
        .project_repo
        .create(Project::new(
            "Pending Capacity Available".to_string(),
            "/test/pending-capacity-available".to_string(),
        ))
        .await
        .unwrap();
    app_state
        .execution_settings_repo
        .update_settings(
            Some(&project.id),
            &ExecutionSettings {
                max_concurrent_tasks: 5,
                project_ideation_max: 5,
                auto_commit: true,
                pause_on_failure: true,
                ..ExecutionSettings::default()
            },
        )
        .await
        .unwrap();

    let pending = app_state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();
    app_state
        .ideation_session_repo
        .set_pending_initial_prompt(pending.id.as_str(), Some("start pending plan".to_string()))
        .await
        .unwrap();

    let mock = Arc::new(MockChatService::new());
    let drain = PendingSessionDrainService::new(
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.execution_settings_repo),
        Arc::clone(&execution_state),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&mock) as Arc<dyn ChatService>,
    );

    drain
        .try_drain_pending_for_project(project.id.as_str())
        .await;

    assert_eq!(mock.call_count(), 1);
    assert_eq!(
        mock.get_sent_messages().await,
        vec!["start pending plan".to_string()]
    );
    let fetched = app_state
        .ideation_session_repo
        .get_by_id(&pending.id)
        .await
        .unwrap()
        .unwrap();
    assert!(fetched.pending_initial_prompt.is_none());
}

#[tokio::test]
async fn pending_drain_preserves_typed_action_metadata() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.set_global_max_concurrent(5);
    execution_state.set_global_ideation_max(5);

    let project = app_state
        .project_repo
        .create(Project::new(
            "Pending Typed Action".to_string(),
            "/test/pending-typed-action".to_string(),
        ))
        .await
        .unwrap();
    app_state
        .execution_settings_repo
        .update_settings(
            Some(&project.id),
            &ExecutionSettings {
                max_concurrent_tasks: 5,
                project_ideation_max: 5,
                auto_commit: true,
                pause_on_failure: true,
                ..ExecutionSettings::default()
            },
        )
        .await
        .unwrap();

    let pending = app_state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();
    let metadata = r#"{"ralphx_action_kind":"verify_plan","ralphx_action_context_id":"session-1","ralphx_action_target_id":"artifact-1"}"#;
    let payload = crate::application::chat_service::encode_pending_initial_prompt(
        "verify the current plan",
        Some(metadata),
    );
    app_state
        .ideation_session_repo
        .set_pending_initial_prompt(pending.id.as_str(), Some(payload))
        .await
        .unwrap();

    let mock = Arc::new(MockChatService::new());
    let drain = PendingSessionDrainService::new(
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.execution_settings_repo),
        Arc::clone(&execution_state),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&mock) as Arc<dyn ChatService>,
    );

    drain
        .try_drain_pending_for_project(project.id.as_str())
        .await;

    assert_eq!(
        mock.get_sent_messages().await,
        vec!["verify the current plan".to_string()]
    );
    let options = mock.get_sent_options().await;
    assert_eq!(options.len(), 1);
    assert_eq!(options[0].metadata.as_deref(), Some(metadata));
}

#[tokio::test]
async fn pending_drain_restores_prompt_when_send_fails() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.set_global_max_concurrent(5);
    execution_state.set_global_ideation_max(5);

    let project = app_state
        .project_repo
        .create(Project::new(
            "Pending Send Failure".to_string(),
            "/test/pending-send-failure".to_string(),
        ))
        .await
        .unwrap();
    app_state
        .execution_settings_repo
        .update_settings(
            Some(&project.id),
            &ExecutionSettings {
                max_concurrent_tasks: 5,
                project_ideation_max: 5,
                auto_commit: true,
                pause_on_failure: true,
                ..ExecutionSettings::default()
            },
        )
        .await
        .unwrap();

    let pending = app_state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();
    app_state
        .ideation_session_repo
        .set_pending_initial_prompt(pending.id.as_str(), Some("retry later".to_string()))
        .await
        .unwrap();

    let mock = Arc::new(MockChatService::new());
    mock.set_available(false).await;
    let drain = PendingSessionDrainService::new(
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.execution_settings_repo),
        Arc::clone(&execution_state),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&mock) as Arc<dyn ChatService>,
    );

    drain
        .try_drain_pending_for_project(project.id.as_str())
        .await;

    assert_eq!(mock.call_count(), 1);
    let fetched = app_state
        .ideation_session_repo
        .get_by_id(&pending.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        fetched.pending_initial_prompt.as_deref(),
        Some("retry later")
    );
}

#[tokio::test]
async fn pending_drain_respects_running_task_project_capacity() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.set_global_max_concurrent(5);
    execution_state.set_global_ideation_max(5);

    let project = app_state
        .project_repo
        .create(Project::new(
            "Pending Task Capacity".to_string(),
            "/test/pending-task-capacity".to_string(),
        ))
        .await
        .unwrap();
    app_state
        .execution_settings_repo
        .update_settings(
            Some(&project.id),
            &ExecutionSettings {
                max_concurrent_tasks: 1,
                project_ideation_max: 5,
                auto_commit: true,
                pause_on_failure: true,
                ..ExecutionSettings::default()
            },
        )
        .await
        .unwrap();

    let pending = app_state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();
    app_state
        .ideation_session_repo
        .set_pending_initial_prompt(pending.id.as_str(), Some("wait for task slot".to_string()))
        .await
        .unwrap();
    let running_task = app_state
        .task_repo
        .create(Task {
            internal_status: InternalStatus::Executing,
            ..Task::new(project.id.clone(), "Running task slot".to_string())
        })
        .await
        .unwrap();
    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("task_execution", running_task.id.as_str()),
            81818,
            "running-task-conversation".to_string(),
            "running-task-run".to_string(),
            None,
            None,
        )
        .await;

    let mock = Arc::new(MockChatService::new());
    let drain = PendingSessionDrainService::new(
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.execution_settings_repo),
        Arc::clone(&execution_state),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&mock) as Arc<dyn ChatService>,
    );

    drain
        .try_drain_pending_for_project(project.id.as_str())
        .await;

    assert_eq!(mock.call_count(), 0);
    let fetched = app_state
        .ideation_session_repo
        .get_by_id(&pending.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        fetched.pending_initial_prompt.as_deref(),
        Some("wait for task slot")
    );
}
