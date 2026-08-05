use std::sync::Arc;

use async_trait::async_trait;

use super::remote_queue_commands::*;
use crate::application::AppState;
use crate::domain::entities::{ChatContextType, ChatConversation, ProjectId, TaskId};
use crate::domain::repositories::QueuedMessageRepository;
use crate::domain::services::{QueueKey, QueuedMessage};
use crate::error::{AppError, AppResult};
use crate::infrastructure::memory::MemoryQueuedMessageRepository;

struct FaultInjectingQueuedMessageRepository {
    inner: MemoryQueuedMessageRepository,
    fail_list: bool,
    fail_delete: bool,
}

impl FaultInjectingQueuedMessageRepository {
    fn new(fail_list: bool, fail_delete: bool) -> Self {
        Self {
            inner: MemoryQueuedMessageRepository::new(),
            fail_list,
            fail_delete,
        }
    }
}

#[async_trait]
impl QueuedMessageRepository for FaultInjectingQueuedMessageRepository {
    async fn enqueue_back(&self, key: &QueueKey, message: &QueuedMessage) -> AppResult<()> {
        self.inner.enqueue_back(key, message).await
    }
    async fn enqueue_front(&self, key: &QueueKey, message: &QueuedMessage) -> AppResult<()> {
        self.inner.enqueue_front(key, message).await
    }
    async fn list(&self, key: &QueueKey) -> AppResult<Vec<QueuedMessage>> {
        if self.fail_list {
            return Err(AppError::Database(
                "simulated queue list failure".to_string(),
            ));
        }
        self.inner.list(key).await
    }
    async fn list_keys(&self) -> AppResult<Vec<QueueKey>> {
        self.inner.list_keys().await
    }
    async fn delete(&self, key: &QueueKey, message_id: &str) -> AppResult<bool> {
        if self.fail_delete {
            return Err(AppError::Database(
                "simulated queue delete failure".to_string(),
            ));
        }
        self.inner.delete(key, message_id).await
    }
    async fn delete_by_id(&self, message_id: &str) -> AppResult<bool> {
        self.inner.delete_by_id(message_id).await
    }
    async fn clear(&self, key: &QueueKey) -> AppResult<()> {
        self.inner.clear(key).await
    }
    async fn pop_front(&self, key: &QueueKey) -> AppResult<Option<QueuedMessage>> {
        self.inner.pop_front(key).await
    }
    async fn remove_stale(
        &self,
        key: &QueueKey,
        threshold_secs: u64,
    ) -> AppResult<Vec<QueuedMessage>> {
        self.inner.remove_stale(key, threshold_secs).await
    }
}

async fn seed_project_conversation(state: &AppState) -> ChatConversation {
    state
        .chat_conversation_repo
        .create(ChatConversation::new_project(ProjectId::from_string(
            "project-1".to_string(),
        )))
        .await
        .expect("seed project conversation")
}

fn message(id: &str, content: &str, metadata: Option<&str>) -> QueuedMessage {
    let mut message = QueuedMessage::with_id(id.to_string(), content.to_string());
    message.metadata_override = metadata.map(str::to_string);
    message
}

