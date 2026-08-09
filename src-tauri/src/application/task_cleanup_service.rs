// Service for task cleanup: stop agent → git cleanup → DB archive → event emission
// Consolidates the inline cleanup logic from delete_ideation_session,
// SessionReopenService::reopen, and permanently_delete_task.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use ralphx_events::{emit_serialized, EventSink};
use serde::{Deserialize, Serialize};

use crate::application::agent_conversation_workspace::expand_worktree_parent_public;
use crate::application::chat_service::AgentRunCompletedPayload;
use crate::application::git_service::GitService;
use crate::application::interactive_process_registry::{
    InteractiveProcessKey, InteractiveProcessRegistry,
};
use crate::commands::execution_commands::AGENT_ACTIVE_STATUSES;
use crate::domain::entities::{
    IdeationSessionId, InternalStatus, Project, ProjectId, Task, TaskCategory, TaskId,
};
use crate::domain::repositories::{ProjectRepository, TaskRepository};
use crate::domain::services::{RunningAgentKey, RunningAgentRegistry};
use crate::domain::state_machine::transition_handler::{
    compute_merge_worktree_path, compute_plan_update_worktree_path, compute_rebase_worktree_path,
    compute_source_update_worktree_path,
};
use crate::error::{AppError, AppResult};
use crate::utils::path_safety::validate_absolute_non_root_path;

/// Abstraction for transitioning a task to Stopped status via the state machine.
/// Implemented by TaskTransitionService in production; allows test doubles.
#[async_trait]
pub trait TaskStopper: Send + Sync {
    /// Transition a task to Stopped, triggering on_exit side effects
    /// (decrement running_count, emit events, etc.).
    async fn transition_to_stopped(&self, task_id: &TaskId) -> AppResult<()>;

    /// Transition a task to Stopped with context capture for smart resume.
    ///
    /// This method captures the from_status and optional reason in metadata,
    /// enabling the "smart resume" feature to restore context when restarted.
    ///
    /// # Arguments
    /// * `task_id` - The task to stop
    /// * `from_status` - The status the task was in when stopped
    /// * `reason` - Optional reason for stopping
    async fn transition_to_stopped_with_context(
        &self,
        task_id: &TaskId,
        from_status: InternalStatus,
        reason: Option<String>,
    ) -> AppResult<()>;
}

/// Controls how running agents are stopped during cleanup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopMode {
    /// Use TransitionHandler to transition task → Stopped.
    /// Triggers on_exit side effects (decrement running_count, etc.).
    /// Use when cleanup is a deliberate user action (e.g., session deletion).
    Graceful,
    /// Directly stop the agent process via registry.stop() without
    /// transitioning through the state machine.
    /// Use when the task will be deleted immediately after stop (e.g., session reopen),
    /// or when the task may be in a transient state with no valid → Stopped transition.
    DirectStop,
}

/// Identifies a group of tasks for bulk operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum TaskGroup {
    /// All tasks belonging to an ideation session.
    #[serde(rename = "session")]
    Session {
        session_id: String,
        project_id: String,
    },
    /// All tasks with a given status in a project.
    #[serde(rename = "status")]
    Status { status: String, project_id: String },
    /// All tasks in a project with no ideation_session_id (standalone tasks).
    #[serde(rename = "uncategorized")]
    Uncategorized { project_id: String },
}

/// Report of cleanup results for batch operations.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CleanupReport {
    pub tasks_stopped: usize,
    pub tasks_archived: usize,
    pub git_cleanups: usize,
    pub errors: Vec<String>,
}

impl CleanupReport {
    /// Convenience accessors matching the Tauri command response field names.
    pub fn archived_count(&self) -> usize {
        self.tasks_archived
    }
    pub fn failed_count(&self) -> usize {
        self.errors.len()
    }
    pub fn stopped_agents(&self) -> usize {
        self.tasks_stopped
    }
}

