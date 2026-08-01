use ralphx_lib::domain::integrations::{
    GranolaIntegrationSettings, GranolaIntegrationSettingsRepository, IntegrationValidationStatus,
};
use ralphx_lib::infrastructure::sqlite::SqliteGranolaIntegrationSettingsRepository;
use ralphx_lib::testing::SqliteTestDb;

#[tokio::test]
async fn sqlite_repo_persists_granola_integration_settings() {
    let db = SqliteTestDb::new("granola-integration-settings");
    let repo = SqliteGranolaIntegrationSettingsRepository::from_shared(db.shared_conn());
    let settings = GranolaIntegrationSettings {
        enabled: true,
        token_secret_ref: Some("integrations/granola/default/api-token".to_string()),
        validation_status: IntegrationValidationStatus::Valid,
        last_error: None,
        ..Default::default()
    };

    repo.upsert(&settings).await.unwrap();
    let stored = repo.get().await.unwrap();

    assert!(stored.enabled);
    assert_eq!(
        stored.token_secret_ref.as_deref(),
        Some("integrations/granola/default/api-token")
    );
    assert_eq!(stored.validation_status, IntegrationValidationStatus::Valid);
}

#[tokio::test]
async fn sqlite_repo_returns_defaults_and_preserves_invalid_state() {
    let db = SqliteTestDb::new("granola-integration-settings-default-invalid");
    let repo = SqliteGranolaIntegrationSettingsRepository::from_shared(db.shared_conn());

    let default_settings = repo.get().await.unwrap();
    assert!(!default_settings.enabled);
    assert!(default_settings.token_secret_ref.is_none());
    assert_eq!(
        default_settings.validation_status,
        IntegrationValidationStatus::NotConfigured
    );

    let settings = GranolaIntegrationSettings {
        token_secret_ref: Some("integrations/granola/default/api-token".to_string()),
        validation_status: IntegrationValidationStatus::Invalid,
        last_error: Some("Granola rejected credentials".to_string()),
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
        Some("Granola rejected credentials")
    );
}
