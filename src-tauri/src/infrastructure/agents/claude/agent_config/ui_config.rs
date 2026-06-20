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
    /// Enable or disable Battle Mode. Default: true (backward compat; disabled via config/ralphx.yaml).
    pub battle_mode: bool,
    /// Enable or disable team mode UI. Default: false.
    pub team_mode: bool,
    /// Enable or disable Atlassian OAuth setup UI. Default: false.
    pub atlassian_oauth: bool,
    /// Enable or disable the read-only ticketing dashboard UI. Default: false.
    pub ticketing_dashboard: bool,
}

impl Default for UiFeatureFlagsConfig {
    fn default() -> Self {
        Self {
            activity_page: true,
            extensibility_page: true,
            battle_mode: true,
            team_mode: false,
            atlassian_oauth: false,
            ticketing_dashboard: false,
        }
    }
}
