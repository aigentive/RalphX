use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::{AppState, LinearIntegrationSettings, LinearIssueSummary};
use crate::domain::services::SecretStore;
use crate::infrastructure::secret_store::MacosKeychainSecretStore;
use crate::infrastructure::sqlite::SqliteLinearWebhookStore;

const LINEAR_WEBHOOK_SIGNING_SECRET_REF: &str =
    "integrations/linear/default/webhook-signing-secret";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinearWebhookConfigResponse {
    pub enabled: bool,
    pub has_signing_secret: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveLinearWebhookSigningSecretInput {
    pub signing_secret: String,
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinearIntegrationSettingsResponse {
    pub enabled: bool,
    pub has_api_token: bool,
    pub validation_status: String,
    pub issue_search_available: bool,
    pub last_validated_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub updated_at: DateTime<Utc>,
}

impl From<LinearIntegrationSettings> for LinearIntegrationSettingsResponse {
    fn from(settings: LinearIntegrationSettings) -> Self {
        Self {
            enabled: settings.enabled,
            has_api_token: settings.token_secret_ref.is_some(),
            validation_status: settings.validation_status.as_str().to_string(),
            issue_search_available: settings.issue_search_available,
            last_validated_at: settings.last_validated_at,
            last_error: settings.last_error,
            updated_at: settings.updated_at,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveLinearIntegrationSettingsInput {
    pub api_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchLinearIssuesInput {
    pub query: String,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchLinearIssuesResponse {
    pub issues: Vec<LinearIssueSummary>,
}

#[tauri::command]
pub async fn get_linear_webhook_config(
    state: State<'_, AppState>,
) -> Result<LinearWebhookConfigResponse, String> {
    let store = SqliteLinearWebhookStore::new(state.db.clone());
    let (enabled, signing_secret_ref) = store
        .get_config()
        .await
        .map_err(|error| error.to_string())?;

    Ok(LinearWebhookConfigResponse {
        enabled,
        has_signing_secret: signing_secret_ref.is_some(),
    })
}

#[tauri::command]
pub async fn get_linear_integration_settings(
    state: State<'_, AppState>,
) -> Result<LinearIntegrationSettingsResponse, String> {
    state
        .linear_integration_service
        .get_settings()
        .await
        .map(LinearIntegrationSettingsResponse::from)
}

#[tauri::command]
pub async fn save_linear_integration_settings(
    input: SaveLinearIntegrationSettingsInput,
    state: State<'_, AppState>,
) -> Result<LinearIntegrationSettingsResponse, String> {
    state
        .linear_integration_service
        .save_settings(input.api_token)
        .await
        .map(LinearIntegrationSettingsResponse::from)
}

#[tauri::command]
pub async fn validate_linear_integration(
    state: State<'_, AppState>,
) -> Result<LinearIntegrationSettingsResponse, String> {
    state
        .linear_integration_service
        .validate_and_enable()
        .await
        .map(LinearIntegrationSettingsResponse::from)
}

#[tauri::command]
pub async fn search_linear_issues(
    input: SearchLinearIssuesInput,
    state: State<'_, AppState>,
) -> Result<SearchLinearIssuesResponse, String> {
    let query = input.query.trim();
    if query.is_empty() {
        return Ok(SearchLinearIssuesResponse { issues: Vec::new() });
    }
    let issues = state
        .linear_integration_service
        .search_issues(query, input.limit.unwrap_or(10))
        .await?;
    Ok(SearchLinearIssuesResponse { issues })
}

#[tauri::command]
pub async fn save_linear_webhook_signing_secret(
    input: SaveLinearWebhookSigningSecretInput,
    state: State<'_, AppState>,
) -> Result<LinearWebhookConfigResponse, String> {
    let signing_secret = input.signing_secret.trim();
    if signing_secret.is_empty() {
        return Err("Linear webhook signing secret cannot be empty".to_string());
    }

    MacosKeychainSecretStore::new()
        .put_secret(LINEAR_WEBHOOK_SIGNING_SECRET_REF, signing_secret)
        .await
        .map_err(|error| error.to_string())?;

    let enabled = input.enabled.unwrap_or(true);
    let store = SqliteLinearWebhookStore::new(state.db.clone());
    store
        .set_signing_secret_ref(Some(LINEAR_WEBHOOK_SIGNING_SECRET_REF.to_string()), enabled)
        .await
        .map_err(|error| error.to_string())?;

    Ok(LinearWebhookConfigResponse {
        enabled,
        has_signing_secret: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::Manager;

    use crate::domain::integrations::IntegrationValidationStatus;

    fn test_app() -> tauri::App<tauri::test::MockRuntime> {
        tauri::test::mock_builder()
            .manage(AppState::new_test())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app should build")
    }

    #[test]
    fn integration_settings_response_reports_secret_presence_without_secret_value() {
        let mut settings = LinearIntegrationSettings::default();
        settings.enabled = true;
        settings.token_secret_ref = Some("linear-secret-ref".to_string());
        settings.validation_status = IntegrationValidationStatus::Valid;
        settings.issue_search_available = true;
        settings.last_error = Some("previous error".to_string());

        let response = LinearIntegrationSettingsResponse::from(settings);

        assert!(response.enabled);
        assert!(response.has_api_token);
        assert_eq!(response.validation_status, "valid");
        assert!(response.issue_search_available);
        assert_eq!(response.last_error.as_deref(), Some("previous error"));
    }

    #[test]
    fn integration_settings_response_handles_unconfigured_settings() {
        let response =
            LinearIntegrationSettingsResponse::from(LinearIntegrationSettings::default());

        assert!(!response.enabled);
        assert!(!response.has_api_token);
        assert_eq!(response.validation_status, "not_configured");
        assert!(!response.issue_search_available);
        assert!(response.last_error.is_none());
    }

    #[tokio::test]
    async fn get_linear_integration_settings_returns_default_state() {
        let app = test_app();

        let settings = get_linear_integration_settings(app.state::<AppState>())
            .await
            .expect("default settings should load");
        assert!(!settings.enabled);
        assert!(!settings.has_api_token);
        assert_eq!(settings.validation_status, "not_configured");
    }

    #[tokio::test]
    async fn search_linear_issues_short_circuits_blank_query() {
        let app = test_app();

        let response = search_linear_issues(
            SearchLinearIssuesInput {
                query: "   \n\t ".to_string(),
                limit: Some(50),
            },
            app.state::<AppState>(),
        )
        .await
        .expect("blank searches should not call Linear");

        assert!(response.issues.is_empty());
    }

    #[tokio::test]
    async fn validate_linear_integration_reports_missing_token() {
        let app = test_app();

        let error = validate_linear_integration(app.state::<AppState>())
            .await
            .expect_err("validation without a token should fail");

        assert!(error.contains("Linear API token is required"));
    }

    #[tokio::test]
    async fn save_linear_webhook_signing_secret_rejects_blank_secret_before_keychain_write() {
        let app = test_app();

        let error = save_linear_webhook_signing_secret(
            SaveLinearWebhookSigningSecretInput {
                signing_secret: "  ".to_string(),
                enabled: Some(true),
            },
            app.state::<AppState>(),
        )
        .await
        .expect_err("blank signing secrets should be rejected");

        assert_eq!(error, "Linear webhook signing secret cannot be empty");
    }
}