pub struct TaskCleanupService {
    task_repo: Arc<dyn TaskRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    running_agent_registry: Arc<dyn RunningAgentRegistry>,
    interactive_process_registry: Option<Arc<InteractiveProcessRegistry>>,
    events: Arc<dyn EventSink>,
    /// Optional task stopper for Graceful mode. When set, Graceful stop will
    /// transition tasks to Stopped via the state machine (triggering on_exit
    /// side effects). When None, Graceful falls back to DirectStop behavior.
    task_stopper: Option<Arc<dyn TaskStopper>>,
}

pub(crate) fn is_agent_active_status(status: InternalStatus) -> bool {
    AGENT_ACTIVE_STATUSES.contains(&status)
}

impl TaskCleanupService {
    pub fn new(
        task_repo: Arc<dyn TaskRepository>,
        project_repo: Arc<dyn ProjectRepository>,
        running_agent_registry: Arc<dyn RunningAgentRegistry>,
        events: Arc<dyn EventSink>,
    ) -> Self {
        Self {
            task_repo,
            project_repo,
            running_agent_registry,
            interactive_process_registry: None,
            events,
            task_stopper: None,
        }
    }

    /// Set the interactive process registry for IPR cleanup on stop (builder pattern).
    pub fn with_interactive_process_registry(
        mut self,
        ipr: Arc<InteractiveProcessRegistry>,
    ) -> Self {
        self.interactive_process_registry = Some(ipr);
        self
    }

    /// Set the task stopper for Graceful mode (builder pattern).
    /// Required when using `StopMode::Graceful` to properly transition tasks
    /// through the state machine.
    pub fn with_task_stopper(mut self, stopper: Arc<dyn TaskStopper>) -> Self {
        self.task_stopper = Some(stopper);
        self
    }

    /// Clean up a single task: stop agent → git cleanup → DB archive → optional event.
    ///
    /// This is the core per-task cleanup unit. Callers control:
    /// - `stop_mode`: How to stop running agents (Graceful vs DirectStop)
    /// - `emit_events`: Whether to emit `task:archived` events for real-time UI updates
    pub async fn cleanup_single_task(
        &self,
        task: &Task,
        stop_mode: StopMode,
        emit_events: bool,
    ) -> AppResult<()> {
        let current_task = self.load_current_task(task).await;
        let project_id_str = current_task.project_id.as_str().to_string();

        // 1. Stop any live task context before cleanup starts.
        self.stop_task_for_cleanup(&current_task, stop_mode).await;

        // 2. Clean up git resources (worktree + branch)
        if let Some(ref branch) = current_task.task_branch {
            tracing::info!(
                task_id = current_task.id.as_str(),
                branch = branch.as_str(),
                "Cleaning up git resources for task"
            );
        }
        self.cleanup_git_resources(&current_task).await;

        // 3. Archive task in DB
        if let Err(e) = self.task_repo.archive(&current_task.id).await {
            tracing::warn!(
                task_id = current_task.id.as_str(),
                error = %e,
                "Failed to archive task during cleanup"
            );
            return Err(e);
        }
        tracing::info!(
            task_id = current_task.id.as_str(),
            "Archived task during cleanup"
        );

        // Final direct-stop sweep. If a task raced into a live context between the
        // first stop attempt and the archive write, kill the leaked worker now.
        if stop_mode == StopMode::DirectStop {
            self.stop_task_contexts_by_identity(&current_task.id).await;
        }

        // 4. Emit event for real-time UI updates
        if emit_events {
            self.emit_task_archived(current_task.id.as_str(), &project_id_str);
        }

        Ok(())
    }

    /// Clean archive a single task by reference (convenience wrapper).
    /// Uses Graceful stop mode, no event emission. Returns whether an agent was stopped.
    pub async fn cleanup_task_ref(&self, task: &Task) -> AppResult<bool> {
        let was_active = AGENT_ACTIVE_STATUSES.contains(&task.internal_status);
        self.cleanup_single_task(task, StopMode::Graceful, false)
            .await?;
        Ok(was_active)
    }

