use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::AppState;
use crate::domain::agents::{
    AgentHarnessKind, LogicalEffort, StoredWorkspaceReviewRuntimeSettings,
    WorkspaceReviewRuntimeSettings,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceReviewRuntimeSettingsResponse {
    pub project_id: Option<String>,
    pub provider: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateWorkspaceReviewRuntimeSettingsInput {
    pub project_id: Option<String>,
    pub provider: String,
    pub model: Option<String>,
    pub effort: Option<String>,
}

fn parse_provider(value: &str) -> Result<AgentHarnessKind, String> {
    value
        .parse::<AgentHarnessKind>()
        .map_err(|err| format!("Invalid provider: {err}"))
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

fn to_response(
    row: StoredWorkspaceReviewRuntimeSettings,
) -> WorkspaceReviewRuntimeSettingsResponse {
    WorkspaceReviewRuntimeSettingsResponse {
        project_id: row.project_id,
        provider: row.provider.to_string(),
        model: row.settings.model,
        effort: row.settings.effort.map(|value| value.to_string()),
        updated_at: row.updated_at.to_rfc3339(),
    }
}

#[tauri::command]
pub async fn get_workspace_review_runtime_settings(
    project_id: Option<String>,
    app_state: State<'_, AppState>,
) -> Result<Vec<WorkspaceReviewRuntimeSettingsResponse>, String> {
    let rows = if let Some(project_id) = project_id {
        app_state
            .workspace_review_runtime_settings_repo
            .list_for_project(&project_id)
            .await
            .map_err(|e| format!("Failed to fetch project Workspace Review settings: {e}"))?
    } else {
        app_state
            .workspace_review_runtime_settings_repo
            .list_global()
            .await
            .map_err(|e| format!("Failed to fetch global Workspace Review settings: {e}"))?
    };

    Ok(rows.into_iter().map(to_response).collect())
}

#[tauri::command]
pub async fn update_workspace_review_runtime_settings(
    input: UpdateWorkspaceReviewRuntimeSettingsInput,
    app_state: State<'_, AppState>,
) -> Result<WorkspaceReviewRuntimeSettingsResponse, String> {
    let provider = parse_provider(&input.provider)?;
    let effort = parse_effort(input.effort.as_deref())?;
    let settings = WorkspaceReviewRuntimeSettings {
        model: input.model,
        effort,
    };

    let row = if let Some(project_id) = input.project_id {
        app_state
            .workspace_review_runtime_settings_repo
            .upsert_for_project(&project_id, provider, &settings)
            .await
            .map_err(|e| format!("Failed to save project Workspace Review settings: {e}"))?
    } else {
        app_state
            .workspace_review_runtime_settings_repo
            .upsert_global(provider, &settings)
            .await
            .map_err(|e| format!("Failed to save global Workspace Review settings: {e}"))?
    };

    Ok(to_response(row))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_helpers_validate_expected_values() {
        assert_eq!(parse_provider("codex").unwrap(), AgentHarnessKind::Codex);
        assert_eq!(
            parse_effort(Some("medium")).unwrap(),
            Some(LogicalEffort::Medium)
        );
    }

    #[test]
    fn parse_helpers_reject_invalid_values() {
        assert!(parse_provider("gemini").is_err());
        assert!(parse_effort(Some("turbo")).is_err());
    }
}
