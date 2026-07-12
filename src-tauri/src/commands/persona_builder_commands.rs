use serde::Deserialize;
use tauri::State;

use crate::application::personas::PERSONA_FEATURE_DISABLED_PREFIX;
use crate::application::AppState;
use crate::commands::unified_chat_commands::{
    agent_conversation_response_for_state, AgentConversationResponse,
};
use crate::domain::entities::{AgentConversationWorkspaceMode, ChatConversation, ProjectId};
use crate::infrastructure::agents::claude::ui_feature_flags_config;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePersonaBuilderConversationInput {
    pub project_id: String,
}

/// Create the sole persisted entry point for a PersonaBuilder conversation.
#[tauri::command]
pub async fn create_persona_builder_conversation(
    input: CreatePersonaBuilderConversationInput,
    state: State<'_, AppState>,
) -> Result<AgentConversationResponse, String> {
    create_persona_builder_conversation_for_state(
        input,
        state.inner(),
        ui_feature_flags_config().agent_personas,
    )
    .await
}

#[doc(hidden)]
pub async fn create_persona_builder_conversation_for_state(
    input: CreatePersonaBuilderConversationInput,
    state: &AppState,
    feature_enabled: bool,
) -> Result<AgentConversationResponse, String> {
    if !feature_enabled {
        return Err(crate::error::AppError::FeatureDisabled(format!(
            "{PERSONA_FEATURE_DISABLED_PREFIX} agent personas feature is disabled]"
        ))
        .to_string());
    }

    let mut conversation = ChatConversation::new_project(ProjectId::from_string(input.project_id));
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::PersonaBuilder));
    conversation.set_title("Persona builder".to_string());
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .map_err(|error| error.to_string())?;

    agent_conversation_response_for_state(state, conversation).await
}
