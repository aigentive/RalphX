// Task Scheduler Service
//
// Production implementation of the TaskScheduler trait for auto-scheduling Ready tasks.
// This service checks execution capacity and transitions the oldest Ready task to Executing
// when slots are available.
//
// Called from:
// - TransitionHandler::on_exit() when an agent-active task completes (slot freed)
// - TransitionHandler::on_enter(Ready) when a task becomes Ready
// - StartupJobRunner after resuming agent-active tasks
// - resume_execution and set_max_concurrent commands (future Phase 26 tasks)

mod helpers;
mod merge_retry;
mod watchdog;

pub use watchdog::ReadyWatchdog;

use async_trait::async_trait;
use chrono::Utc;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc, Mutex,
};
use tauri::AppHandle;
use tokio::sync::{Mutex as TokioMutex, RwLock};

use crate::application::chat_service::uses_execution_slot;
use crate::application::harness_runtime_registry::default_scheduler_runtime_config;
use crate::application::runtime_factory::{
    build_transition_service_with_fallback, RuntimeFactoryDeps,
};
use crate::commands::execution_commands::context_matches_running_status_for_gc;
use crate::commands::ExecutionState;
use crate::domain::entities::{
    task_metadata::{
        MergeFailureSource, MergeRecoveryEvent, MergeRecoveryEventKind, MergeRecoveryMetadata,
        MergeRecoveryReasonCode, MergeRecoverySource, MergeRecoveryState,
    },
    ChatContextType, ExecutionPlanId, IdeationSessionId, InternalStatus, ProjectId, Task,
    TaskCategory,
};
use crate::domain::repositories::{
    ActivityEventRepository, AgentLaneSettingsRepository, AgentProviderSettingsRepository,
    AgentRunRepository, ArtifactRepository, ChatAttachmentRepository, ChatConversationRepository,
    ChatMessageRepository, ExecutionPlanRepository, ExecutionSettingsRepository,
    ExternalEventsRepository, IdeationSessionRepository, MemoryEventRepository,
    PlanBranchRepository, ProjectRepository, TaskDependencyRepository, TaskRepository,
    TaskStepRepository, ValidationRunRepository,
};
use crate::domain::services::{
    GithubServiceTrait, MessageQueue, PlanPrDescriptionDrafter, RunningAgentRegistry,
};
use crate::domain::state_machine::services::{TaskScheduler, WebhookPublisher};

use super::{
    AgentClientBundle, InteractiveProcessRegistry, PrPollerRegistry, TaskTransitionService,
};
use crate::domain::state_machine::transition_handler::freshness::FreshnessMetadata;
use crate::domain::state_machine::transition_handler::{get_trigger_origin, set_trigger_origin};

