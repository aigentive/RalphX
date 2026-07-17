use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::clickup_integration_service::ClickUpTaskListOptions;
use crate::application::ticket_git_strict_start::{
    preview_strict_clickup_ticket_branch_from_services, StrictTicketGitPreview,
};
use crate::application::{
    AppState, ClickUpIntegrationSettingsUpdate, ClickUpTaskSummary, ClickUpWorkspace,
};
use crate::domain::entities::ProjectId;
use crate::domain::integrations::ClickUpIntegrationSettings;

/// Connection/settings view for the ClickUp ticketing provider.
///
/// Mirrors `LinearIntegrationSettingsResponse`: the raw Personal API token is
/// never returned to the frontend — only `has_api_token` signals presence. The
/// selected workspace id is surfaced so the settings UI can show/confirm the
/// active ClickUp Workspace (Team).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClickUpIntegrationSettingsResponse {
    pub enabled: bool,
    pub has_api_token: bool,
    pub workspace_id: Option<String>,
    pub validation_status: String,
    pub task_search_available: bool,
    pub strict_git_naming_enabled: bool,
    pub branch_name_template: String,
    pub commit_subject_template: String,
    pub pr_title_template: String,
    pub last_validated_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub updated_at: DateTime<Utc>,
}

impl From<ClickUpIntegrationSettings> for ClickUpIntegrationSettingsResponse {
    fn from(settings: ClickUpIntegrationSettings) -> Self {
        Self {
            enabled: settings.enabled,
            has_api_token: settings.token_secret_ref.is_some(),
            workspace_id: settings.workspace_id,
            validation_status: settings.validation_status.as_str().to_string(),
            task_search_available: settings.task_search_available,
            strict_git_naming_enabled: settings.strict_git_naming_enabled,
            branch_name_template: settings.branch_name_template,
            commit_subject_template: settings.commit_subject_template,
            pr_title_template: settings.pr_title_template,
            last_validated_at: settings.last_validated_at,
            last_error: settings.last_error,
            updated_at: settings.updated_at,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveClickUpIntegrationSettingsInput {
    /// Tri-state: `None` leaves the stored token untouched, `Some("")` clears
    /// it, `Some(value)` replaces it. The raw token goes straight to the
    /// keychain via the service; it is never persisted in the DB row.
    pub api_token: Option<String>,
    /// Tri-state mirror for the selected Workspace (Team) id.
    pub workspace_id: Option<String>,
    /// `None` preserves the stored opt-in state.
    pub strict_git_naming_enabled: Option<bool>,
    /// `None` preserves the stored template; supplied values are validated.
    pub branch_name_template: Option<String>,
    pub commit_subject_template: Option<String>,
    pub pr_title_template: Option<String>,
}

impl From<SaveClickUpIntegrationSettingsInput> for ClickUpIntegrationSettingsUpdate {
    fn from(input: SaveClickUpIntegrationSettingsInput) -> Self {
        Self {
            api_token: input.api_token,
            workspace_id: input.workspace_id,
            strict_git_naming_enabled: input.strict_git_naming_enabled,
            branch_name_template: input.branch_name_template,
            commit_subject_template: input.commit_subject_template,
            pr_title_template: input.pr_title_template,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchClickUpTasksInput {
    /// ClickUp Space ids to scope the task search to within the selected
    /// workspace. Empty = let the workspace-scoped endpoint decide.
    #[serde(default)]
    pub space_ids: Vec<String>,
    /// Optional client text query. ClickUp's filtered task endpoint has no
    /// native free-text task-title search, so the client scans provider pages
    /// and stops once enough local matches are found.
    pub query: Option<String>,
    /// Maximum number of matching tasks to return.
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchClickUpTasksResponse {
    pub tasks: Vec<ClickUpTaskSummary>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListClickUpWorkspacesResponse {
    pub workspaces: Vec<ClickUpWorkspace>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewClickUpTicketGitConventionInput {
    pub project_id: String,
    pub task_id: String,
    pub target_base_ref: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewClickUpTicketGitConventionResponse {
    pub convention: Option<StrictTicketGitPreview>,
}

#[tauri::command]
pub async fn get_clickup_integration_settings(
    state: State<'_, AppState>,
) -> Result<ClickUpIntegrationSettingsResponse, String> {
    state
        .clickup_integration_service
        .get_settings()
        .await
        .map(ClickUpIntegrationSettingsResponse::from)
}

#[tauri::command]
pub async fn save_clickup_integration_settings(
    input: SaveClickUpIntegrationSettingsInput,
    state: State<'_, AppState>,
) -> Result<ClickUpIntegrationSettingsResponse, String> {
    state
        .clickup_integration_service
        .save_settings_with_git_convention(input.into())
        .await
        .map(ClickUpIntegrationSettingsResponse::from)
}

#[tauri::command]
pub async fn validate_clickup_integration(
    state: State<'_, AppState>,
) -> Result<ClickUpIntegrationSettingsResponse, String> {
    state
        .clickup_integration_service
        .validate_and_enable()
        .await
        .map(ClickUpIntegrationSettingsResponse::from)
}

#[tauri::command]
pub async fn disconnect_clickup_integration(
    state: State<'_, AppState>,
) -> Result<ClickUpIntegrationSettingsResponse, String> {
    state
        .clickup_integration_service
        .disconnect()
        .await
        .map(ClickUpIntegrationSettingsResponse::from)
}

/// Lists the ClickUp Workspaces (Teams) the saved token can see so the user can
/// pick one. Requires a validated/enabled connection (the service gates on it).
#[tauri::command]
pub async fn list_clickup_workspaces(
    state: State<'_, AppState>,
) -> Result<ListClickUpWorkspacesResponse, String> {
    let workspaces = state.clickup_integration_service.list_workspaces().await?;
    Ok(ListClickUpWorkspacesResponse { workspaces })
}

/// Lists tasks for the selected workspace, optionally scoped to specific Spaces.
#[tauri::command]
pub async fn search_clickup_tasks(
    input: SearchClickUpTasksInput,
    state: State<'_, AppState>,
) -> Result<SearchClickUpTasksResponse, String> {
    let limit = input.limit.unwrap_or(10).clamp(1, 25);
    let tasks = state
        .clickup_integration_service
        .list_tasks(
            input.space_ids,
            ClickUpTaskListOptions {
                query: input.query,
                limit: Some(limit),
                assignee_ids: Vec::new(),
            },
        )
        .await?;
    Ok(SearchClickUpTasksResponse { tasks })
}

/// Read-only strict ticket convention preflight. This never creates a binding,
/// branch, worktree, push, or cycle transition.
#[tauri::command]
pub async fn preview_clickup_ticket_git_convention(
    input: PreviewClickUpTicketGitConventionInput,
    state: State<'_, AppState>,
) -> Result<PreviewClickUpTicketGitConventionResponse, String> {
    let project_id = ProjectId::from_string(input.project_id);
    let project = state
        .project_repo
        .get_by_id(&project_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Project not found: {project_id}"))?;
    let task = state
        .clickup_integration_service
        .fetch_task(&input.task_id)
        .await?;
    let target_base_ref = input
        .target_base_ref
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or(project.base_branch)
        .unwrap_or_else(|| "main".to_string());
    let convention = preview_strict_clickup_ticket_branch_from_services(
        state.inner(),
        &project_id,
        &task,
        &target_base_ref,
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(PreviewClickUpTicketGitConventionResponse { convention })
}
