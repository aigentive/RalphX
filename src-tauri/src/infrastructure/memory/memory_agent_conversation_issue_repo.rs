use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::domain::entities::{
    AgentConversationIssue, ChatConversationId, AGENT_CONVERSATION_ISSUE_STATUS_DISMISSED,
    AGENT_CONVERSATION_ISSUE_STATUS_OPEN, AGENT_CONVERSATION_ISSUE_STATUS_RESOLVED,
};
use crate::domain::repositories::AgentConversationIssueRepository;
use crate::error::AppResult;

#[derive(Default)]
pub struct MemoryAgentConversationIssueRepository {
    issues: Arc<RwLock<HashMap<String, AgentConversationIssue>>>,
}

impl MemoryAgentConversationIssueRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl AgentConversationIssueRepository for MemoryAgentConversationIssueRepository {
    async fn save(&self, issue: &AgentConversationIssue) -> AppResult<AgentConversationIssue> {
        let mut issues = self.issues.write().await;
        issues.insert(issue.id.clone(), issue.clone());
        Ok(issue.clone())
    }

    async fn get_by_id(&self, issue_id: &str) -> AppResult<Option<AgentConversationIssue>> {
        Ok(self.issues.read().await.get(issue_id).cloned())
    }

    async fn list_by_conversation(
        &self,
        conversation_id: &ChatConversationId,
        include_resolved: bool,
    ) -> AppResult<Vec<AgentConversationIssue>> {
        let mut results: Vec<_> = self
            .issues
            .read()
            .await
            .values()
            .filter(|issue| issue.conversation_id == *conversation_id)
            .filter(|issue| {
                include_resolved
                    || (issue.status != AGENT_CONVERSATION_ISSUE_STATUS_RESOLVED
                        && issue.status != AGENT_CONVERSATION_ISSUE_STATUS_DISMISSED)
            })
            .cloned()
            .collect();
        results.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(results)
    }

    async fn find_open_by_fingerprint(
        &self,
        conversation_id: &ChatConversationId,
        source_task_id: Option<&str>,
        issue_kind: &str,
        blocker_fingerprint: &str,
    ) -> AppResult<Option<AgentConversationIssue>> {
        Ok(self
            .issues
            .read()
            .await
            .values()
            .filter(|issue| {
                issue.conversation_id == *conversation_id
                    && issue.status == AGENT_CONVERSATION_ISSUE_STATUS_OPEN
                    && issue.source_task_id.as_deref() == source_task_id
                    && issue.issue_kind == issue_kind
                    && issue.blocker_fingerprint.as_deref() == Some(blocker_fingerprint)
            })
            .max_by(|a, b| a.updated_at.cmp(&b.updated_at))
            .cloned())
    }

    async fn update_status(
        &self,
        issue_id: &str,
        status: &str,
    ) -> AppResult<Option<AgentConversationIssue>> {
        let mut issues = self.issues.write().await;
        let Some(issue) = issues.get_mut(issue_id) else {
            return Ok(None);
        };
        issue.status = status.to_string();
        issue.updated_at = chrono::Utc::now();
        issue.resolved_at =
            (status != AGENT_CONVERSATION_ISSUE_STATUS_OPEN).then_some(issue.updated_at);
        Ok(Some(issue.clone()))
    }

    async fn link_followup_conversation(
        &self,
        issue_id: &str,
        followup_conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentConversationIssue>> {
        let mut issues = self.issues.write().await;
        let Some(issue) = issues.get_mut(issue_id) else {
            return Ok(None);
        };
        issue.linked_followup_conversation_id = Some(followup_conversation_id.clone());
        issue.updated_at = chrono::Utc::now();
        Ok(Some(issue.clone()))
    }
}
