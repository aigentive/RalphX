use super::*;
use crate::domain::entities::{IdeationSession, Project};
use crate::testing::SqliteTestDb;

fn setup_test_db() -> (
    SqliteTestDb,
    SqlitePlanSelectionStatsRepository,
    Project,
    IdeationSession,
) {
    let db = SqliteTestDb::new("sqlite-plan-selection-stats-repo");
    let project = db.seed_project("Test Project");
    let session = db.seed_ideation_session(project.id.clone());
    let repo = SqlitePlanSelectionStatsRepository::from_shared(db.shared_conn());

    (db, repo, project, session)
}

#[tokio::test]
async fn test_record_selection_creates_new_entry() {
    let (_db, repo, project, session) = setup_test_db();
    let timestamp = Utc::now();

    repo.record_selection(
        &project.id,
        &session.id,
        SelectionSource::KanbanInline,
        timestamp,
    )
    .await
    .unwrap();

    let stats = repo.get_stats(&project.id, &session.id).await.unwrap();
    assert!(stats.is_some());
    let stats = stats.unwrap();
    assert_eq!(stats.selected_count, 1);
    assert_eq!(
        stats.last_selected_source,
        Some("kanban_inline".to_string())
    );
}

#[tokio::test]
async fn test_record_selection_increments_count() {
    let (_db, repo, project, session) = setup_test_db();
    let timestamp1 = Utc::now();

    repo.record_selection(
        &project.id,
        &session.id,
        SelectionSource::KanbanInline,
        timestamp1,
    )
    .await
    .unwrap();

    let timestamp2 = Utc::now();
    repo.record_selection(
        &project.id,
        &session.id,
        SelectionSource::QuickSwitcher,
        timestamp2,
    )
    .await
    .unwrap();

    let stats = repo
        .get_stats(&project.id, &session.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stats.selected_count, 2);
    assert_eq!(
        stats.last_selected_source,
        Some("quick_switcher".to_string())
    );
}

#[tokio::test]
async fn test_get_stats_batch() {
    let (db, repo, project, session1) = setup_test_db();
    let session2 = db.seed_ideation_session(project.id.clone());
    let session3 = IdeationSessionId::new();
    let timestamp = Utc::now();

    repo.record_selection(
        &project.id,
        &session1.id,
        SelectionSource::KanbanInline,
        timestamp,
    )
    .await
    .unwrap();
    repo.record_selection(
        &project.id,
        &session2.id,
        SelectionSource::GraphInline,
        timestamp,
    )
    .await
    .unwrap();

    let results = repo
        .get_stats_batch(
            &project.id,
            &[session1.id.clone(), session2.id.clone(), session3.clone()],
        )
        .await
        .unwrap();

    assert_eq!(results.len(), 3);
    assert!(results[0].is_some());
    assert_eq!(results[0].as_ref().unwrap().ideation_session_id, session1.id);
    assert!(results[1].is_some());
    assert_eq!(results[1].as_ref().unwrap().ideation_session_id, session2.id);
    assert!(results[2].is_none());
}

#[tokio::test]
async fn test_get_stats_nonexistent() {
    let (_db, repo, project, session) = setup_test_db();

    let stats = repo.get_stats(&project.id, &session.id).await.unwrap();
    assert!(stats.is_none());
}
