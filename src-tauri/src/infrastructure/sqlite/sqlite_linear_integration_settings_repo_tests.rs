use chrono::Utc;

use crate::domain::integrations::{LinearIntegrationSettings, LinearIntegrationSettingsRepository};
use crate::domain::integrations::IntegrationValidationStatus;
use crate::infrastructure::sqlite::{DbConnection, SqliteLinearIntegrationSettingsRepository};
use crate::testing::SqliteTestDb;

#[tokio::test]
async fn sqlite_repo_covers_default_insert_update_and_date_parsing() {
    let db = SqliteTestDb::new("lib-linear-integration-settings");
    let repo = SqliteLinearIntegrationSettingsRepository::from_shared(db.shared_conn());

    let default_settings = repo.get().await.unwrap();
    assert!(!default_settings.enabled);
    assert!(default_settings.token_secret_ref.is_none());
    assert_eq!(
        default_settings.validation_status,
        IntegrationValidationStatus::NotConfigured
    );

    let validated_at = Utc::now();
    let settings = LinearIntegrationSettings {
        enabled: true,
        token_secret_ref: Some("integrations/linear/default/api-token".to_string()),
        validation_status: IntegrationValidationStatus::Valid,
        issue_search_available: true,
        last_validated_at: Some(validated_at),
        updated_at: validated_at,
        ..Default::default()
    };

    let saved = repo.upsert(&settings).await.unwrap();
    assert!(saved.enabled);
    let stored = repo.get().await.unwrap();
    assert!(stored.enabled);
    assert_eq!(
        stored.token_secret_ref.as_deref(),
        Some("integrations/linear/default/api-token")
    );
    assert_eq!(stored.validation_status, IntegrationValidationStatus::Valid);
    assert!(stored.issue_search_available);
    assert!(stored.last_validated_at.is_some());

    let mut invalid = stored;
    invalid.enabled = false;
    invalid.validation_status = IntegrationValidationStatus::Invalid;
    invalid.issue_search_available = false;
    invalid.last_error = Some("Linear rejected credentials".to_string());
    repo.upsert(&invalid).await.unwrap();

    let stored_invalid = repo.get().await.unwrap();
    assert!(!stored_invalid.enabled);
    assert_eq!(
        stored_invalid.validation_status,
        IntegrationValidationStatus::Invalid
    );
    assert_eq!(
        stored_invalid.last_error.as_deref(),
        Some("Linear rejected credentials")
    );
}

#[tokio::test]
async fn sqlite_repo_from_db_uses_existing_connection_wrapper() {
    let db = SqliteTestDb::new("lib-linear-integration-settings-from-db");
    let repo = SqliteLinearIntegrationSettingsRepository::from_db(DbConnection::from_shared(
        db.shared_conn(),
    ));

    let settings = LinearIntegrationSettings {
        token_secret_ref: Some("secret-ref".to_string()),
        validation_status: IntegrationValidationStatus::Pending,
        ..Default::default()
    };

    repo.upsert(&settings).await.unwrap();

    let stored = repo.get().await.unwrap();
    assert_eq!(stored.token_secret_ref.as_deref(), Some("secret-ref"));
    assert_eq!(stored.validation_status, IntegrationValidationStatus::Pending);
}
