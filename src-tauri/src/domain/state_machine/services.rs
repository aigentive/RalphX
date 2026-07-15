// Service traits for state machine dependency injection
// These traits define the interfaces for services used by the state machine
// during state transitions. Actual implementations are provided in Phase 4+.

use async_trait::async_trait;
use ralphx_domain::entities::{BranchUpdateOperation, EventType, InternalStatus, ProjectId, Task};
use ralphx_domain::repositories::{BranchUpdateRepository, TaskRepository};
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchUpdateWorkflowOutcome {
    Completed { destination: InternalStatus },
    ContinuationPending,
    NeedsAgent,
    Blocked,
}

#[async_trait]
pub trait BranchUpdateWorkflow: Send + Sync {
    async fn execute_programmatic(
        &self,
        repository: Arc<dyn BranchUpdateRepository>,
        task_repository: Arc<dyn TaskRepository>,
        repo_path: &Path,
        operation: &BranchUpdateOperation,
        update_status: InternalStatus,
        fencing_epoch: u64,
    ) -> crate::error::AppResult<BranchUpdateWorkflowOutcome>;

    async fn publish_post_merge(
        &self,
        repository: Arc<dyn BranchUpdateRepository>,
        repo_path: &Path,
        operation: &BranchUpdateOperation,
        update_status: InternalStatus,
    ) -> crate::error::AppResult<InternalStatus>;

    async fn start_resolver(
        &self,
        task_id: &str,
        prompt: &str,
        workspace: &Path,
    ) -> crate::error::AppResult<()>;
}

/// Trait for spawning and managing AI agents.
///
/// The state machine uses this trait to spawn worker, reviewer, and QA agents
/// during state transitions. The actual implementation connects to Claude Code
/// or other agent providers.
///
/// Implementations are provided in Phase 4 (Agentic Client).
#[async_trait]
pub trait AgentSpawner: Send + Sync {
    /// Spawns an agent synchronously and waits for it to complete.
    ///
    /// # Arguments
    /// * `agent_type` - The type of agent to spawn (e.g., "worker", "reviewer", "qa-prep")
    /// * `task_id` - The ID of the task the agent should work on
    async fn spawn(&self, agent_type: &str, task_id: &str);

    /// Spawns an agent in the background without waiting.
    ///
    /// Used for QA prep which runs in parallel with execution.
    ///
    /// # Arguments
    /// * `agent_type` - The type of agent to spawn
    /// * `task_id` - The ID of the task the agent should work on
    async fn spawn_background(&self, agent_type: &str, task_id: &str);

    /// Waits for a background agent to complete.
    ///
    /// # Arguments
    /// * `agent_type` - The type of agent to wait for
    /// * `task_id` - The task ID the agent is working on
    async fn wait_for(&self, agent_type: &str, task_id: &str);

    /// Stops a running agent.
    ///
    /// # Arguments
    /// * `agent_type` - The type of agent to stop
    /// * `task_id` - The task ID the agent is working on
    async fn stop(&self, agent_type: &str, task_id: &str);
}

/// Trait for emitting events to the frontend.
///
/// The state machine emits events on state transitions that the frontend
/// uses to update the UI in real-time. Events are sent via Tauri's event system.
///
/// Implementations are provided in Phase 5 (Frontend Core).
#[async_trait]
pub trait EventEmitter: Send + Sync {
    /// Emits an event for a task state change.
    ///
    /// # Arguments
    /// * `event_type` - The type of event (e.g., "task_started", "task_blocked", "qa_passed")
    /// * `task_id` - The ID of the task that triggered the event
    async fn emit(&self, event_type: &str, task_id: &str);

    /// Emits an event with additional payload data.
    ///
    /// # Arguments
    /// * `event_type` - The type of event
    /// * `task_id` - The ID of the task
    /// * `payload` - JSON string with additional event data
    async fn emit_with_payload(&self, event_type: &str, task_id: &str, payload: &str);

    /// Emits a `task:status_changed` event with the correct payload format
    /// expected by the frontend (`{ task_id, old_status, new_status }`).
    ///
    /// Default implementation falls back to `emit("task:status_changed", task_id)`
    /// for backwards compatibility.
    async fn emit_status_change(&self, task_id: &str, _old_status: &str, _new_status: &str) {
        self.emit("task:status_changed", task_id).await;
    }
}

/// Context captured by the transition owner after its status history row commits.
///
/// The history entry id is deliberately passed through the seam: notification producers must
/// never query the latest history entry, because a revision loop can make that lookup stale.
#[derive(Debug, Clone)]
pub struct NotificationContext {
    pub task: Task,
    pub history_entry_id: String,
    pub project_id: ProjectId,
}

#[derive(Debug, Clone)]
pub enum TaskNotification {
    StateEntered(InternalStatus),
    ReviewError {
        message: String,
    },
    /// An entry action failed after its status transition committed.
    TaskStuck {
        message: String,
    },
}

/// Domain seam for best-effort task-pipeline notification effects.
#[async_trait]
pub trait Notifier: Send + Sync {
    async fn notify(&self, context: NotificationContext, notification: TaskNotification);
}