    /// Clean up multiple tasks in batch.
    pub async fn cleanup_tasks(
        &self,
        tasks: &[Task],
        stop_mode: StopMode,
        emit_events: bool,
    ) -> CleanupReport {
        let mut report = CleanupReport::default();
        let mut stopped_task_ids = HashSet::new();
        let mut current_tasks = Vec::with_capacity(tasks.len());

        // Re-fetch each task so cleanup decisions do not rely on a stale caller snapshot.
        for task in tasks {
            let current_task = self.load_current_task(task).await;
            if self.stop_task_for_cleanup(&current_task, stop_mode).await
                && stopped_task_ids.insert(current_task.id.clone())
            {
                report.tasks_stopped += 1;
            }
            current_tasks.push(current_task);
        }

        // Git cleanup for all tasks
        for task in &current_tasks {
            if task.task_branch.is_some() || task.worktree_path.is_some() {
                self.cleanup_git_resources(task).await;
                report.git_cleanups += 1;
            }
        }

        // Archive tasks in DB and emit events
        for task in &current_tasks {
            let project_id_str = task.project_id.as_str().to_string();
            if let Err(e) = self.task_repo.archive(&task.id).await {
                tracing::warn!(
                    task_id = task.id.as_str(),
                    error = %e,
                    "Failed to archive task during batch cleanup"
                );
                report
                    .errors
                    .push(format!("Archive {}: {}", task.id.as_str(), e));
            } else {
                report.tasks_archived += 1;
                if emit_events {
                    self.emit_task_archived(task.id.as_str(), &project_id_str);
                }
            }
        }

        if stop_mode == StopMode::DirectStop {
            for task in &current_tasks {
                if self.stop_task_contexts_by_identity(&task.id).await
                    && stopped_task_ids.insert(task.id.clone())
                {
                    report.tasks_stopped += 1;
                }
            }
        }

        report
    }

