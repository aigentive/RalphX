// Agent run repository trait - domain layer abstraction
//
// This trait defines the contract for agent run persistence.
// Agent runs track the execution status of provider-backed agents for conversations.

use async_trait::async_trait;

use crate::domain::entities::{
    AgentRun, AgentRunAttribution, AgentRunId, AgentRunStatus, AgentRunUsage, ChatConversationId,
    InterruptedConversation,
};
use crate::error::AppResult;

/// Error marker used when startup cancels a run that was left active by a prior app session.
pub const ORPHANED_AGENT_RUN_ON_APP_RESTART: &str = "Orphaned on app restart";

/// Repository trait for AgentRun persistence.
/// Implementations can use SQLite, PostgreSQL, in-memory, etc.
#[async_trait]
pub trait AgentRunRepository: Send + Sync {
    /// Create a new agent run
    async fn create(&self, run: AgentRun) -> AppResult<AgentRun>;

    /// Get run by ID
    async fn get_by_id(&self, id: &AgentRunId) -> AppResult<Option<AgentRun>>;

    /// Get the most recent run for a conversation (active or completed)
    async fn get_latest_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentRun>>;

    /// Get the active (running) run for a conversation, if any
    async fn get_active_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentRun>>;

    /// Get all runs for a conversation, ordered by started_at DESC
    async fn get_by_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<AgentRun>>;

    /// Update run status
    async fn update_status(&self, id: &AgentRunId, status: AgentRunStatus) -> AppResult<()>;

    /// Persist usage/cost metadata for a run without changing its lifecycle status.
    async fn update_usage(&self, id: &AgentRunId, usage: &AgentRunUsage) -> AppResult<()>;

    /// Persist provider/harness/model attribution for a run without changing lifecycle status.
    async fn update_attribution(
        &self,
        id: &AgentRunId,
        attribution: &AgentRunAttribution,
    ) -> AppResult<()>;

    /// Complete a run (set status to Completed and completed_at timestamp)
    async fn complete(&self, id: &AgentRunId) -> AppResult<()>;

    /// Fail a run (set status to Failed, completed_at timestamp, and error message)
    async fn fail(&self, id: &AgentRunId, error_message: &str) -> AppResult<()>;

    /// Cancel a run (set status to Cancelled and completed_at timestamp)
    async fn cancel(&self, id: &AgentRunId) -> AppResult<()>;

    /// Delete a run
    async fn delete(&self, id: &AgentRunId) -> AppResult<()>;

    /// Delete all runs for a conversation
    async fn delete_by_conversation(&self, conversation_id: &ChatConversationId) -> AppResult<()>;

    /// Count runs by status for a conversation
    async fn count_by_status(
        &self,
        conversation_id: &ChatConversationId,
        status: AgentRunStatus,
    ) -> AppResult<u32>;

    /// Cancel all runs currently in "running" status
    ///
    /// Used on startup to clean up orphaned agent runs from previous sessions
    /// that didn't complete properly (e.g., app crash or force quit).
    /// Returns the number of runs cancelled.
    async fn cancel_all_running(&self) -> AppResult<u32>;

    /// Cancel running runs that started before the current app boot cutoff.
    ///
    /// Used by startup previous-session cleanup so delayed recovery cannot mark
    /// current-boot runs as orphaned.
    async fn cancel_running_started_before(
        &self,
        cutoff: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<u32>;

    /// Get conversations that were interrupted during app shutdown
    ///
    /// Returns conversations where:
    /// - a provider session ID is present (can use provider-specific resume)
    /// - latest agent_run status is 'cancelled'
    /// - latest agent_run error_message is `ORPHANED_AGENT_RUN_ON_APP_RESTART`
    ///
    /// Used by ChatResumptionRunner to resume interrupted conversations on startup.
    async fn get_interrupted_conversations(&self) -> AppResult<Vec<InterruptedConversation>>;
}

#[cfg(test)]
#[path = "agent_run_repository_tests.rs"]
mod tests;
