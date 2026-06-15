// Mock Chat Service
//
// Extracted from chat_service.rs to improve modularity and reduce file size.
// Provides a mock implementation for testing purposes.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::domain::entities::{
    AgentRun, ChatContextType, ChatConversation, ChatConversationId, IdeationSessionId, ProjectId,
    TaskId,
};
use crate::domain::services::{MessageQueue, QueuedMessage};
use crate::infrastructure::agents::claude::ToolCall;

use super::{
    AgentRunningState, ChatConversationWithMessages, ChatService, ChatServiceError,
    SendMessageOptions, SendResult,
};

// ============================================================================
// MockChatService - For testing
// ============================================================================

/// Mock chat service for testing
pub struct MockChatService {
    responses: Mutex<Vec<MockChatResponse>>,
    is_available: Mutex<bool>,
    conversations: Mutex<Vec<ChatConversation>>,
    active_run: Mutex<Option<AgentRun>>,
    message_queue: Arc<MessageQueue>,
    call_count: std::sync::atomic::AtomicU32,
    /// When set, send_message returns Ok(was_queued: true) after this many successful calls.
    already_running_after: Mutex<Option<u32>>,
    /// Configurable per-context agent running state for is_agent_running().
    /// Key: "{context_type}/{context_id}". Defaults to false if not set.
    running_agents: Mutex<HashMap<String, bool>>,
    /// Records each (context_type, context_id) pair passed to stop_agent.
    stop_agent_calls: Mutex<Vec<(ChatContextType, String)>>,
    /// Records each message string passed to send_message (for content assertions in tests).
    sent_messages: Mutex<Vec<String>>,
    /// Records the send options passed to send_message for replay/resume assertions.
    sent_options: Mutex<Vec<SendMessageOptions>>,
}

pub struct MockChatResponse {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub claude_session_id: Option<String>,
}

impl MockChatService {
    pub fn new() -> Self {
        Self {
            responses: Mutex::new(Vec::new()),
            is_available: Mutex::new(true),
            conversations: Mutex::new(Vec::new()),
            active_run: Mutex::new(None),
            message_queue: Arc::new(MessageQueue::new()),
            call_count: std::sync::atomic::AtomicU32::new(0),
            already_running_after: Mutex::new(None),
            running_agents: Mutex::new(HashMap::new()),
            stop_agent_calls: Mutex::new(Vec::new()),
            sent_messages: Mutex::new(Vec::new()),
            sent_options: Mutex::new(Vec::new()),
        }
    }

    pub fn with_queue(message_queue: Arc<MessageQueue>) -> Self {
        Self {
            responses: Mutex::new(Vec::new()),
            is_available: Mutex::new(true),
            conversations: Mutex::new(Vec::new()),
            active_run: Mutex::new(None),
            message_queue,
            call_count: std::sync::atomic::AtomicU32::new(0),
            already_running_after: Mutex::new(None),
            running_agents: Mutex::new(HashMap::new()),
            stop_agent_calls: Mutex::new(Vec::new()),
            sent_messages: Mutex::new(Vec::new()),
            sent_options: Mutex::new(Vec::new()),
        }
    }

    /// Returns a snapshot of all (context_type, context_id) pairs passed to stop_agent.
    pub async fn get_stop_agent_calls(&self) -> Vec<(ChatContextType, String)> {
        self.stop_agent_calls.lock().await.clone()
    }

    /// Returns a snapshot of all message strings passed to send_message.
    /// Used in tests to assert on message content (e.g., recovery prompt format).
    pub async fn get_sent_messages(&self) -> Vec<String> {
        self.sent_messages.lock().await.clone()
    }

    pub async fn get_sent_options(&self) -> Vec<SendMessageOptions> {
        self.sent_options.lock().await.clone()
    }

    /// Set the agent running state for a specific context.
    /// Key format: "{context_type}/{context_id}" — e.g., "review/task-abc-123".
    /// Used in tests to simulate a running agent without spawning a real process.
    pub async fn set_agent_running(&self, context_key: String, running: bool) {
        self.running_agents
            .lock()
            .await
            .insert(context_key, running);
    }