    /// Stop task runtimes and clean task git resources without archiving rows.
    ///
    /// Use this when the caller needs to perform a larger database transition atomically
    /// after runtime resources have been torn down.
    pub async fn preflight_tasks_for_replacement(
        &self,
        tasks: &[Task],
        preserved_branch: Option<&str>,
    ) -> AppResult<()> {
        for task in tasks {
            let current_task = self.load_current_task_strict(task).await?;
            if current_task.task_branch.is_some() || current_task.worktree_path.is_some() {
                self.validate_git_resources_strict(&current_task, preserved_branch)
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn stop_tasks_for_replacement(
        &self,
        tasks: &[Task],
        stop_mode: StopMode,
    ) -> AppResult<usize> {
        let mut stopped_task_ids = HashSet::new();
        for task in tasks {
            let current_task = self.load_current_task_strict(task).await?;
            if self.stop_task_for_cleanup(&current_task, stop_mode).await {
                stopped_task_ids.insert(current_task.id);
            }
        }
        Ok(stopped_task_ids.len())
    }

    pub async fn prepare_tasks_for_replacement(
        &self,
        tasks: &[Task],
        stop_mode: StopMode,
        preserved_branch: Option<&str>,
    ) -> CleanupReport {
        let mut report = CleanupReport::default();
        let mut stopped_task_ids = HashSet::new();
        let mut current_tasks = Vec::with_capacity(tasks.len());

        for task in tasks {
            let current_task = match self.load_current_task_strict(task).await {
                Ok(task) => task,
                Err(error) => {
                    report
                        .errors
                        .push(format!("Task reload {}: {error}", task.id.as_str()));
                    return report;
                }
            };
            current_tasks.push(current_task);
        }

        // Validate every target before stopping agents or mutating any worktree/ref.
        // A later unsafe task must not leave earlier tasks partially torn down.
        for task in &current_tasks {
            if task.task_branch.is_some() || task.worktree_path.is_some() {
                if let Err(error) = self
                    .validate_git_resources_strict(task, preserved_branch)
                    .await
                {
                    report
                        .errors
                        .push(format!("Git cleanup {}: {error}", task.id.as_str()));
                    return report;
                }
            }
        }

        for current_task in &current_tasks {
            if self.stop_task_for_cleanup(current_task, stop_mode).await
                && stopped_task_ids.insert(current_task.id.clone())
            {
                report.tasks_stopped += 1;
            }
        }

        for task in &current_tasks {
            if task.task_branch.is_some() || task.worktree_path.is_some() {
                match self
                    .cleanup_git_resources_strict(task, preserved_branch)
                    .await
                {
                    Ok(()) => report.git_cleanups += 1,
                    Err(error) => report
                        .errors
                        .push(format!("Git cleanup {}: {error}", task.id.as_str())),
                }
            }
        }

        if stop_mode == StopMode::DirectStop {
            for task in &current_tasks {
                if self.stop_task_contexts_by_identity(&task.id).await
                    && stopped_task_ids.insert(task.id.clone())
                {
                    report.tasks_stopped += 1;
                }
            }
        }

        report
    }

    /// Clean up all tasks in a group. Uses Graceful stop mode and emits events.
    /// Skips plan_merge tasks (system-managed).
    pub async fn cleanup_tasks_in_group(&self, group: TaskGroup) -> AppResult<CleanupReport> {
        let tasks = self.resolve_group_tasks(&group).await?;
        // Filter out plan_merge tasks (system-managed)
        let filtered: Vec<Task> = tasks
            .into_iter()
            .filter(|t| t.category != TaskCategory::PlanMerge)
            .collect();
        Ok(self
            .cleanup_tasks(&filtered, StopMode::Graceful, true)
            .await)
    }

    /// Stop the interactive Claude CLI process associated with an ideation session.
    ///
    /// Probes both `"ideation"` (Tauri IPC path) and `"session"` (HTTP external path)
    /// IPR keys, since the context_type string differs by spawn path. At most one will
    /// exist per session.
    ///
    /// Returns `true` if a process was found and cleaned up, `false` otherwise.
    pub async fn stop_ideation_session_agent(&self, session_id: &str) -> bool {
        let ipr = match self.interactive_process_registry.as_ref() {
            Some(ipr) => ipr,
            None => {
                tracing::warn!(
                    session_id = %session_id,
                    "IPR cleanup: interactive_process_registry not set; \
                     call .with_interactive_process_registry() on TaskCleanupService"
                );
                return false;
            }
        };

        // Try "ideation" key first (Tauri IPC spawn path), then "session" (HTTP spawn path).
        let context_types = ["ideation", "session"];
        let mut matched_context_type: Option<&str> = None;

        for ct in &context_types {
            let key = InteractiveProcessKey::new(*ct, session_id);
            if ipr.has_process(&key).await {
                ipr.remove(&key).await;
                matched_context_type = Some(ct);
                break;
            }
        }

        let matched_context_type = match matched_context_type {
            Some(ct) => ct,
            None => return false,
        };

        // Stop agent in running_agent_registry (SIGTERM + unregister).
        let registry_key = RunningAgentKey::new(matched_context_type, session_id);
        match self.running_agent_registry.stop(&registry_key).await {
            Ok(Some(info)) => {
                let _ = emit_serialized(
                    self.events.as_ref(),
                    "agent:stopped",
                    &serde_json::json!({
                        "conversation_id": info.conversation_id,
                        "agent_run_id": info.agent_run_id,
                        "context_type": matched_context_type,
                        "context_id": session_id,
                    }),
                );
                let _ = emit_serialized(
                    self.events.as_ref(),
                    "agent:run_completed",
                    &AgentRunCompletedPayload::with_provider_session(
                        info.conversation_id,
                        matched_context_type.to_string(),
                        session_id.to_string(),
                        None,
                        None,
                        None,
                    ),
                );
            }
            Ok(None) => {
                tracing::debug!(
                    session_id = %session_id,
                    "IPR cleanup: no running agent registry entry for session"
                );
            }
            Err(e) => {
                tracing::warn!(
                    session_id = %session_id,
                    error = %e,
                    "IPR cleanup: failed to stop agent for session"
                );
            }
        }

        true
    }

    // ── Private helpers ──────────────────────────────────────────────────

    /// Stop a running agent for a task.
    ///
    /// - `Graceful`: stop agent process, then transition to Stopped via state machine
    ///   (triggers on_exit side effects like decrement running_count).
    /// - `DirectStop`: stop agent process only, bypass state machine.
    async fn stop_task_agent(&self, task: &Task, stop_mode: StopMode) -> bool {
        // Step 1: Always stop the agent process
        let context_type = match task.internal_status {
            InternalStatus::Reviewing => "review",
            InternalStatus::Merging => "merge",
            _ => "task_execution",
        };

        // Remove from interactive process registry first — closes stdin pipe
        // so the process doesn't linger waiting for input after SIGTERM.
        if let Some(ref ipr) = self.interactive_process_registry {
            let ipr_key = InteractiveProcessKey::new(context_type, task.id.as_str());
            ipr.remove(&ipr_key).await;
        }

        let key = RunningAgentKey::new(context_type, task.id.as_str());
        let stopped = self
            .running_agent_registry
            .stop(&key)
            .await
            .ok()
            .flatten()
            .is_some();

        // Step 2: For Graceful mode, also transition to Stopped via state machine
        if stop_mode == StopMode::Graceful {
            if let Some(ref stopper) = self.task_stopper {
                if let Err(e) = stopper.transition_to_stopped(&task.id).await {
                    tracing::warn!(
                        task_id = task.id.as_str(),
                        error = %e,
                        "Failed to transition task to Stopped during cleanup (non-fatal)"
                    );
                }
            }
        }

        stopped
    }

    async fn load_current_task(&self, task: &Task) -> Task {
        match self.task_repo.get_by_id(&task.id).await {
            Ok(Some(current)) => current,
            _ => task.clone(),
        }
    }

    async fn load_current_task_strict(&self, task: &Task) -> AppResult<Task> {
        self.task_repo.get_by_id(&task.id).await?.ok_or_else(|| {
            AppError::NotFound(format!(
                "Task {} no longer exists during replacement cleanup",
                task.id
            ))
        })
    }

    async fn stop_task_for_cleanup(&self, task: &Task, stop_mode: StopMode) -> bool {
        match stop_mode {
            StopMode::Graceful => {
                if AGENT_ACTIVE_STATUSES.contains(&task.internal_status) {
                    self.stop_task_agent(task, stop_mode).await
                } else {
                    false
                }
            }
            StopMode::DirectStop => self.stop_task_contexts_by_identity(&task.id).await,
        }
    }

    async fn stop_task_contexts_by_identity(&self, task_id: &TaskId) -> bool {
        match self.stop_task_runtime_contexts_strict(task_id).await {
            Ok(stopped) => stopped,
            Err(error) => {
                tracing::warn!(
                    task_id = task_id.as_str(),
                    error = %error,
                    "Failed to stop every task runtime context"
                );
                false
            }
        }
    }

    pub(crate) async fn stop_task_runtime_contexts_strict(
        &self,
        task_id: &TaskId,
    ) -> AppResult<bool> {
        let mut stopped_any = false;

        for context_type in ["task_execution", "review", "merge", "branch_update"] {
            if let Some(ref ipr) = self.interactive_process_registry {
                let ipr_key = InteractiveProcessKey::new(context_type, task_id.as_str());
                ipr.remove(&ipr_key).await;
            }

            let key = RunningAgentKey::new(context_type, task_id.as_str());
            if self
                .running_agent_registry
                .stop(&key)
                .await
                .map_err(AppError::Infrastructure)?
                .is_some()
            {
                stopped_any = true;
            }
        }

        Ok(stopped_any)
    }

    /// Clean task Git resources for destructive attempt replacement.
    async fn validate_git_resources_strict(
        &self,
        task: &Task,
        preserved_branch: Option<&str>,
    ) -> AppResult<()> {
        self.handle_git_resources_strict(task, preserved_branch, false)
            .await
    }

    async fn cleanup_git_resources_strict(
        &self,
        task: &Task,
        preserved_branch: Option<&str>,
    ) -> AppResult<()> {
        self.handle_git_resources_strict(task, preserved_branch, true)
            .await
    }

    async fn handle_git_resources_strict(
        &self,
        task: &Task,
        preserved_branch: Option<&str>,
        mutate: bool,
    ) -> AppResult<()> {
        let project = self
            .project_repo
            .get_by_id(&task.project_id)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "Project not found during replacement cleanup: {}",
                    task.project_id
                ))
            })?;
        let repo_path = validate_absolute_non_root_path(
            Path::new(&project.working_directory),
            "replacement cleanup project checkout",
        )?;

