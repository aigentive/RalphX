use tauri::Manager;

use super::granola_commands::{
    get_granola_integration_settings, save_granola_integration_settings,
    validate_granola_integration_settings, GranolaIntegrationSettingsResponse,
    SaveGranolaIntegrationSettingsInput,
};
use crate::application::AppState;
use crate::domain::integrations::{GranolaIntegrationSettings, IntegrationValidationStatus};

fn test_app() -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build")
}

#[test]
fn integration_settings_response_reports_presence_without_leaking_token_ref() {
    let settings = GranolaIntegrationSettings {
        enabled: true,
        token_secret_ref: Some("integrations/granola/default/api-token".to_string()),
        validation_status: IntegrationValidationStatus::Valid,
        last_error: Some("previous error".to_string()),
        ..Default::default()
    };

    let response = GranolaIntegrationSettingsResponse::from(settings);

    assert!(response.enabled);
    assert!(response.has_api_token);
    assert_eq!(response.validation_status, "valid");
    assert_eq!(response.last_error.as_deref(), Some("previous error"));

    // The keychain reference (and therefore the token) must never serialize out.
    let json = serde_json::to_string(&response).expect("response serializes");
    assert!(
        !json.contains("integrations/granola/default/api-token"),
        "response leaked the secret ref: {json}"
    );
    assert!(
        !json.contains("tokenSecretRef"),
        "response leaked the ref field: {json}"
    );
}

#[test]
fn integration_settings_response_handles_unconfigured_settings() {
    let response = GranolaIntegrationSettingsResponse::from(GranolaIntegrationSettings::default());

    assert!(!response.enabled);
    assert!(!response.has_api_token);
    assert_eq!(response.validation_status, "not_configured");
    assert!(response.last_error.is_none());
}

#[tokio::test]
async fn get_granola_integration_settings_returns_default_state() {
    let app = test_app();

    let settings = get_granola_integration_settings(app.state::<AppState>())
        .await
        .expect("default settings should load");

    assert!(!settings.enabled);
    assert!(!settings.has_api_token);
    assert_eq!(settings.validation_status, "not_configured");
}

#[tokio::test]
async fn save_granola_integration_settings_hides_token_and_returns_pending() {
    let app = test_app();

    let saved = save_granola_integration_settings(
        SaveGranolaIntegrationSettingsInput {
            api_token: Some("grn_secret_token".to_string()),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("save should succeed");

    assert!(saved.has_api_token);
    assert!(
        !saved.enabled,
        "saving returns a pending, not-enabled state"
    );
    assert_eq!(saved.validation_status, "pending");

    // The raw token must never appear in the command response.
    let json = serde_json::to_string(&saved).expect("response serializes");
    assert!(
        !json.contains("grn_secret_token"),
        "save response leaked the token: {json}"
    );
}

#[tokio::test]
async fn validate_granola_integration_settings_reports_missing_token() {
    let app = test_app();

    let error = validate_granola_integration_settings(app.state::<AppState>())
        .await
        .expect_err("validation without a token should fail");

    assert!(
        error.contains("Granola API token is required"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn save_then_validate_enables_then_blank_token_resets() {
    let app = test_app();

    save_granola_integration_settings(
        SaveGranolaIntegrationSettingsInput {
            api_token: Some("grn_token".to_string()),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("save Granola settings");

    let validated = validate_granola_integration_settings(app.state::<AppState>())
        .await
        .expect("validate enables the integration");
    assert!(validated.enabled);
    assert_eq!(validated.validation_status, "valid");
    assert!(validated.last_error.is_none());
    assert!(validated.last_validated_at.is_some());

    // A blank token disconnects: secret cleared, not-configured, no token.
    let cleared = save_granola_integration_settings(
        SaveGranolaIntegrationSettingsInput {
            api_token: Some(String::new()),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("blank token resets Granola settings");
    assert!(!cleared.enabled);
    assert!(!cleared.has_api_token);
    assert_eq!(cleared.validation_status, "not_configured");
}
