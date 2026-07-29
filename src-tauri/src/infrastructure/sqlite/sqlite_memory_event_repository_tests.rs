// Tests for SqliteMemoryEventRepository

use super::sqlite_memory_event_repository::SqliteMemoryEventRepository;
use crate::domain::entities::{MemoryActorType, MemoryEvent, ProjectId};
use crate::domain::repositories::MemoryEventRepository;
use crate::testing::SqliteTestDb;
use serde_json::json;

fn setup_test_db() -> SqliteTestDb {
    SqliteTestDb::new("sqlite_memory_event_repository_tests")
}

fn create_test_project(db: &SqliteTestDb) -> ProjectId {
    let id = ProjectId::new();
    let working_dir = format!("/tmp/test/{}", id.as_str());
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO projects (id, name, working_directory, git_mode, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'), strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))",
            rusqlite::params![id.as_str(), "Test Project", working_dir, "local"],
        )
        .unwrap();
    });
    id
}

fn make_event(
    project_id: ProjectId,
    event_type: &str,
    actor: MemoryActorType,
) -> MemoryEvent {
    MemoryEvent::new(project_id, event_type, actor, json!({"key": "value"}))
}

// ==================== CREATE TESTS ====================

#[tokio::test]
async fn test_create_returns_event() {
    let db = setup_test_db();
    let project_id = create_test_project(&db);
    let repo = SqliteMemoryEventRepository::from_shared(db.shared_conn());

    let event = make_event(project_id.clone(), "memory_created", MemoryActorType::System);
    let event_id = event.id.clone();

    let result = repo.create(event).await;

    assert!(result.is_ok());
    let created = result.unwrap();
    assert_eq!(created.id, event_id);
    assert_eq!(created.project_id, project_id);
    assert_eq!(created.event_type, "memory_created");
    assert_eq!(created.actor_type, MemoryActorType::System);
}

#[tokio::test]
async fn test_create_all_actor_types() {
    let db = setup_test_db();
    let project_id = create_test_project(&db);
    let repo = SqliteMemoryEventRepository::from_shared(db.shared_conn());

    let e1 = make_event(project_id.clone(), "system_event", MemoryActorType::System);
    let e2 = make_event(
        project_id.clone(),
        "maintain_event",
        MemoryActorType::MemoryMaintainer,
    );
    let e3 = make_event(
        project_id.clone(),
        "capture_event",
        MemoryActorType::MemoryCapture,
    );

    repo.create(e1).await.unwrap();
    repo.create(e2).await.unwrap();
    repo.create(e3).await.unwrap();

    let events = repo.get_by_project(&project_id).await.unwrap();
    assert_eq!(events.len(), 3);

    let actor_types: Vec<MemoryActorType> = events.iter().map(|e| e.actor_type).collect();
    assert!(actor_types.contains(&MemoryActorType::System));
    assert!(actor_types.contains(&MemoryActorType::MemoryMaintainer));
    assert!(actor_types.contains(&MemoryActorType::MemoryCapture));
}

#[tokio::test]
async fn test_create_serializes_complex_json_details() {
    let db = setup_test_db();
    let project_id = create_test_project(&db);
    let repo = SqliteMemoryEventRepository::from_shared(db.shared_conn());

    let complex_details = json!({
        "memory_id": "mem-abc",
        "count": 42,
        "tags": ["rust", "sqlite"],
        "nested": {"inner": true}
    });

    let event = MemoryEvent::new(
        project_id.clone(),
        "complex_event",
        MemoryActorType::MemoryCapture,
        complex_details.clone(),
    );
    let event_id = event.id.clone();

    repo.create(event).await.unwrap();

    let events = repo.get_by_project(&project_id).await.unwrap();
    let found = events.iter().find(|e| e.id == event_id).unwrap();
    assert_eq!(found.details, complex_details);
}

#[tokio::test]
async fn test_create_duplicate_id_fails() {
    let db = setup_test_db();
    let project_id = create_test_project(&db);
    let repo = SqliteMemoryEventRepository::from_shared(db.shared_conn());

    let event = make_event(project_id.clone(), "memory_created", MemoryActorType::System);
    repo.create(event.clone()).await.unwrap();

    let result = repo.create(event).await;

    assert!(result.is_err());
}

// ==================== GET BY PROJECT TESTS ====================

#[tokio::test]
async fn test_get_by_project_returns_all_events() {
    let db = setup_test_db();
    let project_id = create_test_project(&db);
    let repo = SqliteMemoryEventRepository::from_shared(db.shared_conn());

    let e1 = make_event(project_id.clone(), "event_1", MemoryActorType::System);
    let e2 = make_event(
        project_id.clone(),
        "event_2",
        MemoryActorType::MemoryMaintainer,
    );
    let e3 = make_event(project_id.clone(), "event_3", MemoryActorType::MemoryCapture);

    repo.create(e1).await.unwrap();
    repo.create(e2).await.unwrap();
    repo.create(e3).await.unwrap();

    let result = repo.get_by_project(&project_id).await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 3);
}