        if let Some(worktree_path) = task.worktree_path.as_deref() {
            let worktree_path = validate_replacement_worktree_path(&project, task, worktree_path)?;
            let registered_owner = GitService::list_worktrees(&repo_path)
                .await?
                .into_iter()
                .find(|worktree| {
                    validate_absolute_non_root_path(
                        Path::new(&worktree.path),
                        "registered replacement worktree",
                    )
                    .is_ok_and(|registered_path| {
                        replacement_paths_match(&registered_path, &worktree_path)
                    })
                });
            match registered_owner {
                Some(owner)
                    if owner.branch.as_deref() == task.task_branch.as_deref()
                        || owner.branch.as_deref() == preserved_branch =>
                {
                    if mutate {
                        GitService::delete_worktree(&repo_path, &worktree_path).await?;
                    }
                }
                Some(owner) => {
                    return Err(AppError::Validation(format!(
                        "Task {} worktree is registered to unexpected branch {:?}",
                        task.id, owner.branch
                    )));
                }
                None if worktree_path.exists() => {
                    return Err(AppError::Validation(format!(
                        "Task {} worktree exists without a matching Git registration",
                        task.id
                    )));
                }
                None => {}
            }
        }

        let Some(task_branch) = task.task_branch.as_deref() else {
            return Ok(());
        };
        if preserved_branch == Some(task_branch) {
            return Ok(());
        }

