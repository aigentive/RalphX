// UI feature flag commands — expose runtime config flags to the frontend

use serde::Serialize;

use crate::application::harness_runtime_registry::default_ui_feature_flags;

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

/// Returns the current UI feature flag configuration.
/// Reads from the OnceLock-cached runtime config; safe to call repeatedly.
#[tauri::command]
pub fn get_ui_feature_flags() -> UiFeatureFlagsResponse {
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
        agent_personas: flags.agent_personas,
    }
}

#[cfg(test)]
#[path = "ui_commands_tests.rs"]
mod tests;
