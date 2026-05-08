use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::{
    harness_runtime_registry::HarnessRuntimeProbe, probe_supported_harnesses, AppState, AGENT_LANES,
};
use crate::domain::agents::{
    generic_harness_lane_defaults, AgentHarnessKind, AgentLaneSettings, AgentProviderSettings,
    LogicalEffort,
};
use crate::infrastructure::agents::claude::apply_claude_provider_permission_settings;

const PROVIDER_SETTINGS_DISPLAY_ORDER: [AgentHarnessKind; 2] =
    [AgentHarnessKind::Codex, AgentHarnessKind::Claude];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProviderSettingsResponse {
    pub provider: String,
    pub enabled: bool,
    pub is_default: bool,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
    pub claude_permission_mode: Option<String>,
    pub claude_dangerously_skip_permissions: bool,
    pub claude_allow_dangerously_skip_permissions: bool,
    pub available: bool,
    pub binary_found: bool,
    pub binary_path: Option<String>,
    pub status: String,
    pub error: Option<String>,
    pub missing_core_exec_features: Vec<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProvidersSettingsResponse {
    pub providers: Vec<AgentProviderSettingsResponse>,
    pub default_provider: Option<String>,
    pub requires_onboarding: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAgentProviderSettingsInput {
    pub provider: String,
    pub enabled: Option<bool>,
    pub is_default: Option<bool>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
    pub claude_permission_mode: Option<String>,
    pub claude_dangerously_skip_permissions: Option<bool>,
    pub claude_allow_dangerously_skip_permissions: Option<bool>,
    #[serde(default)]
    pub apply_to_all_lanes: bool,
}

fn parse_provider(value: &str) -> Result<AgentHarnessKind, String> {
    value
        .parse::<AgentHarnessKind>()
        .map_err(|err| format!("Invalid provider: {err}"))
}

fn parse_effort(value: Option<String>) -> Result<Option<LogicalEffort>, String> {
    value
        .map(|effort| {
            effort
                .parse::<LogicalEffort>()
                .map_err(|err| format!("Invalid provider effort: {err}"))
        })
        .transpose()
}

fn merge_input(
    mut settings: AgentProviderSettings,
    input: UpdateAgentProviderSettingsInput,
    provider_available: bool,
) -> Result<AgentProviderSettings, String> {
    if let Some(model) = input.model {
        settings.model = if model.trim().is_empty() {
            None
        } else {
            Some(model)
        };
    }
    if input.effort.is_some() {
        settings.effort = parse_effort(input.effort)?;
    }
    if let Some(approval_policy) = input.approval_policy {
        settings.approval_policy = if approval_policy.trim().is_empty() {
            None
        } else {
            Some(approval_policy)
        };
    }
    if let Some(sandbox_mode) = input.sandbox_mode {
        settings.sandbox_mode = if sandbox_mode.trim().is_empty() {
            None
        } else {
            Some(sandbox_mode)
        };
    }
    if let Some(permission_mode) = input.claude_permission_mode {
        settings.claude_permission_mode = if permission_mode.trim().is_empty() {
            None
        } else {
            Some(permission_mode)
        };
    }
    if let Some(skip) = input.claude_dangerously_skip_permissions {
        settings.claude_dangerously_skip_permissions = skip;
    }
    if let Some(allow) = input.claude_allow_dangerously_skip_permissions {
        settings.claude_allow_dangerously_skip_permissions = allow;
    }
    if let Some(enabled) = input.enabled {
        if enabled && !provider_available {
            return Err(format!(
                "{} cannot be enabled until its CLI is available and ready",
                settings.provider
            ));
        }
        settings.enabled = enabled;
        if !enabled {
            settings.is_default = false;
        }
    }
    if let Some(is_default) = input.is_default {
        if is_default && !settings.enabled {
            return Err(format!(
                "{} cannot be the default until it is enabled",
                settings.provider
            ));
        }
        settings.is_default = is_default;
    }
    Ok(settings)
}

fn to_lane_settings(
    settings: &AgentProviderSettings,
    lane: crate::domain::agents::AgentLane,
) -> AgentLaneSettings {
    let mut lane_settings = generic_harness_lane_defaults(settings.provider, lane);
    if let Some(model) = &settings.model {
        lane_settings.model = Some(model.clone());
    }
    if let Some(effort) = settings.effort {
        lane_settings.effort = Some(effort);
    }
    lane_settings.approval_policy = settings.approval_policy.clone();
    lane_settings.sandbox_mode = settings.sandbox_mode.clone();
    lane_settings
}

fn provider_status(
    provider: AgentHarnessKind,
    available: bool,
    binary_path: Option<&str>,
    error: Option<&str>,
) -> String {
    if available {
        if let Some(path) = binary_path {
            return format!("Available {provider} detected at {path}.");
        }
        return format!("Available {provider} detected.");
    }
    error
        .map(str::to_string)
        .unwrap_or_else(|| format!("{provider} CLI is not ready."))
}

fn to_response(
    settings: AgentProviderSettings,
    probe: crate::application::harness_runtime_registry::HarnessRuntimeProbe,
) -> AgentProviderSettingsResponse {
    let status = provider_status(
        settings.provider,
        probe.available,
        probe.binary_path.as_deref(),
        probe.error.as_deref(),
    );
    AgentProviderSettingsResponse {
        provider: settings.provider.to_string(),
        enabled: settings.enabled,
        is_default: settings.is_default,
        model: settings.model,
        effort: settings.effort.map(|value| value.to_string()),
        approval_policy: settings.approval_policy,
        sandbox_mode: settings.sandbox_mode,
        claude_permission_mode: settings.claude_permission_mode,
        claude_dangerously_skip_permissions: settings.claude_dangerously_skip_permissions,
        claude_allow_dangerously_skip_permissions: settings
            .claude_allow_dangerously_skip_permissions,
        available: probe.available,
        binary_found: probe.binary_found,
        binary_path: probe.binary_path,
        status,
        error: probe.error,
        missing_core_exec_features: probe.missing_core_exec_features,
        updated_at: settings.updated_at.to_rfc3339(),
    }
}

async fn read_provider_settings(
    state: &AppState,
) -> Result<AgentProvidersSettingsResponse, String> {
    let probes = probe_supported_harnesses();
    read_provider_settings_with_probes(state, &probes).await
}

async fn read_provider_settings_with_probes(
    state: &AppState,
    probes: &HashMap<AgentHarnessKind, HarnessRuntimeProbe>,
) -> Result<AgentProvidersSettingsResponse, String> {
    let stored = state
        .agent_provider_settings_repo
        .list()
        .await
        .map_err(|err| err.to_string())?;
    let default_provider = stored
        .iter()
        .find(|row| row.enabled && row.is_default)
        .map(|row| row.provider.to_string());
    let providers = PROVIDER_SETTINGS_DISPLAY_ORDER
        .into_iter()
        .map(|provider| {
            let settings = stored
                .iter()
                .find(|row| row.provider == provider)
                .cloned()
                .unwrap_or_else(|| AgentProviderSettings::disabled_defaults(provider));
            let probe = probes
                .get(&provider)
                .cloned()
                .unwrap_or_else(|| HarnessRuntimeProbe {
                    binary_path: None,
                    binary_found: false,
                    probe_succeeded: false,
                    available: false,
                    missing_core_exec_features: Vec::new(),
                    error: Some(format!("{provider} probe unavailable")),
                });
            to_response(settings, probe)
        })
        .collect::<Vec<_>>();

    let requires_onboarding = default_provider.is_none();
    Ok(AgentProvidersSettingsResponse {
        providers,
        default_provider,
        requires_onboarding,
    })
}

async fn apply_provider_to_global_lanes(
    state: &AppState,
    settings: &AgentProviderSettings,
) -> Result<(), String> {
    for lane in AGENT_LANES {
        state
            .agent_lane_settings_repo
            .upsert_global(lane, &to_lane_settings(settings, lane))
            .await
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn get_agent_provider_settings(
    state: State<'_, AppState>,
) -> Result<AgentProvidersSettingsResponse, String> {
    read_provider_settings(&state).await
}

#[tauri::command]
pub async fn update_agent_provider_settings(
    input: UpdateAgentProviderSettingsInput,
    state: State<'_, AppState>,
) -> Result<AgentProvidersSettingsResponse, String> {
    let probes = probe_supported_harnesses();
    update_provider_settings_with_probes(input, &state, &probes).await
}

async fn update_provider_settings_with_probes(
    input: UpdateAgentProviderSettingsInput,
    state: &AppState,
    probes: &HashMap<AgentHarnessKind, HarnessRuntimeProbe>,
) -> Result<AgentProvidersSettingsResponse, String> {
    let provider = parse_provider(&input.provider)?;
    let probe = probes
        .get(&provider)
        .ok_or_else(|| format!("{provider} probe unavailable"))?;
    let existing = state
        .agent_provider_settings_repo
        .get(provider)
        .await
        .map_err(|err| err.to_string())?
        .unwrap_or_else(|| AgentProviderSettings::disabled_defaults(provider));
    let apply_to_all_lanes = input.apply_to_all_lanes;
    let settings = merge_input(existing, input, probe.available)?;
    let saved = state
        .agent_provider_settings_repo
        .upsert(&settings)
        .await
        .map_err(|err| err.to_string())?;

    if saved.provider == AgentHarnessKind::Claude {
        apply_claude_provider_permission_settings(&saved);
    }

    if saved.is_default && apply_to_all_lanes {
        apply_provider_to_global_lanes(&state, &saved).await?;
    }

    read_provider_settings_with_probes(state, probes).await
}

#[cfg(test)]
#[path = "harness_provider_commands_tests.rs"]
mod tests;
