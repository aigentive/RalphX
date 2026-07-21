// Streaming State Cache
//
// In-memory cache for tracking streaming state per conversation.
// Used to hydrate frontend when navigating to an active agent execution
// where streaming events were missed during component mount.
//
// This is intentionally NOT persisted — streaming state is transient UI feedback.

use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::infrastructure::agents::claude::ToolCallStats;

/// A cached tool call that's currently in progress or recently completed.
#[derive(Debug, Clone, Serialize)]
pub struct CachedToolCall {
    /// Unique tool call ID (e.g., "toolu_01A...")
    pub id: String,
    /// Tool name (e.g., "bash", "read", "edit")
    pub name: String,
    /// Current arguments (may be partial during streaming)
    pub arguments: serde_json::Value,
    /// Result if completed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// Diff context for Edit/Write tools
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff_context: Option<serde_json::Value>,
    /// Parent tool use ID for nested tool calls
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_tool_use_id: Option<String>,
}

/// A cached streaming task (subagent execution).
#[derive(Debug, Clone, Serialize)]
pub struct CachedStreamingTask {
    /// Tool use ID that started this task
    pub tool_use_id: String,
    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Subagent type (e.g., "ralphx:coder")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent_type: Option<String>,
    /// Model being used
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Current status: "running" or "completed"
    pub status: String,
    /// Agent ID if available
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// RalphX native delegation job id
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegated_job_id: Option<String>,
    /// Delegated session id backing the child runtime
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegated_session_id: Option<String>,
    /// Delegated conversation id for child transcript expansion
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegated_conversation_id: Option<String>,
    /// Delegated agent run id for latest child run attribution
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegated_agent_run_id: Option<String>,
    /// Delegated harness/provider
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_harness: Option<String>,
    /// Delegated provider session continuity id
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<String>,
    /// Upstream provider captured by the delegated run
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_provider: Option<String>,
    /// Provider profile captured by the delegated run
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_profile: Option<String>,
    /// Logical model requested
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_model: Option<String>,
    /// Effective model used by the harness
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_model_id: Option<String>,
    /// Logical effort requested
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_effort: Option<String>,
    /// Effective effort used by the harness
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_effort: Option<String>,
    /// Approval policy used by the delegated run
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_policy: Option<String>,
    /// Sandbox mode used by the delegated run
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox_mode: Option<String>,
    /// Total tokens used (from TaskCompleted stats)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
    /// Total tool uses count (from TaskCompleted stats)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tool_uses: Option<u64>,
    /// Duration in milliseconds (from TaskCompleted stats)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Input tokens used by the latest run
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    /// Output tokens used by the latest run
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    /// Cache creation tokens used by the latest run
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<u64>,
    /// Cache read tokens used by the latest run
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    /// Estimated USD cost for the latest run
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_usd: Option<f64>,
    /// Final delegated output when available
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_output: Option<String>,
}

/// Complete streaming state for a single conversation.
#[derive(Debug, Clone, Serialize, Default)]
pub struct ConversationStreamingState {
    /// Owning run for this transient projection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// Tool calls currently in progress or recently completed
    pub tool_calls: Vec<CachedToolCall>,
    /// Streaming tasks (subagents) currently running or completed
    pub streaming_tasks: Vec<CachedStreamingTask>,
    /// Partial text content accumulated from agent:chunk events
    pub partial_text: String,
    /// When this state was last updated
    pub updated_at: DateTime<Utc>,
}

impl ConversationStreamingState {
    /// Create a new empty state
    pub fn new() -> Self {
        Self {
            run_id: None,
            tool_calls: Vec::new(),
            streaming_tasks: Vec::new(),
            partial_text: String::new(),
            updated_at: Utc::now(),
        }
    }
}

/// In-memory cache for streaming state, keyed by conversation_id.
#[derive(Debug, Clone)]
pub struct StreamingStateCache {
    states: Arc<Mutex<HashMap<String, ConversationStreamingState>>>,
}

