// Task repository trait - domain layer abstraction
//
// This trait defines the contract for task persistence.
// Implementations can use SQLite, PostgreSQL, in-memory, etc.

use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::domain::entities::{
    ExecutionPlanId, IdeationSessionId, InternalStatus, ProjectId, Task, TaskCategory, TaskId,
    TaskStepId,
};
use crate::domain::ideation::TasksFeatureAction;
use crate::domain::repositories::StatusTransition;
use crate::error::AppResult;

/// Metadata to attach to state history entries for linking conversations to states
#[derive(Debug, Clone)]
pub struct StateHistoryMetadata {
    /// The conversation ID associated with this state entry
    pub conversation_id: String,
    /// The agent run ID that was started for this state
    pub agent_run_id: String,
}

/// Repository trait for Task persistence.
/// Implementations can use SQLite, PostgreSQL, in-memory, etc.
#[async_trait]
pub trait TaskRepository: Send + Sync {
    // ═══════════════════════════════════════════════════════════════════════
    // CRUD Operations
    // ═══════════════════════════════════════════════════════════════════════

    /// Create a new task
    async fn create(&self, task: Task) -> AppResult<Task>;

    /// Get task by ID
    async fn get_by_id(&self, id: &TaskId) -> AppResult<Option<Task>>;

    /// Get tasks by ID in a single repository call.
    async fn get_by_ids(&self, ids: &[TaskId]) -> AppResult<Vec<Task>> {
        let mut tasks = Vec::new();
        for id in ids {
            if let Some(task) = self.get_by_id(id).await? {
                tasks.push(task);
            }
        }
        Ok(tasks)
    }

    /// Get all tasks for a project
    async fn get_by_project(&self, project_id: &ProjectId) -> AppResult<Vec<Task>>;

    /// Update a task
    async fn update(&self, task: &Task) -> AppResult<()>;

    /// Update a task only if its current status matches `expected_status` (optimistic lock).
    ///
    /// Adds `AND internal_status = expected_status` to the WHERE clause so that
    /// concurrent transitions of the same task are serialized at the DB level.
    /// Returns `true` if the update was applied (status matched), `false` if
    /// another caller already transitioned the task (0 rows affected).
    async fn update_with_expected_status(
        &self,
        task: &Task,
        expected_status: InternalStatus,
    ) -> AppResult<bool>;

    async fn update_with_expected_status_for_action(
        &self,
        task: &Task,
        expected_status: InternalStatus,
        _action: TasksFeatureAction,
    ) -> AppResult<bool> {
        self.update_with_expected_status(task, expected_status)
            .await
    }

    /// Atomically applies a validated task update and its status-history row.
    /// Production repositories override this so a caller can never observe the
    /// status/metadata write without the matching audit record.
    async fn update_with_expected_status_and_history_for_action(
        &self,
        task: &Task,
        expected_status: InternalStatus,
        trigger: &str,
        action: TasksFeatureAction,
    ) -> AppResult<Option<String>> {
        if !self
            .update_with_expected_status_for_action(task, expected_status, action)
            .await?
        {
            return Ok(None);
        }
        self.persist_status_change_for_action(
            &task.id,
            expected_status,
            task.internal_status,
            trigger,
            action,
        )
        .await
        .map(Some)
    }

    /// Atomically commits terminal restart cleanup, failed-step resets, Ready status,
    /// and the matching history row. The returned count is the number of failed
    /// step rows reset by the repository transaction.
    async fn restart_terminal_task_to_ready_with_history_for_action(
        &self,
        task: &Task,
        expected_status: InternalStatus,
        _failed_step_ids: &[TaskStepId],
        trigger: &str,
        action: TasksFeatureAction,
    ) -> AppResult<Option<(String, u32)>> {
        self.update_with_expected_status_and_history_for_action(
            task,
            expected_status,
            trigger,
            action,
        )
        .await
        .map(|result| result.map(|history_id| (history_id, 0)))
    }