#[tokio::test]
async fn list_remote_queue_merges_durable_first_and_hides_internal_rows() {
    let state = AppState::new_test();
    let conversation = seed_project_conversation(&state).await;
    let key = QueueKey::new(ChatContextType::Project, conversation.id.as_str());

    for queued in [
        message("durable", "durable only", None),
        message("collision", "durable wins", None),
        message(
            "hidden",
            "must not leak",
            Some(r#"{"hidden_from_ui":true}"#),
        ),
        message(
            "recovery",
            "must not leak either",
            Some(r#"{"recovery_context":true}"#),
        ),
    ] {
        state
            .queued_message_repo
            .enqueue_back(&key, &queued)
            .await
            .expect("seed durable queue");
    }
    state.message_queue.queue_back_existing(
        ChatContextType::Project,
        conversation.id.as_str(),
        message("collision", "memory loses", None),
    );
    state.message_queue.queue_back_existing(
        ChatContextType::Project,
        conversation.id.as_str(),
        message("memory", "memory only", None),
    );

    let response = list_remote_queued_agent_messages_for_state(&state, &conversation.id.as_str())
        .await
        .expect("queue hydrates");
    let rows: Vec<_> = response
        .iter()
        .map(|row| (row.id.as_str(), row.content.as_str()))
        .collect();
    assert_eq!(
        rows,
        vec![
            ("durable", "durable only"),
            ("collision", "durable wins"),
            ("memory", "memory only"),
        ]
    );
    assert!(response.iter().all(|row| row.id != "hidden"));
    assert!(response.iter().all(|row| row.id != "recovery"));
}

#[tokio::test]
async fn list_remote_queue_propagates_durable_read_failure() {
    let mut state = AppState::new_test();
    let conversation = seed_project_conversation(&state).await;
    state.queued_message_repo = Arc::new(FaultInjectingQueuedMessageRepository::new(true, false));
    state.message_queue.queue(
        ChatContextType::Project,
        conversation.id.as_str(),
        "memory must not become a successful response".to_string(),
    );

    let error = list_remote_queued_agent_messages_for_state(&state, &conversation.id.as_str())
        .await
        .expect_err("repository error must fail closed");
    assert!(error.contains("simulated queue list failure"));
}

#[tokio::test]
async fn delete_remote_queue_is_durable_first_on_failure() {
    let mut state = AppState::new_test();
    let conversation = seed_project_conversation(&state).await;
    let repo = Arc::new(FaultInjectingQueuedMessageRepository::new(false, true));
    let key = QueueKey::new(ChatContextType::Project, conversation.id.as_str());
    let queued = message("both", "keep memory on durable failure", None);
    repo.enqueue_back(&key, &queued)
        .await
        .expect("seed durable");
    state.queued_message_repo = repo;
    state.message_queue.queue_back_existing(
        ChatContextType::Project,
        conversation.id.as_str(),
        queued,
    );

    cancel_remote_queued_agent_message_for_state(&state, &conversation.id.as_str(), "both")
        .await
        .expect_err("durable delete fails");
    assert_eq!(
        state
            .message_queue
            .get_queued(ChatContextType::Project, &conversation.id.as_str())
            .len(),
        1,
        "memory half must remain when durable deletion fails"
    );
}

#[tokio::test]
async fn delete_remote_queue_reports_true_then_false_and_removes_both_halves() {
    let state = AppState::new_test();
    let conversation = seed_project_conversation(&state).await;
    let key = QueueKey::new(ChatContextType::Project, conversation.id.as_str());
    let queued = message("both", "delete me", None);
    state
        .queued_message_repo
        .enqueue_back(&key, &queued)
        .await
        .expect("seed durable");
    state.message_queue.queue_back_existing(
        ChatContextType::Project,
        conversation.id.as_str(),
        queued,
    );

    let first =
        cancel_remote_queued_agent_message_for_state(&state, &conversation.id.as_str(), "both")
            .await
            .expect("delete both");
    assert!(first.deleted);
    assert!(state
        .queued_message_repo
        .list(&key)
        .await
        .unwrap()
        .is_empty());
    assert!(state
        .message_queue
        .get_queued(ChatContextType::Project, &conversation.id.as_str())
        .is_empty());

    let second =
        cancel_remote_queued_agent_message_for_state(&state, &conversation.id.as_str(), "both")
            .await
            .expect("already absent is not an error");
    assert!(!second.deleted);
}

#[tokio::test]
async fn list_and_delete_refuse_missing_conversations() {
    let state = AppState::new_test();
    let missing = "missing-conversation";
    state
        .message_queue
        .queue(ChatContextType::Project, missing, "untouched".to_string());

    assert_eq!(
        list_remote_queued_agent_messages_for_state(&state, missing)
            .await
            .unwrap_err(),
        REMOTE_QUEUE_CONVERSATION_NOT_FOUND
    );
    assert_eq!(
        cancel_remote_queued_agent_message_for_state(&state, missing, "anything")
            .await
            .unwrap_err(),
        REMOTE_QUEUE_CONVERSATION_NOT_FOUND
    );
    assert_eq!(
        state
            .message_queue
            .get_queued(ChatContextType::Project, missing)
            .len(),
        1
    );
}

#[tokio::test]
async fn list_and_delete_refuse_archived_conversations_without_touching_queue() {
    let state = AppState::new_test();
    let conversation = seed_project_conversation(&state).await;
    let queued = state.message_queue.queue(
        ChatContextType::Project,
        conversation.id.as_str(),
        "untouched".to_string(),
    );
    state
        .chat_conversation_repo
        .archive(&conversation.id)
        .await
        .expect("archive conversation");

    assert_eq!(
        list_remote_queued_agent_messages_for_state(&state, &conversation.id.as_str())
            .await
            .unwrap_err(),
        REMOTE_QUEUE_CONVERSATION_ARCHIVED
    );
    assert_eq!(
        cancel_remote_queued_agent_message_for_state(&state, &conversation.id.as_str(), &queued.id)
            .await
            .unwrap_err(),
        REMOTE_QUEUE_CONVERSATION_ARCHIVED
    );
    assert_eq!(
        state
            .message_queue
            .get_queued(ChatContextType::Project, &conversation.id.as_str())
            .len(),
        1
    );
}

#[tokio::test]
async fn list_and_delete_refuse_non_project_conversations() {
    let state = AppState::new_test();
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_task(TaskId::from_string(
            "task-1".to_string(),
        )))
        .await
        .expect("seed task conversation");

    assert_eq!(
        list_remote_queued_agent_messages_for_state(&state, &conversation.id.as_str())
            .await
            .unwrap_err(),
        REMOTE_QUEUE_CONVERSATION_NOT_PROJECT
    );
    assert_eq!(
        cancel_remote_queued_agent_message_for_state(&state, &conversation.id.as_str(), "id")
            .await
            .unwrap_err(),
        REMOTE_QUEUE_CONVERSATION_NOT_PROJECT
    );
}
