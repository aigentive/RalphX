// UI feature flag commands — expose runtime config flags to the frontend

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::{
    agent_capability_gate::AgentCapabilities, harness_runtime_registry::default_ui_feature_flags,
    AppState,
};
use crate::infrastructure::agents::{
    agent_personas_enabled, set_agent_personas_override, standalone_conversations_enabled,
};

/// Response struct for UI feature flags.
/// Fields use camelCase for frontend consumption via Tauri invoke.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiFeatureFlagsResponse {
    pub activity_page: bool,
    pub extensibility_page: bool,
    pub automations_page: bool,
    pub atlassian_oauth: bool,
    pub ticketing_dashboard: bool,
    pub agent_personas: bool,
    pub standalone_conversations: bool,
    pub agent_conversation_team: bool,
    pub agent_conversation_workflows: bool,
    pub agent_conversation_autopilot: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUiFeatureFlagsInput {
    pub agent_personas: Option<bool>,
    pub agent_conversation_team: Option<bool>,
    pub agent_conversation_workflows: Option<bool>,
    pub agent_conversation_autopilot: Option<bool>,
}

fn ui_feature_flags_response(state: &AppState) -> UiFeatureFlagsResponse {
    ui_feature_flags_response_with_standalone(state, standalone_conversations_enabled())
}

fn ui_feature_flags_response_with_standalone(
    state: &AppState,
    standalone_conversations: bool,
) -> UiFeatureFlagsResponse {
    let flags = default_ui_feature_flags();
    let agent_capabilities = state.agent_capability_gate.snapshot();
    UiFeatureFlagsResponse {
        activity_page: flags.activity_page,
        extensibility_page: flags.extensibility_page,
        automations_page: flags.automations_page,
        atlassian_oauth: flags.atlassian_oauth,
        ticketing_dashboard: flags.ticketing_dashboard,
        agent_personas: agent_personas_enabled(),
        standalone_conversations,
        agent_conversation_team: agent_capabilities.team,
        agent_conversation_workflows: agent_capabilities.workflows,
        agent_conversation_autopilot: agent_capabilities.autopilot,
    }
}

async fn persist_agent_personas_override(
    state: &AppState,
    value: Option<bool>,
) -> Result<(), String> {
    let Some(value) = value else {
        return Ok(());
    };
    state
        .ui_feature_flag_overrides_repo
        .set_agent_personas(Some(value))
        .await
        .map_err(|error| error.to_string())?;
    set_agent_personas_override(Some(value));
    Ok(())
}

/// Returns the current UI feature flag configuration.
/// Reads from the OnceLock-cached runtime config; safe to call repeatedly.
#[tauri::command]
pub fn get_ui_feature_flags(state: State<'_, AppState>) -> UiFeatureFlagsResponse {
    ui_feature_flags_response(state.inner())
}

#[tauri::command]
pub async fn update_ui_feature_flags(
    input: UpdateUiFeatureFlagsInput,
    state: State<'_, AppState>,
) -> Result<UiFeatureFlagsResponse, String> {
    update_ui_feature_flags_for_state(input, state.inner()).await
}

#[doc(hidden)]
pub async fn update_ui_feature_flags_for_state(
    input: UpdateUiFeatureFlagsInput,
    state: &AppState,
) -> Result<UiFeatureFlagsResponse, String> {
    persist_agent_personas_override(state, input.agent_personas).await?;
    if input.agent_conversation_team.is_some()
        || input.agent_conversation_workflows.is_some()
        || input.agent_conversation_autopilot.is_some()
    {
        let persisted = state
            .ui_feature_flag_overrides_repo
            .update_agent_capabilities(
                input.agent_conversation_team,
                input.agent_conversation_workflows,
                input.agent_conversation_autopilot,
            )
            .await
            .map_err(|error| error.to_string())?;
        state.agent_capability_gate.replace(AgentCapabilities {
            team: persisted.agent_conversation_team,
            workflows: persisted.agent_conversation_workflows,
            autopilot: persisted.agent_conversation_autopilot,
        });
    }
    Ok(ui_feature_flags_response(state))
}

#[cfg(test)]
#[path = "ui_commands_tests.rs"]
mod tests;
