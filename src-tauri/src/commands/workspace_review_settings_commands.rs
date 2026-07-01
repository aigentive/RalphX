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
    use tauri::Manager;

    fn test_app() -> tauri::App<tauri::test::MockRuntime> {
        tauri::test::mock_builder()
            .manage(AppState::new_test())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app should build")
    }

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

    #[tokio::test]
    async fn commands_round_trip_global_and_project_runtime_settings() {
        let app = test_app();

        let empty_global = get_workspace_review_runtime_settings(None, app.state::<AppState>())
            .await
            .expect("global settings should load");
        assert!(empty_global.is_empty());

        let saved_global = update_workspace_review_runtime_settings(
            UpdateWorkspaceReviewRuntimeSettingsInput {
                project_id: None,
                provider: "codex".to_string(),
                model: Some("gpt-5.4".to_string()),
                effort: Some("high".to_string()),
            },
            app.state::<AppState>(),
        )
        .await
        .expect("global settings should save");
        assert_eq!(saved_global.project_id, None);
        assert_eq!(saved_global.provider, "codex");
        assert_eq!(saved_global.model.as_deref(), Some("gpt-5.4"));
        assert_eq!(saved_global.effort.as_deref(), Some("high"));
        assert!(!saved_global.updated_at.is_empty());

        let global_rows = get_workspace_review_runtime_settings(None, app.state::<AppState>())
            .await
            .expect("saved global settings should load");
        assert_eq!(global_rows.len(), 1);
        assert_eq!(global_rows[0].provider, "codex");

        let saved_project = update_workspace_review_runtime_settings(
            UpdateWorkspaceReviewRuntimeSettingsInput {
                project_id: Some("project-1".to_string()),
                provider: "claude".to_string(),
                model: None,
                effort: Some("medium".to_string()),
            },
            app.state::<AppState>(),
        )
        .await
        .expect("project settings should save");
        assert_eq!(saved_project.project_id.as_deref(), Some("project-1"));
        assert_eq!(saved_project.provider, "claude");
        assert_eq!(saved_project.model, None);
        assert_eq!(saved_project.effort.as_deref(), Some("medium"));

        let project_rows = get_workspace_review_runtime_settings(
            Some("project-1".to_string()),
            app.state::<AppState>(),
        )
        .await
        .expect("saved project settings should load");
        assert_eq!(project_rows.len(), 1);
        assert_eq!(project_rows[0].project_id.as_deref(), Some("project-1"));
    }

    #[tokio::test]
    async fn update_command_rejects_invalid_runtime_values() {
        let app = test_app();

        let provider_error = update_workspace_review_runtime_settings(
            UpdateWorkspaceReviewRuntimeSettingsInput {
                project_id: None,
                provider: "gemini".to_string(),
                model: None,
                effort: None,
            },
            app.state::<AppState>(),
        )
        .await
        .expect_err("invalid provider should fail");
        assert!(provider_error.contains("Invalid provider"));

        let effort_error = update_workspace_review_runtime_settings(
            UpdateWorkspaceReviewRuntimeSettingsInput {
                project_id: None,
                provider: "codex".to_string(),
                model: None,
                effort: Some("turbo".to_string()),
            },
            app.state::<AppState>(),
        )
        .await
        .expect_err("invalid effort should fail");
        assert!(effort_error.contains("Invalid effort"));
    }
}