    pub fn call_count(&self) -> u32 {
        self.call_count.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub async fn set_available(&self, available: bool) {
        *self.is_available.lock().await = available;
    }

    pub async fn queue_response(&self, response: MockChatResponse) {
        self.responses.lock().await.push(response);
    }

    pub async fn queue_text_response(&self, text: impl Into<String>) {
        self.queue_response(MockChatResponse {
            text: text.into(),
            tool_calls: Vec::new(),
            claude_session_id: Some(uuid::Uuid::new_v4().to_string()),
        })
        .await;
    }

    /// After `n` successful send_message calls, subsequent calls return Ok(was_queued: true).
    pub async fn set_already_running_after(&self, n: u32) {
        *self.already_running_after.lock().await = Some(n);
    }

    pub async fn set_active_run(&self, run: Option<AgentRun>) {
        *self.active_run.lock().await = run;
    }

    pub async fn add_conversation(&self, conv: ChatConversation) {
        self.conversations.lock().await.push(conv);
    }
}

impl Default for MockChatService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChatService for MockChatService {
    async fn send_message(
        &self,
        context_type: ChatContextType,
        context_id: &str,
        message: &str,
        options: SendMessageOptions,
    ) -> Result<SendResult, ChatServiceError> {
        self.sent_messages.lock().await.push(message.to_string());
        self.sent_options.lock().await.push(options);

        let current = self
            .call_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;

        if let Some(threshold) = *self.already_running_after.lock().await {
            if current > threshold {
                return Ok(SendResult {
                    was_queued: true,
                    queued_message_id: Some("mock-queued-id".to_string()),
                    ..Default::default()
                });
            }
        }

        if !*self.is_available.lock().await {
            return Err(ChatServiceError::AgentNotAvailable(
                "Mock agent not available".to_string(),
            ));
        }

        let (conversation, _) = self
            .get_or_create_conversation(context_type, context_id)
            .await?;
        let agent_run = AgentRun::new(conversation.id);

        Ok(SendResult {
            conversation_id: conversation.id.as_str().to_string(),
            agent_run_id: agent_run.id.as_str().to_string(),
            is_new_conversation: conversation.provider_session_ref().is_none(),
            ..Default::default()
        })
    }

    async fn queue_message(
        &self,
        context_type: ChatContextType,
        context_id: &str,
        content: &str,
        client_id: Option<&str>,
    ) -> Result<QueuedMessage, ChatServiceError> {
        Ok(match client_id {
            Some(id) => self.message_queue.queue_with_client_id(
                context_type,
                context_id,
                content.to_string(),
                id.to_string(),
            ),
            None => self
                .message_queue
                .queue(context_type, context_id, content.to_string()),
        })
    }

    async fn get_queued_messages(
        &self,
        context_type: ChatContextType,
        context_id: &str,
    ) -> Result<Vec<QueuedMessage>, ChatServiceError> {
        Ok(self.message_queue.get_queued(context_type, context_id))
    }

    async fn delete_queued_message(
        &self,
        context_type: ChatContextType,
        context_id: &str,
        message_id: &str,
    ) -> Result<bool, ChatServiceError> {
        Ok(self
            .message_queue
            .delete(context_type, context_id, message_id))
    }

    async fn send_queued_message_now(
        &self,
        context_type: ChatContextType,
        context_id: &str,
        message_id: &str,
    ) -> Result<SendResult, ChatServiceError> {
        let queued_msg = self
            .message_queue
            .take(context_type, context_id, message_id)
            .ok_or_else(|| {
                ChatServiceError::ContextNotFound(format!(
                    "Queued message not found for {}/{}: {}",
                    context_type, context_id, message_id
                ))
            })?;

        self.send_message(
            context_type,
            context_id,
            &queued_msg.content,
            SendMessageOptions {
                metadata: queued_msg.metadata_override.clone(),
                harness_override: queued_msg.harness_override,
                model_override: queued_msg.model_override.clone(),
                logical_effort_override: queued_msg.logical_effort_override,
                force_new_provider_session: queued_msg.force_new_provider_session,
                composer_project_references: queued_msg.composer_project_references.clone(),
                composer_integration_references: queued_msg.composer_integration_references.clone(),
                attachment_ids: queued_msg.attachment_ids.clone(),
                ..Default::default()
            },
        )
        .await
    }

