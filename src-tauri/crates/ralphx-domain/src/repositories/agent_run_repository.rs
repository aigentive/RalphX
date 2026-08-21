// Agent run repository trait - domain layer abstraction
//
// This trait defines the contract for agent run persistence.
// Agent runs track the execution status of provider-backed agents for conversations.

use std::collections::HashMap;

use async_trait::async_trait;

use crate::agents::AgentHarnessKind;
use crate::domain::entities::agent_run::PersonaRunAttribution;
use crate::domain::entities::{
    AgentRun, AgentRunActionKind, AgentRunAttribution, AgentRunId, AgentRunStatus, AgentRunUsage,
    ChatConversationId, InterruptedConversation, UsageCapture,
};
use crate::error::AppResult;

/// Error marker used when startup cancels a run that was left active by a prior app session.
pub const ORPHANED_AGENT_RUN_ON_APP_RESTART: &str = "Orphaned on app restart";

/// Error marker used when the system pruner cancels a stale running registry entry.
pub const PRUNED_STALE_AGENT_RUN: &str = "pruned_stale_running_registry_entry";

/// Repository trait for AgentRun persistence.
/// Implementations can use SQLite, PostgreSQL, in-memory, etc.
#[async_trait]
pub trait AgentRunRepository: Send + Sync {
    /// Create a new agent run
    async fn create(&self, run: AgentRun) -> AppResult<AgentRun>;

    /// Get run by ID
    async fn get_by_id(&self, id: &AgentRunId) -> AppResult<Option<AgentRun>>;

    /// Get runs by ID. Implementations should batch this when backed by a database.
    async fn get_by_ids(&self, ids: &[AgentRunId]) -> AppResult<Vec<AgentRun>> {
        let mut runs = Vec::new();
        for id in ids {
            if let Some(run) = self.get_by_id(id).await? {
                runs.push(run);
            }
        }
        Ok(runs)
    }

    /// Get the most recent run for a conversation (active or completed)
    async fn get_latest_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentRun>>;

    /// Get the most recent run for each of several conversations.
    ///
    /// Conversations with no runs are absent from the map. Implementations should batch this when
    /// backed by a database; the default loops so non-database implementations need no change.
    async fn get_latest_for_conversations(
        &self,
        conversation_ids: &[ChatConversationId],
    ) -> AppResult<HashMap<ChatConversationId, AgentRun>> {
        let mut latest = HashMap::new();
        for conversation_id in conversation_ids {
            if let Some(run) = self.get_latest_for_conversation(conversation_id).await? {
                latest.insert(conversation_id.clone(), run);
            }
        }
        Ok(latest)
    }

    /// Get the latest successful run that owns one provider-native session.
    async fn get_latest_completed_for_provider_session(
        &self,
        conversation_id: &ChatConversationId,
        harness: AgentHarnessKind,
        provider_session_id: &str,
    ) -> AppResult<Option<AgentRun>> {
        Ok(self
            .get_by_conversation(conversation_id)
            .await?
            .into_iter()
            .filter(|run| {
                run.status == AgentRunStatus::Completed
                    && run.harness == Some(harness)
                    && run.provider_session_id.as_deref() == Some(provider_session_id)
            })
            .max_by_key(|run| run.started_at))
    }

    /// Get the active (running) run for a conversation, if any
    async fn get_active_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentRun>>;

    /// Get the latest run for one backend-owned action authority tuple.
    async fn get_latest_action(
        &self,
        conversation_id: &ChatConversationId,
        action_kind: AgentRunActionKind,
        action_context_id: &str,
        action_target_id: &str,
    ) -> AppResult<Option<AgentRun>> {
        Ok(self
            .get_by_conversation(conversation_id)
            .await?
            .into_iter()
            .filter(|run| {
                run.action_kind == Some(action_kind)
                    && run.action_context_id.as_deref() == Some(action_context_id)
                    && run.action_target_id.as_deref() == Some(action_target_id)
            })
            .max_by_key(|run| run.started_at))
    }

    /// Get a running action for one exact owner conversation and action tuple.
    async fn get_active_action(
        &self,
        conversation_id: &ChatConversationId,
        action_kind: AgentRunActionKind,
        action_context_id: &str,
        action_target_id: &str,
    ) -> AppResult<Option<AgentRun>> {
        Ok(self
            .get_by_conversation(conversation_id)
            .await?
            .into_iter()
            .find(|run| {
                run.status == AgentRunStatus::Running
                    && run.action_kind == Some(action_kind)
                    && run.action_context_id.as_deref() == Some(action_context_id)
                    && run.action_target_id.as_deref() == Some(action_target_id)
            }))
    }

    /// Get all runs for a conversation, ordered by started_at DESC
    async fn get_by_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<AgentRun>>;

    /// Update run status
    async fn update_status(&self, id: &AgentRunId, status: AgentRunStatus) -> AppResult<()>;

    /// Persist usage/cost metadata for a run without changing its lifecycle status.
    async fn update_usage(&self, id: &AgentRunId, usage: &AgentRunUsage) -> AppResult<()>;

    /// Atomically replace the complete normalized/raw capture for a run.
    /// Implementations must return an error when the run does not exist.
    ///
    /// The compatibility default keeps existing test doubles source-compatible;
    /// durable and memory repositories override it with full replacement semantics.
    async fn replace_usage_capture(
        &self,
        id: &AgentRunId,
        capture: &UsageCapture,
    ) -> AppResult<()> {
        if self.get_by_id(id).await?.is_none() {
            return Err(crate::error::AppError::NotFound(format!(
                "Agent run not found: {}",
                id.as_str()
            )));
        }
        self.update_usage(id, &capture.normalized).await
    }

    /// Persist provider/harness/model attribution for a run without changing lifecycle status.
    async fn update_attribution(
        &self,
        id: &AgentRunId,
        attribution: &AgentRunAttribution,
    ) -> AppResult<()>;

    /// Persist body-free persona identity and injection outcome for one run.
    async fn set_persona_attribution(
        &self,
        _id: &AgentRunId,
        _attribution: PersonaRunAttribution,
    ) -> AppResult<()> {
        Err(crate::error::AppError::Infrastructure(
            "AgentRunRepository does not support persona attribution".to_string(),
        ))
    }

    /// Complete a run (set status to Completed and completed_at timestamp)
    async fn complete(&self, id: &AgentRunId) -> AppResult<()>;

    /// Complete only when this caller owns the current Running -> Completed transition.
    async fn complete_if_running(&self, id: &AgentRunId) -> AppResult<bool> {
        let Some(run) = self.get_by_id(id).await? else {
            return Ok(false);
        };
        if run.status != AgentRunStatus::Running {
            return Ok(false);
        }
        self.complete(id).await?;
        Ok(true)
    }

    /// Complete only when this caller repairs a system prune cancellation.
    async fn complete_if_prune_cancelled(&self, id: &AgentRunId) -> AppResult<bool>;

    /// Fail a run (set status to Failed, completed_at timestamp, and error message)
    async fn fail(&self, id: &AgentRunId, error_message: &str) -> AppResult<()>;

    /// Cancel a run (set status to Cancelled and completed_at timestamp)
    async fn cancel(&self, id: &AgentRunId) -> AppResult<()>;

    /// Cancel a currently running run and persist the attributable system reason.
    async fn cancel_with_reason(&self, id: &AgentRunId, reason: &str) -> AppResult<()>;

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
