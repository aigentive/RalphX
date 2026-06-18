use async_trait::async_trait;

use crate::entities::{AgentConversationJiraIssueLink, ChatConversationId};
use crate::error::AppResult;

#[async_trait]
pub trait AgentConversationJiraIssueRepository: Send + Sync {
    async fn get_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentConversationJiraIssueLink>>;

    async fn upsert(
        &self,
        link: AgentConversationJiraIssueLink,
    ) -> AppResult<AgentConversationJiraIssueLink>;

    async fn insert_if_absent(
        &self,
        link: AgentConversationJiraIssueLink,
    ) -> AppResult<AgentConversationJiraIssueLink>;

    async fn clear(&self, conversation_id: &ChatConversationId) -> AppResult<()>;
}
