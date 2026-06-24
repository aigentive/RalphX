use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::AppState;
use crate::domain::integrations::GranolaIntegrationSettings;

/// Connection/settings view for the Granola integration.
///
/// The raw Granola API token is never returned to the frontend — only
/// `has_api_token` signals presence. The keychain reference is likewise withheld.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GranolaIntegrationSettingsResponse {
    pub enabled: bool,
    pub has_api_token: bool,
    pub validation_status: String,
    pub last_validated_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub updated_at: DateTime<Utc>,
}

impl From<GranolaIntegrationSettings> for GranolaIntegrationSettingsResponse {
    fn from(settings: GranolaIntegrationSettings) -> Self {
        Self {
            enabled: settings.enabled,
            has_api_token: settings.token_secret_ref.is_some(),
            validation_status: settings.validation_status.as_str().to_string(),
            last_validated_at: settings.last_validated_at,
            last_error: settings.last_error,
            updated_at: settings.updated_at,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveGranolaIntegrationSettingsInput {
    /// Tri-state: `None` leaves the stored token untouched, `Some("")` clears
    /// it, `Some(value)` replaces it. The raw token goes straight to the
    /// keychain via the service; it is never persisted in the DB row.
    pub api_token: Option<String>,
}

#[tauri::command]
pub async fn get_granola_integration_settings(
    state: State<'_, AppState>,
) -> Result<GranolaIntegrationSettingsResponse, String> {
    state
        .granola_integration_service
        .get_settings()
        .await
        .map(GranolaIntegrationSettingsResponse::from)
}

#[tauri::command]
pub async fn save_granola_integration_settings(
    input: SaveGranolaIntegrationSettingsInput,
    state: State<'_, AppState>,
) -> Result<GranolaIntegrationSettingsResponse, String> {
    state
        .granola_integration_service
        .save_settings(input.api_token)
        .await
        .map(GranolaIntegrationSettingsResponse::from)
}

#[tauri::command]
pub async fn validate_granola_integration_settings(
    state: State<'_, AppState>,
) -> Result<GranolaIntegrationSettingsResponse, String> {
    state
        .granola_integration_service
        .validate_and_enable()
        .await
        .map(GranolaIntegrationSettingsResponse::from)
}