    /// Update only the metadata field of a task
    ///
    /// This method performs a targeted UPDATE of only the `metadata` and `updated_at`
    /// columns, preventing full-row clobbering when on_enter handlers write metadata
    /// after `transition_task()` has already persisted the status.
    ///
    /// # Arguments
    /// * `id` - The task ID
    /// * `metadata` - The new metadata JSON string, or None to clear
    ///
    /// # Returns
    /// * `Ok(())` - Metadata was updated successfully (or task doesn't exist)
    /// * `Err` - Database error
    async fn update_metadata(&self, id: &TaskId, metadata: Option<String>) -> AppResult<()>;

    /// Delete a task
    async fn delete(&self, id: &TaskId) -> AppResult<()>;

    // ═══════════════════════════════════════════════════════════════════════
    // Status Operations (Phase 3 will add full state machine integration)
    // ═══════════════════════════════════════════════════════════════════════

    /// Get tasks by status
    async fn get_by_status(
        &self,
        project_id: &ProjectId,
        status: InternalStatus,
    ) -> AppResult<Vec<Task>>;

    /// Get tasks in a status whose JSON metadata has a boolean key set to `true`.
    async fn get_by_status_with_metadata_bool(
        &self,
        project_id: &ProjectId,
        status: InternalStatus,
        metadata_key: &str,
    ) -> AppResult<Vec<Task>> {
        let tasks = self.get_by_status(project_id, status).await?;
        Ok(tasks
            .into_iter()
            .filter(|task| {
                task.metadata
                    .as_deref()
                    .and_then(|metadata| serde_json::from_str::<serde_json::Value>(metadata).ok())
                    .and_then(|value| value.get(metadata_key)?.as_bool())
                    .unwrap_or(false)
            })
            .collect())
    }

    /// Find candidate `(ideation_session_id, execution_plan_id)` pairs that have
    /// at least one merged, unarchived Regular task in a project.
    async fn find_merged_regular_plan_keys(
        &self,
        project_id: &ProjectId,
        plan_keys: &[(IdeationSessionId, ExecutionPlanId)],
    ) -> AppResult<HashSet<(IdeationSessionId, ExecutionPlanId)>> {
        if plan_keys.is_empty() {
            return Ok(HashSet::new());
        }

        let requested: HashSet<(IdeationSessionId, ExecutionPlanId)> =
            plan_keys.iter().cloned().collect();
        let tasks = self
            .get_by_status(project_id, InternalStatus::Merged)
            .await?;
        Ok(tasks
            .into_iter()
            .filter(|task| task.category == TaskCategory::Regular && task.archived_at.is_none())
            .filter_map(|task| Some((task.ideation_session_id?, task.execution_plan_id?)))
            .filter(|key| requested.contains(key))
            .collect())
    }

    /// Persist a status change with audit log entry
    async fn persist_status_change(
        &self,
        id: &TaskId,
        from: InternalStatus,
        to: InternalStatus,
        trigger: &str,
    ) -> AppResult<String>;

    async fn persist_status_change_for_action(
        &self,
        id: &TaskId,
        from: InternalStatus,
        to: InternalStatus,
        trigger: &str,
        _action: TasksFeatureAction,
    ) -> AppResult<String> {
        self.persist_status_change(id, from, to, trigger).await
    }

    /// Get status history for audit
    async fn get_status_history(&self, id: &TaskId) -> AppResult<Vec<StatusTransition>>;

    /// Get status history for multiple tasks in a single query (batch version).
    /// Returns a map from task_id to its status transitions ordered by created_at ASC.
    /// Tasks with no history will not have an entry in the returned map.
    async fn get_status_history_batch(
        &self,
        task_ids: &[TaskId],
    ) -> AppResult<HashMap<TaskId, Vec<StatusTransition>>>;

    /// Get the timestamp when a task first entered a specific status
    ///
    /// Used for deterministic merge arbitration to identify which task
    /// entered `pending_merge` first. Returns None if the task never
    /// entered the specified status.
    ///
    /// # Arguments
    /// * `task_id` - The task ID to query
    /// * `status` - The status to search for in history
    ///
    /// # Returns
    /// * `Some(DateTime<Utc>)` - The timestamp when the task first entered this status
    /// * `None` - The task never entered this status
    async fn get_status_entered_at(
        &self,
        task_id: &TaskId,
        status: InternalStatus,
    ) -> AppResult<Option<DateTime<Utc>>>;

