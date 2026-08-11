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
        external_mcp_supervisor: None,
    }
}

fn chat_service(state: &HttpServerState) -> AppChatService {
    let app = &state.app_state;
    app.build_chat_service_with_execution_state(Arc::clone(&state.execution_state))
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
