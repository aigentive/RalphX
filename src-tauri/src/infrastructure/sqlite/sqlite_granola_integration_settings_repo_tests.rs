use chrono::Utc;

use crate::domain::integrations::{
    GranolaIntegrationSettings, GranolaIntegrationSettingsRepository, IntegrationValidationStatus,
};
use crate::infrastructure::sqlite::{DbConnection, SqliteGranolaIntegrationSettingsRepository};
use crate::testing::SqliteTestDb;

fn repo(name: &str) -> (SqliteTestDb, SqliteGranolaIntegrationSettingsRepository) {
    let db = SqliteTestDb::new(name);
    let repo = SqliteGranolaIntegrationSettingsRepository::from_shared(db.shared_conn());
    (db, repo)
}

#[tokio::test]
async fn get_returns_seeded_defaults() {
    let (_db, repo) = repo("granola-settings-defaults");

    let settings = repo.get().await.unwrap();

    assert!(!settings.enabled);
    assert!(settings.token_secret_ref.is_none());
    assert_eq!(
        settings.validation_status,
        IntegrationValidationStatus::NotConfigured
    );
    assert!(settings.last_validated_at.is_none());
    assert!(settings.last_error.is_none());
}

#[tokio::test]
async fn upsert_round_trips_all_granola_settings_fields() {
    let (_db, repo) = repo("granola-settings-round-trip");
    let validated_at = Utc::now();
    let updated_at = Utc::now();
    let settings = GranolaIntegrationSettings {
        enabled: true,
        token_secret_ref: Some("integrations/granola/default/api-token".to_string()),
        validation_status: IntegrationValidationStatus::Valid,
        last_validated_at: Some(validated_at),
        last_error: Some("previous warning".to_string()),
        updated_at,
    };

    let saved = repo.upsert(&settings).await.unwrap();
    let stored = repo.get().await.unwrap();

    assert!(saved.enabled);
    assert_eq!(stored.token_secret_ref, settings.token_secret_ref);
    assert_eq!(stored.validation_status, IntegrationValidationStatus::Valid);
    assert_eq!(
        stored.last_validated_at.map(|value| value.to_rfc3339()),
        Some(validated_at.to_rfc3339())
    );
    assert_eq!(stored.last_error.as_deref(), Some("previous warning"));
    assert_eq!(stored.updated_at.to_rfc3339(), updated_at.to_rfc3339());
}

#[tokio::test]
async fn upsert_does_not_persist_raw_token_in_any_column() {
    let (db, repo) = repo("granola-settings-no-plaintext");

    // Saving only ever stores a keychain reference, never the token itself.
    repo.upsert(&GranolaIntegrationSettings {
        enabled: true,
        token_secret_ref: Some("integrations/granola/default/api-token".to_string()),
        validation_status: IntegrationValidationStatus::Valid,
        ..Default::default()
    })
    .await
    .unwrap();

    let raw_secret = "grn_super_secret_raw_token_value";
    let shared = db.shared_conn();
    let conn = shared.lock().await;
    let mut stmt = conn
        .prepare("SELECT id, enabled, token_secret_ref, validation_status, last_validated_at, last_error, updated_at FROM granola_integration_settings")
        .unwrap();
    let mut rows = stmt.query([]).unwrap();
    while let Some(row) = rows.next().unwrap() {
        for idx in 0..7 {
            let value: Option<String> = row.get(idx).ok().flatten();
            if let Some(value) = value {
                assert!(
                    !value.contains(raw_secret),
                    "raw token leaked into column {idx}: {value}"
                );
                assert!(
                    !value.starts_with("grn_"),
                    "column {idx} looks like a raw Granola token: {value}"
                );
            }
        }
    }
}

#[tokio::test]
async fn upsert_replaces_prior_connection_state() {
    let (_db, repo) = repo("granola-settings-replace");
    repo.upsert(&GranolaIntegrationSettings {
        enabled: true,
        token_secret_ref: Some("old-secret".to_string()),
        validation_status: IntegrationValidationStatus::Valid,
        last_error: None,
        ..Default::default()
    })
    .await
    .unwrap();

    let replacement = GranolaIntegrationSettings {
        enabled: false,
        token_secret_ref: None,
        validation_status: IntegrationValidationStatus::Invalid,
        last_error: Some("Granola rejected the token".to_string()),
        ..Default::default()
    };

    repo.upsert(&replacement).await.unwrap();
    let stored = repo.get().await.unwrap();

    assert!(!stored.enabled);
    assert!(stored.token_secret_ref.is_none());
    assert_eq!(
        stored.validation_status,
        IntegrationValidationStatus::Invalid
    );
    assert_eq!(
        stored.last_error.as_deref(),
        Some("Granola rejected the token")
    );
}

#[tokio::test]
async fn get_accepts_legacy_sqlite_datetime_strings() {
    let (db, repo) = repo("granola-settings-legacy-datetime");
    {
        let shared = db.shared_conn();
        let conn = shared.lock().await;
        conn.execute(
            "UPDATE granola_integration_settings
                SET enabled = 1,
                    token_secret_ref = 'legacy-secret',
                    validation_status = 'valid',
                    last_validated_at = '2026-06-23 15:30:00',
                    updated_at = '2026-06-23 15:31:00'
              WHERE id = 'default'",
            [],
        )
        .unwrap();
    }

    let stored = repo.get().await.unwrap();

    assert!(stored.enabled);
    assert_eq!(stored.token_secret_ref.as_deref(), Some("legacy-secret"));
    assert_eq!(
        stored.last_validated_at.map(|value| value.to_rfc3339()),
        Some("2026-06-23T15:30:00+00:00".to_string())
    );
    assert_eq!(stored.updated_at.to_rfc3339(), "2026-06-23T15:31:00+00:00");
}

#[tokio::test]
async fn from_db_constructor_uses_the_same_repository_path() {
    let db = SqliteTestDb::new("granola-settings-from-db");
    let repo = SqliteGranolaIntegrationSettingsRepository::from_db(DbConnection::from_shared(
        db.shared_conn(),
    ));

    let stored = repo
        .upsert(&GranolaIntegrationSettings {
            token_secret_ref: Some("integrations/granola/default/api-token".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(
        stored.token_secret_ref.as_deref(),
        Some("integrations/granola/default/api-token")
    );
}
