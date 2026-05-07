use super::*;
use crate::application::session_namer_agent::{spawn_session_namer_agent, SessionNamerTarget};

/// Build a fully configured app chat service from shared app + execution state.
/// Extracted to avoid duplicating the 12-arg constructor chain across multiple handlers.
pub(crate) fn build_chat_service(
    app: &crate::application::AppState,
    execution_state: &std::sync::Arc<crate::commands::ExecutionState>,
) -> crate::application::AppChatService {
    app.build_chat_service_with_execution_state(Arc::clone(execution_state))
}

/// Fire-and-forget: spawn the session namer agent to auto-name the session.
pub(super) async fn spawn_session_namer(
    app: &crate::application::AppState,
    project_id: &str,
    session_id: String,
    prompt: String,
) {
    if let Err(error) = spawn_session_namer_agent(
        app,
        SessionNamerTarget::SessionInitial {
            session_id,
            user_message: prompt,
        },
    )
    .await
    {
        tracing::warn!(
            project_id,
            "Failed to prepare external ideation session namer: {}",
            error
        );
    }
}

/// Determine agent tri-state status for a session:
/// "idle" | "generating" | "waiting_for_input"
pub(crate) async fn determine_agent_status(
    running_agent_registry: &dyn crate::domain::services::running_agent_registry::RunningAgentRegistry,
    interactive_process_registry: &crate::application::InteractiveProcessRegistry,
    context_id: &str,
) -> String {
    let agent_key =
        crate::domain::services::running_agent_registry::RunningAgentKey::new("ideation", context_id);
    if running_agent_registry.is_running(&agent_key).await {
        let ipr_key = crate::application::InteractiveProcessKey {
            context_type: "ideation".to_string(),
            context_id: context_id.to_string(),
        };
        if interactive_process_registry.has_process(&ipr_key).await {
            "waiting_for_input".to_string()
        } else {
            "generating".to_string()
        }
    } else {
        "idle".to_string()
    }
}