        let base_branch = project.base_branch.as_deref().unwrap_or("main");
        let current_branch = GitService::get_current_branch(&repo_path).await?;
        let registered_worktrees = GitService::list_worktrees(&repo_path).await?;
        for owner in registered_worktrees {
            if owner.branch.as_deref() != Some(task_branch) {
                continue;
            }
            let owner_path = validate_absolute_non_root_path(
                Path::new(&owner.path),
                "registered replacement branch owner",
            )?;
            let is_project_root = replacement_paths_match(&owner_path, &repo_path);
            let is_task_worktree = task
                .worktree_path
                .as_deref()
                .and_then(|path| validate_replacement_worktree_path(&project, task, path).ok())
                .is_some_and(|path| replacement_paths_match(&owner_path, &path));
            if !is_project_root && !is_task_worktree {
                return Err(AppError::Validation(format!(
                    "Task {} branch is checked out by an unexpected worktree",
                    task.id
                )));
            }
        }
        if mutate && current_branch == task_branch {
            GitService::checkout_branch(&repo_path, base_branch).await?;
        }
        if mutate && GitService::branch_exists_strict(&repo_path, task_branch).await? {
            GitService::delete_branch(&repo_path, task_branch, true).await?;
        }
        Ok(())
    }

    /// Clean up git resources (worktree + branch) for a task.
    /// Best-effort — errors are logged but not propagated.
    async fn cleanup_git_resources(&self, task: &Task) {
        let project = match self.project_repo.get_by_id(&task.project_id).await {
            Ok(Some(p)) => p,
            _ => return,
        };

        let repo_path = PathBuf::from(&project.working_directory);
        let base_branch = project.base_branch.as_deref().unwrap_or("main");
        let task_branch = match &task.task_branch {
            Some(branch) => branch.clone(),
            None => return,
        };

        // Delete worktree first if it exists
        if let Some(ref worktree_path) = task.worktree_path {
            let worktree_path_buf = PathBuf::from(worktree_path);
            if let Err(e) = GitService::delete_worktree(&repo_path, &worktree_path_buf).await {
                tracing::warn!(
                    worktree = worktree_path.as_str(),
                    error = %e,
                    "Failed to delete worktree during cleanup (non-fatal)"
                );
            }
        }

        // Only checkout base branch if the task branch is currently checked out in main repo.
        // In Worktree mode the task branch lives in a worktree, not the main checkout,
        // so this is normally a no-op. Guards against edge cases from old Local mode.
        let current_branch = GitService::get_current_branch(&repo_path)
            .await
            .unwrap_or_default();
        if current_branch == task_branch {
            if let Err(e) = GitService::checkout_branch(&repo_path, base_branch).await {
                tracing::warn!(
                    base_branch = base_branch,
                    error = %e,
                    "Failed to checkout base branch during cleanup (non-fatal)"
                );
            }
        }

        // Delete task branch
        if let Err(e) = GitService::delete_branch(&repo_path, &task_branch, true).await {
            tracing::warn!(
                branch = task_branch.as_str(),
                error = %e,
                "Failed to delete branch during cleanup (non-fatal)"
            );
        }
    }

    /// Resolve a TaskGroup to the actual tasks.
    async fn resolve_group_tasks(&self, group: &TaskGroup) -> AppResult<Vec<Task>> {
        match group {
            TaskGroup::Session { session_id, .. } => {
                let session_id = IdeationSessionId::from_string(session_id.clone());
                self.task_repo.get_by_ideation_session(&session_id).await
            }
            TaskGroup::Status { status, project_id } => {
                let project_id = ProjectId::from_string(project_id.clone());
                let internal_status: InternalStatus = status.parse().map_err(|_| {
                    crate::error::AppError::Validation(format!("Invalid status: {}", status))
                })?;
                self.task_repo
                    .get_by_status(&project_id, internal_status)
                    .await
            }
            TaskGroup::Uncategorized { project_id } => {
                let project_id = ProjectId::from_string(project_id.clone());
                let all_tasks = self.task_repo.get_by_project(&project_id).await?;
                Ok(all_tasks
                    .into_iter()
                    .filter(|t| t.ideation_session_id.is_none())
                    .collect())
            }
        }
    }

    /// Emit a task:archived event for real-time UI updates.
    fn emit_task_archived(&self, task_id: &str, project_id: &str) {
        let _ = emit_serialized(
            self.events.as_ref(),
            "task:archived",
            &serde_json::json!({
                "taskId": task_id,
                "projectId": project_id,
            }),
        );
    }
}

