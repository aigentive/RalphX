use async_trait::async_trait;

use crate::entities::{AgentConversationMute, ChatConversationId};
use crate::error::AppResult;

#[async_trait]
pub trait AgentConversationMuteRepository: Send + Sync {
    async fn set_muted(&self, mute: AgentConversationMute) -> AppResult<()>;

    async fn clear(&self, conversation_id: &ChatConversationId) -> AppResult<()>;

    async fn get_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentConversationMute>>;

    async fn list_by_conversation_ids(
        &self,
        conversation_ids: &[ChatConversationId],
    ) -> AppResult<Vec<AgentConversationMute>>;
}
