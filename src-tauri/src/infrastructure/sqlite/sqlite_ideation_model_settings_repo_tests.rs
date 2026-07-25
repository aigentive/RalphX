use super::*;
use crate::domain::ideation::model_settings::ModelLevel;
use crate::testing::SqliteTestDb;

#[tokio::test]
async fn upsert_and_get_global_row() {
    let db = SqliteTestDb::new("sqlite_ideation_model_settings_repo_tests-global");
    let repo = SqliteIdeationModelSettingsRepository::from_shared(db.shared_conn());

    // No row yet — get_global returns None
    let before = repo.get_global().await.unwrap();
    assert!(before.is_none());

    // Upsert global row
    let result = repo
        .upsert_global("sonnet", "opus", "inherit", "inherit")
        .await
        .unwrap();
    assert_eq!(result.primary_model, ModelLevel::Sonnet);
    assert_eq!(result.verifier_model, ModelLevel::Opus);
    assert!(result.project_id.is_none());

    // Get it back
    let fetched = repo.get_global().await.unwrap();
    let fetched = fetched.expect("global row should exist after upsert");
    assert_eq!(fetched.primary_model, ModelLevel::Sonnet);
    assert_eq!(fetched.verifier_model, ModelLevel::Opus);
    assert!(fetched.project_id.is_none());

    // Update it
    let updated = repo
        .upsert_global("haiku", "inherit", "inherit", "inherit")
        .await
        .unwrap();
    assert_eq!(updated.primary_model, ModelLevel::Haiku);
    assert_eq!(updated.verifier_model, ModelLevel::Inherit);
}

#[tokio::test]
async fn upsert_and_get_project_row() {
    let db = SqliteTestDb::new("sqlite_ideation_model_settings_repo_tests-project");
    let repo = SqliteIdeationModelSettingsRepository::from_shared(db.shared_conn());

    // Upsert project-specific row
    let project_id = "proj-abc";
    let result = repo
        .upsert_for_project(project_id, "opus", "inherit", "inherit", "inherit")
        .await
        .unwrap();
    assert_eq!(result.primary_model, ModelLevel::Opus);
    assert_eq!(result.verifier_model, ModelLevel::Inherit);
    assert!(result.project_id.is_some());
    assert_eq!(result.project_id.as_ref().unwrap().as_str(), project_id);

    // Get it back
    let fetched = repo
        .get_for_project(project_id)
        .await
        .unwrap()
        .expect("project row should exist after upsert");
    assert_eq!(fetched.primary_model, ModelLevel::Opus);
    assert_eq!(fetched.verifier_model, ModelLevel::Inherit);

    // Global row should still not exist
    let global = repo.get_global().await.unwrap();
    assert!(global.is_none());
}

#[tokio::test]
async fn exact_opus_ids_round_trip_for_global_and_project_rows() {
    let db = SqliteTestDb::new("sqlite_ideation_model_settings_repo_tests-exact-opus");
    let repo = SqliteIdeationModelSettingsRepository::from_shared(db.shared_conn());
    repo.upsert_global("claude-opus-4-7", "claude-opus-4-8", "inherit", "inherit")
        .await
        .unwrap();
    repo.upsert_for_project("proj-abc", "claude-opus-5", "inherit", "inherit", "inherit")
        .await
        .unwrap();

    let global = repo.get_global().await.unwrap().unwrap();
    let project = repo.get_for_project("proj-abc").await.unwrap().unwrap();
    assert_eq!(global.primary_model.to_string(), "claude-opus-4-7");
    assert_eq!(global.verifier_model.to_string(), "claude-opus-4-8");
    assert_eq!(project.primary_model.to_string(), "claude-opus-5");
}

#[tokio::test]
async fn get_missing_returns_none() {
    let db = SqliteTestDb::new("sqlite_ideation_model_settings_repo_tests-missing");
    let repo = SqliteIdeationModelSettingsRepository::from_shared(db.shared_conn());

    let result = repo.get_for_project("nonexistent-project").await.unwrap();
    assert!(result.is_none());

    let global = repo.get_global().await.unwrap();
    assert!(global.is_none());
}

#[tokio::test]
async fn upsert_is_idempotent() {
    let db = SqliteTestDb::new("sqlite_ideation_model_settings_repo_tests-idempotent");
    let repo = SqliteIdeationModelSettingsRepository::from_shared(db.shared_conn());

    // Upsert global twice with same values
    let first = repo
        .upsert_global("sonnet", "opus", "inherit", "inherit")
        .await
        .unwrap();
    let second = repo
        .upsert_global("sonnet", "opus", "inherit", "inherit")
        .await
        .unwrap();
    assert_eq!(first.id, second.id);
    assert_eq!(second.primary_model, ModelLevel::Sonnet);
    assert_eq!(second.verifier_model, ModelLevel::Opus);

    // Upsert project twice with same values
    let proj_first = repo
        .upsert_for_project("proj-1", "haiku", "inherit", "inherit", "inherit")
        .await
        .unwrap();
    let proj_second = repo
        .upsert_for_project("proj-1", "haiku", "inherit", "inherit", "inherit")
        .await
        .unwrap();
    assert_eq!(proj_first.id, proj_second.id);
    assert_eq!(proj_second.primary_model, ModelLevel::Haiku);
}

#[tokio::test]
async fn upsert_updates_existing_row() {
    let db = SqliteTestDb::new("sqlite_ideation_model_settings_repo_tests-update");
    let repo = SqliteIdeationModelSettingsRepository::from_shared(db.shared_conn());

    // Insert global
    repo.upsert_global("sonnet", "sonnet", "inherit", "inherit")
        .await
        .unwrap();
    // Update global
    let updated = repo
        .upsert_global("opus", "haiku", "inherit", "inherit")
        .await
        .unwrap();
    assert_eq!(updated.primary_model, ModelLevel::Opus);
    assert_eq!(updated.verifier_model, ModelLevel::Haiku);

    // Confirm fetched matches
    let fetched = repo.get_global().await.unwrap().unwrap();
    assert_eq!(fetched.primary_model, ModelLevel::Opus);
    assert_eq!(fetched.verifier_model, ModelLevel::Haiku);
}

#[tokio::test]
async fn global_and_project_rows_are_independent() {
    let db = SqliteTestDb::new("sqlite_ideation_model_settings_repo_tests-independent");
    let repo = SqliteIdeationModelSettingsRepository::from_shared(db.shared_conn());

    repo.upsert_global("sonnet", "opus", "inherit", "inherit")
        .await
        .unwrap();
    repo.upsert_for_project("proj-1", "haiku", "inherit", "inherit", "inherit")
        .await
        .unwrap();

    let global = repo.get_global().await.unwrap().unwrap();
    let project = repo.get_for_project("proj-1").await.unwrap().unwrap();

    assert_eq!(global.primary_model, ModelLevel::Sonnet);
    assert_eq!(project.primary_model, ModelLevel::Haiku);
    assert!(global.project_id.is_none());
    assert!(project.project_id.is_some());
}