    async fn get_or_create_conversation(
        &self,
        context_type: ChatContextType,
        context_id: &str,
    ) -> Result<(ChatConversation, bool), ChatServiceError> {
        let conversations = self.conversations.lock().await;

        if let Some(conv) = conversations
            .iter()
            .find(|c| c.context_type == context_type && c.context_id == context_id)
        {
            return Ok((conv.clone(), false));
        }
        drop(conversations);

        let conv = match context_type {
            ChatContextType::Ideation => {
                ChatConversation::new_ideation(IdeationSessionId::from_string(context_id))
            }
            ChatContextType::Delegation => ChatConversation::new_delegation(
                crate::domain::entities::DelegatedSessionId::from_string(context_id),
            ),
            ChatContextType::Task => {
                ChatConversation::new_task(TaskId::from_string(context_id.to_string()))
            }
            ChatContextType::Project => {
                ChatConversation::new_project(ProjectId::from_string(context_id.to_string()))
            }
            ChatContextType::TaskExecution => {
                ChatConversation::new_task_execution(TaskId::from_string(context_id.to_string()))
            }
            ChatContextType::Review => {
                ChatConversation::new_review(TaskId::from_string(context_id.to_string()))
            }
            ChatContextType::Merge => {
                ChatConversation::new_merge(TaskId::from_string(context_id.to_string()))
            }
        };

        self.conversations.lock().await.push(conv.clone());
        Ok((conv, true))
    }

    async fn get_conversation_with_messages(
        &self,
        conversation_id: &ChatConversationId,
    ) -> Result<Option<ChatConversationWithMessages>, ChatServiceError> {
        let conversations = self.conversations.lock().await;
        let conv = conversations
            .iter()
            .find(|c| c.id == *conversation_id)
            .cloned();

        Ok(conv.map(|c| ChatConversationWithMessages {
            conversation: c,
            messages: Vec::new(),
        }))
    }

    async fn list_conversations(
        &self,
        context_type: ChatContextType,
        context_id: &str,
    ) -> Result<Vec<ChatConversation>, ChatServiceError> {
        let conversations = self.conversations.lock().await;
        Ok(conversations
            .iter()
            .filter(|c| c.context_type == context_type && c.context_id == context_id)
            .cloned()
            .collect())
    }

    async fn get_active_run(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> Result<Option<AgentRun>, ChatServiceError> {
        Ok(self.active_run.lock().await.clone())
    }

    async fn is_available(&self) -> bool {
        *self.is_available.lock().await
    }

    async fn stop_agent(
        &self,
        context_type: ChatContextType,
        context_id: &str,
    ) -> Result<bool, ChatServiceError> {
        self.stop_agent_calls
            .lock()
            .await
            .push((context_type, context_id.to_string()));
        Ok(false)
    }

    async fn is_agent_running(&self, context_type: ChatContextType, context_id: &str) -> bool {
        let key = format!("{}/{}", context_type, context_id);
        self.running_agents
            .lock()
            .await
            .get(&key)
            .copied()
            .unwrap_or(false)
    }

    async fn get_agent_running_states(
        &self,
        context_type: ChatContextType,
        context_ids: &[String],
    ) -> HashMap<String, AgentRunningState> {
        let running_agents = self.running_agents.lock().await;
        context_ids
            .iter()
            .filter(|context_id| !context_id.is_empty())
            .map(|context_id| {
                let key = format!("{}/{}", context_type, context_id);
                let state = if running_agents.get(&key).copied().unwrap_or(false) {
                    AgentRunningState::generating()
                } else {
                    AgentRunningState::idle()
                };
                (context_id.clone(), state)
            })
            .collect()
    }
}
