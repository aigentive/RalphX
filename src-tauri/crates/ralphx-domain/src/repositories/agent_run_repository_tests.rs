use super::*;
use crate::agents::{AgentHarnessKind, LogicalEffort};
use crate::domain::entities::{AgentRun, AgentRunAttribution, AgentRunUsage, ChatConversationId};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

// Mock implementation for testing trait object usage
struct MockAgentRunRepository {
    runs: Vec<AgentRun>,
    completed: Mutex<Vec<AgentRunId>>,
}

impl MockAgentRunRepository {
    fn new() -> Self {
        Self {
            runs: vec![],
            completed: Mutex::new(Vec::new()),
        }
    }

    fn with_runs(runs: Vec<AgentRun>) -> Self {
        Self {
            runs,
            completed: Mutex::new(Vec::new()),
        }
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

    async fn complete(&self, id: &AgentRunId) -> AppResult<()> {
        self.completed.lock().unwrap().push(*id);
        Ok(())
    }

    async fn complete_if_prune_cancelled(&self, _id: &AgentRunId) -> AppResult<bool> {
        Ok(false)
    }

    async fn fail(&self, _id: &AgentRunId, _error_message: &str) -> AppResult<()> {
        Ok(())
    }

    async fn cancel(&self, _id: &AgentRunId) -> AppResult<()> {
        Ok(())
    }

    async fn cancel_with_reason(&self, _id: &AgentRunId, _reason: &str) -> AppResult<()> {
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

#[tokio::test]
async fn test_default_get_by_ids_returns_matching_runs() {
    let conversation_id = ChatConversationId::new();
    let run1 = AgentRun::new(conversation_id);
    let run1_id = run1.id;
    let run2 = AgentRun::new(conversation_id);
    let run2_id = run2.id;
    let repo = MockAgentRunRepository::with_runs(vec![run1, run2]);

    let runs = repo
        .get_by_ids(&[run2_id, AgentRunId::new(), run1_id])
        .await
        .unwrap();
    let ids: HashSet<_> = runs.iter().map(|run| run.id).collect();

    assert_eq!(runs.len(), 2);
    assert!(ids.contains(&run1_id));
    assert!(ids.contains(&run2_id));
}

#[tokio::test]
async fn default_action_queries_scope_by_owner_tuple_and_lifecycle() {
    let conversation_id = ChatConversationId::new();
    let mut older = AgentRun::new(conversation_id);
    older.action_kind = Some(AgentRunActionKind::VerifyPlan);
    older.action_context_id = Some("session-1".to_string());
    older.action_target_id = Some("plan-1".to_string());

    let mut newer = older.clone();
    newer.id = AgentRunId::new();
    newer.started_at = older.started_at + chrono::Duration::seconds(1);
    newer.status = AgentRunStatus::Completed;

    let mut wrong_target = AgentRun::new(conversation_id);
    wrong_target.action_kind = Some(AgentRunActionKind::VerifyPlan);
    wrong_target.action_context_id = Some("session-1".to_string());
    wrong_target.action_target_id = Some("plan-2".to_string());
    wrong_target.started_at = newer.started_at + chrono::Duration::seconds(1);

    let repo = MockAgentRunRepository::with_runs(vec![older.clone(), newer.clone(), wrong_target]);

    let latest = repo
        .get_latest_action(
            &conversation_id,
            AgentRunActionKind::VerifyPlan,
            "session-1",
            "plan-1",
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.id, newer.id);

    let active = repo
        .get_active_action(
            &conversation_id,
            AgentRunActionKind::VerifyPlan,
            "session-1",
            "plan-1",
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(active.id, older.id);
    assert!(repo
        .get_active_action(
            &ChatConversationId::new(),
            AgentRunActionKind::VerifyPlan,
            "session-1",
            "plan-1",
        )
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn default_conditional_completion_applies_only_to_running_runs() {
    let conversation_id = ChatConversationId::new();
    let running = AgentRun::new(conversation_id);
    let mut completed = AgentRun::new(conversation_id);
    completed.status = AgentRunStatus::Completed;
    let repo = MockAgentRunRepository::with_runs(vec![running.clone(), completed.clone()]);

    assert!(!repo.complete_if_running(&AgentRunId::new()).await.unwrap());
    assert!(!repo.complete_if_running(&completed.id).await.unwrap());
    assert!(repo.complete_if_running(&running.id).await.unwrap());
    assert_eq!(*repo.completed.lock().unwrap(), vec![running.id]);
}
