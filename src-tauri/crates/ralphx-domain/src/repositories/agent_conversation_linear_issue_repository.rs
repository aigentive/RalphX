use async_trait::async_trait;

use crate::entities::{AgentConversationLinearIssueLink, ChatConversationId};
use crate::error::AppResult;

#[async_trait]
pub trait AgentConversationLinearIssueRepository: Send + Sync {
    async fn get_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentConversationLinearIssueLink>>;

    async fn upsert(
        &self,
        link: AgentConversationLinearIssueLink,
    ) -> AppResult<AgentConversationLinearIssueLink>;

    async fn insert_if_absent(
        &self,
        link: AgentConversationLinearIssueLink,
    ) -> AppResult<AgentConversationLinearIssueLink>;

    async fn clear(&self, conversation_id: &ChatConversationId) -> AppResult<()>;
}
