use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::{
    build_lane_harness_availability, probe_supported_harnesses, resolve_lane_harness_config,
    AppState, AGENT_LANES, IDEATION_LANES,
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
        probe_supported_harnesses()
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
mod tests {
    use super::*;

    use crate::application::AppState;
    use crate::domain::agents::AgentHarnessKind;

    #[tokio::test]
    async fn availability_helper_loads_stored_provider_probes_for_requested_lanes() {
        let state = AppState::new_test();
        let project_id = Some("project-availability".to_string());

        let responses =
            get_harness_availability_for_lanes(project_id.clone(), &state, &IDEATION_LANES, false)
                .await
                .expect("availability should load");

        assert_eq!(responses.len(), IDEATION_LANES.len());
        assert!(responses
            .iter()
            .all(|response| response.project_id == project_id));
        assert!(responses
            .iter()
            .all(|response| !response.effective_harness.is_empty()));
    }

    #[test]
    fn availability_response_maps_lane_probe_and_error_fields() {
        let availability =
            crate::application::ideation_harness_availability::LaneHarnessAvailability {
                lane: AgentLane::ExecutionWorker,
                configured_harness: Some(AgentHarnessKind::Codex),
                effective_harness: AgentHarnessKind::Codex,
                binary_path: Some("/usr/local/bin/codex".to_string()),
                binary_found: true,
                probe_succeeded: false,
                available: false,
                missing_core_exec_features: vec!["exec".to_string()],
                error: Some("codex missing exec support".to_string()),
            };

        let response = to_response(&Some("project-1".to_string()), availability);

        assert_eq!(response.project_id.as_deref(), Some("project-1"));
        assert_eq!(response.lane, "execution_worker");
        assert_eq!(response.configured_harness.as_deref(), Some("codex"));
        assert_eq!(response.effective_harness, "codex");
        assert_eq!(
            response.binary_path.as_deref(),
            Some("/usr/local/bin/codex")
        );
        assert!(response.binary_found);
        assert!(!response.probe_succeeded);
        assert!(!response.available);
        assert_eq!(
            response.missing_core_exec_features,
            vec!["exec".to_string()]
        );
        assert_eq!(
            response.error.as_deref(),
            Some("codex missing exec support")
        );
    }
}
