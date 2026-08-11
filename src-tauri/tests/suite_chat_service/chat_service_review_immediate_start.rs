use ralphx_lib::application::chat_service::{
    AppChatService, ChatService, ChatServiceError, SendMessageOptions, SendQueuePolicy,
};
use ralphx_lib::application::AppState;
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{AgentRunId, ChatContextType, Project};
use ralphx_lib::http_server::types::HttpServerState;
use std::sync::Arc;

fn test_state() -> HttpServerState {
    let app_state = Arc::new(AppState::new_test());
    let execution_state = Arc::new(ExecutionState::new());
    HttpServerState {
        app_state,
        execution_state,
        delegation_service: Default::default(),
    }
}

fn chat_service(state: &HttpServerState) -> AppChatService {
    let app = &state.app_state;
    AppChatService::new(
        Arc::clone(&app.chat_message_repo),
        Arc::clone(&app.chat_attachment_repo),
        Arc::clone(&app.artifact_repo),
        Arc::clone(&app.chat_conversation_repo),
        Arc::clone(&app.agent_run_repo),
        Arc::clone(&app.project_repo),
        Arc::clone(&app.task_repo),
        Arc::clone(&app.task_dependency_repo),
        Arc::clone(&app.ideation_session_repo),
        Arc::clone(&app.delegated_session_repo),
        Arc::clone(&app.activity_event_repo),
        Arc::clone(&app.message_queue),
        Arc::clone(&app.running_agent_registry),
        Arc::clone(&app.memory_event_repo),
    )
    .with_execution_state(Arc::clone(&state.execution_state))
    .with_execution_settings_repo(Arc::clone(&app.execution_settings_repo))
    .with_plan_branch_repo(Arc::clone(&app.plan_branch_repo))
    .with_task_proposal_repo(Arc::clone(&app.task_proposal_repo))
    .with_interactive_process_registry(Arc::clone(&app.interactive_process_registry))
}

#[tokio::test]
async fn paused_immediate_project_send_rejects_without_queueing() {
    let state = test_state();
    let project = Project::new(
        "Immediate Review".to_string(),
        "/tmp/immediate-review".to_string(),
    );
    state
        .app_state
        .project_repo
        .create(project.clone())
        .await
        .expect("seed project");
    state.execution_state.pause();

    let result = chat_service(&state)
        .send_message(
            ChatContextType::Project,
            project.id.as_str(),
            "Start reserved workspace Review",
            SendMessageOptions {
                preallocated_agent_run_id: Some(AgentRunId::new()),
                queue_policy: SendQueuePolicy::RequireImmediateStart,
                ..SendMessageOptions::default()
            },
        )
        .await;

    assert!(matches!(
        result,
        Err(ChatServiceError::ImmediateStartRejected(ref message))
            if message.contains("immediate start required")
    ));
    assert!(state
        .app_state
        .message_queue
        .get_queued(ChatContextType::Project, project.id.as_str())
        .is_empty());
    assert!(state
        .app_state
        .chat_conversation_repo
        .get_active_for_context(ChatContextType::Project, project.id.as_str())
        .await
        .expect("conversation lookup should succeed")
        .is_none());
}