fn validate_replacement_worktree_path(
    project: &Project,
    task: &Task,
    stored_path: &str,
) -> AppResult<PathBuf> {
    let stored_path =
        validate_absolute_non_root_path(Path::new(stored_path), "replacement task worktree")?;
    let allowed_paths = [
        project
            .task_worktree_path(task.id.as_str())
            .to_string_lossy()
            .into_owned(),
        compute_merge_worktree_path(project, task.id.as_str()),
        compute_rebase_worktree_path(project, task.id.as_str()),
        compute_plan_update_worktree_path(project, task.id.as_str()),
        compute_source_update_worktree_path(project, task.id.as_str()),
    ]
    .map(PathBuf::from);
    if !allowed_paths.iter().any(|path| path == &stored_path) {
        return Err(AppError::Validation(format!(
            "Task {} worktree does not match a process-owned derived path",
            task.id
        )));
    }

    let worktree_root = expand_worktree_parent_public(project.worktree_parent_or_default())?;
    let worktree_root =
        validate_absolute_non_root_path(&worktree_root, "configured worktree root")?;
    let canonical_path = stored_path
        .canonicalize()
        .unwrap_or_else(|_| stored_path.clone());
    let canonical_root = worktree_root.canonicalize().unwrap_or(worktree_root);
    if canonical_path == canonical_root || !canonical_path.starts_with(&canonical_root) {
        return Err(AppError::Validation(format!(
            "Task {} worktree escapes the configured worktree root",
            task.id
        )));
    }
    Ok(stored_path)
}

fn replacement_paths_match(left: &Path, right: &Path) -> bool {
    left == right
        || left
            .canonicalize()
            .ok()
            .zip(right.canonicalize().ok())
            .is_some_and(|(left, right)| left == right)
}
