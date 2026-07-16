use tauri::Manager;

use super::clickup_commands::{
    disconnect_clickup_integration, get_clickup_integration_settings, list_clickup_workspaces,
    save_clickup_integration_settings, search_clickup_tasks, validate_clickup_integration,
    ClickUpIntegrationSettingsResponse, SaveClickUpIntegrationSettingsInput,
    SearchClickUpTasksInput,
};
use crate::application::AppState;
use crate::domain::integrations::{ClickUpIntegrationSettings, IntegrationValidationStatus};

fn test_app() -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build")
}

#[test]
fn integration_settings_response_reports_presence_without_leaking_token_ref() {
    let settings = ClickUpIntegrationSettings {
        enabled: true,
        token_secret_ref: Some("integrations/clickup/default/api-token/abc".to_string()),
        workspace_id: Some("9000".to_string()),
        validation_status: IntegrationValidationStatus::Valid,
        task_search_available: true,
        last_error: Some("previous error".to_string()),
        ..Default::default()
    };

    let response = ClickUpIntegrationSettingsResponse::from(settings);

    assert!(response.enabled);
    assert!(response.has_api_token);
    assert_eq!(response.workspace_id.as_deref(), Some("9000"));
    assert_eq!(response.validation_status, "valid");
    assert!(response.task_search_available);
    assert_eq!(response.last_error.as_deref(), Some("previous error"));
    assert!(!response.strict_git_naming_enabled);
    assert_eq!(
        response.branch_name_template,
        ":taskId:_:taskName:_:username:"
    );
    assert_eq!(response.commit_subject_template, ":taskId: - :taskName:");
    assert_eq!(response.pr_title_template, ":taskId: - :taskName:");

    // The keychain reference (and therefore the token) must never serialize out.
    let json = serde_json::to_string(&response).expect("response serializes");
    assert!(
        !json.contains("integrations/clickup/default/api-token"),
        "response leaked the secret ref: {json}"
    );
    assert!(
        !json.contains("tokenSecretRef"),
        "response leaked the ref field: {json}"
    );
}

#[test]
fn integration_settings_response_handles_unconfigured_settings() {
    let response = ClickUpIntegrationSettingsResponse::from(ClickUpIntegrationSettings::default());

    assert!(!response.enabled);
    assert!(!response.has_api_token);
    assert_eq!(response.workspace_id, None);
    assert_eq!(response.validation_status, "not_configured");
    assert!(!response.task_search_available);
    assert!(response.last_error.is_none());
    assert!(!response.strict_git_naming_enabled);
}

#[tokio::test]
async fn get_clickup_integration_settings_returns_default_state() {
    let app = test_app();

    let settings = get_clickup_integration_settings(app.state::<AppState>())
        .await
        .expect("default settings should load");

    assert!(!settings.enabled);
    assert!(!settings.has_api_token);
    assert_eq!(settings.workspace_id, None);
    assert_eq!(settings.validation_status, "not_configured");
}

#[tokio::test]
async fn save_clickup_integration_settings_persists_workspace_and_hides_token() {
    let app = test_app();

    let saved = save_clickup_integration_settings(
        SaveClickUpIntegrationSettingsInput {
            api_token: Some("pk_secret_token".to_string()),
            workspace_id: Some("9000".to_string()),
            ..Default::default()
        },
        app.state::<AppState>(),
    )
    .await
    .expect("save should succeed");

    assert!(saved.has_api_token);
    assert!(
        !saved.enabled,
        "saving returns to a pending, not-enabled state"
    );
    assert_eq!(saved.workspace_id.as_deref(), Some("9000"));
    assert_eq!(saved.validation_status, "pending");
    assert!(!saved.task_search_available);

    // The raw token must never appear in the command response.
    let json = serde_json::to_string(&saved).expect("response serializes");
    assert!(
        !json.contains("pk_secret_token"),
        "save response leaked the token: {json}"
    );
}