    /// Get the latest timestamp when a task entered a specific status.
    ///
    /// Used for execution attempt guards where re-enterable statuses must be
    /// scoped to the current execution episode, not the first historical entry.
    async fn get_status_last_entered_at(
        &self,
        task_id: &TaskId,
        status: InternalStatus,
    ) -> AppResult<Option<DateTime<Utc>>>;

    // ═══════════════════════════════════════════════════════════════════════
    // Query Operations
    // ═══════════════════════════════════════════════════════════════════════

    /// Get next task ready for execution (READY status, no unsatisfied dependencies)
    async fn get_next_executable(&self, project_id: &ProjectId) -> AppResult<Option<Task>>;

    /// Get all tasks belonging to an ideation session
    ///
    /// Used by cascade delete to find all tasks that should be deleted
    /// when an ideation session is removed.
    async fn get_by_ideation_session(&self, session_id: &IdeationSessionId)
        -> AppResult<Vec<Task>>;

    // ═══════════════════════════════════════════════════════════════════════
    // Archive Operations (Phase 18 - Soft Delete)
    // ═══════════════════════════════════════════════════════════════════════

    /// Get tasks by project, optionally including archived
    async fn get_by_project_filtered(
        &self,
        project_id: &ProjectId,
        include_archived: bool,
    ) -> AppResult<Vec<Task>>;

    /// Archive a task (soft delete)
    async fn archive(&self, task_id: &TaskId) -> AppResult<Task>;

    /// Restore an archived task
    async fn restore(&self, task_id: &TaskId) -> AppResult<Task>;

    /// Count archived tasks for a project
    ///
    /// # Arguments
    /// * `project_id` - The project ID
    /// * `ideation_session_id` - Optional ideation session ID to filter tasks
    ///
    /// # Returns
    /// * Count of archived tasks matching the criteria
    async fn get_archived_count(
        &self,
        project_id: &ProjectId,
        ideation_session_id: Option<&str>,
    ) -> AppResult<u32>;

    // ═══════════════════════════════════════════════════════════════════════
    // Pagination Operations (Phase 18 - Infinite Scroll)
    // ═══════════════════════════════════════════════════════════════════════

    /// Get paginated tasks for a project
    ///
    /// # Arguments
    /// * `project_id` - The project ID
    /// * `statuses` - Optional list of status filters (OR logic - matches any)
    /// * `offset` - Number of tasks to skip
    /// * `limit` - Maximum number of tasks to return
    /// * `include_archived` - Whether to include archived tasks
    /// * `ideation_session_id` - Optional ideation session ID to filter tasks
    /// * `execution_plan_id` - Optional execution plan ID to filter tasks (mutually exclusive with ideation_session_id)
    /// * `categories` - Optional list of category strings to filter by (OR logic - matches any)
    ///
    /// # Returns
    /// * Tasks ordered by created_at DESC (newest first)
    async fn list_paginated(
        &self,
        project_id: &ProjectId,
        statuses: Option<Vec<InternalStatus>>,
        offset: u32,
        limit: u32,
        include_archived: bool,
        ideation_session_id: Option<&str>,
        execution_plan_id: Option<&str>,
        categories: Option<&[String]>,
    ) -> AppResult<Vec<Task>>;

    /// Count total tasks for a project
    ///
    /// # Arguments
    /// * `project_id` - The project ID
    /// * `include_archived` - Whether to include archived tasks in the count
    /// * `ideation_session_id` - Optional ideation session ID to filter tasks
    /// * `execution_plan_id` - Optional execution plan ID to filter tasks (mutually exclusive with ideation_session_id)
    ///
    /// # Returns
    /// * Total count of tasks matching the criteria
    async fn count_tasks(
        &self,
        project_id: &ProjectId,
        include_archived: bool,
        ideation_session_id: Option<&str>,
        execution_plan_id: Option<&str>,
    ) -> AppResult<u32>;

    // ═══════════════════════════════════════════════════════════════════════
    // Search Operations (Phase 18 - Task Search)
    // ═══════════════════════════════════════════════════════════════════════

