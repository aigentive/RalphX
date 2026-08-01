use ralphx_lib::application::{LinearIntegrationSettings, LinearIntegrationSettingsRepository};
use ralphx_lib::domain::integrations::IntegrationValidationStatus;
use ralphx_lib::infrastructure::sqlite::SqliteLinearIntegrationSettingsRepository;
use ralphx_lib::testing::SqliteTestDb;

#[tokio::test]
async fn sqlite_repo_persists_linear_integration_settings() {
    let db = SqliteTestDb::new("linear-integration-settings");
    let repo = SqliteLinearIntegrationSettingsRepository::from_shared(db.shared_conn());
    let settings = LinearIntegrationSettings {
        enabled: true,
        token_secret_ref: Some("integrations/linear/default/api-token".to_string()),
        validation_status: IntegrationValidationStatus::Valid,
        issue_search_available: true,
        last_error: None,
        ..Default::default()
    };

    repo.upsert(&settings).await.unwrap();
    let stored = repo.get().await.unwrap();

    assert!(stored.enabled);
    assert_eq!(
        stored.token_secret_ref.as_deref(),
        Some("integrations/linear/default/api-token")
    );
    assert_eq!(stored.validation_status, IntegrationValidationStatus::Valid);
    assert!(stored.issue_search_available);
}

#[tokio::test]
async fn sqlite_repo_returns_defaults_and_preserves_invalid_state() {
    let db = SqliteTestDb::new("linear-integration-settings-default-invalid");
    let repo = SqliteLinearIntegrationSettingsRepository::from_shared(db.shared_conn());

    let default_settings = repo.get().await.unwrap();
    assert!(!default_settings.enabled);
    assert!(default_settings.token_secret_ref.is_none());
    assert_eq!(
        default_settings.validation_status,
        IntegrationValidationStatus::NotConfigured
    );
    assert!(!default_settings.issue_search_available);

    let settings = LinearIntegrationSettings {
        token_secret_ref: Some("integrations/linear/default/api-token/invalid".to_string()),
        validation_status: IntegrationValidationStatus::Invalid,
        last_error: Some("Linear rejected credentials".to_string()),
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
        Some("Linear rejected credentials")
    );
}
