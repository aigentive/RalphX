// Memory-based TaskRepository implementation for testing
// Uses RwLock<HashMap> for thread-safe storage without a real database

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

use async_trait::async_trait;
use chrono::Utc;

use crate::domain::entities::{IdeationSessionId, InternalStatus, ProjectId, Task, TaskId};
use crate::domain::ideation::TasksFeatureAction;
use crate::domain::repositories::{StateHistoryMetadata, StatusTransition, TaskRepository};
use crate::error::AppResult;

/// In-memory implementation of TaskRepository for testing
/// Uses RwLock<HashMap> for thread-safe storage
pub struct MemoryTaskRepository {
    tasks: Arc<RwLock<HashMap<TaskId, Task>>>,
    history: Arc<RwLock<Vec<(TaskId, StatusTransition)>>>,
}

impl Default for MemoryTaskRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryTaskRepository {
    /// Create a new empty in-memory task repository
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            history: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Create with pre-populated tasks (for tests)
    pub fn with_tasks(tasks: Vec<Task>) -> Self {
        let map: HashMap<TaskId, Task> = tasks.into_iter().map(|t| (t.id.clone(), t)).collect();
        Self {
            tasks: Arc::new(RwLock::new(map)),
            history: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

#[async_trait]
impl TaskRepository for MemoryTaskRepository {
    async fn create(&self, task: Task) -> AppResult<Task> {
        let mut tasks = self.tasks.write().await;
        tasks.insert(task.id.clone(), task.clone());
        Ok(task)
    }

    async fn get_by_id(&self, id: &TaskId) -> AppResult<Option<Task>> {
        let tasks = self.tasks.read().await;
        Ok(tasks.get(id).cloned())
    }

    async fn get_by_project(&self, project_id: &ProjectId) -> AppResult<Vec<Task>> {
        let tasks = self.tasks.read().await;
        let mut result: Vec<Task> = tasks
            .values()
            .filter(|t| t.project_id == *project_id)
            .cloned()
            .collect();
        // Sort by priority (desc) then created_at (asc)
        result.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| a.created_at.cmp(&b.created_at))
        });
        Ok(result)
    }

    async fn get_by_ideation_session(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<Vec<Task>> {
        let tasks = self.tasks.read().await;
        let mut result: Vec<Task> = tasks
            .values()
            .filter(|t| {
                t.archived_at.is_none()
                    && t.ideation_session_id
                        .as_ref()
                        .map(|id| id == session_id)
                        .unwrap_or(false)
            })
            .cloned()
            .collect();
        result.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        Ok(result)
    }

    async fn update(&self, task: &Task) -> AppResult<()> {
        let mut tasks = self.tasks.write().await;
        tasks.insert(task.id.clone(), task.clone());
        Ok(())
    }

    async fn update_with_expected_status(
        &self,
        task: &Task,
        expected_status: InternalStatus,
    ) -> AppResult<bool> {
        let mut tasks = self.tasks.write().await;
        if let Some(existing) = tasks.get(&task.id) {
            if existing.internal_status != expected_status {
                return Ok(false); // Another caller already transitioned
            }
        } else {
            return Ok(false); // Task not found
        }
        tasks.insert(task.id.clone(), task.clone());
        Ok(true)
    }

    async fn update_with_expected_status_and_history_for_action(
        &self,
        task: &Task,
        expected_status: InternalStatus,
        trigger: &str,
        _action: TasksFeatureAction,
    ) -> AppResult<Option<String>> {
        let mut tasks = self.tasks.write().await;
        if tasks
            .get(&task.id)
            .is_none_or(|existing| existing.internal_status != expected_status)
        {
            return Ok(None);
        }
        tasks.insert(task.id.clone(), task.clone());
        let mut history = self.history.write().await;
        let history_id = uuid::Uuid::new_v4().to_string();
        history.push((
            task.id.clone(),
            StatusTransition::new(expected_status, task.internal_status, trigger),
        ));
        Ok(Some(history_id))
    }

    async fn update_metadata(&self, id: &TaskId, metadata: Option<String>) -> AppResult<()> {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(id) {
            task.metadata = metadata;
            task.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn delete(&self, id: &TaskId) -> AppResult<()> {
        let mut tasks = self.tasks.write().await;
        tasks.remove(id);
        Ok(())
    }

    async fn get_by_status(
        &self,
        project_id: &ProjectId,
        status: InternalStatus,
    ) -> AppResult<Vec<Task>> {
        let tasks = self.tasks.read().await;
        let mut result: Vec<Task> = tasks
            .values()
            .filter(|t| {
                t.project_id == *project_id
                    && t.internal_status == status
                    && t.archived_at.is_none()
            })
            .cloned()
            .collect();
        result.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| a.created_at.cmp(&b.created_at))
        });
        Ok(result)
    }

