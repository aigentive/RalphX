// In-memory ChatMessageRepository implementation for testing
// Uses RwLock<HashMap> for thread-safe in-memory storage

use std::cmp::Reverse;
use std::collections::HashMap;
use std::sync::RwLock;

use async_trait::async_trait;

use crate::domain::agents::ProviderSessionRef;
use crate::domain::entities::{
    AgentRunUsage, ChatConversationId, ChatMessage, ChatMessageAttribution, ChatMessageId,
    IdeationSessionId, MessageRole, ProjectId, TaskId, UsageCapture,
};
use crate::domain::repositories::ChatMessageRepository;
use crate::error::{AppError, AppResult};

/// In-memory implementation of ChatMessageRepository for testing
pub struct MemoryChatMessageRepository {
    messages: RwLock<HashMap<String, ChatMessage>>,
}

impl MemoryChatMessageRepository {
    /// Create a new empty repository
    pub fn new() -> Self {
        Self {
            messages: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for MemoryChatMessageRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChatMessageRepository for MemoryChatMessageRepository {
    async fn create(&self, message: ChatMessage) -> AppResult<ChatMessage> {
        self.messages
            .write()
            .unwrap()
            .insert(message.id.to_string(), message.clone());
        Ok(message)
    }

    async fn get_by_id(&self, id: &ChatMessageId) -> AppResult<Option<ChatMessage>> {
        Ok(self.messages.read().unwrap().get(&id.to_string()).cloned())
    }

    async fn get_by_session(&self, session_id: &IdeationSessionId) -> AppResult<Vec<ChatMessage>> {
        let mut messages: Vec<_> = self
            .messages
            .read()
            .unwrap()
            .values()
            .filter(|m| m.session_id.as_ref() == Some(session_id))
            .cloned()
            .collect();
        messages.sort_by_key(|m| m.created_at);
        Ok(messages)
    }

    async fn get_by_project(&self, project_id: &ProjectId) -> AppResult<Vec<ChatMessage>> {
        let mut messages: Vec<_> = self
            .messages
            .read()
            .unwrap()
            .values()
            .filter(|m| m.project_id.as_ref() == Some(project_id) && m.session_id.is_none())
            .cloned()
            .collect();
        messages.sort_by_key(|m| m.created_at);
        Ok(messages)
    }

    async fn get_by_task(&self, task_id: &TaskId) -> AppResult<Vec<ChatMessage>> {
        let mut messages: Vec<_> = self
            .messages
            .read()
            .unwrap()
            .values()
            .filter(|m| m.task_id.as_ref() == Some(task_id))
            .cloned()
            .collect();
        messages.sort_by_key(|m| m.created_at);
        Ok(messages)
    }

    async fn get_by_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<ChatMessage>> {
        let mut messages: Vec<_> = self
            .messages
            .read()
            .unwrap()
            .values()
            .filter(|m| m.conversation_id.as_ref() == Some(conversation_id))
            .cloned()
            .collect();
        messages.sort_by_key(|m| m.created_at);
        Ok(messages)
    }

    async fn get_recent_by_conversation_paginated(
        &self,
        conversation_id: &ChatConversationId,
        limit: u32,
        offset: u32,
    ) -> AppResult<Vec<ChatMessage>> {
        let mut messages: Vec<_> = self
            .messages
            .read()
            .unwrap()
            .values()
            .filter(|m| m.conversation_id.as_ref() == Some(conversation_id))
            .cloned()
            .collect();
        messages.sort_by_key(|m| Reverse(m.created_at));
        let mut messages: Vec<_> = messages
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .collect();
        messages.reverse();
        Ok(messages)
    }

    async fn delete_by_session(&self, session_id: &IdeationSessionId) -> AppResult<()> {
        self.messages
            .write()
            .unwrap()
            .retain(|_, m| m.session_id.as_ref() != Some(session_id));
        Ok(())
    }

    async fn delete_by_project(&self, project_id: &ProjectId) -> AppResult<()> {
        self.messages
            .write()
            .unwrap()
            .retain(|_, m| m.project_id.as_ref() != Some(project_id));
        Ok(())
    }

    async fn delete_by_task(&self, task_id: &TaskId) -> AppResult<()> {
        self.messages
            .write()
            .unwrap()
            .retain(|_, m| m.task_id.as_ref() != Some(task_id));
        Ok(())
    }

    async fn delete(&self, id: &ChatMessageId) -> AppResult<()> {
        self.messages.write().unwrap().remove(&id.to_string());
        Ok(())
    }

    async fn count_by_session(&self, session_id: &IdeationSessionId) -> AppResult<u32> {
        Ok(self
            .messages
            .read()
            .unwrap()
            .values()
            .filter(|m| m.session_id.as_ref() == Some(session_id))
            .count() as u32)
    }

    async fn get_recent_by_session(
        &self,
        session_id: &IdeationSessionId,
        limit: u32,
    ) -> AppResult<Vec<ChatMessage>> {
        let mut messages: Vec<_> = self
            .messages
            .read()
            .unwrap()
            .values()
            .filter(|m| m.session_id.as_ref() == Some(session_id))
            .cloned()
            .collect();
        messages.sort_by_key(|m| Reverse(m.created_at));
        messages.truncate(limit as usize);
        messages.reverse();
        Ok(messages)
    }

    async fn get_recent_by_session_paginated(
        &self,
        session_id: &IdeationSessionId,
        limit: u32,
        offset: u32,
    ) -> AppResult<Vec<ChatMessage>> {
        let mut messages: Vec<_> = self
            .messages
            .read()
            .unwrap()
            .values()
            .filter(|m| m.session_id.as_ref() == Some(session_id))
            .cloned()
            .collect();
        messages.sort_by_key(|m| Reverse(m.created_at));
        let mut messages: Vec<_> = messages
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .collect();
        messages.reverse();
        Ok(messages)
    }

    async fn update_content(
        &self,
        id: &ChatMessageId,
        content: &str,
        tool_calls: Option<&str>,
        content_blocks: Option<&str>,
    ) -> AppResult<()> {
        let mut messages = self.messages.write().unwrap();
        if let Some(msg) = messages.get_mut(&id.to_string()) {
            msg.content = content.to_string();
            msg.tool_calls = tool_calls.map(|s| s.to_string());
            msg.content_blocks = content_blocks.map(|s| s.to_string());
        }
        Ok(())
    }

    async fn update_provider_session_ref(
        &self,
        id: &ChatMessageId,
        session_ref: &ProviderSessionRef,
    ) -> AppResult<()> {
        let mut messages = self.messages.write().unwrap();
        if let Some(message) = messages.get_mut(&id.to_string()) {
            message.update_provider_session_ref(session_ref);
        }
        Ok(())
    }

    async fn update_usage(&self, id: &ChatMessageId, usage: &AgentRunUsage) -> AppResult<()> {
        let mut messages = self.messages.write().unwrap();
        if let Some(message) = messages.get_mut(&id.to_string()) {
            message.apply_usage(usage);
        }
        Ok(())
    }

    async fn replace_usage_capture(
        &self,
        id: &ChatMessageId,
        capture: &UsageCapture,
    ) -> AppResult<()> {
        let mut messages = self.messages.write().unwrap();
        let message = messages
            .get_mut(&id.to_string())
            .ok_or_else(|| AppError::NotFound(format!("Chat message not found: {id}")))?;
        message.replace_usage_capture(capture);
        Ok(())
    }

    async fn update_attribution(
        &self,
        id: &ChatMessageId,
        attribution: &ChatMessageAttribution,
    ) -> AppResult<()> {
        let mut messages = self.messages.write().unwrap();
        if let Some(message) = messages.get_mut(&id.to_string()) {
            message.apply_attribution(attribution);
        }
        Ok(())
    }

    async fn count_unread_assistant_messages(
        &self,
        session_id: &str,
        after_message_id: Option<&str>,
    ) -> AppResult<u32> {
        let messages = self.messages.read().unwrap();

        // Find the created_at of the cursor message if provided
        let cursor_created_at =
            after_message_id.and_then(|id| messages.get(id).map(|m| m.created_at));

        let count = messages
            .values()
            .filter(|m| {
                m.session_id.as_ref().map(|s| s.as_str()) == Some(session_id)
                    && (m.role == MessageRole::Orchestrator)
                    && cursor_created_at
                        .map(|cursor_ts| m.created_at > cursor_ts)
                        .unwrap_or(true)
            })
            .count();

        Ok(count as u32)
    }

    async fn count_unread_messages(
        &self,
        session_id: &str,
        cursor_message_id: Option<&str>,
    ) -> AppResult<i64> {
        let messages = self.messages.read().unwrap();

        let cursor_created_at =
            cursor_message_id.and_then(|id| messages.get(id).map(|m| m.created_at));

        let count = messages
            .values()
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
        context_type: &str,
        context_id: &str,
    ) -> AppResult<Option<String>> {
        let messages = self.messages.read().unwrap();

        let mut matching: Vec<_> = messages
            .values()
            .filter(|m| {
                m.role == MessageRole::User
                    && match context_type {
                        "ideation" => m.session_id.as_ref().map(|s| s.as_str()) == Some(context_id),
                        "task" | "task_execution" => {
                            m.task_id.as_ref().map(|s| s.as_str()) == Some(context_id)
                        }
                        "project" => {
                            m.project_id.as_ref().map(|s| s.as_str()) == Some(context_id)
                                && m.session_id.is_none()
                        }
                        "standalone" => {
                            // Standalone conversations are self-keyed
                            // (context_id == conversation_id); they never set
                            // session_id, so the generic fallback arm below would
                            // silently return None forever.
                            m.conversation_id.as_ref().map(|c| c.as_str())
                                == Some(context_id.to_string())
                        }
                        _ => m.session_id.as_ref().map(|s| s.as_str()) == Some(context_id),
                    }
            })
            .collect();

        matching.sort_by_key(|m| m.created_at);
        Ok(matching.first().map(|m| m.content.clone()))
    }

    async fn get_latest_message_by_role(
        &self,
        session_id: &IdeationSessionId,
        role: &str,
    ) -> AppResult<Option<ChatMessage>> {
        let messages = self.messages.read().unwrap();
        let mut matching: Vec<_> = messages
            .values()
            .filter(|m| {
                m.session_id.as_ref().map(|s| s.as_str()) == Some(session_id.as_str())
                    && m.role.to_string() == role
            })
            .cloned()
            .collect();
        matching.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(matching.into_iter().next())
    }

    async fn exists_verification_result_in_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<bool> {
        let messages = self.messages.read().unwrap();
        let exists = messages.values().any(|m| {
            m.conversation_id.as_ref() == Some(conversation_id)
                && (m.content.contains(
                    crate::application::reconciliation::verification_handoff::VERIFICATION_RESULT_MARKER,
                ) || m.metadata.as_deref().is_some_and(|metadata| {
                    metadata.contains(crate::application::reconciliation::verification_handoff::VERIFICATION_RESULT_METADATA_KEY)
                }))
        });
        Ok(exists)
    }
}

#[cfg(test)]
#[path = "memory_chat_message_repo_tests.rs"]
mod tests;
