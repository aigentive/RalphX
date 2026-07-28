use async_trait::async_trait;

use ralphx_lib::domain::agents::ProviderSessionRef;
use ralphx_lib::domain::entities::{
    AgentRunUsage, ChatConversationId, ChatMessage, ChatMessageAttribution, ChatMessageId,
    IdeationSessionId, ProjectId, TaskId,
};
use ralphx_lib::domain::repositories::ChatMessageRepository;
use ralphx_lib::error::{AppError, AppResult};
use ralphx_lib::infrastructure::memory::MemoryChatMessageRepository;

pub const CHAT_MESSAGE_CREATE_FAILURE: &str = "forced chat-message create failure";

/// Test-only repository that fails the write under test while preserving normal read seams.
pub struct FailingChatMessageRepository {
    inner: MemoryChatMessageRepository,
}

impl FailingChatMessageRepository {
    pub fn new() -> Self {
        Self {
            inner: MemoryChatMessageRepository::new(),
        }
    }
}

#[async_trait]
impl ChatMessageRepository for FailingChatMessageRepository {
    async fn create(&self, _message: ChatMessage) -> AppResult<ChatMessage> {
        Err(AppError::Infrastructure(
            CHAT_MESSAGE_CREATE_FAILURE.to_string(),
        ))
    }

    async fn get_by_id(&self, id: &ChatMessageId) -> AppResult<Option<ChatMessage>> {
        self.inner.get_by_id(id).await
    }

    async fn get_by_session(&self, session_id: &IdeationSessionId) -> AppResult<Vec<ChatMessage>> {
        self.inner.get_by_session(session_id).await
    }

    async fn get_by_project(&self, project_id: &ProjectId) -> AppResult<Vec<ChatMessage>> {
        self.inner.get_by_project(project_id).await
    }

    async fn get_by_task(&self, task_id: &TaskId) -> AppResult<Vec<ChatMessage>> {
        self.inner.get_by_task(task_id).await
    }

    async fn get_by_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<ChatMessage>> {
        self.inner.get_by_conversation(conversation_id).await
    }

    async fn get_recent_by_conversation_paginated(
        &self,
        conversation_id: &ChatConversationId,
        limit: u32,
        offset: u32,
    ) -> AppResult<Vec<ChatMessage>> {
        self.inner
            .get_recent_by_conversation_paginated(conversation_id, limit, offset)
            .await
    }

    async fn delete_by_session(&self, session_id: &IdeationSessionId) -> AppResult<()> {
        self.inner.delete_by_session(session_id).await
    }

    async fn delete_by_project(&self, project_id: &ProjectId) -> AppResult<()> {
        self.inner.delete_by_project(project_id).await
    }

    async fn delete_by_task(&self, task_id: &TaskId) -> AppResult<()> {
        self.inner.delete_by_task(task_id).await
    }

    async fn delete(&self, id: &ChatMessageId) -> AppResult<()> {
        self.inner.delete(id).await
    }

    async fn count_by_session(&self, session_id: &IdeationSessionId) -> AppResult<u32> {
        self.inner.count_by_session(session_id).await
    }

    async fn get_recent_by_session(
        &self,
        session_id: &IdeationSessionId,
        limit: u32,
    ) -> AppResult<Vec<ChatMessage>> {
        self.inner.get_recent_by_session(session_id, limit).await
    }

    async fn get_recent_by_session_paginated(
        &self,
        session_id: &IdeationSessionId,
        limit: u32,
        offset: u32,
    ) -> AppResult<Vec<ChatMessage>> {
        self.inner
            .get_recent_by_session_paginated(session_id, limit, offset)
            .await
    }

    async fn update_content(
        &self,
        id: &ChatMessageId,
        content: &str,
        tool_calls: Option<&str>,
        content_blocks: Option<&str>,
    ) -> AppResult<()> {
        self.inner
            .update_content(id, content, tool_calls, content_blocks)
            .await
    }

    async fn update_provider_session_ref(
        &self,
        id: &ChatMessageId,
        session_ref: &ProviderSessionRef,
    ) -> AppResult<()> {
        self.inner
            .update_provider_session_ref(id, session_ref)
            .await
    }

    async fn update_usage(&self, id: &ChatMessageId, usage: &AgentRunUsage) -> AppResult<()> {
        self.inner.update_usage(id, usage).await
    }

    async fn update_attribution(
        &self,
        id: &ChatMessageId,
        attribution: &ChatMessageAttribution,
    ) -> AppResult<()> {
        self.inner.update_attribution(id, attribution).await
    }

    async fn count_unread_assistant_messages(
        &self,
        session_id: &str,
        after_message_id: Option<&str>,
    ) -> AppResult<u32> {
        self.inner
            .count_unread_assistant_messages(session_id, after_message_id)
            .await
    }

    async fn count_unread_messages(
        &self,
        session_id: &str,
        cursor_message_id: Option<&str>,
    ) -> AppResult<i64> {
        self.inner
            .count_unread_messages(session_id, cursor_message_id)
            .await
    }

    async fn get_first_user_message_by_context(
        &self,
        context_type: &str,
        context_id: &str,
    ) -> AppResult<Option<String>> {
        self.inner
            .get_first_user_message_by_context(context_type, context_id)
            .await
    }

    async fn get_latest_message_by_role(
        &self,
        session_id: &IdeationSessionId,
        role: &str,
    ) -> AppResult<Option<ChatMessage>> {
        self.inner
            .get_latest_message_by_role(session_id, role)
            .await
    }

    async fn exists_verification_result_in_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<bool> {
        self.inner
            .exists_verification_result_in_conversation(conversation_id)
            .await
    }
}
