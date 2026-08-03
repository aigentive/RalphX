use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::{
    harness_runtime_registry::{
        clear_harness_runtime_caches_for_harness, refresh_harness_runtime_probe_with_force,
        refresh_supported_harnesses_with_force, HarnessRuntimeProbe,
    },
    AppState, AGENT_LANES,
};
use crate::domain::agents::{
    generic_harness_lane_defaults, AgentHarnessKind, AgentLaneSettings,
    AgentProviderCliManagementMode, AgentProviderSettings, LogicalEffort,
    CODEX_DEFAULT_APPROVAL_POLICY, CODEX_DEFAULT_SANDBOX_MODE, STANDARD_AGENT_HARNESSES,
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
    pub service_tier: Option<String>,
    pub claude_permission_mode: Option<String>,
    pub claude_dangerously_skip_permissions: bool,
    pub claude_allow_dangerously_skip_permissions: bool,
    pub cli_management_mode: String,
    pub auto_update_enabled: bool,
    pub custom_binary_enabled: bool,
    pub custom_binary_path: Option<String>,
    pub custom_env_file_enabled: bool,
    pub custom_env_file_path: Option<String>,
    pub available: bool,
    pub binary_found: bool,
    pub binary_path: Option<String>,
    pub status: String,
    pub error: Option<String>,
    pub missing_core_exec_features: Vec<String>,
    pub cli_version: Option<String>,
    pub supported_model_aliases: Option<Vec<String>>,
    pub supported_efforts: Option<Vec<String>>,
    pub ultra_supported_models: Vec<String>,
    pub supports_fast_mode: bool,
    pub fast_mode_supported_models: Vec<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProvidersSettingsResponse {
    pub providers: Vec<AgentProviderSettingsResponse>,
    pub default_provider: Option<String>,
    pub requires_onboarding: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GetAgentProviderSettingsInput {
    #[serde(default)]
    pub refresh_runtime: bool,
    #[serde(default)]
    pub force_runtime: bool,
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
    #[serde(default)]
    pub service_tier: Option<Option<String>>,
    pub claude_permission_mode: Option<String>,
    pub claude_dangerously_skip_permissions: Option<bool>,
    pub claude_allow_dangerously_skip_permissions: Option<bool>,
    pub cli_management_mode: Option<String>,
    pub auto_update_enabled: Option<bool>,
    #[serde(default)]
    pub custom_binary_enabled: Option<bool>,
    #[serde(default)]
    pub custom_binary_path: Option<Option<String>>,
    #[serde(default)]
    pub custom_env_file_enabled: Option<bool>,
    #[serde(default)]
    pub custom_env_file_path: Option<Option<String>>,
    #[serde(default)]
    pub reset_to_defaults: bool,
    #[serde(default)]
    pub apply_to_all_lanes: bool,
}

fn parse_provider(value: &str) -> Result<AgentHarnessKind, String> {
    value
        .parse::<AgentHarnessKind>()
        .map_err(|err| format!("Invalid provider: {err}"))
}

fn parse_effort(value: Option<String>) -> Result<Option<LogicalEffort>, String> {
    match value {
        Some(effort) if effort.trim().is_empty() => Ok(None),
        Some(effort) => effort
            .parse::<LogicalEffort>()
            .map(Some)
            .map_err(|err| format!("Invalid provider effort: {err}")),
        None => Ok(None),
    }
}

fn parse_cli_management_mode(
    value: Option<String>,
) -> Result<Option<AgentProviderCliManagementMode>, String> {
    match value {
        Some(mode) if mode.trim().is_empty() => {
            Ok(Some(AgentProviderCliManagementMode::UserManaged))
        }
        Some(mode) => mode
            .parse::<AgentProviderCliManagementMode>()
            .map(Some)
            .map_err(|err| format!("Invalid provider CLI management mode: {err}")),
        None => Ok(None),
    }
}

fn reset_configurable_defaults(settings: &mut AgentProviderSettings) {
    let defaults = AgentProviderSettings::disabled_defaults(settings.provider);
    settings.model = defaults.model;
    settings.effort = defaults.effort;
    settings.approval_policy = defaults.approval_policy;
    settings.sandbox_mode = defaults.sandbox_mode;
    settings.service_tier = defaults.service_tier;
    settings.claude_permission_mode = defaults.claude_permission_mode;
    settings.claude_dangerously_skip_permissions = defaults.claude_dangerously_skip_permissions;
    settings.claude_allow_dangerously_skip_permissions =
        defaults.claude_allow_dangerously_skip_permissions;
    settings.cli_management_mode = defaults.cli_management_mode;
    settings.auto_update_enabled = defaults.auto_update_enabled;
    settings.custom_binary_enabled = defaults.custom_binary_enabled;
    settings.custom_binary_path = defaults.custom_binary_path;
    settings.custom_env_file_enabled = defaults.custom_env_file_enabled;
    settings.custom_env_file_path = defaults.custom_env_file_path;
}

fn enforce_provider_constraints(settings: &mut AgentProviderSettings) {
    if settings.provider == AgentHarnessKind::Codex {
        settings.approval_policy = Some(CODEX_DEFAULT_APPROVAL_POLICY.to_string());
        settings.sandbox_mode = Some(CODEX_DEFAULT_SANDBOX_MODE.to_string());
    }
}

fn normalize_custom_binary_path(path: Option<String>) -> Result<Option<String>, String> {
    normalize_optional_provider_path(path)
}

fn normalize_custom_env_file_path(path: Option<String>) -> Result<Option<String>, String> {
    normalize_optional_provider_path(path)
}

fn normalize_service_tier(value: Option<String>) -> Option<String> {
    value.and_then(|tier| {
        let trimmed = tier.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("standard") {
            None
        } else {
            Some(trimmed.to_ascii_lowercase())
        }
    })
}

fn validate_codex_fast_mode_selection(
    settings: &AgentProviderSettings,
    probe: &HarnessRuntimeProbe,
) -> Result<(), String> {
    if settings.provider != AgentHarnessKind::Codex
        || settings.service_tier.as_deref() != Some("fast")
    {
        return Ok(());
    }

    if !probe.supports_fast_mode {
        return Err(
            "Codex Fast mode is not supported by the selected Codex CLI or model catalog."
                .to_string(),
        );
    }

    let Some(model) = settings.model.as_deref() else {
        return Ok(());
    };
    if !probe.fast_mode_supported_models.is_empty()
        && !probe
            .fast_mode_supported_models
            .iter()
            .any(|supported_model| supported_model == model)
    {
        return Err(format!(
            "Codex Fast mode is not available for model {model}."
        ));
    }

    Ok(())
}

fn normalize_optional_provider_path(path: Option<String>) -> Result<Option<String>, String> {
    let Some(value) = path else {
        return Ok(None);
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    expand_provider_user_path(trimmed).map(Some)
}

fn provider_path_home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

fn expand_provider_user_path(path: &str) -> Result<String, String> {
    if path == "~" {
        return provider_path_home_dir()
            .map(|home| home.to_string_lossy().into_owned())
            .ok_or_else(home_expansion_error);
    }
    let Some(rest) = path.strip_prefix("~/") else {
        if path.starts_with('~') {
            return Err(
                "Custom provider paths support only ~/ for the user home directory; ~user paths are not supported"
                    .to_string(),
            );
        }
        return Ok(path.to_string());
    };
    let home = provider_path_home_dir().ok_or_else(home_expansion_error)?;
    Ok(join_home_relative_path(&home, rest))
}

fn join_home_relative_path(home: &Path, rest: &str) -> String {
    let mut expanded = home.to_path_buf();
    expanded.push(rest);
    expanded.to_string_lossy().into_owned()
}

fn home_expansion_error() -> String {
    "Cannot expand custom provider path because the user home directory could not be determined"
        .to_string()
}

fn merge_input(
    mut settings: AgentProviderSettings,
    input: UpdateAgentProviderSettingsInput,
    provider_available: bool,
) -> Result<AgentProviderSettings, String> {
    if input.reset_to_defaults {
        reset_configurable_defaults(&mut settings);
    }
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
    if let Some(service_tier) = input.service_tier {
        settings.service_tier = normalize_service_tier(service_tier);
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
    if input.cli_management_mode.is_some() {
        let mode = parse_cli_management_mode(input.cli_management_mode)?
            .unwrap_or(AgentProviderCliManagementMode::UserManaged);
        settings.cli_management_mode = mode;
        if mode == AgentProviderCliManagementMode::RxManaged
            && input.custom_binary_enabled != Some(true)
        {
            settings.custom_binary_enabled = false;
        }
    }
    if let Some(auto_update_enabled) = input.auto_update_enabled {
        settings.auto_update_enabled = auto_update_enabled;
    }
    if let Some(custom_binary_path) = input.custom_binary_path {
        settings.custom_binary_path = normalize_custom_binary_path(custom_binary_path)?;
    }
    if let Some(custom_binary_enabled) = input.custom_binary_enabled {
        settings.custom_binary_enabled = custom_binary_enabled;
    }
    if let Some(custom_env_file_path) = input.custom_env_file_path {
        settings.custom_env_file_path = normalize_custom_env_file_path(custom_env_file_path)?;
    }
    if let Some(custom_env_file_enabled) = input.custom_env_file_enabled {
        settings.custom_env_file_enabled = custom_env_file_enabled;
    }
    if settings.custom_binary_enabled {
        if settings
            .custom_binary_path
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
        {
            return Err(format!(
                "Custom {} binary path is required before enabling custom binary mode",
                settings.provider
            ));
        }
        settings.cli_management_mode = AgentProviderCliManagementMode::UserManaged;
        settings.auto_update_enabled = false;
    }
    if settings.custom_env_file_enabled
        && settings
            .custom_env_file_path
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
    {
        return Err(format!(
            "Custom {} env file path is required before enabling custom env file mode",
            settings.provider
        ));
    }
    if settings.cli_management_mode != AgentProviderCliManagementMode::RxManaged {
        settings.auto_update_enabled = false;
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
    enforce_provider_constraints(&mut settings);
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
    probe_succeeded: bool,
    binary_path: Option<&str>,
    error: Option<&str>,
) -> String {
    if available {
        if !probe_succeeded {
            return format!("{provider} is enabled in Settings.");
        }
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
        probe.probe_succeeded,
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
        service_tier: settings.service_tier,
        claude_permission_mode: settings.claude_permission_mode,
        claude_dangerously_skip_permissions: settings.claude_dangerously_skip_permissions,
        claude_allow_dangerously_skip_permissions: settings
            .claude_allow_dangerously_skip_permissions,
        cli_management_mode: settings.cli_management_mode.to_string(),
        auto_update_enabled: settings.auto_update_enabled,
        custom_binary_enabled: settings.custom_binary_enabled,
        custom_binary_path: settings.custom_binary_path,
        custom_env_file_enabled: settings.custom_env_file_enabled,
        custom_env_file_path: settings.custom_env_file_path,
        available: probe.available,
        binary_found: probe.binary_found,
        binary_path: probe.binary_path,
        status,
        error: probe.error,
        missing_core_exec_features: probe.missing_core_exec_features,
        cli_version: probe.cli_version,
        supported_model_aliases: probe.supported_model_aliases,
        supported_efforts: probe.supported_efforts,
        ultra_supported_models: probe.ultra_supported_models,
        supports_fast_mode: probe.supports_fast_mode,
        fast_mode_supported_models: probe.fast_mode_supported_models,
        updated_at: settings.updated_at.to_rfc3339(),
    }
}

pub(crate) fn provider_settings_snapshot_probe(
    settings: &AgentProviderSettings,
) -> HarnessRuntimeProbe {
    if settings.enabled {
        return HarnessRuntimeProbe {
            binary_path: None,
            binary_found: true,
            probe_succeeded: false,
            available: true,
            missing_core_exec_features: Vec::new(),
            cli_version: None,
            supported_model_aliases: None,
            supported_efforts: None,
            ultra_supported_models: Vec::new(),
            supports_fast_mode: false,
            fast_mode_supported_models: Vec::new(),
            error: None,
        };
    }

    HarnessRuntimeProbe {
        binary_path: None,
        binary_found: false,
        probe_succeeded: false,
        available: false,
        missing_core_exec_features: Vec::new(),
        cli_version: None,
        supported_model_aliases: None,
        supported_efforts: None,
        ultra_supported_models: Vec::new(),
        supports_fast_mode: false,
        fast_mode_supported_models: Vec::new(),
        error: Some(format!(
            "{} is disabled. Enable and validate it in Settings before use.",
            settings.provider
        )),
    }
}

fn disabled_provider_snapshot_probe(provider: AgentHarnessKind) -> HarnessRuntimeProbe {
    provider_settings_snapshot_probe(&AgentProviderSettings::disabled_defaults(provider))
}

pub(crate) fn snapshot_probes_from_provider_settings(
    stored: &[AgentProviderSettings],
) -> HashMap<AgentHarnessKind, HarnessRuntimeProbe> {
    STANDARD_AGENT_HARNESSES
        .into_iter()
        .map(|provider| {
            let probe = stored
                .iter()
                .find(|row| row.provider == provider)
                .map(provider_settings_snapshot_probe)
                .unwrap_or_else(|| disabled_provider_snapshot_probe(provider));
            (provider, probe)
        })
        .collect()
}

async fn read_provider_settings(
    state: &AppState,
    refresh_runtime: bool,
    force_runtime: bool,
) -> Result<AgentProvidersSettingsResponse, String> {
    let started_at = std::time::Instant::now();
    let phase_started_at = std::time::Instant::now();
    let stored = state
        .agent_provider_settings_repo
        .list()
        .await
        .map_err(|err| err.to_string())?;
    tracing::info!(
        operation = "agent_provider_settings_phase",
        phase = "load_settings",
        refresh_runtime,
        force_runtime,
        provider_rows = stored.len(),
        elapsed_ms = phase_started_at.elapsed().as_millis() as u64,
        total_elapsed_ms = started_at.elapsed().as_millis() as u64,
        "Agent provider settings phase completed"
    );
    let phase_started_at = std::time::Instant::now();
    let mut probes = if refresh_runtime {
        refresh_supported_harnesses_with_force(force_runtime)
    } else {
        snapshot_probes_from_provider_settings(&stored)
    };
    tracing::info!(
        operation = "agent_provider_settings_phase",
        phase = "resolve_runtime_probes",
        refresh_runtime,
        force_runtime,
        probe_count = probes.len(),
        elapsed_ms = phase_started_at.elapsed().as_millis() as u64,
        total_elapsed_ms = started_at.elapsed().as_millis() as u64,
        "Agent provider settings phase completed"
    );
    let phase_started_at = std::time::Instant::now();
    if refresh_runtime {
        overlay_managed_provider_runtime_probes(&stored, &mut probes);
    }
    tracing::info!(
        operation = "agent_provider_settings_phase",
        phase = "overlay_managed_runtime_probes",
        refresh_runtime,
        force_runtime,
        elapsed_ms = phase_started_at.elapsed().as_millis() as u64,
        total_elapsed_ms = started_at.elapsed().as_millis() as u64,
        "Agent provider settings phase completed"
    );
    let phase_started_at = std::time::Instant::now();
    let response = read_provider_settings_with_stored_and_probes(stored, &probes).await?;
    tracing::info!(
        operation = "agent_provider_settings_phase",
        phase = "build_response",
        refresh_runtime,
        force_runtime,
        provider_rows = response.providers.len(),
        elapsed_ms = phase_started_at.elapsed().as_millis() as u64,
        total_elapsed_ms = started_at.elapsed().as_millis() as u64,
        "Agent provider settings phase completed"
    );
    tracing::info!(
        operation = "agent_provider_settings_phase",
        phase = "total",
        refresh_runtime,
        force_runtime,
        provider_rows = response.providers.len(),
        elapsed_ms = started_at.elapsed().as_millis() as u64,
        total_elapsed_ms = started_at.elapsed().as_millis() as u64,
        "Agent provider settings phase completed"
    );
    Ok(response)
}

fn overlay_managed_provider_runtime_probes(
    stored: &[AgentProviderSettings],
    probes: &mut HashMap<AgentHarnessKind, HarnessRuntimeProbe>,
) {
    for settings in stored {
        if let Some(probe) =
            crate::application::managed_provider_cli::provider_runtime_probe(settings)
        {
            probes.insert(settings.provider, probe);
        }
    }
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
    read_provider_settings_with_stored_and_probes(stored, probes).await
}

async fn read_provider_settings_with_stored_and_probes(
    stored: Vec<AgentProviderSettings>,
    probes: &HashMap<AgentHarnessKind, HarnessRuntimeProbe>,
) -> Result<AgentProvidersSettingsResponse, String> {
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
                    cli_version: None,
                    supported_model_aliases: None,
                    supported_efforts: None,
                    ultra_supported_models: Vec::new(),
                    supports_fast_mode: false,
                    fast_mode_supported_models: Vec::new(),
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
    input: Option<GetAgentProviderSettingsInput>,
    state: State<'_, AppState>,
) -> Result<AgentProvidersSettingsResponse, String> {
    let input = input.unwrap_or_default();
    read_provider_settings(&state, input.refresh_runtime, input.force_runtime).await
}

#[tauri::command]
pub async fn update_agent_provider_settings(
    input: UpdateAgentProviderSettingsInput,
    state: State<'_, AppState>,
) -> Result<AgentProvidersSettingsResponse, String> {
    let provider = parse_provider(&input.provider)?;
    let stored = state
        .agent_provider_settings_repo
        .list()
        .await
        .map_err(|err| err.to_string())?;
    let mut probes = snapshot_probes_from_provider_settings(&stored);
    let should_refresh_runtime_probe = input.enabled == Some(true)
        || (provider == AgentHarnessKind::Codex
            && (input.service_tier.is_some() || input.model.is_some()));
    if should_refresh_runtime_probe {
        let existing = stored
            .iter()
            .find(|row| row.provider == provider)
            .cloned()
            .unwrap_or_else(|| AgentProviderSettings::disabled_defaults(provider));
        let probe = if let Some(probe) =
            crate::application::managed_provider_cli::provider_runtime_probe(&existing)
        {
            probe
        } else {
            refresh_harness_runtime_probe_with_force(provider, true)
        };
        probes.insert(provider, probe);
    }
    update_provider_settings_with_probes(input, &state, &probes).await
}

async fn update_provider_settings_with_probes(
    input: UpdateAgentProviderSettingsInput,
    state: &AppState,
    probes: &HashMap<AgentHarnessKind, HarnessRuntimeProbe>,
) -> Result<AgentProvidersSettingsResponse, String> {
    let provider = parse_provider(&input.provider)?;
    let stored = state
        .agent_provider_settings_repo
        .list()
        .await
        .map_err(|err| err.to_string())?;
    let existing = stored
        .iter()
        .find(|row| row.provider == provider)
        .cloned()
        .unwrap_or_else(|| AgentProviderSettings::disabled_defaults(provider));
    let first_enabled_provider =
        input.enabled == Some(true) && stored.iter().all(|row| !row.enabled);
    let apply_to_all_lanes = input.apply_to_all_lanes || first_enabled_provider;
    let base_probe = probes.get(&provider).cloned();
    let candidate = merge_input(existing.clone(), input.clone(), true)?;
    let effective_probe =
        crate::application::managed_provider_cli::provider_runtime_probe(&candidate)
            .or(base_probe)
            .ok_or_else(|| format!("{provider} probe unavailable"))?;
    if candidate.custom_binary_enabled && !effective_probe.available {
        return Err(effective_probe
            .error
            .unwrap_or_else(|| format!("Custom {provider} binary is not available and ready")));
    }
    if provider == AgentHarnessKind::Codex
        && (input.service_tier.is_some() || input.model.is_some())
    {
        validate_codex_fast_mode_selection(&candidate, &effective_probe)?;
    }
    crate::application::provider_env_file::validate_provider_custom_env_file_settings(&candidate)?;
    if input.enabled == Some(true) && !effective_probe.available {
        return Err(effective_probe.error.unwrap_or_else(|| {
            format!("{provider} cannot be enabled until its CLI is available and ready")
        }));
    }
    let cli_source_changed = existing.cli_management_mode != candidate.cli_management_mode
        || existing.custom_binary_enabled != candidate.custom_binary_enabled
        || existing.custom_binary_path != candidate.custom_binary_path;
    let mut settings = merge_input(existing, input, effective_probe.available)?;
    if first_enabled_provider {
        settings.is_default = true;
    }
    let saved = state
        .agent_provider_settings_repo
        .upsert(&settings)
        .await
        .map_err(|err| err.to_string())?;

    if cli_source_changed {
        clear_harness_runtime_caches_for_harness(saved.provider);
    }

    if saved.provider == AgentHarnessKind::Claude {
        apply_claude_provider_permission_settings(&saved);
    }

    if saved.is_default && apply_to_all_lanes {
        apply_provider_to_global_lanes(state, &saved).await?;
    }

    let mut response_probes = probes.clone();
    if let Some(probe) = crate::application::managed_provider_cli::provider_runtime_probe(&saved) {
        response_probes.insert(saved.provider, probe);
    }

    read_provider_settings_with_probes(state, &response_probes).await
}

#[cfg(test)]
#[path = "harness_provider_commands_tests.rs"]
mod tests;
