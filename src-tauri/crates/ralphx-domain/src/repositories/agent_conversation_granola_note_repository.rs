use async_trait::async_trait;

use crate::entities::{AgentConversationGranolaNoteLink, ChatConversationId};
use crate::error::AppResult;

#[async_trait]
pub trait AgentConversationGranolaNoteRepository: Send + Sync {
    async fn get_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentConversationGranolaNoteLink>>;

    async fn upsert(
        &self,
        link: AgentConversationGranolaNoteLink,
    ) -> AppResult<AgentConversationGranolaNoteLink>;

    async fn insert_if_absent(
        &self,
        link: AgentConversationGranolaNoteLink,
    ) -> AppResult<AgentConversationGranolaNoteLink>;

    async fn clear(&self, conversation_id: &ChatConversationId) -> AppResult<()>;
}
