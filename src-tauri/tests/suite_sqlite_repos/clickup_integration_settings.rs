use ralphx_lib::domain::integrations::{
    ClickUpIntegrationSettings, ClickUpIntegrationSettingsRepository, IntegrationValidationStatus,
};
use ralphx_lib::infrastructure::sqlite::SqliteClickUpIntegrationSettingsRepository;
use ralphx_lib::testing::SqliteTestDb;

#[tokio::test]
async fn sqlite_repo_persists_clickup_integration_settings() {
    let db = SqliteTestDb::new("clickup-integration-settings");
    let repo = SqliteClickUpIntegrationSettingsRepository::from_shared(db.shared_conn());
    let settings = ClickUpIntegrationSettings {
        enabled: true,
        token_secret_ref: Some("integrations/clickup/default/api-token".to_string()),
        workspace_id: Some("9000123".to_string()),
        validation_status: IntegrationValidationStatus::Valid,
        task_search_available: true,
        last_error: None,
        ..Default::default()
    };

    repo.upsert(&settings).await.unwrap();
    let stored = repo.get().await.unwrap();

    assert!(stored.enabled);
    assert_eq!(
        stored.token_secret_ref.as_deref(),
        Some("integrations/clickup/default/api-token")
    );
    assert_eq!(stored.workspace_id.as_deref(), Some("9000123"));
    assert_eq!(stored.validation_status, IntegrationValidationStatus::Valid);
    assert!(stored.task_search_available);
}

#[tokio::test]
async fn sqlite_repo_returns_defaults_and_preserves_invalid_state() {
    let db = SqliteTestDb::new("clickup-integration-settings-default-invalid");
    let repo = SqliteClickUpIntegrationSettingsRepository::from_shared(db.shared_conn());

    let default_settings = repo.get().await.unwrap();
    assert!(!default_settings.enabled);
    assert!(default_settings.token_secret_ref.is_none());
    assert!(default_settings.workspace_id.is_none());
    assert_eq!(
        default_settings.validation_status,
        IntegrationValidationStatus::NotConfigured
    );
    assert!(!default_settings.task_search_available);

    let settings = ClickUpIntegrationSettings {
        token_secret_ref: Some("integrations/clickup/default/api-token/invalid".to_string()),
        validation_status: IntegrationValidationStatus::Invalid,
        last_error: Some("ClickUp rejected credentials".to_string()),
        ..Default::default()
    };

    let saved = repo.upsert(&settings).await.unwrap();
    let stored = repo.get().await.unwrap();

    assert_eq!(
        saved.validation_status,
        IntegrationValidationStatus::Invalid
    );
    assert_eq!(
        stored.validation_status,
        IntegrationValidationStatus::Invalid
    );
    assert_eq!(
        stored.last_error.as_deref(),
        Some("ClickUp rejected credentials")
    );
}
