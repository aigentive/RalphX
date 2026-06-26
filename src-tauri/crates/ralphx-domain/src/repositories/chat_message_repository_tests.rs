use super::*;
use crate::agents::ProviderSessionRef;
use crate::domain::entities::{AgentRunUsage, ChatMessageAttribution};
use std::sync::Arc;

// Mock implementation for testing trait object usage
struct MockChatMessageRepository {
    return_message: Option<ChatMessage>,
    messages: Vec<ChatMessage>,
}

impl MockChatMessageRepository {
    fn new() -> Self {
        Self {
            return_message: None,
            messages: vec![],
        }
    }

    fn with_message(message: ChatMessage) -> Self {
        Self {
            return_message: Some(message.clone()),
            messages: vec![message],
        }
    }

    fn with_messages(messages: Vec<ChatMessage>) -> Self {
        Self {
            return_message: messages.first().cloned(),
            messages,
        }
    }
}

#[async_trait]
impl ChatMessageRepository for MockChatMessageRepository {
    async fn create(&self, message: ChatMessage) -> AppResult<ChatMessage> {
        Ok(message)
    }

    async fn get_by_id(&self, _id: &ChatMessageId) -> AppResult<Option<ChatMessage>> {
        Ok(self.return_message.clone())
    }

    async fn get_by_session(&self, session_id: &IdeationSessionId) -> AppResult<Vec<ChatMessage>> {
        let mut filtered: Vec<_> = self
            .messages
            .iter()
            .filter(|m| m.session_id.as_ref() == Some(session_id))
            .cloned()
            .collect();
        filtered.sort_by_key(|m| m.created_at);
        Ok(filtered)
    }

    async fn get_by_project(&self, project_id: &ProjectId) -> AppResult<Vec<ChatMessage>> {
        let mut filtered: Vec<_> = self
            .messages
            .iter()
            .filter(|m| m.project_id.as_ref() == Some(project_id) && m.session_id.is_none())
            .cloned()
            .collect();
        filtered.sort_by_key(|m| m.created_at);
        Ok(filtered)
    }

    async fn get_by_task(&self, task_id: &TaskId) -> AppResult<Vec<ChatMessage>> {
        let mut filtered: Vec<_> = self
            .messages
            .iter()
            .filter(|m| m.task_id.as_ref() == Some(task_id))
            .cloned()
            .collect();
        filtered.sort_by_key(|m| m.created_at);
        Ok(filtered)
    }

    async fn get_by_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<ChatMessage>> {
        let mut filtered: Vec<_> = self
            .messages
            .iter()
            .filter(|m| m.conversation_id.as_ref() == Some(conversation_id))
            .cloned()
            .collect();
        filtered.sort_by_key(|m| m.created_at);
        Ok(filtered)
    }

    async fn get_recent_by_conversation_paginated(
        &self,
        conversation_id: &ChatConversationId,
        limit: u32,
        offset: u32,
    ) -> AppResult<Vec<ChatMessage>> {
        let mut filtered: Vec<_> = self
            .messages
            .iter()
            .filter(|m| m.conversation_id.as_ref() == Some(conversation_id))
            .cloned()
            .collect();
        filtered.sort_by_key(|m| std::cmp::Reverse(m.created_at));
        let mut filtered: Vec<_> = filtered
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .collect();
        filtered.reverse();
        Ok(filtered)
    }

    async fn delete_by_session(&self, _session_id: &IdeationSessionId) -> AppResult<()> {
        Ok(())
    }

    async fn delete_by_project(&self, _project_id: &ProjectId) -> AppResult<()> {
        Ok(())
    }

    async fn delete_by_task(&self, _task_id: &TaskId) -> AppResult<()> {
        Ok(())
    }

    async fn delete(&self, _id: &ChatMessageId) -> AppResult<()> {
        Ok(())
    }

    async fn count_by_session(&self, session_id: &IdeationSessionId) -> AppResult<u32> {
        use crate::domain::entities::MessageRole;
        Ok(self
            .messages
            .iter()
            .filter(|m| {
                m.session_id.as_ref() == Some(session_id)
                    && matches!(m.role, MessageRole::User | MessageRole::Orchestrator)
            })
            .count() as u32)
    }

