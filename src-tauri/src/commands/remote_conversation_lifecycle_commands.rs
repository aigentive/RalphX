//! Spawn-free conversation lifecycle twins for the remote facade.

use tauri::State;

use crate::application::remote_conversation_lifecycle_intent::*;
use crate::application::AppState;
use crate::commands::agent_conversation_mute_commands::{
    set_agent_conversation_muted_for_state_without_repair_recovery, SetAgentConversationMutedInput,
};
use crate::commands::unified_chat_commands::{
    switch_agent_conversation_persona_for_state_rejecting_running_agent,
    SwitchAgentConversationPersonaInput, SwitchAgentConversationPersonaResponse,
};

#[tauri::command]
pub async fn set_remote_agent_conversation_muted(
    input: SetAgentConversationMutedInput,
    state: State<'_, AppState>,
) -> Result<(), String> {
    set_agent_conversation_muted_for_state_without_repair_recovery(input, &state).await
}

#[tauri::command]
pub async fn request_remote_conversation_archive(
    input: RequestRemoteConversationArchiveInput,
    state: State<'_, AppState>,
) -> Result<RemoteConversationLifecycleIntentResponse, String> {
    request_remote_conversation_archive_for_state(&state, input).await
}

#[tauri::command]
pub async fn request_remote_conversation_fork(
    input: RequestRemoteConversationForkInput,
    state: State<'_, AppState>,
) -> Result<RemoteConversationLifecycleIntentResponse, String> {
    request_remote_conversation_fork_for_state(&state, input).await
}

#[tauri::command]
pub async fn get_remote_conversation_lifecycle_request(
    request_id: String,
    state: State<'_, AppState>,
) -> Result<RemoteConversationLifecycleRequestView, String> {
    get_remote_conversation_lifecycle_request_for_state(&state, request_id).await
}

#[tauri::command]
pub async fn switch_remote_agent_conversation_persona(
    input: SwitchAgentConversationPersonaInput,
    state: State<'_, AppState>,
) -> Result<SwitchAgentConversationPersonaResponse, String> {
    switch_agent_conversation_persona_for_state_rejecting_running_agent(input, &state).await
}
