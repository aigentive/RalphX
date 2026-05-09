use super::*;
use crate::agents::{AgentHarnessKind, LogicalEffort};
use crate::domain::entities::{AgentRun, AgentRunAttribution, AgentRunUsage, ChatConversationId};
use std::sync::Arc;

// Mock implementation for testing trait object usage
struct MockAgentRunRepository {
    runs: Vec<AgentRun>,
}

impl MockAgentRunRepository {
    fn new() -> Self {
        Self { runs: vec![] }
    }

    fn with_runs(runs: Vec<AgentRun>) -> Self {
        Self { runs }
    }
}

#[async_trait]
impl AgentRunRepository for MockAgentRunRepository {
    async fn create(&self, run: AgentRun) -> AppResult<AgentRun> {
        Ok(run)
    }

    async fn get_by_id(&self, id: &AgentRunId) -> AppResult<Option<AgentRun>> {
        Ok(self.runs.iter().find(|r| r.id == *id).cloned())
    }

    async fn get_latest_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentRun>> {
        Ok(self
            .runs
            .iter()
            .filter(|r| r.conversation_id == *conversation_id)
            .max_by_key(|r| r.started_at)
            .cloned())
    }

    async fn get_active_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentRun>> {
        Ok(self
            .runs
            .iter()
            .find(|r| r.conversation_id == *conversation_id && r.is_active())
            .cloned())
    }

    async fn get_by_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<AgentRun>> {
        Ok(self
            .runs
            .iter()
            .filter(|r| r.conversation_id == *conversation_id)
            .cloned()
            .collect())
    }

    async fn update_status(&self, _id: &AgentRunId, _status: AgentRunStatus) -> AppResult<()> {
        Ok(())
    }

    async fn update_usage(&self, _id: &AgentRunId, _usage: &AgentRunUsage) -> AppResult<()> {
        Ok(())
    }

    async fn update_attribution(
        &self,
        _id: &AgentRunId,
        _attribution: &AgentRunAttribution,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn complete(&self, _id: &AgentRunId) -> AppResult<()> {
        Ok(())
    }

    async fn fail(&self, _id: &AgentRunId, _error_message: &str) -> AppResult<()> {
        Ok(())
    }

    async fn cancel(&self, _id: &AgentRunId) -> AppResult<()> {
        Ok(())
    }

    async fn delete(&self, _id: &AgentRunId) -> AppResult<()> {
        Ok(())
    }

    async fn delete_by_conversation(&self, _conversation_id: &ChatConversationId) -> AppResult<()> {
        Ok(())
    }

    async fn count_by_status(
        &self,
        conversation_id: &ChatConversationId,
        status: AgentRunStatus,
    ) -> AppResult<u32> {
        Ok(self
            .runs
            .iter()
            .filter(|r| r.conversation_id == *conversation_id && r.status == status)
            .count() as u32)
    }

    async fn cancel_all_running(&self) -> AppResult<u32> {
        // Mock just returns 0 - not needed for mock tests
        Ok(0)
    }

    async fn cancel_running_started_before(
        &self,
        _cutoff: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<u32> {
        // Mock just returns 0 - not needed for mock tests
        Ok(0)
    }

    async fn get_interrupted_conversations(&self) -> AppResult<Vec<InterruptedConversation>> {
        // Mock returns empty - actual filtering would need conversation data
        Ok(vec![])
    }
}

#[test]
fn test_trait_object_safety() {
    let repo = MockAgentRunRepository::new();
    let _: Arc<dyn AgentRunRepository> = Arc::new(repo);
}

#[test]
fn test_mock_with_runs() {
    let conversation_id = ChatConversationId::new();
    let mut run = AgentRun::new(conversation_id);
    run.harness = Some(AgentHarnessKind::Codex);
    run.logical_effort = Some(LogicalEffort::Medium);
    let repo = MockAgentRunRepository::with_runs(vec![run.clone()]);

    assert_eq!(repo.runs.len(), 1);
    assert_eq!(repo.runs[0].id, run.id);
    assert_eq!(repo.runs[0].harness, Some(AgentHarnessKind::Codex));
}
