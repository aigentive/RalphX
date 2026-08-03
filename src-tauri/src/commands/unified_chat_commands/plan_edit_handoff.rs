use crate::application::{AppState, ChatService};
use crate::domain::entities::{ChatContextType, ChatConversation};
use crate::domain::services::RunningAgentKey;

/// Stops the Plan runtime before the durable Plan-to-Edit authority transition.
pub(crate) async fn stop_plan_to_edit_handoff_before_commit(
    state: &AppState,
    chat_service: &dyn ChatService,
    conversation: &ChatConversation,
) -> Result<(), String> {
    let running_key = RunningAgentKey::new(
        ChatContextType::Project.to_string(),
        conversation.id.as_str(),
    );
    if !state.running_agent_registry.is_running(&running_key).await {
        return Ok(());
    }

    let context_id = conversation.id.as_str();
    let stopped = chat_service
        .stop_agent(ChatContextType::Project, &context_id)
        .await
        .map_err(|error| error.to_string())?;
    tracing::info!(
        conversation_id = %conversation.id,
        stopped,
        "Stopped running Plan agent before Plan-to-Edit handoff"
    );
    if state.running_agent_registry.is_running(&running_key).await {
        return Err("Cannot change mode while the agent is running".to_string());
    }
    Ok(())
}

/// Retires the idle Plan runtime and clears its provider session after Edit mode commits.
pub(crate) async fn finish_plan_to_edit_handoff_after_commit(
    state: &AppState,
    chat_service: &dyn ChatService,
    conversation: &mut ChatConversation,
) -> Result<(), String> {
    let retired = chat_service
        .retire_idle_interactive_process(ChatContextType::Project, &conversation.id.as_str())
        .await
        .map_err(|error| error.to_string())?;
    if !retired {
        return Err(
            "Plan-to-Edit runtime handoff is still active or could not be verified; the mode change is saved, retry the action to finish the handoff"
                .to_string(),
        );
    }
    tracing::info!(
        conversation_id = %conversation.id,
        "Retired idle Plan runtime after Plan-to-Edit handoff"
    );

    clear_plan_provider_session_after_commit(state, conversation).await
}

/// Preserve the legacy clear-only behavior for mode-switch callers that do not
/// own a live chat service and therefore cannot perform runtime retirement.
pub(crate) async fn clear_plan_provider_session_after_commit(
    state: &AppState,
    conversation: &mut ChatConversation,
) -> Result<(), String> {
    if conversation.provider_session_ref().is_some() {
        state
            .chat_conversation_repo
            .clear_provider_session_ref(&conversation.id)
            .await
            .map_err(|error| error.to_string())?;
        conversation.clear_provider_session_ref();
        tracing::info!(
            conversation_id = %conversation.id,
            "Cleared planning provider session after Plan-to-Edit handoff"
        );
    }
    Ok(())
}
