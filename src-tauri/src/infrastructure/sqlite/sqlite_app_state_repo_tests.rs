use super::*;
use crate::testing::SqliteTestDb;

#[tokio::test]
async fn test_get_default_app_state() {
    let db = SqliteTestDb::new("sqlite_app_state_repo_tests-default");
    let repo = SqliteAppStateRepository::from_shared(db.shared_conn());

    let settings = repo.get().await.unwrap();
    assert!(settings.active_project_id.is_none());
    assert_eq!(settings.execution_halt_mode, ExecutionHaltMode::Running);
    assert!(settings.last_seen_release_notes_version.is_none());
}

#[tokio::test]
async fn test_set_and_get_active_project() {
    let db = SqliteTestDb::new("sqlite_app_state_repo_tests-set-active");
    let repo = SqliteAppStateRepository::from_shared(db.shared_conn());

    let project_id = ProjectId::from_string("proj-123".to_string());
    repo.set_active_project(Some(&project_id)).await.unwrap();

    let settings = repo.get().await.unwrap();
    assert_eq!(
        settings.active_project_id,
        Some(ProjectId::from_string("proj-123".to_string()))
    );
}

#[tokio::test]
async fn test_clear_active_project() {
    let db = SqliteTestDb::new("sqlite_app_state_repo_tests-clear-active");
    let repo = SqliteAppStateRepository::from_shared(db.shared_conn());

    // Set a project
    let project_id = ProjectId::from_string("proj-123".to_string());
    repo.set_active_project(Some(&project_id)).await.unwrap();

    // Clear it
    repo.set_active_project(None).await.unwrap();

    let settings = repo.get().await.unwrap();
    assert!(settings.active_project_id.is_none());
}

#[tokio::test]
async fn test_shared_connection() {
    let db = SqliteTestDb::new("sqlite_app_state_repo_tests-shared");
    let shared_conn = db.shared_conn();

    let repo = SqliteAppStateRepository::from_shared(Arc::clone(&shared_conn));

    let settings = repo.get().await.unwrap();
    assert!(settings.active_project_id.is_none());
    assert_eq!(settings.execution_halt_mode, ExecutionHaltMode::Running);
}

#[tokio::test]
async fn test_set_active_project_overwrites_previous_value() {
    // Verifies singleton behavior: only one active_project_id at a time
    let db = SqliteTestDb::new("sqlite_app_state_repo_tests-overwrite");
    let repo = SqliteAppStateRepository::from_shared(db.shared_conn());

    let project_a = ProjectId::from_string("proj-a".to_string());
    let project_b = ProjectId::from_string("proj-b".to_string());

    repo.set_active_project(Some(&project_a)).await.unwrap();
    let after_a = repo.get().await.unwrap();
    assert_eq!(
        after_a.active_project_id,
        Some(ProjectId::from_string("proj-a".to_string()))
    );

    // Setting project B should replace A (singleton table, no new rows)
    repo.set_active_project(Some(&project_b)).await.unwrap();
    let after_b = repo.get().await.unwrap();
    assert_eq!(
        after_b.active_project_id,
        Some(ProjectId::from_string("proj-b".to_string()))
    );

    // Only one active project at a time — not project A
    assert_ne!(
        after_b.active_project_id,
        Some(ProjectId::from_string("proj-a".to_string()))
    );
}

#[tokio::test]
async fn test_set_and_get_execution_halt_mode_paused() {
    let db = SqliteTestDb::new("sqlite_app_state_repo_tests-set-paused");
    let repo = SqliteAppStateRepository::from_shared(db.shared_conn());

    repo.set_execution_halt_mode(ExecutionHaltMode::Paused)
        .await
        .unwrap();

    let settings = repo.get().await.unwrap();
    assert_eq!(settings.execution_halt_mode, ExecutionHaltMode::Paused);
}

#[tokio::test]
async fn test_set_and_get_execution_halt_mode_stopped() {
    let db = SqliteTestDb::new("sqlite_app_state_repo_tests-set-stopped");
    let repo = SqliteAppStateRepository::from_shared(db.shared_conn());

    repo.set_execution_halt_mode(ExecutionHaltMode::Stopped)
        .await
        .unwrap();

    let settings = repo.get().await.unwrap();
    assert_eq!(settings.execution_halt_mode, ExecutionHaltMode::Stopped);
}

#[tokio::test]
async fn test_set_and_get_last_seen_release_notes_version() {
    let db = SqliteTestDb::new("sqlite_app_state_repo_tests-release-notes");
    let repo = SqliteAppStateRepository::from_shared(db.shared_conn());

    repo.set_last_seen_release_notes_version(Some("0.9.0"))
        .await
        .unwrap();

    let settings = repo.get().await.unwrap();
    assert_eq!(
        settings.last_seen_release_notes_version,
        Some("0.9.0".to_string())
    );

    repo.set_last_seen_release_notes_version(None)
        .await
        .unwrap();

    let settings = repo.get().await.unwrap();
    assert!(settings.last_seen_release_notes_version.is_none());
}
