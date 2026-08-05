//! Spawn-free conversation lifecycle twins for the remote facade.

use tauri::State;

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
pub async fn switch_remote_agent_conversation_persona(
    input: SwitchAgentConversationPersonaInput,
    state: State<'_, AppState>,
) -> Result<SwitchAgentConversationPersonaResponse, String> {
    switch_agent_conversation_persona_for_state_rejecting_running_agent(input, &state).await
}