/// Trait for managing dependent tasks.
///
/// When a task completes, we may need to unblock dependent tasks.
/// This trait provides methods for managing task dependencies.
#[async_trait]
pub trait DependencyManager: Send + Sync {
    /// Resolves blockers for tasks that were waiting on the completed task.
    ///
    /// # Arguments
    /// * `completed_task_id` - The ID of the task that just completed
    async fn unblock_dependents(&self, completed_task_id: &str);

    /// Checks if a task has any unresolved blockers.
    ///
    /// # Arguments
    /// * `task_id` - The ID of the task to check
    async fn has_unresolved_blockers(&self, task_id: &str) -> bool;

    /// Gets the IDs of tasks blocking the given task.
    ///
    /// # Arguments
    /// * `task_id` - The ID of the task to check
    async fn get_blocking_tasks(&self, task_id: &str) -> Vec<String>;
}

/// Result of starting a review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewStartResult {
    /// Review started successfully with the review ID
    Started { review_id: String },
    /// Review was not started because AI review is disabled
    Disabled,
    /// Review failed to start due to an error
    Error(String),
}

/// Trait for starting reviews on tasks.
///
/// The state machine uses this trait to initiate AI reviews when
/// a task enters the PendingReview state. The actual implementation
/// connects to the ReviewService.
///
/// Implementations are provided in Phase 9 (Review & Supervision).
#[async_trait]
pub trait ReviewStarter: Send + Sync {
    /// Starts an AI review for a task.
    ///
    /// Creates a Review record and prepares for AI review.
    /// The actual reviewer agent is spawned separately via AgentSpawner.
    ///
    /// # Arguments
    /// * `task_id` - The ID of the task to review
    /// * `project_id` - The ID of the project the task belongs to
    ///
    /// # Returns
    /// * `ReviewStartResult::Started` - Review started with review ID
    /// * `ReviewStartResult::Disabled` - AI review is disabled in settings
    /// * `ReviewStartResult::Error` - Failed to start review
    async fn start_ai_review(&self, task_id: &str, project_id: &str) -> ReviewStartResult;
}

/// Trait for scheduling Ready tasks when execution slots are available.
///
/// The TransitionHandler uses this trait to automatically schedule Ready tasks
/// when capacity becomes available (e.g., on slot free, on enter Ready, on unpause).
/// The implementation queries for Ready tasks across projects and transitions
/// the oldest one to Executing state.
///
/// Implementations are provided in Phase 26 (Auto-Scheduler).
#[async_trait]
pub trait TaskScheduler: Send + Sync {
    /// Tries to schedule Ready tasks if execution slots are available.
    ///
    /// This method:
    /// 1. Checks if execution is paused or at capacity
    /// 2. Finds the oldest Ready task across all projects
    /// 3. Transitions it to Executing state via the state machine
    ///
    /// Called from:
    /// - on_exit() when exiting agent-active states (slot freed)
    /// - on_enter(Ready) when a task becomes Ready
    /// - startup when resuming after app restart
    /// - unpause/capacity increase commands
    async fn try_schedule_ready_tasks(&self);

    /// Re-trigger deferred merges for a project after a competing merge completes.
    ///
    /// Finds tasks in PendingMerge with `merge_deferred` metadata, clears the flag,
    /// and re-invokes their entry actions so `attempt_programmatic_merge()` runs again.
    async fn try_retry_deferred_merges(&self, project_id: &str);

    /// Retry main-branch merges that were deferred because agents were running.
    ///
    /// Called when the global running_count transitions to 0 (all agents idle).
    /// Finds tasks in PendingMerge with `main_merge_deferred` metadata, clears the flag,
    /// and re-invokes their entry actions to retry the main-branch merge.
    async fn try_retry_main_merges(&self);
}

/// Trait for publishing events to registered webhook endpoints.
///
/// This is the interface that emission call sites use. The concrete implementation
/// (WebhookPublisher struct) is in infrastructure and will be wired in separately.
/// Optional — if None, webhook delivery is simply skipped.
#[async_trait]
pub trait WebhookPublisher: Send + Sync {
    /// Publish an event to all registered webhook endpoints for the given project.
    async fn publish(&self, event_type: EventType, project_id: &str, payload: serde_json::Value);

    /// Evict a project's webhooks from the publisher cache after a registration mutation.
    ///
    /// Call after `upsert()` so the next `publish()` re-queries fresh data from the repo.
    /// Default implementation is a no-op (suitable for test/mock publishers).
    fn invalidate_project(&self, _project_id: &str) {}
}

/// No-op webhook publisher for tests.
pub struct MockWebhookPublisher;

#[async_trait]
impl WebhookPublisher for MockWebhookPublisher {
    async fn publish(
        &self,
        _event_type: EventType,
        _project_id: &str,
        _payload: serde_json::Value,
    ) {
        // no-op in tests
    }
}

#[cfg(test)]
#[path = "services_tests.rs"]
mod tests;