    async fn get_recent_by_session(
        &self,
        session_id: &IdeationSessionId,
        limit: u32,
    ) -> AppResult<Vec<ChatMessage>> {
        use crate::domain::entities::MessageRole;
        let mut filtered: Vec<_> = self
            .messages
            .iter()
            .filter(|m| {
                m.session_id.as_ref() == Some(session_id)
                    && matches!(m.role, MessageRole::User | MessageRole::Orchestrator)
            })
            .cloned()
            .collect();
        filtered.sort_by_key(|m| std::cmp::Reverse(m.created_at));
        filtered.truncate(limit as usize);
        filtered.reverse(); // Return in ascending order
        Ok(filtered)
    }

    async fn get_recent_by_session_paginated(
        &self,
        session_id: &IdeationSessionId,
        limit: u32,
        offset: u32,
    ) -> AppResult<Vec<ChatMessage>> {
        use crate::domain::entities::MessageRole;
        let mut filtered: Vec<_> = self
            .messages
            .iter()
            .filter(|m| {
                m.session_id.as_ref() == Some(session_id)
                    && matches!(m.role, MessageRole::User | MessageRole::Orchestrator)
            })
            .cloned()
            .collect();
        filtered.sort_by_key(|m| std::cmp::Reverse(m.created_at));
        let mut filtered: Vec<_> = filtered
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .collect();
        filtered.reverse();
        Ok(filtered)
    }