/// Production implementation of TaskScheduler for auto-scheduling Ready tasks.
///
/// This service queries for the oldest Ready task across all projects and
/// transitions it to Executing when execution slots are available.
///
/// Phase 82: Supports optional project scoping via `active_project_id` filter.
/// When set, only tasks from that project will be scheduled.
pub struct TaskSchedulerService {
    pub(super) execution_state: Arc<ExecutionState>,
    pub(super) project_repo: Arc<dyn ProjectRepository>,
    pub(super) task_repo: Arc<dyn TaskRepository>,
    pub(super) task_step_repo: Option<Arc<dyn TaskStepRepository>>,
    pub(super) validation_run_repo: Option<Arc<dyn ValidationRunRepository>>,
    pub(super) external_events_repo: Option<Arc<dyn ExternalEventsRepository>>,
    pub(super) webhook_publisher: Option<Arc<dyn WebhookPublisher>>,
    pub(super) task_dependency_repo: Arc<dyn TaskDependencyRepository>,
    pub(super) artifact_repo: Arc<dyn ArtifactRepository>,
    pub(super) chat_message_repo: Arc<dyn ChatMessageRepository>,
    pub(super) chat_attachment_repo: Arc<dyn ChatAttachmentRepository>,
    pub(super) conversation_repo: Arc<dyn ChatConversationRepository>,
    pub(super) agent_run_repo: Arc<dyn AgentRunRepository>,
    pub(super) ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    pub(super) activity_event_repo: Arc<dyn ActivityEventRepository>,
    pub(super) message_queue: Arc<MessageQueue>,
    pub(super) running_agent_registry: Arc<dyn RunningAgentRegistry>,
    pub(super) memory_event_repo: Arc<dyn MemoryEventRepository>,
    pub(super) app_handle: Option<AppHandle>,
    /// Optional plan branch repository for feature branch resolution.
    pub(super) plan_branch_repo: Option<Arc<dyn PlanBranchRepository>>,
    /// Optional execution plan repository for filtering out superseded plan tasks.
    pub(super) execution_plan_repo: Option<Arc<dyn ExecutionPlanRepository>>,
    /// Optional shared AppState InteractiveProcessRegistry for stdin message delivery.
    pub(super) interactive_process_registry: Option<Arc<InteractiveProcessRegistry>>,
    /// Optional per-project execution settings repository for project-aware admission checks.
    pub(super) execution_settings_repo: Option<Arc<dyn ExecutionSettingsRepository>>,
    /// Optional lane settings repository so fallback transition services can resolve Codex lanes.
    pub(super) agent_lane_settings_repo: Option<Arc<dyn AgentLaneSettingsRepository>>,
    /// Optional provider settings repository so fallback transition services honor onboarding.
    pub(super) agent_provider_settings_repo: Option<Arc<dyn AgentProviderSettingsRepository>>,
    /// Optional provider-neutral client bundle for fallback transition construction.
    pub(super) agent_clients: Option<AgentClientBundle>,
    /// Optional PR poller registry so scheduled plan merges can start GitHub polling.
    pub(super) pr_poller_registry: Option<Arc<PrPollerRegistry>>,
    /// Optional GitHub service so scheduled plan merges can take the PR-mode path.
    pub(super) github_service: Option<Arc<dyn GithubServiceTrait>>,
    /// Optional PR description drafter required before PR body writes.
    pub(super) plan_pr_description_drafter: Option<Arc<dyn PlanPrDescriptionDrafter>>,
    /// Self-reference for propagating scheduler through build_transition_service().
    /// Set after Arc-wrapping via set_self_ref(). Uses Mutex since it's written once at init.
    pub(super) self_ref: Mutex<Option<Arc<dyn TaskScheduler>>>,
    /// Phase 82: Optional project ID to scope scheduling to a single project.
    /// When set, only Ready tasks from this project are considered.
    pub(super) active_project_id: RwLock<Option<ProjectId>>,
    /// Optional execution plan ID to scope scheduling to one implementation attempt.
    pub(super) active_execution_plan_id: RwLock<Option<ExecutionPlanId>>,
    /// Guard to prevent concurrent scheduling from causing duplicate transitions.
    /// Multiple triggers can fire try_schedule_ready_tasks() simultaneously
    /// (e.g., on_enter(Ready) delayed tokio::spawn + on_exit(agent_state) direct call),
    /// leading to TOCTOU races where two invocations both find the same Ready task
    /// and both transition it to Executing, causing duplicate on_enter(Executing).
    pub(super) scheduling_lock: TokioMutex<()>,
    /// Number of pending contention-retry spawns currently in flight.
    /// Wrapped in Arc so spawned retry closures can decrement it without downcasting.
    /// Bounded by the default scheduler runtime config's `max_contention_retries`.
    pub(super) contention_retry_pending: Arc<AtomicU32>,
}

