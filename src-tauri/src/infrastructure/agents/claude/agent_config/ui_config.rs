use serde::{Deserialize, Serialize};

/// UI configuration section from config/ralphx.yaml.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UiConfig {
    /// Feature flags controlling page visibility.
    #[serde(default)]
    pub feature_flags: Option<UiFeatureFlagsConfig>,
}

/// Per-page feature flag configuration.
///
/// Defaults to all pages enabled for backward compatibility with configs
/// that do not have a `ui.feature_flags` section.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiFeatureFlagsConfig {
    /// Show or hide the Activity page. Default: true.
    pub activity_page: bool,
    /// Show or hide the Extensibility page. Default: true.
    pub extensibility_page: bool,
    /// Show or hide the Automations page. Default: true.
    pub automations_page: bool,
    /// Enable or disable Atlassian OAuth setup UI. Default: false.
    pub atlassian_oauth: bool,
    /// Enable or disable the read-only ticketing dashboard UI. Default: false.
    pub ticketing_dashboard: bool,
    /// Enable or disable agent personas. Default: false.
    pub agent_personas: bool,
    /// Force a new provider session after a persona switch. Default: false.
    pub persona_switch_forces_fresh_provider_session: bool,
    /// Enable or disable projectless (standalone) conversations. Default: false.
    pub standalone_conversations: bool,
}

impl Default for UiFeatureFlagsConfig {
    fn default() -> Self {
        Self {
            activity_page: true,
            extensibility_page: true,
            automations_page: true,
            atlassian_oauth: false,
            ticketing_dashboard: false,
            agent_personas: false,
            persona_switch_forces_fresh_provider_session: false,
            standalone_conversations: false,
        }
    }
}