    async fn persist_status_change(
        &self,
        id: &TaskId,
        from: InternalStatus,
        to: InternalStatus,
        trigger: &str,
    ) -> AppResult<String> {
        // Update task status
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(id) {
            task.internal_status = to;
            task.updated_at = Utc::now();
        }
        drop(tasks);

        // Record history
        let mut history = self.history.write().await;
        let history_id = uuid::Uuid::new_v4().to_string();
        history.push((id.clone(), StatusTransition::new(from, to, trigger)));

        Ok(history_id)
    }

    async fn get_status_history(&self, id: &TaskId) -> AppResult<Vec<StatusTransition>> {
        let history = self.history.read().await;
        let transitions: Vec<StatusTransition> = history
            .iter()
            .filter(|(task_id, _)| task_id == id)
            .map(|(_, transition)| transition.clone())
            .collect();
        Ok(transitions)
    }

    async fn get_status_history_batch(
        &self,
        task_ids: &[TaskId],
    ) -> AppResult<HashMap<TaskId, Vec<StatusTransition>>> {
        let history = self.history.read().await;
        let id_set: HashSet<&TaskId> = task_ids.iter().collect();
        let mut result: HashMap<TaskId, Vec<StatusTransition>> = HashMap::new();
        for (task_id, transition) in history.iter() {
            if id_set.contains(task_id) {
                result
                    .entry(task_id.clone())
                    .or_default()
                    .push(transition.clone());
            }
        }
        Ok(result)
    }

    async fn get_status_entered_at(
        &self,
        task_id: &TaskId,
        status: InternalStatus,
    ) -> AppResult<Option<chrono::DateTime<Utc>>> {
        let history = self.history.read().await;

        // Find all transitions where the task entered the specified status
        let matching_timestamps: Vec<chrono::DateTime<Utc>> = history
            .iter()
            .filter(|(id, transition)| id == task_id && transition.to == status)
            .map(|(_, transition)| transition.timestamp)
            .collect();

        // Return the earliest timestamp, or None if no matches
        Ok(matching_timestamps.into_iter().min())
    }

    async fn get_status_last_entered_at(
        &self,
        task_id: &TaskId,
        status: InternalStatus,
    ) -> AppResult<Option<chrono::DateTime<Utc>>> {
        let history = self.history.read().await;

        let matching_timestamps: Vec<chrono::DateTime<Utc>> = history
            .iter()
            .filter(|(id, transition)| id == task_id && transition.to == status)
            .map(|(_, transition)| transition.timestamp)
            .collect();

        Ok(matching_timestamps.into_iter().max())
    }