#[tokio::test]
async fn test_get_by_project_returns_empty_for_no_events() {
    let db = setup_test_db();
    let project_id = create_test_project(&db);
    let repo = SqliteMemoryEventRepository::from_shared(db.shared_conn());

    let result = repo.get_by_project(&project_id).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_get_by_project_filters_by_project() {
    let db = setup_test_db();
    let project_id1 = create_test_project(&db);
    let project_id2 = create_test_project(&db);
    let repo = SqliteMemoryEventRepository::from_shared(db.shared_conn());

    let e1 = make_event(project_id1.clone(), "event_p1", MemoryActorType::System);
    let e2 = make_event(project_id2.clone(), "event_p2", MemoryActorType::System);

    repo.create(e1).await.unwrap();
    repo.create(e2).await.unwrap();

    let p1_events = repo.get_by_project(&project_id1).await.unwrap();
    let p2_events = repo.get_by_project(&project_id2).await.unwrap();

    assert_eq!(p1_events.len(), 1);
    assert_eq!(p2_events.len(), 1);
    assert_eq!(p1_events[0].project_id, project_id1);
    assert_eq!(p2_events[0].project_id, project_id2);
}

#[tokio::test]
async fn test_get_by_project_returns_in_desc_order() {
    let db = setup_test_db();
    let project_id = create_test_project(&db);
    let repo = SqliteMemoryEventRepository::from_shared(db.shared_conn());

    let e1 = make_event(project_id.clone(), "first", MemoryActorType::System);
    repo.create(e1).await.unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let e2 = make_event(project_id.clone(), "second", MemoryActorType::System);
    repo.create(e2).await.unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let e3 = make_event(project_id.clone(), "third", MemoryActorType::System);
    repo.create(e3).await.unwrap();

    let events = repo.get_by_project(&project_id).await.unwrap();

    // get_by_project orders by created_at DESC (newest first)
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].event_type, "third");
    assert_eq!(events[1].event_type, "second");
    assert_eq!(events[2].event_type, "first");
}

// ==================== GET BY TYPE TESTS ====================

#[tokio::test]
async fn test_get_by_type_returns_matching_events() {
    let db = setup_test_db();
    let project_id = create_test_project(&db);
    let repo = SqliteMemoryEventRepository::from_shared(db.shared_conn());

    let e1 = make_event(project_id.clone(), "memory_created", MemoryActorType::System);
    let e2 = make_event(
        project_id.clone(),
        "memory_updated",
        MemoryActorType::MemoryMaintainer,
    );
    let e3 = make_event(
        project_id.clone(),
        "memory_created",
        MemoryActorType::MemoryCapture,
    );

    repo.create(e1).await.unwrap();
    repo.create(e2).await.unwrap();
    repo.create(e3).await.unwrap();

    let result = repo.get_by_type("memory_created").await;

    assert!(result.is_ok());
    let events = result.unwrap();
    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|e| e.event_type == "memory_created"));
}

#[tokio::test]
async fn test_get_by_type_returns_empty_for_unknown_type() {
    let db = setup_test_db();
    let repo = SqliteMemoryEventRepository::from_shared(db.shared_conn());

    let result = repo.get_by_type("nonexistent_type").await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_get_by_type_is_cross_project() {
    // get_by_type has no project scoping — returns events from ALL projects
    let db = setup_test_db();
    let project_id1 = create_test_project(&db);
    let project_id2 = create_test_project(&db);
    let repo = SqliteMemoryEventRepository::from_shared(db.shared_conn());

    let e1 = make_event(project_id1.clone(), "shared_type", MemoryActorType::System);
    let e2 = make_event(project_id2.clone(), "shared_type", MemoryActorType::System);

    repo.create(e1).await.unwrap();
    repo.create(e2).await.unwrap();

    let events = repo.get_by_type("shared_type").await.unwrap();

    // Cross-project: both returned
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn test_get_by_type_returns_in_desc_order() {
    let db = setup_test_db();
    let project_id = create_test_project(&db);
    let repo = SqliteMemoryEventRepository::from_shared(db.shared_conn());

    let e1 = make_event(project_id.clone(), "target_type", MemoryActorType::System);
    repo.create(e1.clone()).await.unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let e2 = make_event(project_id.clone(), "target_type", MemoryActorType::System);
    repo.create(e2.clone()).await.unwrap();

    let events = repo.get_by_type("target_type").await.unwrap();

    assert_eq!(events.len(), 2);
    // DESC order: most recent first
    assert_eq!(events[0].id, e2.id);
    assert_eq!(events[1].id, e1.id);
}

// ==================== FROM SHARED TESTS ====================

#[tokio::test]
async fn test_from_shared_creates_and_retrieves() {
    let db = setup_test_db();
    let project_id = create_test_project(&db);
    let shared_conn = db.shared_conn();
    let repo = SqliteMemoryEventRepository::from_shared(shared_conn);

    let event = make_event(project_id.clone(), "test_event", MemoryActorType::System);
    let result = repo.create(event).await;

    assert!(result.is_ok());

    let events = repo.get_by_project(&project_id).await.unwrap();
    assert_eq!(events.len(), 1);
}
