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
async fn enqueue_back_replaces_a_stable_id_at_the_back() {
    let repo = setup_repo();
    let key = QueueKey::new(ChatContextType::Ideation, "session-1");
    let first = QueuedMessage::with_id("first".to_string(), "First".to_string());
    let outdated = QueuedMessage::with_id("replace-me".to_string(), "Outdated".to_string());
    let third = QueuedMessage::with_id("third".to_string(), "Third".to_string());
    let replacement = QueuedMessage::with_id("replace-me".to_string(), "Updated".to_string());

    repo.enqueue_back(&key, &first).await.unwrap();
    repo.enqueue_back(&key, &outdated).await.unwrap();
    repo.enqueue_back(&key, &third).await.unwrap();
    repo.enqueue_back(&key, &replacement).await.unwrap();

    assert_eq!(
        repo.list(&key)
            .await
            .unwrap()
            .into_iter()
            .map(|message| message.id)
            .collect::<Vec<_>>(),
        vec!["first", "third", "replace-me"],
        "the upsert must move a replacement ID to the back without reordering other messages"
    );
    assert_eq!(repo.pop_front(&key).await.unwrap(), Some(first));
    assert_eq!(repo.pop_front(&key).await.unwrap(), Some(third));
    assert_eq!(repo.pop_front(&key).await.unwrap(), Some(replacement));
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

#[tokio::test]
async fn list_keys_delete_by_id_and_clear_cover_durable_index_paths() {
    let repo = setup_repo();
    let first_key = QueueKey::new(ChatContextType::Project, "project-1");
    let second_key = QueueKey::new(ChatContextType::Task, "task-1");
    let first = QueuedMessage::with_id("first".to_string(), "First".to_string());
    let second = QueuedMessage::with_id("second".to_string(), "Second".to_string());

    assert!(repo.pop_front(&first_key).await.unwrap().is_none());
    repo.enqueue_back(&first_key, &first).await.unwrap();
    repo.enqueue_back(&second_key, &second).await.unwrap();

    let keys = repo.list_keys().await.unwrap();
    assert_eq!(keys.len(), 2);
    assert!(keys.contains(&first_key));
    assert!(keys.contains(&second_key));

    assert!(!repo.delete_by_id("missing").await.unwrap());
    assert!(repo.delete_by_id("first").await.unwrap());
    assert!(repo.list(&first_key).await.unwrap().is_empty());
    assert_eq!(repo.list(&second_key).await.unwrap(), vec![second]);

    repo.clear(&second_key).await.unwrap();
    assert!(repo.list_keys().await.unwrap().is_empty());
}

#[tokio::test]
async fn remove_stale_retains_old_user_messages_and_drops_hidden_recovery_rows() {
    let repo = setup_repo();
    let key = QueueKey::new(ChatContextType::Project, "project-1");
    let mut stale = QueuedMessage::with_id("stale".to_string(), "Old".to_string());
    stale.created_at = "2020-01-01T00:00:00Z".to_string();
    let mut hidden_recovery = QueuedMessage::with_id(
        "hidden-recovery".to_string(),
        "Internal recovery".to_string(),
    );
    hidden_recovery.created_at = "2020-01-01T00:00:00Z".to_string();
    hidden_recovery.metadata_override = Some(
        r#"{"recovery_context":true,"recovery_reason":"silent_completion_after_tool_activity"}"#
            .to_string(),
    );
    let mut fresh = QueuedMessage::with_id("fresh".to_string(), "Fresh".to_string());
    fresh.created_at = chrono::Utc::now().to_rfc3339();
    let mut unparsable = QueuedMessage::with_id("unparsable".to_string(), "Keep".to_string());
    unparsable.created_at = "not a timestamp".to_string();

    repo.enqueue_back(&key, &stale).await.unwrap();
    repo.enqueue_back(&key, &hidden_recovery).await.unwrap();
    repo.enqueue_back(&key, &fresh).await.unwrap();
    repo.enqueue_back(&key, &unparsable).await.unwrap();

    let dropped = repo.remove_stale(&key, 60).await.unwrap();
    assert_eq!(dropped, vec![hidden_recovery]);
    assert_eq!(
        repo.list(&key)
            .await
            .unwrap()
            .into_iter()
            .map(|message| message.id)
            .collect::<Vec<_>>(),
        vec![
            "stale".to_string(),
            "fresh".to_string(),
            "unparsable".to_string()
        ]
    );
}

#[tokio::test]
async fn invalid_context_or_payload_rows_return_errors() {
    let conn = Arc::new(Mutex::new(
        Connection::open_in_memory().expect("create in-memory db"),
    ));
    {
        let guard = conn.lock().await;
        guard
            .execute_batch(
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
                INSERT INTO queued_messages (
                    id, context_type, context_id, content, created_at, is_editing,
                    sequence, payload_json, inserted_at, updated_at
                ) VALUES (
                    'invalid-context', 'unknown', 'ctx', 'Bad', '2026-06-19T10:00:00Z',
                    0, 1, '{}', '2026-06-19T10:00:00Z', '2026-06-19T10:00:00Z'
                );
                INSERT INTO queued_messages (
                    id, context_type, context_id, content, created_at, is_editing,
                    sequence, payload_json, inserted_at, updated_at
                ) VALUES (
                    'invalid-json', 'project', 'project-1', 'Bad', '2026-06-19T10:00:00Z',
                    0, 1, '{', '2026-06-19T10:00:00Z', '2026-06-19T10:00:00Z'
                );",
            )
            .unwrap();
    }
    let repo = SqliteQueuedMessageRepository::from_shared(conn);

    assert!(repo.list_keys().await.is_err());
    assert!(repo
        .list(&QueueKey::new(ChatContextType::Project, "project-1"))
        .await
        .is_err());
}
