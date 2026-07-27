use crate::application::harness_runtime_registry::{
    probe_supported_harnesses, refresh_supported_harnesses, HarnessRuntimeProbe,
};
use crate::application::AppState;
use crate::domain::entities::{ChatContextType, IdeationSessionId, TaskId};
use std::collections::HashMap;
use std::sync::Arc;

use crate::domain::agents::{
    AgentHarnessKind, AgentLane, AgentProviderSettings, StoredAgentLaneSettings,
    DEFAULT_AGENT_HARNESS,
};
use crate::domain::repositories::{AgentLaneSettingsRepository, AgentProviderSettingsRepository};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedLaneHarnessConfig {
    pub lane: AgentLane,
    pub configured_harness: Option<AgentHarnessKind>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LaneHarnessAvailability {
    pub lane: AgentLane,
    pub configured_harness: Option<AgentHarnessKind>,
    pub effective_harness: AgentHarnessKind,
    pub binary_path: Option<String>,
    pub binary_found: bool,
    pub probe_succeeded: bool,
    pub available: bool,
    pub missing_core_exec_features: Vec<String>,
    pub error: Option<String>,
}

pub(crate) const IDEATION_LANES: [AgentLane; 4] = [
    AgentLane::IdeationPrimary,
    AgentLane::IdeationSubagent,
    AgentLane::IdeationVerifier,
    AgentLane::IdeationVerifierSubagent,
];

pub(crate) const AGENT_LANES: [AgentLane; 9] = [
    AgentLane::IdeationPrimary,
    AgentLane::IdeationSubagent,
    AgentLane::IdeationVerifier,
    AgentLane::IdeationVerifierSubagent,
    AgentLane::ExecutionWorker,
    AgentLane::ExecutionReviewer,
    AgentLane::ExecutionReexecutor,
    AgentLane::ExecutionMerger,
    AgentLane::ExecutionBranchUpdater,
];

pub(crate) fn overlay_provider_runtime_probes(
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

async fn overlay_provider_runtime_probes_from_repo(
    repo: &Arc<dyn AgentProviderSettingsRepository>,
    probes: &mut HashMap<AgentHarnessKind, HarnessRuntimeProbe>,
) -> Result<(), String> {
    let stored = repo
        .list()
        .await
        .map_err(|error| format!("Failed to read provider settings: {error}"))?;
    overlay_provider_runtime_probes(&stored, probes);
    Ok(())
}

async fn overlay_provider_runtime_probes_from_state(
    state: &AppState,
    probes: &mut HashMap<AgentHarnessKind, HarnessRuntimeProbe>,
) -> Result<(), String> {
    overlay_provider_runtime_probes_from_repo(&state.agent_provider_settings_repo, probes).await
}

pub(crate) async fn provider_aware_runtime_probes_for_repo(
    repo: &Arc<dyn AgentProviderSettingsRepository>,
) -> Result<HashMap<AgentHarnessKind, HarnessRuntimeProbe>, String> {
    let mut probes = probe_supported_harnesses();
    overlay_provider_runtime_probes_from_repo(repo, &mut probes).await?;
    Ok(probes)
}

pub(crate) async fn provider_aware_runtime_probes(
    state: &AppState,
) -> Result<HashMap<AgentHarnessKind, HarnessRuntimeProbe>, String> {
    provider_aware_runtime_probes_for_repo(&state.agent_provider_settings_repo).await
}

pub(crate) async fn refreshed_provider_aware_runtime_probes(
    state: &AppState,
) -> Result<HashMap<AgentHarnessKind, HarnessRuntimeProbe>, String> {
    let mut probes = refresh_supported_harnesses();
    overlay_provider_runtime_probes_from_state(state, &mut probes).await?;
    Ok(probes)
}

pub(crate) async fn resolve_primary_ideation_harness_availability_for_state(
    state: &AppState,
    project_id: Option<&str>,
) -> Result<LaneHarnessAvailability, String> {
    let probes = provider_aware_runtime_probes(state).await?;
    let config = resolve_lane_harness_config(
        &state.agent_lane_settings_repo,
        project_id,
        AgentLane::IdeationPrimary,
    )
    .await;
    Ok(build_lane_harness_availability(config, &probes))
}

fn format_harness_runtime_unavailable(surface_name: &str, harness: AgentHarnessKind) -> String {
    format!(
        "{surface_name} requires the {} harness runtime but it is not available",
        harness
    )
}

#[cfg(test)]
pub(crate) fn validate_claude_runtime_path(
    availability: &LaneHarnessAvailability,
    surface_name: &str,
) -> Result<(), String> {
    if !availability.available {
        return Err(availability
            .error
            .clone()
            .unwrap_or_else(|| "Configured ideation harness is not available".to_string()));
    }

    if availability.effective_harness != AgentHarnessKind::Claude {
        return Err(format!(
            "Ideation primary lane resolves to {} but {} still routes through the Claude runtime",
            availability.effective_harness, surface_name
        ));
    }

    Ok(())
}

pub(crate) async fn validate_chat_runtime_for_context(
    state: &AppState,
    context_type: ChatContextType,
    context_id: &str,
    surface_name: &str,
) -> Result<(), String> {
    validate_chat_runtime_for_context_with_override(
        state,
        context_type,
        context_id,
        surface_name,
        None,
    )
    .await
    .map(|_| ())
}

pub(crate) async fn validate_chat_runtime_for_context_with_override(
    state: &AppState,
    context_type: ChatContextType,
    context_id: &str,
    surface_name: &str,
    harness_override: Option<AgentHarnessKind>,
) -> Result<AgentHarnessKind, String> {
    crate::application::resolve_enabled_default_provider(
        &state.agent_provider_settings_repo,
        surface_name,
    )
    .await?;

    let availability =
        resolve_context_runtime_availability(state, context_type, context_id, harness_override)
            .await?;

    if availability.available {
        crate::application::ensure_provider_spawn_enabled(
            &state.agent_provider_settings_repo,
            availability.effective_harness,
            surface_name,
        )
        .await?;
        Ok(availability.effective_harness)
    } else {
        let error = availability.error.clone().unwrap_or_else(|| {
            format_harness_runtime_unavailable(surface_name, availability.effective_harness)
        });
        tracing::warn!(
            %surface_name,
            context_type = %context_type,
            context_id = %context_id,
            harness = %availability.effective_harness,
            binary_path = ?availability.binary_path,
            probe_succeeded = availability.probe_succeeded,
            missing_core_exec_features = ?availability.missing_core_exec_features,
            error = %error,
            "Chat runtime unavailable"
        );
        Err(error)
    }
}

async fn resolve_context_runtime_availability(
    state: &AppState,
    context_type: ChatContextType,
    context_id: &str,
    harness_override: Option<AgentHarnessKind>,
) -> Result<LaneHarnessAvailability, String> {
    let probes = provider_aware_runtime_probes(state).await?;

    if let Some(harness) = harness_override {
        return Ok(build_harness_override_availability(
            context_type,
            harness,
            &probes,
        ));
    }

    let Some(lane) = runtime_lane_for_context(context_type) else {
        return Ok(build_default_harness_availability(state, &probes).await);
    };

    let project_id = project_id_for_context(state, context_type, context_id).await;
    let config =
        resolve_lane_harness_config(&state.agent_lane_settings_repo, project_id.as_deref(), lane)
            .await;
    Ok(build_lane_harness_availability(config, &probes))
}

pub(crate) fn build_harness_override_availability(
    context_type: ChatContextType,
    harness: AgentHarnessKind,
    probes: &HashMap<AgentHarnessKind, HarnessRuntimeProbe>,
) -> LaneHarnessAvailability {
    let lane = runtime_lane_for_context(context_type).unwrap_or(AgentLane::IdeationPrimary);
    build_lane_harness_availability(
        ResolvedLaneHarnessConfig {
            lane,
            configured_harness: Some(harness),
        },
        probes,
    )
}

async fn build_default_harness_availability(
    state: &AppState,
    probes: &HashMap<AgentHarnessKind, HarnessRuntimeProbe>,
) -> LaneHarnessAvailability {
    let harness = crate::application::resolve_enabled_default_provider(
        &state.agent_provider_settings_repo,
        "default chat runtime",
    )
    .await
    .map(|settings| settings.provider)
    .unwrap_or(DEFAULT_AGENT_HARNESS);
    let probe = probe_for_harness(probes, harness);
    LaneHarnessAvailability {
        lane: AgentLane::IdeationPrimary,
        configured_harness: None,
        effective_harness: harness,
        binary_path: probe.binary_path.clone(),
        binary_found: probe.binary_found,
        probe_succeeded: probe.probe_succeeded,
        available: probe.available,
        missing_core_exec_features: probe.missing_core_exec_features.clone(),
        error: probe.error.clone(),
    }
}

fn runtime_lane_for_context(context_type: ChatContextType) -> Option<AgentLane> {
    match context_type {
        ChatContextType::Ideation => Some(AgentLane::IdeationPrimary),
        ChatContextType::TaskExecution => Some(AgentLane::ExecutionWorker),
        ChatContextType::Review => Some(AgentLane::ExecutionReviewer),
        ChatContextType::Merge => Some(AgentLane::ExecutionMerger),
        ChatContextType::BranchUpdate => Some(AgentLane::ExecutionBranchUpdater),
        ChatContextType::Delegation
        | ChatContextType::Task
        | ChatContextType::Project
        | ChatContextType::Standalone => None,
    }
}

async fn project_id_for_context(
    state: &AppState,
    context_type: ChatContextType,
    context_id: &str,
) -> Option<String> {
    match context_type {
        ChatContextType::Ideation => state
            .ideation_session_repo
            .get_by_id(&IdeationSessionId::from_string(context_id))
            .await
            .ok()
            .flatten()
            .map(|session| session.project_id.as_str().to_string()),
        ChatContextType::Delegation => state
            .delegated_session_repo
            .get_by_id(&crate::domain::entities::DelegatedSessionId::from_string(
                context_id,
            ))
            .await
            .ok()
            .flatten()
            .map(|session| session.project_id.as_str().to_string()),
        ChatContextType::TaskExecution
        | ChatContextType::Review
        | ChatContextType::Merge
        | ChatContextType::BranchUpdate => state
            .task_repo
            .get_by_id(&TaskId::from_string(context_id.to_string()))
            .await
            .ok()
            .flatten()
            .map(|task| task.project_id.as_str().to_string()),
        ChatContextType::Task | ChatContextType::Project | ChatContextType::Standalone => None,
    }
}

pub(crate) async fn resolve_lane_harness_config(
    repo: &Arc<dyn AgentLaneSettingsRepository>,
    project_id: Option<&str>,
    lane: AgentLane,
) -> ResolvedLaneHarnessConfig {
    let project_row = if let Some(project_id) = project_id {
        repo.get_for_project(project_id, lane)
            .await
            .inspect_err(|error| {
                tracing::warn!(
                    %project_id,
                    lane = %lane,
                    %error,
                    "Failed to fetch project-scoped agent lane settings for harness availability"
                );
            })
            .ok()
            .flatten()
    } else {
        None
    };

    let global_row = repo
        .get_global(lane)
        .await
        .inspect_err(|error| {
            tracing::warn!(
                lane = %lane,
                %error,
                "Failed to fetch global agent lane settings for harness availability"
            );
        })
        .ok()
        .flatten();

    ResolvedLaneHarnessConfig {
        lane,
        configured_harness: lane_harness(project_row.as_ref(), global_row.as_ref()),
    }
}

fn missing_harness_probe(harness: AgentHarnessKind) -> HarnessRuntimeProbe {
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
        error: Some(format!("No harness probe registered for {}", harness)),
    }
}

fn probe_for_harness(
    probes: &HashMap<AgentHarnessKind, HarnessRuntimeProbe>,
    harness: AgentHarnessKind,
) -> HarnessRuntimeProbe {
    probes
        .get(&harness)
        .cloned()
        .unwrap_or_else(|| missing_harness_probe(harness))
}

pub(crate) fn build_lane_harness_availability(
    config: ResolvedLaneHarnessConfig,
    probes: &HashMap<AgentHarnessKind, HarnessRuntimeProbe>,
) -> LaneHarnessAvailability {
    let configured_harness = config.configured_harness.unwrap_or(DEFAULT_AGENT_HARNESS);
    let configured_probe = probe_for_harness(probes, configured_harness);

    if configured_probe.available {
        return LaneHarnessAvailability {
            lane: config.lane,
            configured_harness: config.configured_harness,
            effective_harness: configured_harness,
            binary_path: configured_probe.binary_path.clone(),
            binary_found: configured_probe.binary_found,
            probe_succeeded: configured_probe.probe_succeeded,
            available: true,
            missing_core_exec_features: configured_probe.missing_core_exec_features.clone(),
            error: configured_probe.error.clone(),
        };
    }

    LaneHarnessAvailability {
        lane: config.lane,
        configured_harness: config.configured_harness,
        effective_harness: configured_harness,
        binary_path: configured_probe.binary_path.clone(),
        binary_found: configured_probe.binary_found,
        probe_succeeded: configured_probe.probe_succeeded,
        available: false,
        missing_core_exec_features: configured_probe.missing_core_exec_features.clone(),
        error: configured_probe.error.clone(),
    }
}

fn lane_harness(
    project_row: Option<&StoredAgentLaneSettings>,
    global_row: Option<&StoredAgentLaneSettings>,
) -> Option<AgentHarnessKind> {
    project_row
        .map(|row| row.settings.harness)
        .or_else(|| global_row.map(|row| row.settings.harness))
}
