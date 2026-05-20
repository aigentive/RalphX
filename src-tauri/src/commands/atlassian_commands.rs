use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::harness_runtime_registry::default_ui_feature_flags;
use crate::application::{
    AppState, AtlassianOAuthAuthorization, AtlassianResourceKind, AtlassianResourceSummary,
};
use crate::domain::integrations::{AtlassianAuthMethod, AtlassianIntegrationSettings};

const ATLASSIAN_OAUTH_DISABLED_MESSAGE: &str =
    "Atlassian OAuth setup is disabled. Use API token setup for now.";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AtlassianIntegrationSettingsResponse {
    pub enabled: bool,
    pub auth_method: String,
    pub site_url: Option<String>,
    pub email: Option<String>,
    pub has_api_token: bool,
    pub oauth_client_id: Option<String>,
    pub oauth_redirect_uri: Option<String>,
    pub has_oauth_client_secret: bool,
    pub has_oauth_token: bool,
    pub oauth_cloud_id: Option<String>,
    pub oauth_scopes: Option<String>,
    pub validation_status: String,
    pub jira_available: bool,
    pub confluence_available: bool,
    pub last_validated_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub updated_at: DateTime<Utc>,
}

impl From<AtlassianIntegrationSettings> for AtlassianIntegrationSettingsResponse {
    fn from(settings: AtlassianIntegrationSettings) -> Self {
        Self {
            enabled: settings.enabled,
            auth_method: settings.auth_method.as_str().to_string(),
            site_url: settings.site_url,
            email: settings.email,
            has_api_token: settings.token_secret_ref.is_some(),
            oauth_client_id: settings.oauth_client_id,
            oauth_redirect_uri: settings.oauth_redirect_uri,
            has_oauth_client_secret: settings.oauth_client_secret_ref.is_some(),
            has_oauth_token: settings.oauth_access_token_ref.is_some(),
            oauth_cloud_id: settings.oauth_cloud_id,
            oauth_scopes: settings.oauth_scopes,
            validation_status: settings.validation_status.as_str().to_string(),
            jira_available: settings.jira_available,
            confluence_available: settings.confluence_available,
            last_validated_at: settings.last_validated_at,
            last_error: settings.last_error,
            updated_at: settings.updated_at,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveAtlassianIntegrationSettingsInput {
    pub auth_method: Option<String>,
    pub site_url: Option<String>,
    pub email: Option<String>,
    pub api_token: Option<String>,
    pub oauth_client_id: Option<String>,
    pub oauth_client_secret: Option<String>,
    pub oauth_redirect_uri: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExchangeAtlassianOAuthCodeInput {
    pub authorization_code: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompleteAtlassianOAuthLocalCallbackInput {
    pub state: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchAtlassianResourcesInput {
    pub kind: String,
    pub query: String,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchAtlassianResourcesResponse {
    pub resources: Vec<AtlassianResourceSummary>,
}

fn atlassian_oauth_enabled() -> bool {
    default_ui_feature_flags().atlassian_oauth
}

fn ensure_atlassian_oauth_enabled() -> Result<(), String> {
    if atlassian_oauth_enabled() {
        Ok(())
    } else {
        Err(ATLASSIAN_OAUTH_DISABLED_MESSAGE.to_string())
    }
}

fn save_input_requests_oauth(input: &SaveAtlassianIntegrationSettingsInput) -> bool {
    matches!(input.auth_method.as_deref(), Some("oauth"))
        || input
            .oauth_client_id
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        || input
            .oauth_client_secret
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        || input
            .oauth_redirect_uri
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
}

#[tauri::command]
pub async fn get_atlassian_integration_settings(
    state: State<'_, AppState>,
) -> Result<AtlassianIntegrationSettingsResponse, String> {
    state
        .atlassian_integration_service
        .get_settings()
        .await
        .map(AtlassianIntegrationSettingsResponse::from)
}

#[tauri::command]
pub async fn save_atlassian_integration_settings(
    input: SaveAtlassianIntegrationSettingsInput,
    state: State<'_, AppState>,
) -> Result<AtlassianIntegrationSettingsResponse, String> {
    if save_input_requests_oauth(&input) {
        ensure_atlassian_oauth_enabled()?;
    }
    let auth_method = input
        .auth_method
        .as_deref()
        .map(str::parse::<AtlassianAuthMethod>)
        .transpose()?;
    state
        .atlassian_integration_service
        .save_settings(
            auth_method,
            input.site_url,
            input.email,
            input.api_token,
            input.oauth_client_id,
            input.oauth_client_secret,
            input.oauth_redirect_uri,
        )
        .await
        .map(AtlassianIntegrationSettingsResponse::from)
}

#[tauri::command]
pub async fn build_atlassian_oauth_authorization_url(
    state: State<'_, AppState>,
) -> Result<AtlassianOAuthAuthorization, String> {
    ensure_atlassian_oauth_enabled()?;
    state
        .atlassian_integration_service
        .build_oauth_authorization()
        .await
}

#[tauri::command]
pub async fn start_atlassian_oauth_local_callback(
    state: State<'_, AppState>,
) -> Result<AtlassianOAuthAuthorization, String> {
    ensure_atlassian_oauth_enabled()?;
    state
        .atlassian_integration_service
        .start_oauth_local_callback()
        .await
}

#[tauri::command]
pub async fn complete_atlassian_oauth_local_callback(
    input: CompleteAtlassianOAuthLocalCallbackInput,
    state: State<'_, AppState>,
) -> Result<AtlassianIntegrationSettingsResponse, String> {
    ensure_atlassian_oauth_enabled()?;
    state
        .atlassian_integration_service
        .complete_oauth_local_callback(input.state)
        .await
        .map(AtlassianIntegrationSettingsResponse::from)
}

#[tauri::command]
pub async fn exchange_atlassian_oauth_code(
    input: ExchangeAtlassianOAuthCodeInput,
    state: State<'_, AppState>,
) -> Result<AtlassianIntegrationSettingsResponse, String> {
    ensure_atlassian_oauth_enabled()?;
    state
        .atlassian_integration_service
        .exchange_oauth_code(input.authorization_code)
        .await
        .map(AtlassianIntegrationSettingsResponse::from)
}

#[tauri::command]
pub async fn validate_atlassian_integration(
    state: State<'_, AppState>,
) -> Result<AtlassianIntegrationSettingsResponse, String> {
    let settings = state.atlassian_integration_service.get_settings().await?;
    if settings.auth_method == AtlassianAuthMethod::OAuth {
        ensure_atlassian_oauth_enabled()?;
    }
    state
        .atlassian_integration_service
        .validate_and_enable()
        .await
        .map(AtlassianIntegrationSettingsResponse::from)
}

#[tauri::command]
pub async fn search_atlassian_resources(
    input: SearchAtlassianResourcesInput,
    state: State<'_, AppState>,
) -> Result<SearchAtlassianResourcesResponse, String> {
    let settings = state.atlassian_integration_service.get_settings().await?;
    if settings.auth_method == AtlassianAuthMethod::OAuth {
        ensure_atlassian_oauth_enabled()?;
    }
    let kind = input.kind.parse::<AtlassianResourceKind>()?;
    let query = input.query.trim();
    if query.is_empty() {
        return Ok(SearchAtlassianResourcesResponse {
            resources: Vec::new(),
        });
    }
    let resources = state
        .atlassian_integration_service
        .search_resources(kind, query, input.limit.unwrap_or(10))
        .await?;
    Ok(SearchAtlassianResourcesResponse { resources })
}