impl Default for StreamingStateCache {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamingStateCache {
    /// Create a new empty cache
    pub fn new() -> Self {
        Self {
            states: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Bind subsequent cached state to the current run for stale-attempt rejection.
    pub async fn set_run_id(&self, conversation_id: &str, run_id: Option<String>) {
        let mut states = self.states.lock().await;
        let state = states
            .entry(conversation_id.to_string())
            .or_insert_with(ConversationStreamingState::new);
        if state.run_id != run_id {
            state.tool_calls.clear();
            state.streaming_tasks.clear();
            state.partial_text.clear();
        }
        state.run_id = run_id;
        state.updated_at = Utc::now();
    }

    /// Upsert a tool call into the cache.
    ///
    /// If a tool call with the same ID exists, it's updated.
    /// Otherwise, a new entry is added.
    pub async fn upsert_tool_call(&self, conversation_id: &str, tool_call: CachedToolCall) {
        let mut states = self.states.lock().await;
        let state = states
            .entry(conversation_id.to_string())
            .or_insert_with(|| {
                tracing::debug!(
                    conversation_id,
                    tool_id = %tool_call.id,
                    "StreamingStateCache: creating new state for upsert_tool_call"
                );
                ConversationStreamingState::new()
            });

        // Find existing tool call with same ID and update, or add new
        if let Some(existing) = state.tool_calls.iter_mut().find(|tc| tc.id == tool_call.id) {
            *existing = tool_call;
            tracing::trace!(
                conversation_id,
                tool_id = %existing.id,
                "StreamingStateCache: updated existing tool_call"
            );
        } else {
            tracing::debug!(
                conversation_id,
                tool_id = %tool_call.id,
                tool_name = %tool_call.name,
                "StreamingStateCache: added new tool_call"
            );
            state.tool_calls.push(tool_call);
        }
        state.updated_at = Utc::now();
    }

    /// Add a streaming task (subagent started event).
    pub async fn add_task(&self, conversation_id: &str, task: CachedStreamingTask) {
        let mut states = self.states.lock().await;
        let state = states
            .entry(conversation_id.to_string())
            .or_insert_with(|| {
                tracing::debug!(
                    conversation_id,
                    tool_use_id = %task.tool_use_id,
                    "StreamingStateCache: creating new state for add_task"
                );
                ConversationStreamingState::new()
            });

        Self::upsert_task_state(state, conversation_id, task);
    }

    /// Add a streaming task only when the conversation cache still belongs to `run_id`.
    pub async fn add_task_for_run(
        &self,
        conversation_id: &str,
        run_id: &str,
        task: CachedStreamingTask,
    ) -> bool {
        let mut states = self.states.lock().await;
        let Some(state) = states.get_mut(conversation_id) else {
            return false;
        };
        if state.run_id.as_deref() != Some(run_id) {
            return false;
        }
        Self::upsert_task_state(state, conversation_id, task);
        true
    }

    fn upsert_task_state(
        state: &mut ConversationStreamingState,
        conversation_id: &str,
        task: CachedStreamingTask,
    ) {
        if let Some(existing) = state
            .streaming_tasks
            .iter_mut()
            .find(|existing| existing.tool_use_id == task.tool_use_id)
        {
            tracing::debug!(
                conversation_id,
                tool_use_id = %existing.tool_use_id,
                subagent_type = ?task.subagent_type,
                "StreamingStateCache: updated existing streaming task"
            );
            *existing = task;
        } else {
            tracing::debug!(
                conversation_id,
                tool_use_id = %task.tool_use_id,
                subagent_type = ?task.subagent_type,
                "StreamingStateCache: added streaming task"
            );
            state.streaming_tasks.push(task);
        }
        state.updated_at = Utc::now();
    }

    /// Mark a streaming task as completed.
    pub async fn complete_task(
        &self,
        conversation_id: &str,
        tool_use_id: &str,
        stats: Option<ToolCallStats>,
    ) {
        let mut states = self.states.lock().await;
        if let Some(state) = states.get_mut(conversation_id) {
            if let Some(task) = state
                .streaming_tasks
                .iter_mut()
                .find(|t| t.tool_use_id == tool_use_id)
            {
                task.status = "completed".to_string();
                if let Some(s) = stats {
                    task.total_tokens = s.total_tokens;
                    task.total_tool_uses = s.total_tool_uses;
                    task.duration_ms = s.duration_ms;
                }
                state.updated_at = Utc::now();
                tracing::debug!(
                    conversation_id,
                    tool_use_id,
                    "StreamingStateCache: marked task as completed"
                );
            }
        }
    }

    /// Append text to the partial content buffer.
    pub async fn append_text(&self, conversation_id: &str, text: &str) {
        let mut states = self.states.lock().await;
        let state = states
            .entry(conversation_id.to_string())
            .or_insert_with(|| {
                tracing::debug!(
                    conversation_id,
                    text_len = text.len(),
                    "StreamingStateCache: creating new state for append_text"
                );
                ConversationStreamingState::new()
            });

        state.partial_text.push_str(text);
        state.updated_at = Utc::now();
        tracing::trace!(
            conversation_id,
            text_len = text.len(),
            total_len = state.partial_text.len(),
            "StreamingStateCache: appended text"
        );
    }

    /// Clear all streaming state for a conversation.
    ///
    /// Called when agent:run_completed, agent:turn_completed (interactive), or agent:error fires.
    pub async fn clear(&self, conversation_id: &str) {
        let mut states = self.states.lock().await;
        if states.remove(conversation_id).is_some() {
            tracing::debug!(conversation_id, "StreamingStateCache: cleared state");
        }
    }

    /// Get the current streaming state for a conversation.
    ///
    /// Returns None if no state exists (conversation not currently streaming).
    pub async fn get(&self, conversation_id: &str) -> Option<ConversationStreamingState> {
        let states = self.states.lock().await;
        states.get(conversation_id).cloned()
    }

    /// Returns a raw pointer to the inner Arc's allocation for use in Arc::ptr_eq tests.
    #[cfg(test)]
    pub fn states_arc(&self) -> &Arc<Mutex<HashMap<String, ConversationStreamingState>>> {
        &self.states
    }
}

#[cfg(test)]
#[path = "streaming_state_cache_tests.rs"]
mod tests;
