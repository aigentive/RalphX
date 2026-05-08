use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::AppState;
use crate::domain::agents::{
    AgentHarnessKind, AgentLane, AgentLaneSettings, LogicalEffort, StoredAgentLaneSettings,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentLaneSettingsResponse {
    pub project_id: Option<String>,
    pub lane: String,
    pub harness: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAgentLaneSettingsInput {
    pub project_id: Option<String>,
    pub lane: String,
    pub harness: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
}

fn parse_lane(value: &str) -> Result<AgentLane, String> {
    value
        .parse::<AgentLane>()
        .map_err(|err| format!("Invalid lane: {err}"))
}

fn parse_harness(value: &str) -> Result<AgentHarnessKind, String> {
    value
        .parse::<AgentHarnessKind>()
        .map_err(|err| format!("Invalid harness: {err}"))
}

fn parse_effort(value: Option<&str>) -> Result<Option<LogicalEffort>, String> {
    value
        .map(|effort| {
            effort
                .parse::<LogicalEffort>()
                .map_err(|err| format!("Invalid effort: {err}"))
        })
        .transpose()
}

fn to_response(row: StoredAgentLaneSettings) -> AgentLaneSettingsResponse {
    AgentLaneSettingsResponse {
        project_id: row.project_id,
        lane: row.lane.to_string(),
        harness: row.settings.harness.to_string(),
        model: row.settings.model,
        effort: row.settings.effort.map(|value| value.to_string()),
        approval_policy: row.settings.approval_policy,
        sandbox_mode: row.settings.sandbox_mode,
        updated_at: row.updated_at.to_rfc3339(),
    }
}

#[tauri::command]
pub async fn get_agent_lane_settings(
    project_id: Option<String>,
    app_state: State<'_, AppState>,
) -> Result<Vec<AgentLaneSettingsResponse>, String> {
    let rows = if let Some(project_id) = project_id {
        app_state
            .agent_lane_settings_repo
            .list_for_project(&project_id)
            .await
            .map_err(|e| format!("Failed to fetch project lane settings: {e}"))?
    } else {
        app_state
            .agent_lane_settings_repo
            .list_global()
            .await
            .map_err(|e| format!("Failed to fetch global lane settings: {e}"))?
    };

    Ok(rows.into_iter().map(to_response).collect())
}

#[tauri::command]
pub async fn update_agent_lane_settings(
    input: UpdateAgentLaneSettingsInput,
    app_state: State<'_, AppState>,
) -> Result<AgentLaneSettingsResponse, String> {
    let lane = parse_lane(&input.lane)?;
    let harness = parse_harness(&input.harness)?;
    let effort = parse_effort(input.effort.as_deref())?;

    let settings = AgentLaneSettings {
        harness,
        model: input.model,
        effort,
        approval_policy: input.approval_policy,
        sandbox_mode: input.sandbox_mode,
    };

    let row = if let Some(project_id) = input.project_id {
        app_state
            .agent_lane_settings_repo
            .upsert_for_project(&project_id, lane, &settings)
            .await
            .map_err(|e| format!("Failed to save project lane settings: {e}"))?
    } else {
        app_state
            .agent_lane_settings_repo
            .upsert_global(lane, &settings)
            .await
            .map_err(|e| format!("Failed to save global lane settings: {e}"))?
    };

    Ok(to_response(row))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_helpers_validate_expected_values() {
        assert_eq!(
            parse_lane("ideation_primary").unwrap(),
            AgentLane::IdeationPrimary
        );
        assert_eq!(parse_harness("codex").unwrap(), AgentHarnessKind::Codex);
        assert_eq!(
            parse_effort(Some("xhigh")).unwrap(),
            Some(LogicalEffort::XHigh)
        );
        assert_eq!(parse_effort(Some("max")).unwrap(), Some(LogicalEffort::Max));
    }

    #[test]
    fn parse_helpers_reject_invalid_values() {
        assert!(parse_lane("unknown").is_err());
        assert!(parse_harness("unknown").is_err());
        assert!(parse_effort(Some("turbo")).is_err());
    }
}