impl TaskSchedulerService {
    /// Create a new TaskSchedulerService with all required dependencies.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        execution_state: Arc<ExecutionState>,
        project_repo: Arc<dyn ProjectRepository>,
        task_repo: Arc<dyn TaskRepository>,
        task_dependency_repo: Arc<dyn TaskDependencyRepository>,
        artifact_repo: Arc<dyn ArtifactRepository>,
        chat_message_repo: Arc<dyn ChatMessageRepository>,
        chat_attachment_repo: Arc<dyn ChatAttachmentRepository>,
        conversation_repo: Arc<dyn ChatConversationRepository>,
        agent_run_repo: Arc<dyn AgentRunRepository>,
        ideation_session_repo: Arc<dyn IdeationSessionRepository>,
        activity_event_repo: Arc<dyn ActivityEventRepository>,
        message_queue: Arc<MessageQueue>,
        running_agent_registry: Arc<dyn RunningAgentRegistry>,
        memory_event_repo: Arc<dyn MemoryEventRepository>,
        app_handle: Option<AppHandle>,
    ) -> Self {
        Self {
            execution_state,
            project_repo,
            task_repo,
            task_step_repo: None,
            validation_run_repo: None,
            external_events_repo: None,
            webhook_publisher: None,
            task_dependency_repo,
            artifact_repo,
            chat_message_repo,
            chat_attachment_repo,
            conversation_repo,
            agent_run_repo,
            ideation_session_repo,
            activity_event_repo,
            message_queue,
            running_agent_registry,
            memory_event_repo,
            app_handle,
            plan_branch_repo: None,
            execution_plan_repo: None,
            interactive_process_registry: None,
            execution_settings_repo: None,
            agent_lane_settings_repo: None,
            agent_provider_settings_repo: None,
            agent_clients: None,
            pr_poller_registry: None,
            github_service: None,
            plan_pr_description_drafter: None,
            self_ref: Mutex::new(None),
            active_project_id: RwLock::new(None),
            active_execution_plan_id: RwLock::new(None),
            scheduling_lock: TokioMutex::new(()),
            contention_retry_pending: Arc::new(AtomicU32::new(0)),
        }
    }

    /// Set the plan branch repository for feature branch resolution (builder pattern).
    pub fn with_plan_branch_repo(mut self, repo: Arc<dyn PlanBranchRepository>) -> Self {
        self.plan_branch_repo = Some(repo);
        self
    }

    pub fn with_execution_plan_repo(mut self, repo: Arc<dyn ExecutionPlanRepository>) -> Self {
        self.execution_plan_repo = Some(repo);
        self
    }

    pub fn with_completion_authority_repositories(
        mut self,
        task_step_repo: Option<Arc<dyn TaskStepRepository>>,
        validation_run_repo: Option<Arc<dyn ValidationRunRepository>>,
    ) -> Self {
        self.task_step_repo = task_step_repo;
        self.validation_run_repo = validation_run_repo;
        self
    }

    pub fn with_completion_event_delivery(
        mut self,
        external_events_repo: Option<Arc<dyn ExternalEventsRepository>>,
        webhook_publisher: Option<Arc<dyn WebhookPublisher>>,
    ) -> Self {
        self.external_events_repo = external_events_repo;
        self.webhook_publisher = webhook_publisher;
        self
    }

    /// Set the shared InteractiveProcessRegistry (builder pattern).
    pub fn with_interactive_process_registry(
        mut self,
        ipr: Arc<InteractiveProcessRegistry>,
    ) -> Self {
        self.interactive_process_registry = Some(ipr);
        self
    }

    pub fn with_execution_settings_repo(
        mut self,
        repo: Arc<dyn ExecutionSettingsRepository>,
    ) -> Self {
        self.execution_settings_repo = Some(repo);
        self
    }

    pub fn with_agent_lane_settings_repo(
        mut self,
        repo: Arc<dyn AgentLaneSettingsRepository>,
    ) -> Self {
        self.agent_lane_settings_repo = Some(repo);
        self
    }

    pub fn with_agent_provider_settings_repo(
        mut self,
        repo: Arc<dyn AgentProviderSettingsRepository>,
    ) -> Self {
        self.agent_provider_settings_repo = Some(repo);
        self
    }

    pub fn with_agent_clients(mut self, agent_clients: AgentClientBundle) -> Self {
        self.agent_clients = Some(agent_clients);
        self
    }

    pub fn with_pr_poller_registry(mut self, registry: Arc<PrPollerRegistry>) -> Self {
        self.pr_poller_registry = Some(registry);
        self
    }

    pub fn with_github_service(mut self, github: Arc<dyn GithubServiceTrait>) -> Self {
        self.github_service = Some(github);
        self
    }

    pub fn with_plan_pr_description_drafter(
        mut self,
        drafter: Arc<dyn PlanPrDescriptionDrafter>,
    ) -> Self {
        self.plan_pr_description_drafter = Some(drafter);
        self
    }

    /// Set the self-reference after wrapping in Arc.
    /// This allows build_transition_service() to propagate the scheduler.
    /// Must be called after Arc::new(scheduler) at each construction site.
    pub fn set_self_ref(&self, scheduler: Arc<dyn TaskScheduler>) {
        *self.self_ref.lock().unwrap() = Some(scheduler);
    }

    /// Set the active project ID for scoped scheduling (Phase 82).
    /// When set, only Ready tasks from this project will be scheduled.
    /// Set to None to schedule across all projects.
    pub async fn set_active_project(&self, project_id: Option<ProjectId>) {
        *self.active_project_id.write().await = project_id;
    }

    /// Get the current active project ID, if any.
    pub async fn get_active_project(&self) -> Option<ProjectId> {
        self.active_project_id.read().await.clone()
    }

    /// Set the active execution plan ID for scoped scheduling.
    /// When set, only Ready tasks from this execution plan will be scheduled.
    pub async fn set_active_execution_plan(&self, execution_plan_id: Option<ExecutionPlanId>) {
        *self.active_execution_plan_id.write().await = execution_plan_id;
    }

    /// Find the oldest schedulable task across all projects (or scoped to active project).
    ///
    /// Phase 82: When active_project_id is set, only tasks from that project are considered.
    /// For Worktree-mode projects, any Ready task is schedulable.
    /// For Local-mode projects, a task is only schedulable if no other task
    /// in the same project is in a "running" state (Executing, ReExecuting,
    /// Reviewing, or Merging).
    ///
    /// Returns None if no schedulable tasks exist or if there's an error querying.
    async fn find_oldest_schedulable_task(&self) -> Option<Task> {
        // Phase 82: Get active project filter
        let active_project = self.active_project_id.read().await.clone();
        let active_execution_plan = self.active_execution_plan_id.read().await.clone();

        // Get a batch of oldest Ready tasks to evaluate
        let ready_tasks = match self.task_repo.get_oldest_ready_tasks(50).await {
            Ok(tasks) => tasks,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to get Ready tasks for scheduling");
                return None;
            }
        };

        for task in ready_tasks {
            if let Some(ref active_plan_id) = active_execution_plan {
                if task.execution_plan_id.as_ref() != Some(active_plan_id) {
                    tracing::debug!(
                        task_id = task.id.as_str(),
                        task_execution_plan = ?task.execution_plan_id.as_ref().map(|id| id.as_str()),
                        active_execution_plan = active_plan_id.as_str(),
                        "Skipping task: not in active execution plan"
                    );
                    continue;
                }
            }

            // Phase 82: If active project is set, skip tasks from other projects
            if let Some(ref active_pid) = active_project {
                if task.project_id != *active_pid {
                    tracing::debug!(
                        task_id = task.id.as_str(),
                        task_project = task.project_id.as_str(),
                        active_project = active_pid.as_str(),
                        "Skipping task: not in active project"
                    );
                    continue;
                }
            }

            // Verify the project exists
            match self.project_repo.get_by_id(&task.project_id).await {
                Ok(Some(_)) => {}
                Ok(None) => {
                    tracing::warn!(
                        task_id = task.id.as_str(),
                        project_id = task.project_id.as_str(),
                        "Task has non-existent project, skipping"
                    );
                    continue;
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        task_id = task.id.as_str(),
                        "Failed to get project for task, skipping"
                    );
                    continue;
                }
            }

            // Dependency gate: skip tasks whose blockers are not satisfied.
            // Re-block the task so it doesn't sit in Ready with unsatisfied deps.
            if self.has_unsatisfied_dependencies(&task).await {
                self.reblock_task(&task).await;
                continue;
            }

            if self.is_execution_plan_inactive(&task).await {
                tracing::info!(
                    task_id = task.id.as_str(),
                    execution_plan_id = ?task.execution_plan_id.as_ref().map(|id| id.as_str()),
                    "Skipping task: execution plan is no longer active"
                );
                continue;
            }

            if self.is_execution_plan_halted(&task).await {
                tracing::info!(
                    task_id = task.id.as_str(),
                    execution_plan_id = ?task.execution_plan_id.as_ref().map(|id| id.as_str()),
                    "Skipping task: execution plan is paused or stopped"
                );
                continue;
            }

            // Plan branch guard: skip tasks whose plan branch is no longer Active.
            // Tasks on Merged or Abandoned branches should not be scheduled.
            if self.is_plan_branch_inactive(&task).await {
                tracing::info!(
                    task_id = task.id.as_str(),
                    "Skipping task: plan branch is no longer active (merged or abandoned)"
                );
                continue;
            }

            // Freshness backoff guard: skip tasks that are within a freshness conflict
            // backoff window. This prevents tasks from consuming execution slots while
            // waiting for branch conflicts to resolve.
            if let Some(ref metadata_str) = task.metadata {
                if let Ok(freshness) = serde_json::from_str::<FreshnessMetadata>(metadata_str) {
                    if freshness.is_in_backoff() {
                        tracing::debug!(
                            task_id = task.id.as_str(),
                            backoff_until = ?freshness.freshness_backoff_until,
                            "Skipping task: in freshness backoff window"
                        );
                        continue;
                    }
                }
            }

            if !self.project_has_execution_capacity(&task.project_id).await {
                tracing::debug!(
                    task_id = task.id.as_str(),
                    project_id = task.project_id.as_str(),
                    "Skipping task: project execution capacity reached"
                );
                continue;
            }

            // This task is schedulable
            return Some(task);
        }

        None
    }

    #[doc(hidden)]
    pub async fn find_oldest_schedulable_task_for_test(&self) -> Option<Task> {
        self.find_oldest_schedulable_task().await
    }

    #[doc(hidden)]
    pub async fn lock_scheduling_for_test(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.scheduling_lock.lock().await
    }

    #[doc(hidden)]
    pub fn contention_retry_pending_for_test(&self) -> u32 {
        self.contention_retry_pending.load(Ordering::Relaxed)
    }

    #[doc(hidden)]
    pub fn set_contention_retry_pending_for_test(&self, value: u32) {
        self.contention_retry_pending
            .store(value, Ordering::Relaxed);
    }

    /// Build a TaskTransitionService for transitioning tasks.
    ///
    /// Creates a fresh instance to avoid circular dependency issues when
    /// the scheduler is called from within TransitionHandler.
    pub(super) fn build_transition_service(&self) -> Arc<TaskTransitionService> {
        let deps = RuntimeFactoryDeps::from_core(
            Arc::clone(&self.task_repo),
            Arc::clone(&self.task_dependency_repo),
            Arc::clone(&self.project_repo),
            Arc::clone(&self.artifact_repo),
            Arc::clone(&self.chat_message_repo),
            Arc::clone(&self.chat_attachment_repo),
            Arc::clone(&self.conversation_repo),
            Arc::clone(&self.agent_run_repo),
            Arc::clone(&self.ideation_session_repo),
            Arc::clone(&self.activity_event_repo),
            Arc::clone(&self.message_queue),
            Arc::clone(&self.running_agent_registry),
            Arc::clone(&self.memory_event_repo),
        )
        .with_agent_clients(self.agent_clients.clone())
        .with_completion_authority_repositories(
            self.task_step_repo.as_ref().map(Arc::clone),
            self.validation_run_repo.as_ref().map(Arc::clone),
        )
        .with_completion_event_delivery(
            self.external_events_repo.as_ref().map(Arc::clone),
            self.webhook_publisher.as_ref().map(Arc::clone),
        )
        .with_runtime_support(
            self.execution_settings_repo.as_ref().map(Arc::clone),
            self.agent_lane_settings_repo.as_ref().map(Arc::clone),
            self.agent_provider_settings_repo.as_ref().map(Arc::clone),
            self.plan_branch_repo.as_ref().map(Arc::clone),
            self.interactive_process_registry.as_ref().map(Arc::clone),
        )
        .with_github_runtime_support(
            self.github_service.as_ref().map(Arc::clone),
            self.pr_poller_registry.as_ref().map(Arc::clone),
        );
        let deps = if let Some(drafter) = self.plan_pr_description_drafter.as_ref() {
            deps.with_plan_pr_description_drafter(Arc::clone(drafter))
        } else {
            deps
        };
        let mut service = build_transition_service_with_fallback(
            &self.app_handle,
            Arc::clone(&self.execution_state),
            &deps,
        );
        if let Some(ref sched) = *self.self_ref.lock().unwrap() {
            service = service.with_task_scheduler(Arc::clone(sched));
        }
        service.into_arc()
    }
}

