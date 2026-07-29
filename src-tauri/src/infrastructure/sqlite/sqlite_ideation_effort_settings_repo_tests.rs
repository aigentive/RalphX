use super::*;
use crate::domain::ideation::EffortLevel;
use crate::testing::SqliteTestDb;

#[tokio::test]
async fn upsert_and_get_global_row() {
    let db = SqliteTestDb::new("sqlite_ideation_effort_settings_repo_tests-global");
    let repo = SqliteIdeationEffortSettingsRepository::from_shared(db.shared_conn());

    // No row yet — get_by_project_id returns None
    let before = repo.get_by_project_id(None).await.unwrap();
    assert!(before.is_none());

    // Upsert global row
    let result = repo.upsert(None, "high", "medium").await.unwrap();
    assert_eq!(result.primary_effort, EffortLevel::High);
    assert_eq!(result.verifier_effort, EffortLevel::Medium);
    assert!(result.project_id.is_none());

    // Get it back
    let fetched = repo.get_by_project_id(None).await.unwrap();
    let fetched = fetched.expect("global row should exist after upsert");
    assert_eq!(fetched.primary_effort, EffortLevel::High);
    assert_eq!(fetched.verifier_effort, EffortLevel::Medium);
    assert!(fetched.project_id.is_none());

    // Update it
    let updated = repo.upsert(None, "max", "low").await.unwrap();
    assert_eq!(updated.primary_effort, EffortLevel::Max);
    assert_eq!(updated.verifier_effort, EffortLevel::Low);
}

#[tokio::test]
async fn upsert_and_get_project_row() {
    let db = SqliteTestDb::new("sqlite_ideation_effort_settings_repo_tests-project");
    let repo = SqliteIdeationEffortSettingsRepository::from_shared(db.shared_conn());

    // Upsert project-specific row
    let project_id = "proj-abc";
    let result = repo.upsert(Some(project_id), "low", "inherit").await.unwrap();
    assert_eq!(result.primary_effort, EffortLevel::Low);
    assert_eq!(result.verifier_effort, EffortLevel::Inherit);
    assert!(result.project_id.is_some());
    assert_eq!(result.project_id.as_ref().unwrap().as_str(), project_id);

    // Get it back
    let fetched = repo
        .get_by_project_id(Some(project_id))
        .await
        .unwrap()
        .expect("project row should exist after upsert");
    assert_eq!(fetched.primary_effort, EffortLevel::Low);
    assert_eq!(fetched.verifier_effort, EffortLevel::Inherit);

    // Global row should still not exist
    let global = repo.get_by_project_id(None).await.unwrap();
    assert!(global.is_none());
}

#[tokio::test]
async fn get_missing_returns_none() {
    let db = SqliteTestDb::new("sqlite_ideation_effort_settings_repo_tests-missing");
    let repo = SqliteIdeationEffortSettingsRepository::from_shared(db.shared_conn());

    let result = repo
        .get_by_project_id(Some("nonexistent-project"))
        .await
        .unwrap();
    assert!(result.is_none());

    let global = repo.get_by_project_id(None).await.unwrap();
    assert!(global.is_none());
}
