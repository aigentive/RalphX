use async_trait::async_trait;

use crate::domain::entities::{
    AgentConversationIssue, ChatConversationId, AGENT_CONVERSATION_ISSUE_STATUS_OPEN,
};
use crate::error::AppResult;

#[async_trait]
pub trait AgentConversationIssueRepository: Send + Sync {
    async fn save(&self, issue: &AgentConversationIssue) -> AppResult<AgentConversationIssue>;

    async fn get_by_id(&self, issue_id: &str) -> AppResult<Option<AgentConversationIssue>>;

    async fn list_by_conversation(
        &self,
        conversation_id: &ChatConversationId,
        include_resolved: bool,
    ) -> AppResult<Vec<AgentConversationIssue>>;

    async fn find_open_by_fingerprint(
        &self,
        conversation_id: &ChatConversationId,
        source_task_id: Option<&str>,
        issue_kind: &str,
        blocker_fingerprint: &str,
    ) -> AppResult<Option<AgentConversationIssue>>;

    async fn update_status(
        &self,
        issue_id: &str,
        status: &str,
    ) -> AppResult<Option<AgentConversationIssue>>;

    async fn link_followup_conversation(
        &self,
        issue_id: &str,
        followup_conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentConversationIssue>>;
}

pub fn is_open_issue_status(status: &str) -> bool {
    status == AGENT_CONVERSATION_ISSUE_STATUS_OPEN
}
