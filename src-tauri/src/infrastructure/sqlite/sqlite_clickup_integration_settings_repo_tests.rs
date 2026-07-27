use chrono::Utc;

use crate::domain::integrations::{
    ClickUpIntegrationSettings, ClickUpIntegrationSettingsRepository, IntegrationValidationStatus,
};
use crate::infrastructure::sqlite::{DbConnection, SqliteClickUpIntegrationSettingsRepository};
use crate::testing::SqliteTestDb;

fn repo(name: &str) -> (SqliteTestDb, SqliteClickUpIntegrationSettingsRepository) {
    let db = SqliteTestDb::new(name);
    let repo = SqliteClickUpIntegrationSettingsRepository::from_shared(db.shared_conn());
    (db, repo)
}

#[tokio::test]
async fn get_returns_seeded_defaults() {
    let (_db, repo) = repo("clickup-settings-defaults");

    let settings = repo.get().await.unwrap();

    assert!(!settings.enabled);
    assert!(settings.token_secret_ref.is_none());
    assert!(settings.workspace_id.is_none());
    assert_eq!(
        settings.validation_status,
        IntegrationValidationStatus::NotConfigured
    );
    assert!(!settings.task_search_available);
    assert!(settings.last_validated_at.is_none());
    assert!(settings.last_error.is_none());
    assert!(!settings.strict_git_naming_enabled);
    assert_eq!(
        settings.branch_name_template,
        ":taskId:_:taskName:_:username:"
    );
    assert_eq!(settings.commit_subject_template, ":taskId: - :taskName:");
    assert_eq!(settings.pr_title_template, ":taskId: - :taskName:");
}

#[tokio::test]
async fn upsert_round_trips_all_clickup_settings_fields() {
    let (_db, repo) = repo("clickup-settings-round-trip");
    let validated_at = Utc::now();
    let updated_at = Utc::now();
    let settings = ClickUpIntegrationSettings {
        enabled: true,
        token_secret_ref: Some("integrations/clickup/default/api-token".to_string()),
        workspace_id: Some("workspace-1".to_string()),
        validation_status: IntegrationValidationStatus::Valid,
        task_search_available: true,
        last_validated_at: Some(validated_at),
        last_error: Some("previous warning".to_string()),
        strict_git_naming_enabled: true,
        branch_name_template: "work/:taskId:_:taskName:".to_string(),
        commit_subject_template: ":taskId: | :summary:".to_string(),
        pr_title_template: ":taskId: | :taskName:".to_string(),
        updated_at,
    };

    let saved = repo.upsert(&settings).await.unwrap();
    let stored = repo.get().await.unwrap();

    assert!(saved.enabled);
    assert_eq!(stored.token_secret_ref, settings.token_secret_ref);
    assert_eq!(stored.workspace_id, settings.workspace_id);
    assert_eq!(stored.validation_status, IntegrationValidationStatus::Valid);
    assert!(stored.task_search_available);
    assert_eq!(
        stored.last_validated_at.map(|value| value.to_rfc3339()),
        Some(validated_at.to_rfc3339())
    );
    assert_eq!(stored.last_error.as_deref(), Some("previous warning"));
    assert!(stored.strict_git_naming_enabled);
    assert_eq!(stored.branch_name_template, settings.branch_name_template);
    assert_eq!(
        stored.commit_subject_template,
        settings.commit_subject_template
    );
    assert_eq!(stored.pr_title_template, settings.pr_title_template);
    assert_eq!(stored.updated_at.to_rfc3339(), updated_at.to_rfc3339());
}

#[tokio::test]
async fn upsert_replaces_prior_connection_state() {
    let (_db, repo) = repo("clickup-settings-replace");
    repo.upsert(&ClickUpIntegrationSettings {
        enabled: true,
        token_secret_ref: Some("old-secret".to_string()),
        workspace_id: Some("old-workspace".to_string()),
        validation_status: IntegrationValidationStatus::Valid,
        task_search_available: true,
        last_error: None,
        ..Default::default()
    })
    .await
    .unwrap();

    let replacement = ClickUpIntegrationSettings {
        enabled: false,
        token_secret_ref: None,
        workspace_id: None,
        validation_status: IntegrationValidationStatus::Invalid,
        task_search_available: false,
        last_error: Some("ClickUp rejected the token".to_string()),
        ..Default::default()
    };

    repo.upsert(&replacement).await.unwrap();
    let stored = repo.get().await.unwrap();

    assert!(!stored.enabled);
    assert!(stored.token_secret_ref.is_none());
    assert!(stored.workspace_id.is_none());
    assert_eq!(
        stored.validation_status,
        IntegrationValidationStatus::Invalid
    );
    assert!(!stored.task_search_available);
    assert_eq!(
        stored.last_error.as_deref(),
        Some("ClickUp rejected the token")
    );
}

#[tokio::test]
async fn get_accepts_legacy_sqlite_datetime_strings() {
    let (db, repo) = repo("clickup-settings-legacy-datetime");
    {
        let shared = db.shared_conn();
        let conn = shared.lock().await;
        conn.execute(
            "UPDATE clickup_integration_settings
                SET enabled = 1,
                    token_secret_ref = 'legacy-secret',
                    workspace_id = 'legacy-workspace',
                    validation_status = 'valid',
                    task_search_available = 1,
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
    assert_eq!(stored.workspace_id.as_deref(), Some("legacy-workspace"));
    assert_eq!(
        stored.last_validated_at.map(|value| value.to_rfc3339()),
        Some("2026-06-23T15:30:00+00:00".to_string())
    );
    assert_eq!(stored.updated_at.to_rfc3339(), "2026-06-23T15:31:00+00:00");
}

#[tokio::test]
async fn from_db_constructor_uses_the_same_repository_path() {
    let db = SqliteTestDb::new("clickup-settings-from-db");
    let repo = SqliteClickUpIntegrationSettingsRepository::from_db(DbConnection::from_shared(
        db.shared_conn(),
    ));

    let stored = repo
        .upsert(&ClickUpIntegrationSettings {
            workspace_id: Some("from-db-workspace".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(stored.workspace_id.as_deref(), Some("from-db-workspace"));
}