    async fn update_content(
        &self,
        _id: &ChatMessageId,
        _content: &str,
        _tool_calls: Option<&str>,
        _content_blocks: Option<&str>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn update_provider_session_ref(
        &self,
        _id: &ChatMessageId,
        _session_ref: &ProviderSessionRef,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn update_usage(&self, _id: &ChatMessageId, _usage: &AgentRunUsage) -> AppResult<()> {
        Ok(())
    }

    async fn update_attribution(
        &self,
        _id: &ChatMessageId,
        _attribution: &ChatMessageAttribution,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn count_unread_assistant_messages(
        &self,
        _session_id: &str,
        _after_message_id: Option<&str>,
    ) -> AppResult<u32> {
        Ok(0)
    }

    async fn count_unread_messages(
        &self,
        session_id: &str,
        cursor_message_id: Option<&str>,
    ) -> AppResult<i64> {
        use crate::domain::entities::MessageRole;
        let cursor_created_at = cursor_message_id.and_then(|id| {
            self.messages
                .iter()
                .find(|m| m.id.0.as_str() == id)
                .map(|m| m.created_at)
        });
        let count = self
            .messages
            .iter()
            .filter(|m| {
                m.session_id.as_ref().map(|s| s.as_str()) == Some(session_id)
                    && matches!(m.role, MessageRole::User | MessageRole::Orchestrator)
                    && cursor_created_at
                        .map(|cursor_ts| m.created_at > cursor_ts)
                        .unwrap_or(true)
            })
            .count();
        Ok(count as i64)
    }

    async fn get_first_user_message_by_context(
        &self,
        _context_type: &str,
        _context_id: &str,
    ) -> AppResult<Option<String>> {
        Ok(None)
    }

    async fn get_latest_message_by_role(
        &self,
        session_id: &IdeationSessionId,
        role: &str,
    ) -> AppResult<Option<ChatMessage>> {
        let mut matching: Vec<_> = self
            .messages
            .iter()
            .filter(|m| m.session_id.as_ref() == Some(session_id) && m.role.to_string() == role)
            .cloned()
            .collect();
        matching.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(matching.into_iter().next())
    }

    async fn exists_verification_result_in_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<bool> {
        Ok(self.messages.iter().any(|m| {
            m.conversation_id.as_ref() == Some(conversation_id)
                && m.content.contains("<verification-result>")
        }))
    }
}

fn create_test_message_in_session(session_id: &IdeationSessionId) -> ChatMessage {
    ChatMessage::user_in_session(session_id.clone(), "Test message")
}

fn create_test_message_in_project(project_id: &ProjectId) -> ChatMessage {
    ChatMessage::user_in_project(project_id.clone(), "Test message")
}

fn create_test_message_about_task(task_id: &TaskId) -> ChatMessage {
    ChatMessage::user_about_task(task_id.clone(), "Test message")
}

#[test]
fn test_chat_message_repository_trait_can_be_object_safe() {
    // Verify that ChatMessageRepository can be used as a trait object
    let repo: Arc<dyn ChatMessageRepository> = Arc::new(MockChatMessageRepository::new());
    assert!(Arc::strong_count(&repo) == 1);
}

#[tokio::test]
async fn test_mock_repository_create() {
    let repo = MockChatMessageRepository::new();
    let session_id = IdeationSessionId::new();
    let message = create_test_message_in_session(&session_id);

    let result = repo.create(message.clone()).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().id, message.id);
}

#[tokio::test]
async fn test_mock_repository_get_by_id_returns_none() {
    let repo = MockChatMessageRepository::new();
    let message_id = ChatMessageId::new();

    let result = repo.get_by_id(&message_id).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[tokio::test]
async fn test_mock_repository_get_by_id_returns_message() {
    let session_id = IdeationSessionId::new();
    let message = create_test_message_in_session(&session_id);
    let repo = MockChatMessageRepository::with_message(message.clone());

    let result = repo.get_by_id(&message.id).await;
    assert!(result.is_ok());
    let returned = result.unwrap();
    assert!(returned.is_some());
    assert_eq!(returned.unwrap().id, message.id);
}

#[tokio::test]
async fn test_mock_repository_get_by_session_empty() {
    let repo = MockChatMessageRepository::new();
    let session_id = IdeationSessionId::new();

    let result = repo.get_by_session(&session_id).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_mock_repository_get_by_session_with_messages() {
    let session_id = IdeationSessionId::new();
    let message1 = create_test_message_in_session(&session_id);
    let message2 = create_test_message_in_session(&session_id);

    let repo = MockChatMessageRepository::with_messages(vec![message1.clone(), message2.clone()]);

    let result = repo.get_by_session(&session_id).await;
    assert!(result.is_ok());
    let messages = result.unwrap();
    assert_eq!(messages.len(), 2);
}

#[tokio::test]
async fn test_mock_repository_get_by_session_filters_by_session() {
    let session_id1 = IdeationSessionId::new();
    let session_id2 = IdeationSessionId::new();
    let message1 = create_test_message_in_session(&session_id1);
    let message2 = create_test_message_in_session(&session_id2);

    let repo = MockChatMessageRepository::with_messages(vec![message1.clone(), message2.clone()]);

    let result = repo.get_by_session(&session_id1).await;
    assert!(result.is_ok());
    let messages = result.unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].session_id, Some(session_id1));
}

#[tokio::test]
async fn test_mock_repository_get_by_project_empty() {
    let repo = MockChatMessageRepository::new();
    let project_id = ProjectId::new();

    let result = repo.get_by_project(&project_id).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_mock_repository_get_by_project_with_messages() {
    let project_id = ProjectId::new();
    let message1 = create_test_message_in_project(&project_id);
    let message2 = create_test_message_in_project(&project_id);

    let repo = MockChatMessageRepository::with_messages(vec![message1.clone(), message2.clone()]);

    let result = repo.get_by_project(&project_id).await;
    assert!(result.is_ok());
    let messages = result.unwrap();
    assert_eq!(messages.len(), 2);
}

#[tokio::test]
async fn test_mock_repository_get_by_task_empty() {
    let repo = MockChatMessageRepository::new();
    let task_id = TaskId::new();

    let result = repo.get_by_task(&task_id).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_mock_repository_get_by_task_with_messages() {
    let task_id = TaskId::new();
    let message1 = create_test_message_about_task(&task_id);
    let message2 = create_test_message_about_task(&task_id);

    let repo = MockChatMessageRepository::with_messages(vec![message1.clone(), message2.clone()]);

    let result = repo.get_by_task(&task_id).await;
    assert!(result.is_ok());
    let messages = result.unwrap();
    assert_eq!(messages.len(), 2);
}

#[tokio::test]
async fn test_mock_repository_delete_by_session() {
    let repo = MockChatMessageRepository::new();
    let session_id = IdeationSessionId::new();

    let result = repo.delete_by_session(&session_id).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_mock_repository_delete_by_project() {
    let repo = MockChatMessageRepository::new();
    let project_id = ProjectId::new();

    let result = repo.delete_by_project(&project_id).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_mock_repository_delete_by_task() {
    let repo = MockChatMessageRepository::new();
    let task_id = TaskId::new();

    let result = repo.delete_by_task(&task_id).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_mock_repository_delete() {
    let repo = MockChatMessageRepository::new();
    let message_id = ChatMessageId::new();

    let result = repo.delete(&message_id).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_mock_repository_count_by_session_zero() {
    let repo = MockChatMessageRepository::new();
    let session_id = IdeationSessionId::new();

    let result = repo.count_by_session(&session_id).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 0);
}

#[tokio::test]
async fn test_mock_repository_count_by_session_counts_correctly() {
    let session_id = IdeationSessionId::new();
    let other_session_id = IdeationSessionId::new();
    let message1 = create_test_message_in_session(&session_id);
    let message2 = create_test_message_in_session(&session_id);
    let message3 = create_test_message_in_session(&other_session_id);

    let repo = MockChatMessageRepository::with_messages(vec![
        message1.clone(),
        message2.clone(),
        message3.clone(),
    ]);

    let result = repo.count_by_session(&session_id).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 2);
}

#[tokio::test]
async fn test_mock_repository_get_recent_by_session() {
    let session_id = IdeationSessionId::new();
    let message1 = create_test_message_in_session(&session_id);
    let message2 = create_test_message_in_session(&session_id);
    let message3 = create_test_message_in_session(&session_id);

    let repo = MockChatMessageRepository::with_messages(vec![
        message1.clone(),
        message2.clone(),
        message3.clone(),
    ]);

    let result = repo.get_recent_by_session(&session_id, 2).await;
    assert!(result.is_ok());
    let messages = result.unwrap();
    assert_eq!(messages.len(), 2);
}

#[tokio::test]
async fn test_repository_trait_object_in_arc() {
    let session_id = IdeationSessionId::new();
    let message = create_test_message_in_session(&session_id);
    let repo: Arc<dyn ChatMessageRepository> =
        Arc::new(MockChatMessageRepository::with_message(message.clone()));

    // Use through trait object
    let result = repo.get_by_id(&message.id).await;
    assert!(result.is_ok());

    let all = repo.get_by_session(&session_id).await;
    assert!(all.is_ok());
    assert_eq!(all.unwrap().len(), 1);
}

#[tokio::test]
async fn test_repository_trait_object_delete_operations() {
    let session_id = IdeationSessionId::new();
    let message = create_test_message_in_session(&session_id);
    let repo: Arc<dyn ChatMessageRepository> =
        Arc::new(MockChatMessageRepository::with_message(message.clone()));

    let result = repo.delete_by_session(&session_id).await;
    assert!(result.is_ok());

    let result = repo.delete(&message.id).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_repository_trait_object_count_and_recent() {
    let session_id = IdeationSessionId::new();
    let message = create_test_message_in_session(&session_id);
    let repo: Arc<dyn ChatMessageRepository> =
        Arc::new(MockChatMessageRepository::with_message(message.clone()));

    let count = repo.count_by_session(&session_id).await;
    assert!(count.is_ok());
    assert_eq!(count.unwrap(), 1);

    let recent = repo.get_recent_by_session(&session_id, 10).await;
    assert!(recent.is_ok());
    assert_eq!(recent.unwrap().len(), 1);
}

// --- count_unread_messages repo-level tests ---

fn make_session_message(
    session_id: &IdeationSessionId,
    role: crate::domain::entities::MessageRole,
    created_at: chrono::DateTime<chrono::Utc>,
) -> ChatMessage {
    let mut msg = ChatMessage::user_in_session(session_id.clone(), "test");
    msg.role = role;
    msg.created_at = created_at;
    msg
}

#[tokio::test]
async fn test_count_unread_messages_null_cursor_counts_all_user_orchestrator() {
    use crate::domain::entities::MessageRole;
    let session_id = IdeationSessionId::new();
    let base = chrono::Utc::now();
    let user_msg = make_session_message(&session_id, MessageRole::User, base);
    let orch_msg = make_session_message(
        &session_id,
        MessageRole::Orchestrator,
        base + chrono::Duration::seconds(1),
    );
    let sys_msg = make_session_message(
        &session_id,
        MessageRole::System,
        base + chrono::Duration::seconds(2),
    );

    let repo = MockChatMessageRepository::with_messages(vec![
        user_msg.clone(),
        orch_msg.clone(),
        sys_msg.clone(),
    ]);

    let count = repo
        .count_unread_messages(session_id.as_str(), None)
        .await
        .unwrap();
    assert_eq!(
        count, 2,
        "NULL cursor must count all User+Orchestrator messages"
    );
}

#[tokio::test]
async fn test_count_unread_messages_cursor_counts_only_after_cursor() {
    use crate::domain::entities::MessageRole;
    let session_id = IdeationSessionId::new();
    let base = chrono::Utc::now();
    let msg1 = make_session_message(&session_id, MessageRole::User, base);
    let msg2 = make_session_message(
        &session_id,
        MessageRole::User,
        base + chrono::Duration::seconds(1),
    );
    let msg3 = make_session_message(
        &session_id,
        MessageRole::User,
        base + chrono::Duration::seconds(2),
    );

    let repo =
        MockChatMessageRepository::with_messages(vec![msg1.clone(), msg2.clone(), msg3.clone()]);

    // With cursor = msg1.id: should count msg2 and msg3 (created after msg1)
    let count = repo
        .count_unread_messages(session_id.as_str(), Some(msg1.id.0.as_str()))
        .await
        .unwrap();
    assert_eq!(
        count, 2,
        "Cursor branch must count only messages after cursor"
    );
}

#[tokio::test]
async fn test_count_unread_messages_excludes_system_worker_reviewer_merger() {
    use crate::domain::entities::MessageRole;
    let session_id = IdeationSessionId::new();
    let base = chrono::Utc::now();

    let messages = vec![
        make_session_message(&session_id, MessageRole::User, base),
        make_session_message(
            &session_id,
            MessageRole::Orchestrator,
            base + chrono::Duration::seconds(1),
        ),
        make_session_message(
            &session_id,
            MessageRole::System,
            base + chrono::Duration::seconds(2),
        ),
        make_session_message(
            &session_id,
            MessageRole::Worker,
            base + chrono::Duration::seconds(3),
        ),
        make_session_message(
            &session_id,
            MessageRole::Reviewer,
            base + chrono::Duration::seconds(4),
        ),
        make_session_message(
            &session_id,
            MessageRole::Merger,
            base + chrono::Duration::seconds(5),
        ),
    ];

    let repo = MockChatMessageRepository::with_messages(messages);

    let count = repo
        .count_unread_messages(session_id.as_str(), None)
        .await
        .unwrap();
    assert_eq!(
        count, 2,
        "System/Worker/Reviewer/Merger must NOT be counted (deadlock prevention)"
    );
}

#[tokio::test]
async fn test_count_unread_messages_parity_null_and_cursor_contract() {
    use crate::domain::entities::MessageRole;
    let session_id = IdeationSessionId::new();
    let base = chrono::Utc::now();
    let msg1 = make_session_message(&session_id, MessageRole::User, base);
    let msg2 = make_session_message(
        &session_id,
        MessageRole::Orchestrator,
        base + chrono::Duration::seconds(1),
    );
    let msg3 = make_session_message(
        &session_id,
        MessageRole::User,
        base + chrono::Duration::seconds(2),
    );

    let repo =
        MockChatMessageRepository::with_messages(vec![msg1.clone(), msg2.clone(), msg3.clone()]);

    // NULL cursor: all 3 visible messages
    let total = repo
        .count_unread_messages(session_id.as_str(), None)
        .await
        .unwrap();
    assert_eq!(
        total, 3,
        "NULL cursor should count all User+Orchestrator messages"
    );

    // Cursor at msg1: should return total - 1 (only msg2 and msg3 are after msg1)
    let after_first = repo
        .count_unread_messages(session_id.as_str(), Some(msg1.id.0.as_str()))
        .await
        .unwrap();
    assert_eq!(
        after_first,
        total - 1,
        "Cursor at first message leaves total-1 messages unread"
    );

    // Cursor at msg3 (last): nothing after it
    let after_last = repo
        .count_unread_messages(session_id.as_str(), Some(msg3.id.0.as_str()))
        .await
        .unwrap();
    assert_eq!(
        after_last, 0,
        "Cursor at last message leaves 0 unread messages"
    );
}
