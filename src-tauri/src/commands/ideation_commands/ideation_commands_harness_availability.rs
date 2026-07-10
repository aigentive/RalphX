use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::{
    build_lane_harness_availability, refreshed_provider_aware_runtime_probes,
    resolve_lane_harness_config, AppState, AGENT_LANES, IDEATION_LANES,
};
use crate::commands::harness_provider_commands::snapshot_probes_from_provider_settings;
use crate::domain::agents::AgentLane;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentHarnessAvailabilityInput {
    pub project_id: Option<String>,
    #[serde(default)]
    pub refresh_runtime: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentLaneHarnessAvailabilityResponse {
    pub project_id: Option<String>,
    pub lane: String,
    pub configured_harness: Option<String>,
    pub effective_harness: String,
    pub binary_path: Option<String>,
    pub binary_found: bool,
    pub probe_succeeded: bool,
    pub available: bool,
    pub missing_core_exec_features: Vec<String>,
    pub error: Option<String>,
}

pub type LaneHarnessAvailabilityResponse = AgentLaneHarnessAvailabilityResponse;
pub type IdeationLaneHarnessAvailabilityResponse = AgentLaneHarnessAvailabilityResponse;

fn to_response(
    project_id: &Option<String>,
    availability: crate::application::ideation_harness_availability::LaneHarnessAvailability,
) -> AgentLaneHarnessAvailabilityResponse {
    AgentLaneHarnessAvailabilityResponse {
        project_id: project_id.clone(),
        lane: availability.lane.to_string(),
        configured_harness: availability
            .configured_harness
            .map(|value| value.to_string()),
        effective_harness: availability.effective_harness.to_string(),
        binary_path: availability.binary_path,
        binary_found: availability.binary_found,
        probe_succeeded: availability.probe_succeeded,
        available: availability.available,
        missing_core_exec_features: availability.missing_core_exec_features,
        error: availability.error,
    }
}

async fn get_harness_availability_for_lanes(
    project_id: Option<String>,
    app_state: &AppState,
    lanes: &[AgentLane],
    refresh_runtime: bool,
) -> Result<Vec<AgentLaneHarnessAvailabilityResponse>, String> {
    let started_at = std::time::Instant::now();
    let probes = if refresh_runtime {
        refreshed_provider_aware_runtime_probes(app_state).await?
    } else {
        let stored = app_state
            .agent_provider_settings_repo
            .list()
            .await
            .map_err(|err| err.to_string())?;
        snapshot_probes_from_provider_settings(&stored)
    };
    let mut responses = Vec::with_capacity(lanes.len());

    for lane in lanes {
        let config = resolve_lane_harness_config(
            &app_state.agent_lane_settings_repo,
            project_id.as_deref(),
            *lane,
        )
        .await;
        let availability = build_lane_harness_availability(config, &probes);
        responses.push(to_response(&project_id, availability));
    }

    tracing::info!(
        refresh_runtime,
        lanes = lanes.len(),
        project_id = ?project_id,
        elapsed_ms = started_at.elapsed().as_millis() as u64,
        "Agent harness availability loaded"
    );
    Ok(responses)
}

#[tauri::command]
pub async fn get_ideation_harness_availability(
    input: Option<AgentHarnessAvailabilityInput>,
    app_state: State<'_, AppState>,
) -> Result<Vec<IdeationLaneHarnessAvailabilityResponse>, String> {
    let input = input.unwrap_or_default();
    get_harness_availability_for_lanes(
        input.project_id,
        app_state.inner(),
        &IDEATION_LANES,
        input.refresh_runtime,
    )
    .await
}

#[tauri::command]
pub async fn get_agent_harness_availability(
    input: Option<AgentHarnessAvailabilityInput>,
    app_state: State<'_, AppState>,
) -> Result<Vec<AgentLaneHarnessAvailabilityResponse>, String> {
    let input = input.unwrap_or_default();
    get_harness_availability_for_lanes(
        input.project_id,
        app_state.inner(),
        &AGENT_LANES,
        input.refresh_runtime,
    )
    .await
}

#[cfg(test)]
#[path = "ideation_commands_harness_availability_tests.rs"]
mod tests;
