// Shared response types for chat-related commands
// Used by both ideation_commands and context_chat_commands

use serde::Serialize;

use crate::domain::entities::ChatMessage;

/// Response for ChatMessage
/// Shared across all chat-related endpoints for API consistency
#[derive(Debug, Serialize, Clone)]
pub struct ChatMessageResponse {
    pub id: String,
    pub session_id: Option<String>,
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub conversation_id: Option<String>,
    pub role: String,
    pub content: String,
    pub metadata: Option<String>,
    pub parent_message_id: Option<String>,
    pub tool_calls: Option<String>,
    pub content_blocks: Option<String>,
    pub attribution_source: Option<String>,
    pub provider_harness: Option<String>,
    pub provider_session_id: Option<String>,
    pub upstream_provider: Option<String>,
    pub provider_profile: Option<String>,
    pub logical_model: Option<String>,
    pub effective_model_id: Option<String>,
    pub logical_effort: Option<String>,
    pub effective_effort: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_creation_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub estimated_usd: Option<f64>,
    pub usage_provenance: Option<String>,
    pub created_at: String,
}

impl From<ChatMessage> for ChatMessageResponse {
    fn from(message: ChatMessage) -> Self {
        Self {
            id: message.id.as_str().to_string(),
            session_id: message.session_id.map(|id| id.as_str().to_string()),
            project_id: message.project_id.map(|id| id.as_str().to_string()),
            task_id: message.task_id.map(|id| id.as_str().to_string()),
            conversation_id: message.conversation_id.map(|id| id.as_str().to_string()),
            role: message.role.to_string(),
            content: message.content,
            metadata: message.metadata,
            parent_message_id: message.parent_message_id.map(|id| id.as_str().to_string()),
            tool_calls: message.tool_calls,
            content_blocks: message.content_blocks,
            attribution_source: message.attribution_source,
            provider_harness: message.provider_harness.map(|value| value.to_string()),
            provider_session_id: message.provider_session_id,
            upstream_provider: message.upstream_provider,
            provider_profile: message.provider_profile,
            logical_model: message.logical_model,
            effective_model_id: message.effective_model_id,
            logical_effort: message.logical_effort.map(|value| value.to_string()),
            effective_effort: message.effective_effort,
            input_tokens: message.input_tokens,
            output_tokens: message.output_tokens,
            cache_creation_tokens: message.cache_creation_tokens,
            cache_read_tokens: message.cache_read_tokens,
            estimated_usd: message.estimated_usd,
            usage_provenance: message.usage_provenance.map(|value| value.to_string()),
            created_at: message.created_at.to_rfc3339(),
        }
    }
}
