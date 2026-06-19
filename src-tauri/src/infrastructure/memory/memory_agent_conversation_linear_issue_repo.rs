use std::collections::HashMap;

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::RwLock;

use crate::domain::entities::{AgentConversationLinearIssueLink, ChatConversationId};
use crate::domain::repositories::AgentConversationLinearIssueRepository;
use crate::error::AppResult;

pub struct MemoryAgentConversationLinearIssueRepository {
    links: RwLock<HashMap<ChatConversationId, AgentConversationLinearIssueLink>>,
}

impl MemoryAgentConversationLinearIssueRepository {
    pub fn new() -> Self {
        Self {
            links: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for MemoryAgentConversationLinearIssueRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentConversationLinearIssueRepository for MemoryAgentConversationLinearIssueRepository {
    async fn get_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentConversationLinearIssueLink>> {
        Ok(self.links.read().await.get(conversation_id).cloned())
    }

    async fn upsert(
        &self,
        mut link: AgentConversationLinearIssueLink,
    ) -> AppResult<AgentConversationLinearIssueLink> {
        let mut links = self.links.write().await;
        if let Some(existing) = links.get(&link.conversation_id) {
            link.created_at = existing.created_at;
        }
        link.updated_at = Utc::now();
        links.insert(link.conversation_id.clone(), link.clone());
        Ok(link)
    }

    async fn insert_if_absent(
        &self,
        link: AgentConversationLinearIssueLink,
    ) -> AppResult<AgentConversationLinearIssueLink> {
        let mut links = self.links.write().await;
        Ok(links
            .entry(link.conversation_id.clone())
            .or_insert(link)
            .clone())
    }

    async fn clear(&self, conversation_id: &ChatConversationId) -> AppResult<()> {
        self.links.write().await.remove(conversation_id);
        Ok(())
    }
}

#[cfg(test)]
#[path = "memory_agent_conversation_linear_issue_repo_tests.rs"]
mod tests;