#[async_trait]
impl TaskScheduler for TaskSchedulerService {
    /// Try to schedule Ready tasks if execution slots are available.
    ///
    /// This method loops to fill all available execution slots:
    /// 1. Checks if execution is paused or at capacity
    /// 2. Finds the oldest Ready task across all projects
    /// 3. Transitions it to Executing state via the state machine
    /// 4. Repeats until no more slots or no more schedulable tasks
    async fn try_schedule_ready_tasks(&self) {
        // Prevent concurrent scheduling to avoid TOCTOU race where two invocations
        // both find the same Ready task and both transition it to Executing.
        // Use try_lock: if another scheduling is already in progress, queue a 200ms retry
        // so the caller's scheduling intent is not silently lost (S6 fix).
        let _guard = match self.scheduling_lock.try_lock() {
            Ok(guard) => guard,
            Err(_) => {
                // Limit concurrent retry spawns to avoid cascading if lock is persistently held.
                let sched_cfg = default_scheduler_runtime_config();
                let max_retries = sched_cfg.max_contention_retries as u32;
                let retry_delay_ms = sched_cfg.contention_retry_delay_ms;
                let pending = self.contention_retry_pending.load(Ordering::Relaxed);
                if pending >= max_retries {
                    tracing::debug!(
                        pending_retries = pending,
                        "Scheduling already in progress; retry limit reached, dropping attempt"
                    );
                    return;
                }
                if let Some(scheduler) = self.self_ref.lock().unwrap().clone() {
                    self.contention_retry_pending
                        .fetch_add(1, Ordering::Relaxed);
                    tracing::debug!(
                        pending_retries = pending + 1,
                        "Scheduling lock contention detected; queuing retry in {retry_delay_ms}ms"
                    );
                    let retry_counter = Arc::clone(&self.contention_retry_pending);
                    tokio::spawn(async move {
                        tokio::time::sleep(tokio::time::Duration::from_millis(retry_delay_ms))
                            .await;
                        // Decrement before the retry attempt so the slot is freed
                        // regardless of whether the retry succeeds or skips.
                        retry_counter.fetch_sub(1, Ordering::Relaxed);
                        scheduler.try_schedule_ready_tasks().await;
                    });
                } else {
                    tracing::debug!(
                        "Scheduling already in progress, skipping concurrent attempt (no self_ref)"
                    );
                }
                return;
            }
        };

        loop {
            // Check capacity on each iteration
            if !self.execution_state.can_start_any_execution_context() {
                tracing::debug!(
                    is_paused = self.execution_state.is_paused(),
                    running_count = self.execution_state.running_count(),
                    global_max_concurrent = self.execution_state.global_max_concurrent(),
                    "Cannot schedule more: at global capacity or paused"
                );
                break;
            }

            // Find next schedulable task (accounting for Local-mode constraints)
            let Some(task) = self.find_oldest_schedulable_task().await else {
                if let Some(task) = self.find_oldest_retryable_pending_review_task().await {
                    self.retry_pending_review_task(&task).await;
                    continue;
                }
                tracing::debug!("No more schedulable tasks");
                break;
            };

            tracing::info!(
                task_id = task.id.as_str(),
                task_title = task.title.as_str(),
                created_at = %task.created_at,
                "Scheduling Ready task for execution"
            );

            // Determine target status: plan_merge tasks skip execution and go directly to merge
            let target_status = if task.category == TaskCategory::PlanMerge {
                tracing::info!(
                    task_id = task.id.as_str(),
                    "Plan merge task: routing to PendingMerge (skip execution)"
                );
                InternalStatus::PendingMerge
            } else {
                InternalStatus::Executing
            };

            // Set trigger_origin to "scheduler" if not already set (preserves retry/recovery origins)
            if get_trigger_origin(&task).is_none() {
                let mut task_mut = task.clone();
                set_trigger_origin(&mut task_mut, "scheduler");
                if let Err(e) = self.task_repo.update(&task_mut).await {
                    tracing::error!(
                        task_id = task.id.as_str(),
                        error = %e,
                        "Failed to set trigger_origin=scheduler in metadata"
                    );
                }
            }

            // Per-task concurrency guard: prevent two concurrent scheduler invocations
            // from both scheduling the same task (TOCTOU race on async on_enter_Executing).
            if !self.execution_state.try_start_scheduling(task.id.as_str()) {
                tracing::debug!(
                    task_id = task.id.as_str(),
                    "Scheduler: task already being scheduled by another caller, skipping"
                );
                continue;
            }

            // Transition the task to the target status
            // For Executing: triggers on_enter(Executing) which spawns worker agent
            // For PendingMerge: triggers on_enter(PendingMerge) which runs attempt_programmatic_merge()
            let transition_service = self.build_transition_service();

            let transition_result = transition_service
                .transition_task(&task.id, target_status)
                .await;

            // Always release the per-task guard after transition completes
            self.execution_state.finish_scheduling(task.id.as_str());

            if let Err(e) = transition_result {
                tracing::error!(
                    task_id = task.id.as_str(),
                    error = %e,
                    target = ?target_status,
                    "Failed to transition Ready task"
                );
                // Stop on error to avoid infinite loop on persistent failures
                break;
            }

            // Continue loop - try to fill next slot
        }
    }