    async fn get_next_executable(&self, project_id: &ProjectId) -> AppResult<Option<Task>> {
        let tasks = self.tasks.read().await;

        // Note: dependency checking is done via TaskDependencyRepository in production SQL.
        // The in-memory impl returns highest-priority ready task without dependency filtering.
        let mut ready_tasks: Vec<&Task> = tasks
            .values()
            .filter(|t| t.project_id == *project_id && t.internal_status == InternalStatus::Ready)
            .collect();

        ready_tasks.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| a.created_at.cmp(&b.created_at))
        });

        Ok(ready_tasks.first().cloned().cloned())
    }

    async fn get_by_project_filtered(
        &self,
        project_id: &ProjectId,
        include_archived: bool,
    ) -> AppResult<Vec<Task>> {
        let tasks = self.tasks.read().await;
        let mut result: Vec<Task> = tasks
            .values()
            .filter(|t| {
                t.project_id == *project_id && (include_archived || t.archived_at.is_none())
            })
            .cloned()
            .collect();
        // Sort by priority (desc) then created_at (asc)
        result.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| a.created_at.cmp(&b.created_at))
        });
        Ok(result)
    }

    async fn archive(&self, task_id: &TaskId) -> AppResult<Task> {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.archived_at = Some(Utc::now());
            task.updated_at = Utc::now();
            Ok(task.clone())
        } else {
            Err(crate::error::AppError::NotFound(format!(
                "Task with id {} not found",
                task_id.as_str()
            )))
        }
    }

    async fn restore(&self, task_id: &TaskId) -> AppResult<Task> {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.archived_at = None;
            task.updated_at = Utc::now();
            Ok(task.clone())
        } else {
            Err(crate::error::AppError::NotFound(format!(
                "Task with id {} not found",
                task_id.as_str()
            )))
        }
    }

    async fn get_archived_count(
        &self,
        project_id: &ProjectId,
        ideation_session_id: Option<&str>,
    ) -> AppResult<u32> {
        let tasks = self.tasks.read().await;
        let count = tasks
            .values()
            .filter(|t| {
                t.project_id == *project_id
                    && t.archived_at.is_some()
                    && (ideation_session_id.is_none()
                        || t.ideation_session_id
                            .as_ref()
                            .is_some_and(|id| id.as_str() == ideation_session_id.unwrap()))
            })
            .count();
        Ok(count as u32)
    }

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
    ) -> AppResult<Vec<Task>> {
        let tasks = self.tasks.read().await;

        // Filter tasks based on criteria
        let mut result: Vec<Task> = tasks
            .values()
            .filter(|t| {
                // Match project
                if t.project_id != *project_id {
                    return false;
                }

                // Match archived status
                if !include_archived && t.archived_at.is_some() {
                    return false;
                }

                // Match status if provided (any of the statuses)
                if let Some(ref status_vec) = statuses {
                    if !status_vec.contains(&t.internal_status) {
                        return false;
                    }
                }

                // Match session_id if provided
                if let Some(sid) = ideation_session_id {
                    if t.ideation_session_id
                        .as_ref()
                        .is_none_or(|id| id.as_str() != sid)
                    {
                        return false;
                    }
                }

                // Match execution_plan_id if provided
                if let Some(epid) = execution_plan_id {
                    if t.execution_plan_id
                        .as_ref()
                        .is_none_or(|id| id.as_str() != epid)
                    {
                        return false;
                    }
                }

                // Match category if provided (any of the categories)
                if let Some(cats) = categories {
                    if !cats.iter().any(|c| *c == t.category.to_string()) {
                        return false;
                    }
                }

                true
            })
            .cloned()
            .collect();

        // Sort by created_at DESC (newest first)
        result.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        // Apply pagination
        let start = offset as usize;
        let paginated = result
            .into_iter()
            .skip(start)
            .take(limit as usize)
            .collect();

        Ok(paginated)
    }

    async fn count_tasks(
        &self,
        project_id: &ProjectId,
        include_archived: bool,
        ideation_session_id: Option<&str>,
        execution_plan_id: Option<&str>,
    ) -> AppResult<u32> {
        let tasks = self.tasks.read().await;
        let count = tasks
            .values()
            .filter(|t| {
                t.project_id == *project_id
                    && (include_archived || t.archived_at.is_none())
                    && (ideation_session_id.is_none()
                        || t.ideation_session_id
                            .as_ref()
                            .is_some_and(|id| id.as_str() == ideation_session_id.unwrap()))
                    && (execution_plan_id.is_none()
                        || t.execution_plan_id
                            .as_ref()
                            .is_some_and(|id| id.as_str() == execution_plan_id.unwrap()))
            })
            .count();
        Ok(count as u32)
    }

    async fn search(
        &self,
        project_id: &ProjectId,
        query: &str,
        include_archived: bool,
    ) -> AppResult<Vec<Task>> {
        let tasks = self.tasks.read().await;

        // Convert query to lowercase for case-insensitive search
        let query_lower = query.to_lowercase();

        let mut result: Vec<Task> = tasks
            .values()
            .filter(|t| {
                // Match project
                if t.project_id != *project_id {
                    return false;
                }

                // Match archived status
                if !include_archived && t.archived_at.is_some() {
                    return false;
                }

                // Search in title OR description (case-insensitive)
                let title_matches = t.title.to_lowercase().contains(&query_lower);
                let description_matches = t
                    .description
                    .as_ref()
                    .map(|d| d.to_lowercase().contains(&query_lower))
                    .unwrap_or(false);

                title_matches || description_matches
            })
            .cloned()
            .collect();

        // Sort by created_at DESC (newest first)
        result.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        Ok(result)
    }

    async fn get_oldest_ready_task(&self) -> AppResult<Option<Task>> {
        let tasks = self.tasks.read().await;

        // Find all Ready tasks that are not archived
        let mut ready_tasks: Vec<&Task> = tasks
            .values()
            .filter(|t| t.internal_status == InternalStatus::Ready && t.archived_at.is_none())
            .collect();

        // Sort by created_at ASC (oldest first) for FIFO
        ready_tasks.sort_by(|a, b| a.created_at.cmp(&b.created_at));

        Ok(ready_tasks.first().cloned().cloned())
    }

    async fn get_oldest_ready_tasks(&self, limit: u32) -> AppResult<Vec<Task>> {
        let tasks = self.tasks.read().await;

        // Find all Ready tasks that are not archived
        let mut ready_tasks: Vec<Task> = tasks
            .values()
            .filter(|t| t.internal_status == InternalStatus::Ready && t.archived_at.is_none())
            .cloned()
            .collect();

        // Sort by created_at ASC (oldest first) for FIFO
        ready_tasks.sort_by(|a, b| a.created_at.cmp(&b.created_at));

        // Apply limit
        ready_tasks.truncate(limit as usize);

        Ok(ready_tasks)
    }

    async fn get_stale_ready_tasks(&self, threshold_secs: u64) -> AppResult<Vec<Task>> {
        use chrono::{Duration, Utc};

        let cutoff = Utc::now() - Duration::seconds(threshold_secs as i64);
        let tasks = self.tasks.read().await;

        // Find Ready tasks not archived and with updated_at before the cutoff
        let mut stale_tasks: Vec<Task> = tasks
            .values()
            .filter(|t| {
                t.internal_status == InternalStatus::Ready
                    && t.archived_at.is_none()
                    && t.updated_at < cutoff
            })
            .cloned()
            .collect();

        // Sort by updated_at ASC (oldest first)
        stale_tasks.sort_by(|a, b| a.updated_at.cmp(&b.updated_at));

        Ok(stale_tasks)
    }

    async fn update_latest_state_history_metadata(
        &self,
        _task_id: &TaskId,
        _metadata: &StateHistoryMetadata,
    ) -> AppResult<()> {
        // In-memory implementation doesn't persist metadata
        Ok(())
    }

    async fn has_task_in_states(
        &self,
        project_id: &ProjectId,
        statuses: &[InternalStatus],
    ) -> AppResult<bool> {
        if statuses.is_empty() {
            return Ok(false);
        }

        let tasks = self.tasks.read().await;
        let has_match = tasks.values().any(|t| {
            t.project_id == *project_id
                && t.archived_at.is_none()
                && statuses.contains(&t.internal_status)
        });

        Ok(has_match)
    }
}

#[cfg(test)]
mod tests;
