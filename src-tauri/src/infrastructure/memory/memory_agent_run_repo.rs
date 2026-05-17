// In-memory implementation of AgentRunRepository for testing

use async_trait::async_trait;
use std::collections::HashMap;
use tokio::sync::RwLock;

use crate::domain::entities::{
    AgentRun, AgentRunAttribution, AgentRunId, AgentRunStatus, AgentRunUsage,
    ChatConversationId,
    InterruptedConversation,
};
use crate::domain::repositories::{AgentRunRepository, ORPHANED_AGENT_RUN_ON_APP_RESTART};
use crate::error::AppResult;

/// In-memory implementation of AgentRunRepository for testing
pub struct MemoryAgentRunRepository {
    runs: RwLock<HashMap<AgentRunId, AgentRun>>,
}

impl MemoryAgentRunRepository {
    pub fn new() -> Self {
        Self {
            runs: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for MemoryAgentRunRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentRunRepository for MemoryAgentRunRepository {
    async fn create(&self, run: AgentRun) -> AppResult<AgentRun> {
        let mut runs = self.runs.write().await;
        runs.insert(run.id, run.clone());
        Ok(run)
    }

    async fn get_by_id(&self, id: &AgentRunId) -> AppResult<Option<AgentRun>> {
        let runs = self.runs.read().await;
        Ok(runs.get(id).cloned())
    }

    async fn get_latest_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentRun>> {
        let runs = self.runs.read().await;
        Ok(runs
            .values()
            .filter(|r| r.conversation_id == *conversation_id)
            .max_by_key(|r| r.started_at)
            .cloned())
    }

    async fn get_active_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentRun>> {
        let runs = self.runs.read().await;
        Ok(runs
            .values()
            .find(|r| r.conversation_id == *conversation_id && r.is_active())
            .cloned())
    }

    async fn get_by_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<AgentRun>> {
        let runs = self.runs.read().await;
        let mut filtered: Vec<AgentRun> = runs
            .values()
            .filter(|r| r.conversation_id == *conversation_id)
            .cloned()
            .collect();
        // Sort by started_at DESC
        filtered.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        Ok(filtered)
    }

    async fn update_status(&self, id: &AgentRunId, status: AgentRunStatus) -> AppResult<()> {
        let mut runs = self.runs.write().await;
        if let Some(run) = runs.get_mut(id) {
            run.status = status;
        }
        Ok(())
    }

    async fn update_usage(&self, id: &AgentRunId, usage: &AgentRunUsage) -> AppResult<()> {
        let mut runs = self.runs.write().await;
        if let Some(run) = runs.get_mut(id) {
            run.apply_usage(usage);
        }
        Ok(())
    }

    async fn update_attribution(
        &self,
        id: &AgentRunId,
        attribution: &AgentRunAttribution,
    ) -> AppResult<()> {
        let mut runs = self.runs.write().await;
        if let Some(run) = runs.get_mut(id) {
            run.apply_attribution(attribution);
        }
        Ok(())
    }

    async fn complete(&self, id: &AgentRunId) -> AppResult<()> {
        let mut runs = self.runs.write().await;
        if let Some(run) = runs.get_mut(id) {
            run.complete();
        }
        Ok(())
    }

    async fn fail(&self, id: &AgentRunId, error_message: &str) -> AppResult<()> {
        let mut runs = self.runs.write().await;
        if let Some(run) = runs.get_mut(id) {
            run.fail(error_message);
        }
        Ok(())
    }

    async fn cancel(&self, id: &AgentRunId) -> AppResult<()> {
        let mut runs = self.runs.write().await;
        if let Some(run) = runs.get_mut(id) {
            run.cancel();
        }
        Ok(())
    }

    async fn delete(&self, id: &AgentRunId) -> AppResult<()> {
        let mut runs = self.runs.write().await;
        runs.remove(id);
        Ok(())
    }

    async fn delete_by_conversation(&self, conversation_id: &ChatConversationId) -> AppResult<()> {
        let mut runs = self.runs.write().await;
        runs.retain(|_, r| r.conversation_id != *conversation_id);
        Ok(())
    }

    async fn count_by_status(
        &self,
        conversation_id: &ChatConversationId,
        status: AgentRunStatus,
    ) -> AppResult<u32> {
        let runs = self.runs.read().await;
        let count = runs
            .values()
            .filter(|r| r.conversation_id == *conversation_id && r.status == status)
            .count();
        Ok(count as u32)
    }

    async fn cancel_all_running(&self) -> AppResult<u32> {
        let mut runs = self.runs.write().await;
        let mut count = 0u32;
        for run in runs.values_mut() {
            if run.status == AgentRunStatus::Running {
                run.cancel();
                run.error_message = Some(ORPHANED_AGENT_RUN_ON_APP_RESTART.to_string());
                count += 1;
            }
        }
        Ok(count)
    }

    async fn cancel_running_started_before(
        &self,
        cutoff: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<u32> {
        let mut runs = self.runs.write().await;
        let mut count = 0u32;
        for run in runs.values_mut() {
            if run.status == AgentRunStatus::Running && run.started_at < cutoff {
                run.cancel();
                run.error_message = Some(ORPHANED_AGENT_RUN_ON_APP_RESTART.to_string());
                count += 1;
            }
        }
        Ok(count)
    }

    async fn get_interrupted_conversations(&self) -> AppResult<Vec<InterruptedConversation>> {
        // Memory repo cannot implement this properly since it doesn't have access to conversations
        // This is only used in production with SQLite
        Ok(vec![])
    }
}

#[cfg(test)]
#[path = "memory_agent_run_repo_tests.rs"]
mod tests;
