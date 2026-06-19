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