    /// Search tasks by title and description (case-insensitive)
    ///
    /// # Arguments
    /// * `project_id` - The project ID
    /// * `query` - The search query string
    /// * `include_archived` - Whether to include archived tasks in search results
    ///
    /// # Returns
    /// * Tasks matching the query in title OR description (case-insensitive)
    async fn search(
        &self,
        project_id: &ProjectId,
        query: &str,
        include_archived: bool,
    ) -> AppResult<Vec<Task>>;

    // ═══════════════════════════════════════════════════════════════════════
    // Cross-Project Scheduling Operations (Phase 26 - Auto-Scheduler)
    // ═══════════════════════════════════════════════════════════════════════

    /// Get the oldest Ready task across all projects
    ///
    /// Used by the auto-scheduler to find the next task to execute when
    /// execution slots become available. Returns the oldest (by created_at)
    /// Ready task from any project, excluding archived tasks.
    ///
    /// # Returns
    /// * `Some(Task)` - The oldest Ready task (FIFO ordering)
    /// * `None` - No Ready tasks exist across any project
    async fn get_oldest_ready_task(&self) -> AppResult<Option<Task>>;

    /// Get Ready tasks across all projects, ordered by created_at (oldest first)
    ///
    /// Used by the scheduler for Local-mode enforcement. Returns up to `limit`
    /// Ready tasks from any project, excluding archived tasks.
    /// The scheduler iterates through these to find one that's schedulable
    /// (accounting for Local-mode single-task constraint).
    ///
    /// # Arguments
    /// * `limit` - Maximum number of tasks to return
    ///
    /// # Returns
    /// * Vec of Ready tasks ordered by created_at ASC
    async fn get_oldest_ready_tasks(&self, limit: u32) -> AppResult<Vec<Task>>;

    /// Get Ready tasks that have been in Ready state longer than a threshold.
    ///
    /// Used by the watchdog to detect tasks stuck in Ready state due to
    /// missed scheduling triggers (lock contention, scheduler unavailable, etc.).
    /// A task is considered stale when `updated_at < (now - threshold_secs)`.
    ///
    /// # Arguments
    /// * `threshold_secs` - Age threshold in seconds; tasks older than this are returned
    ///
    /// # Returns
    /// * Vec of stale Ready tasks ordered by updated_at ASC (oldest first)
    async fn get_stale_ready_tasks(&self, threshold_secs: u64) -> AppResult<Vec<Task>>;

    // ═══════════════════════════════════════════════════════════════════════
    // State History Operations (Phase 64 - Link Conversation IDs)
    // ═══════════════════════════════════════════════════════════════════════

    /// Update the metadata of the most recent state history entry for a task
    ///
    /// Used to link a conversation_id and agent_run_id to the state transition
    /// after the conversation/agent_run is created. This enables history
    /// navigation to show the correct conversation for each historical state.
    ///
    /// # Arguments
    /// * `task_id` - The task ID
    /// * `metadata` - The metadata containing conversation_id and agent_run_id
    ///
    /// # Returns
    /// * `Ok(())` - Metadata was updated successfully
    /// * `Err` - No state history entry exists or database error
    async fn update_latest_state_history_metadata(
        &self,
        task_id: &TaskId,
        metadata: &StateHistoryMetadata,
    ) -> AppResult<()>;

    // ═══════════════════════════════════════════════════════════════════════
    // Git Mode Queue Enforcement (Phase 66 - Local Mode)
    // ═══════════════════════════════════════════════════════════════════════

    /// Check if a project has any task in one of the specified statuses
    ///
    /// Used by the scheduler to enforce single running task per Local-mode project.
    /// Returns true if at least one non-archived task exists in any of the given statuses.
    ///
    /// # Arguments
    /// * `project_id` - The project ID to check
    /// * `statuses` - List of statuses to check for
    ///
    /// # Returns
    /// * `true` - At least one task exists in one of the statuses
    /// * `false` - No tasks in any of the statuses
    async fn has_task_in_states(
        &self,
        project_id: &ProjectId,
        statuses: &[InternalStatus],
    ) -> AppResult<bool>;
}

#[cfg(test)]
#[path = "task_repository_tests.rs"]
mod tests;
