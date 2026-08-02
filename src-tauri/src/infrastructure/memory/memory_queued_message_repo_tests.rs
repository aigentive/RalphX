use crate::domain::entities::ChatContextType;
use crate::domain::repositories::QueuedMessageRepository;
use crate::domain::services::{QueueKey, QueuedMessage};
use crate::infrastructure::memory::MemoryQueuedMessageRepository;

#[tokio::test]
async fn preserves_front_and_back_order() {
    let repo = MemoryQueuedMessageRepository::new();
    let key = QueueKey::new(ChatContextType::Project, "project-1");
    let first = QueuedMessage::with_id("first".to_string(), "First".to_string());
    let second = QueuedMessage::with_id("second".to_string(), "Second".to_string());
    let urgent = QueuedMessage::with_id("urgent".to_string(), "Urgent".to_string());

    repo.enqueue_back(&key, &first).await.unwrap();
    repo.enqueue_back(&key, &second).await.unwrap();
    repo.enqueue_front(&key, &urgent).await.unwrap();

    let queued = repo.list(&key).await.unwrap();
    assert_eq!(
        queued
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        vec!["urgent", "first", "second"]
    );
}

#[tokio::test]
async fn pop_front_removes_only_selected_key() {
    let repo = MemoryQueuedMessageRepository::new();
    let project_key = QueueKey::new(ChatContextType::Project, "project-1");
    let task_key = QueueKey::new(ChatContextType::Task, "task-1");
    let project = QueuedMessage::with_id("project".to_string(), "Project".to_string());
    let task = QueuedMessage::with_id("task".to_string(), "Task".to_string());

    repo.enqueue_back(&project_key, &project).await.unwrap();
    repo.enqueue_back(&task_key, &task).await.unwrap();

    let popped = repo.pop_front(&project_key).await.unwrap().unwrap();
    assert_eq!(popped.id, "project");
    assert!(repo.list(&project_key).await.unwrap().is_empty());
    assert_eq!(repo.list(&task_key).await.unwrap().len(), 1);
}

#[tokio::test]
async fn delete_clear_and_list_keys_cover_empty_and_missing_paths() {
    let repo = MemoryQueuedMessageRepository::default();
    let project_key = QueueKey::new(ChatContextType::Project, "project-1");
    let task_key = QueueKey::new(ChatContextType::Task, "task-1");
    let project = QueuedMessage::with_id("project".to_string(), "Project".to_string());
    let task = QueuedMessage::with_id("task".to_string(), "Task".to_string());

    assert!(!repo.delete(&project_key, "missing").await.unwrap());
    assert!(repo.pop_front(&project_key).await.unwrap().is_none());
    assert!(repo.remove_stale(&project_key, 1).await.unwrap().is_empty());

    repo.enqueue_back(&project_key, &project).await.unwrap();
    repo.enqueue_back(&task_key, &task).await.unwrap();

    let keys = repo.list_keys().await.unwrap();
    assert_eq!(keys.len(), 2);
    assert!(keys.contains(&project_key));
    assert!(keys.contains(&task_key));

    assert!(!repo.delete(&project_key, "other").await.unwrap());
    assert!(repo.delete_by_id("project").await.unwrap());
    assert!(repo.list(&project_key).await.unwrap().is_empty());
    assert_eq!(repo.list_keys().await.unwrap(), vec![task_key.clone()]);

    repo.clear(&task_key).await.unwrap();
    assert!(repo.list_keys().await.unwrap().is_empty());
}

#[tokio::test]
async fn remove_stale_retains_old_user_messages_and_drops_hidden_recovery_messages() {
    let repo = MemoryQueuedMessageRepository::new();
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
