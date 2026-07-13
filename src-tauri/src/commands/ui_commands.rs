// UI feature flag commands — expose runtime config flags to the frontend

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::{harness_runtime_registry::default_ui_feature_flags, AppState};
use crate::infrastructure::agents::claude::{agent_personas_enabled, set_agent_personas_override};

/// Response struct for UI feature flags.
/// Fields use camelCase for frontend consumption via Tauri invoke.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiFeatureFlagsResponse {
    pub activity_page: bool,
    pub extensibility_page: bool,
    pub ideation_page: bool,
    pub automations_page: bool,
    pub battle_mode: bool,
    pub team_mode: bool,
    pub atlassian_oauth: bool,
    pub ticketing_dashboard: bool,
    pub agent_personas: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUiFeatureFlagsInput {
    pub agent_personas: Option<bool>,
}

fn ui_feature_flags_response() -> UiFeatureFlagsResponse {
    let flags = default_ui_feature_flags();
    UiFeatureFlagsResponse {
        activity_page: flags.activity_page,
        extensibility_page: flags.extensibility_page,
        ideation_page: flags.ideation_page,
        automations_page: flags.automations_page,
        battle_mode: flags.battle_mode,
        team_mode: flags.team_mode,
        atlassian_oauth: flags.atlassian_oauth,
        ticketing_dashboard: flags.ticketing_dashboard,
        agent_personas: agent_personas_enabled(),
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
pub fn get_ui_feature_flags() -> UiFeatureFlagsResponse {
    ui_feature_flags_response()
}

#[tauri::command]
pub async fn update_ui_feature_flags(
    input: UpdateUiFeatureFlagsInput,
    state: State<'_, AppState>,
) -> Result<UiFeatureFlagsResponse, String> {
    persist_agent_personas_override(state.inner(), input.agent_personas).await?;
    Ok(ui_feature_flags_response())
}

#[cfg(test)]
#[path = "ui_commands_tests.rs"]
mod tests;
