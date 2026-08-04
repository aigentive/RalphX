use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::domain::entities::{
    AgentRun, AgentRunAttribution, AgentRunId, AgentRunStatus, AgentRunUsage, ChatConversationId,
    InterruptedConversation,
};
use crate::domain::repositories::AgentRunRepository;
use crate::error::{AppError, AppResult};

pub const AGENT_RUN_GET_BY_ID_FAILURE: &str = "injected agent-run get_by_id failure";

/// Test repository that delegates mutations and non-ID queries but fails run reads by ID.
pub struct GetByIdFailingAgentRunRepository {
    inner: Arc<dyn AgentRunRepository>,
}

impl GetByIdFailingAgentRunRepository {
    pub fn new(inner: Arc<dyn AgentRunRepository>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl AgentRunRepository for GetByIdFailingAgentRunRepository {
    async fn create(&self, run: AgentRun) -> AppResult<AgentRun> {
        self.inner.create(run).await
    }

    async fn get_by_id(&self, _id: &AgentRunId) -> AppResult<Option<AgentRun>> {
        Err(AppError::Infrastructure(
            AGENT_RUN_GET_BY_ID_FAILURE.to_string(),
        ))
    }

    async fn get_latest_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentRun>> {
        self.inner
            .get_latest_for_conversation(conversation_id)
            .await
    }

    async fn get_active_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentRun>> {
        self.inner
            .get_active_for_conversation(conversation_id)
            .await
    }

    async fn get_by_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<AgentRun>> {
        self.inner.get_by_conversation(conversation_id).await
    }

    async fn update_status(&self, id: &AgentRunId, status: AgentRunStatus) -> AppResult<()> {
        self.inner.update_status(id, status).await
    }

    async fn update_usage(&self, id: &AgentRunId, usage: &AgentRunUsage) -> AppResult<()> {
        self.inner.update_usage(id, usage).await
    }

    async fn update_attribution(
        &self,
        id: &AgentRunId,
        attribution: &AgentRunAttribution,
    ) -> AppResult<()> {
        self.inner.update_attribution(id, attribution).await
    }

    async fn complete(&self, id: &AgentRunId) -> AppResult<()> {
        self.inner.complete(id).await
    }

    async fn complete_if_prune_cancelled(&self, id: &AgentRunId) -> AppResult<bool> {
        self.inner.complete_if_prune_cancelled(id).await
    }

    async fn fail(&self, id: &AgentRunId, error_message: &str) -> AppResult<()> {
        self.inner.fail(id, error_message).await
    }

    async fn cancel(&self, id: &AgentRunId) -> AppResult<()> {
        self.inner.cancel(id).await
    }

    async fn cancel_with_reason(&self, id: &AgentRunId, reason: &str) -> AppResult<()> {
        self.inner.cancel_with_reason(id, reason).await
    }

    async fn delete(&self, id: &AgentRunId) -> AppResult<()> {
        self.inner.delete(id).await
    }

    async fn delete_by_conversation(&self, conversation_id: &ChatConversationId) -> AppResult<()> {
        self.inner.delete_by_conversation(conversation_id).await
    }

    async fn count_by_status(
        &self,
        conversation_id: &ChatConversationId,
        status: AgentRunStatus,
    ) -> AppResult<u32> {
        self.inner.count_by_status(conversation_id, status).await
    }

    async fn cancel_all_running(&self) -> AppResult<u32> {
        self.inner.cancel_all_running().await
    }

    async fn cancel_running_started_before(&self, cutoff: DateTime<Utc>) -> AppResult<u32> {
        self.inner.cancel_running_started_before(cutoff).await
    }

    async fn get_interrupted_conversations(&self) -> AppResult<Vec<InterruptedConversation>> {
        self.inner.get_interrupted_conversations().await
    }
}