#[tokio::test]
async fn save_clickup_integration_settings_round_trips_git_convention() {
    let app = test_app();

    let saved = save_clickup_integration_settings(
        SaveClickUpIntegrationSettingsInput {
            strict_git_naming_enabled: Some(true),
            branch_name_template: Some("work/:taskId:_:taskName:".to_string()),
            commit_subject_template: Some(":taskId: | :summary:".to_string()),
            pr_title_template: Some(":taskId: | :taskName:".to_string()),
            ..Default::default()
        },
        app.state::<AppState>(),
    )
    .await
    .expect("valid Git convention should save");

    assert!(saved.strict_git_naming_enabled);
    assert_eq!(saved.branch_name_template, "work/:taskId:_:taskName:");
    assert_eq!(saved.commit_subject_template, ":taskId: | :summary:");
    assert_eq!(saved.pr_title_template, ":taskId: | :taskName:");
}

#[tokio::test]
async fn validate_clickup_integration_reports_missing_token() {
    let app = test_app();

    let error = validate_clickup_integration(app.state::<AppState>())
        .await
        .expect_err("validation without a token should fail");

    assert!(
        error.contains("ClickUp API token is required"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn save_then_validate_enables_then_disconnect_resets() {
    let app = test_app();

    save_clickup_integration_settings(
        SaveClickUpIntegrationSettingsInput {
            api_token: Some("pk_token".to_string()),
            workspace_id: Some("9000".to_string()),
            ..Default::default()
        },
        app.state::<AppState>(),
    )
    .await
    .expect("save ClickUp settings");

    let validated = validate_clickup_integration(app.state::<AppState>())
        .await
        .expect("validate enables the integration");
    assert!(validated.enabled);
    assert_eq!(validated.validation_status, "valid");
    assert!(validated.task_search_available);
    assert!(validated.last_error.is_none());
    assert!(validated.last_validated_at.is_some());

    let cleared = disconnect_clickup_integration(app.state::<AppState>())
        .await
        .expect("disconnect ClickUp");
    assert!(!cleared.enabled);
    assert!(!cleared.has_api_token);
    assert_eq!(cleared.workspace_id, None);
    assert_eq!(cleared.validation_status, "not_configured");
    assert!(!cleared.task_search_available);
}

#[tokio::test]
async fn list_clickup_workspaces_requires_enabled_then_succeeds_after_validation() {
    let app = test_app();

    let error = list_clickup_workspaces(app.state::<AppState>())
        .await
        .expect_err("listing workspaces before enabling should fail");
    assert!(error.contains("not enabled"), "unexpected error: {error}");

    save_clickup_integration_settings(
        SaveClickUpIntegrationSettingsInput {
            api_token: Some("pk_token".to_string()),
            workspace_id: None,
            ..Default::default()
        },
        app.state::<AppState>(),
    )
    .await
    .expect("save token");
    validate_clickup_integration(app.state::<AppState>())
        .await
        .expect("enable integration");

    let response = list_clickup_workspaces(app.state::<AppState>())
        .await
        .expect("workspaces load once enabled");
    // The Empty test client returns an empty workspace list (no network).
    assert!(response.workspaces.is_empty());
}

#[tokio::test]
async fn search_clickup_tasks_requires_enabled_then_succeeds_with_workspace() {
    let app = test_app();

    let error = search_clickup_tasks(
        SearchClickUpTasksInput {
            space_ids: vec!["space-1".to_string()],
            query: None,
            limit: None,
        },
        app.state::<AppState>(),
    )
    .await
    .expect_err("searching tasks before enabling should fail");
    assert!(error.contains("not enabled"), "unexpected error: {error}");

    save_clickup_integration_settings(
        SaveClickUpIntegrationSettingsInput {
            api_token: Some("pk_token".to_string()),
            workspace_id: Some("9000".to_string()),
            ..Default::default()
        },
        app.state::<AppState>(),
    )
    .await
    .expect("save token + workspace");
    validate_clickup_integration(app.state::<AppState>())
        .await
        .expect("enable integration");

    let response = search_clickup_tasks(
        SearchClickUpTasksInput {
            space_ids: vec!["space-1".to_string()],
            query: Some("fix".to_string()),
            limit: Some(10),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("tasks load once enabled with a workspace");
    // The Empty test client returns no tasks (no network).
    assert!(response.tasks.is_empty());
}
