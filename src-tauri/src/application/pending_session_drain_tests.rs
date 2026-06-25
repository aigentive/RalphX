use std::sync::Arc;

use crate::application::chat_service::{ChatService, MockChatService};
use crate::application::pending_session_drain::PendingSessionDrainService;
use crate::application::AppState;
use crate::commands::ExecutionState;
use crate::domain::entities::{ChatContextType, IdeationSession, Project};
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
