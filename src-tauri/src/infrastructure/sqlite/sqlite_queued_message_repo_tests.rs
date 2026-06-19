use std::sync::Arc;

use rusqlite::Connection;
use tokio::sync::Mutex;

use crate::domain::entities::{ChatAttachmentId, ChatContextType};
use crate::domain::repositories::QueuedMessageRepository;
use crate::domain::services::{QueueKey, QueuedMessage};
use crate::infrastructure::sqlite::SqliteQueuedMessageRepository;

fn setup_repo() -> SqliteQueuedMessageRepository {
    let conn = Connection::open_in_memory().expect("create in-memory db");
    conn.execute_batch(
        "CREATE TABLE queued_messages (
            id TEXT PRIMARY KEY,
            context_type TEXT NOT NULL,
            context_id TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at TEXT NOT NULL,
            is_editing INTEGER NOT NULL DEFAULT 0,
            sequence INTEGER NOT NULL,
            payload_json TEXT NOT NULL,
            inserted_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX idx_queued_messages_context_order
            ON queued_messages(context_type, context_id, sequence);",
    )
    .unwrap();
    SqliteQueuedMessageRepository::new(conn)
}

#[tokio::test]
async fn persists_full_payload_across_repository_instances() {
    let conn = Connection::open_in_memory().expect("create in-memory db");
    conn.execute_batch(
        "CREATE TABLE queued_messages (
            id TEXT PRIMARY KEY,
            context_type TEXT NOT NULL,
            context_id TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at TEXT NOT NULL,
            is_editing INTEGER NOT NULL DEFAULT 0,
            sequence INTEGER NOT NULL,
            payload_json TEXT NOT NULL,
            inserted_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );",
    )
    .unwrap();
    let shared = Arc::new(Mutex::new(conn));
    let writer = SqliteQueuedMessageRepository::from_shared(Arc::clone(&shared));
    let reader = SqliteQueuedMessageRepository::from_shared(shared);
    let key = QueueKey::new(ChatContextType::Project, "project-1");
    let mut message = QueuedMessage::with_id("queued-1".to_string(), "Prompt".to_string());
    message.metadata_override = Some(r#"{"resume_in_place":true}"#.to_string());
    message.attachment_ids.push(ChatAttachmentId::from_string(
        "00000000-0000-0000-0000-000000000001",
    ));

    writer.enqueue_back(&key, &message).await.unwrap();

    let queued = reader.list(&key).await.unwrap();
    assert_eq!(queued, vec![message]);
}

#[tokio::test]
async fn front_insert_controls_drain_order() {
    let repo = setup_repo();
    let key = QueueKey::new(ChatContextType::Ideation, "session-1");
    let first = QueuedMessage::with_id("first".to_string(), "First".to_string());
    let second = QueuedMessage::with_id("second".to_string(), "Second".to_string());
    let front = QueuedMessage::with_id("front".to_string(), "Front".to_string());

    repo.enqueue_back(&key, &first).await.unwrap();
    repo.enqueue_back(&key, &second).await.unwrap();
    repo.enqueue_front(&key, &front).await.unwrap();

    assert_eq!(repo.pop_front(&key).await.unwrap().unwrap().id, "front");
    assert_eq!(repo.pop_front(&key).await.unwrap().unwrap().id, "first");
    assert_eq!(repo.pop_front(&key).await.unwrap().unwrap().id, "second");
    assert!(repo.pop_front(&key).await.unwrap().is_none());
}

#[tokio::test]
async fn delete_removes_only_matching_context_row() {
    let repo = setup_repo();
    let project_key = QueueKey::new(ChatContextType::Project, "shared-id");
    let task_key = QueueKey::new(ChatContextType::Task, "shared-id");
    let project = QueuedMessage::with_id("project".to_string(), "Project".to_string());
    let task = QueuedMessage::with_id("task".to_string(), "Task".to_string());

    repo.enqueue_back(&project_key, &project).await.unwrap();
    repo.enqueue_back(&task_key, &task).await.unwrap();

    assert!(repo
        .delete(&project_key, "project")
        .await
        .expect("delete queued message"));
    assert!(repo.list(&project_key).await.unwrap().is_empty());
    assert_eq!(repo.list(&task_key).await.unwrap(), vec![task]);
}
