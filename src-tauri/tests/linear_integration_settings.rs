use ralphx_lib::application::{LinearIntegrationSettings, LinearIntegrationSettingsRepository};
use ralphx_lib::domain::integrations::IntegrationValidationStatus;
use ralphx_lib::infrastructure::sqlite::SqliteLinearIntegrationSettingsRepository;
use ralphx_lib::testing::SqliteTestDb;

#[tokio::test]
async fn sqlite_repo_persists_linear_integration_settings() {
    let db = SqliteTestDb::new("linear-integration-settings");
    let repo = SqliteLinearIntegrationSettingsRepository::from_shared(db.shared_conn());
    let mut settings = LinearIntegrationSettings::default();
    settings.enabled = true;
    settings.token_secret_ref = Some("integrations/linear/default/api-token".to_string());
    settings.validation_status = IntegrationValidationStatus::Valid;
    settings.issue_search_available = true;
    settings.last_error = None;

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