    /// Re-trigger deferred merges for a project after a competing merge completes.
    async fn try_retry_deferred_merges(&self, project_id: &str) {
        self.retry_deferred_merges_impl(project_id).await;
    }

    /// Retry main-branch merges that were deferred because agents were running.
    async fn try_retry_main_merges(&self) {
        self.retry_main_merges_impl().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::AppState;
    use crate::domain::entities::{AgentWorkspacePrDescription, PlanBranch, Project};
    use crate::domain::services::PrReviewState;
    use crate::error::AppResult;

    struct StaticPlanPrDescriptionDrafter;

    #[async_trait]
    impl PlanPrDescriptionDrafter for StaticPlanPrDescriptionDrafter {
        async fn draft_plan_description(
            &self,
            _project: &Project,
            _plan_branch: &PlanBranch,
            _review_base: &str,
            _review_state: PrReviewState,
        ) -> AppResult<AgentWorkspacePrDescription> {
            Ok(AgentWorkspacePrDescription::new(
                None,
                "## Summary\n\nScheduler test PR description".to_string(),
            ))
        }
    }

    fn scheduler_for_state(state: &AppState) -> TaskSchedulerService {
        TaskSchedulerService::new(
            Arc::new(ExecutionState::new()),
            Arc::clone(&state.project_repo),
            Arc::clone(&state.task_repo),
            Arc::clone(&state.task_dependency_repo),
            Arc::clone(&state.artifact_repo),
            Arc::clone(&state.chat_message_repo),
            Arc::clone(&state.chat_attachment_repo),
            Arc::clone(&state.chat_conversation_repo),
            Arc::clone(&state.agent_run_repo),
            Arc::clone(&state.ideation_session_repo),
            Arc::clone(&state.activity_event_repo),
            Arc::clone(&state.message_queue),
            Arc::clone(&state.running_agent_registry),
            Arc::clone(&state.memory_event_repo),
            None,
        )
    }

    #[test]
    fn scheduler_plan_pr_description_drafter_defaults_to_none() {
        let state = AppState::new_test();
        let scheduler = scheduler_for_state(&state);

        assert!(scheduler.plan_pr_description_drafter.is_none());
    }

    #[test]
    fn scheduler_build_transition_service_carries_plan_pr_description_drafter() {
        let state = AppState::new_test();
        let scheduler = scheduler_for_state(&state)
            .with_plan_pr_description_drafter(Arc::new(StaticPlanPrDescriptionDrafter));

        assert!(scheduler.plan_pr_description_drafter.is_some());
        let _service = scheduler.build_transition_service();
    }
}
