use super::sqlite_delegated_session_repo::SqliteDelegatedSessionRepository;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{DelegatedSession, DelegatedSessionId, Project};
use crate::domain::repositories::{DelegatedSessionRepository, ProjectRepository};
use crate::testing::SqliteTestDb;
use crate::infrastructure::sqlite::SqliteProjectRepository;

fn setup_test_db() -> SqliteTestDb {
    SqliteTestDb::new("sqlite_delegated_session_repo_tests")
}

async fn create_project(db: &SqliteTestDb) -> crate::domain::entities::ProjectId {
    let repo = SqliteProjectRepository::from_shared(db.shared_conn());
    let project = Project::new(
        "Delegated Session Test Project".to_string(),
        "/tmp/ralphx-delegated-session-test".to_string(),
    );
    let project_id = project.id.clone();
    repo.create(project).await.unwrap();
    project_id
}

#[tokio::test]
async fn test_create_and_get_by_id() {
    let db = setup_test_db();
    let repo = SqliteDelegatedSessionRepository::from_shared(db.shared_conn());
    let project_id = create_project(&db).await;

    let session = DelegatedSession::new(
        project_id,
        "task_execution",
        "task-1",
        "ralphx-execution-worker",
        AgentHarnessKind::Codex,
    );
    let id = session.id.clone();

    repo.create(session).await.unwrap();

    let found = repo.get_by_id(&id).await.unwrap().unwrap();
    assert_eq!(found.parent_context_type, "task_execution");
    assert_eq!(found.parent_context_id, "task-1");
    assert_eq!(found.harness, AgentHarnessKind::Codex);
}

#[tokio::test]
async fn test_get_by_parent_context_orders_latest_first() {
    let db = setup_test_db();
    let repo = SqliteDelegatedSessionRepository::from_shared(db.shared_conn());
    let project_id = create_project(&db).await;

    let older = DelegatedSession::new(
        project_id.clone(),
        "review",
        "review-1",
        "ralphx-execution-reviewer",
        AgentHarnessKind::Claude,
    );
    let older_id = older.id.clone();
    repo.create(older).await.unwrap();
    repo.update_status(&older_id, "failed", Some("oops".to_string()), None)
        .await
        .unwrap();

    let newer = DelegatedSession::new(
        project_id,
        "review",
        "review-1",
        "ralphx-execution-reviewer",
        AgentHarnessKind::Codex,
    );
    let newer_id = newer.id.clone();
    repo.create(newer).await.unwrap();

    let sessions = repo.get_by_parent_context("review", "review-1").await.unwrap();
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].id, newer_id);
    assert_eq!(sessions[1].id, older_id);
}

#[tokio::test]
async fn test_update_runtime_fields() {
    let db = setup_test_db();
    let repo = SqliteDelegatedSessionRepository::from_shared(db.shared_conn());
    let project_id = create_project(&db).await;

    let session = DelegatedSession::new(
        project_id,
        "merge",
        "task-42",
        "ralphx-execution-merger",
        AgentHarnessKind::Codex,
    );
    let id = session.id.clone();
    repo.create(session).await.unwrap();

    repo.update_provider_session_id(&id, Some("provider-42".to_string()))
        .await
        .unwrap();
    repo.update_status(&id, "completed", None, Some(chrono::Utc::now()))
        .await
        .unwrap();

    let found = repo.get_by_id(&id).await.unwrap().unwrap();
    assert_eq!(found.provider_session_id.as_deref(), Some("provider-42"));
    assert_eq!(found.status, "completed");
    assert!(found.completed_at.is_some());
}

#[tokio::test]
async fn test_get_by_id_returns_none_for_missing_session() {
    let db = setup_test_db();
    let repo = SqliteDelegatedSessionRepository::from_shared(db.shared_conn());

    let missing = DelegatedSessionId::from_string("missing");
    assert!(repo.get_by_id(&missing).await.unwrap().is_none());
}
