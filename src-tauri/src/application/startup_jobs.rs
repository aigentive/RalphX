// Startup Job Runner
//
// Handles automatic task resumption when the app restarts.
// Tasks that were in agent-active states (Executing, QaRefining, QaTesting, Reviewing, ReExecuting)
// when the app shut down are automatically resumed on startup, respecting pause state and
// max_concurrent limits.
//
// Also cleans up orphaned agent runs that were left in "running" status from previous sessions.
//
// Usage:
// - Called once during app initialization after HTTP server is ready
// - Cleans up orphaned agent runs from previous sessions
// - Iterates all projects to find tasks in agent-active states
// - Re-executes entry actions to respawn agents
// - Stops early if max_concurrent is reached

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tracing::{debug, info};

use crate::application::chat_service::uses_execution_slot;
use crate::application::chat_service::{ChatService, SendCallerContext, SendMessageOptions};
use crate::application::git_service::GitService;
use crate::application::recovery_queue::{RecoveryItem, RecoveryPriority, RecoveryQueue};
use crate::application::ReconciliationRunner;
use crate::commands::execution_commands::{
    context_matches_running_status_for_gc, ActiveProjectState, ExecutionState,
    AGENT_ACTIVE_STATUSES, AUTO_TRANSITION_STATES,
};
use crate::domain::entities::ideation::{IdeationSessionFlow, IdeationSessionStatus};
use crate::domain::entities::{
    app_state::ExecutionHaltMode, AgentConversationWorkspaceMode, AgentRunStatus, ChatContextType,
    ChatConversationId, IdeationSessionId, InternalStatus, ProjectId, ReviewNote, ReviewOutcome,
    ReviewerType,
};
use crate::domain::repositories::{
    AgentRunRepository, AppStateRepository, ChatConversationRepository,
    ExecutionSettingsRepository, IdeationSessionRepository, NotificationRepository,
    ProjectRepository, ReviewRepository, TaskDependencyRepository, TaskRepository,
    ORPHANED_AGENT_RUN_ON_APP_RESTART,
};
use crate::domain::services::{QueueKey, QueuedMessage, RunningAgentKey};
use crate::domain::state_machine::services::TaskScheduler;
use crate::error::AppResult;

use super::TaskTransitionService;
use crate::infrastructure::agents::claude::{stream_timeouts, StreamTimeoutsConfig};
use crate::application::data_retention_service::DataRetentionService;
use crate::infrastructure::sqlite::DbConnection;

/// Environment variable that disables startup recovery mechanisms when present.
pub const RALPHX_DISABLE_STARTUP_RECOVERY_ENV: &str = "RALPHX_DISABLE_STARTUP_RECOVERY";

fn notification_retention_prune_args(
    config: &StreamTimeoutsConfig,
    now: chrono::DateTime<chrono::Utc>,
) -> (chrono::DateTime<chrono::Utc>, u32) {
    (
        now - chrono::Duration::days(config.notification_retention_read_days as i64),
        u32::try_from(config.notification_retention_max_rows).unwrap_or(u32::MAX),
    )
}

#[doc(hidden)]
pub fn is_startup_recovery_disabled_var(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some()
}

/// Returns true when startup recovery should be skipped for this process.
/// Always returns false in test builds to avoid env-var leakage from outer processes.
pub fn is_startup_recovery_disabled() -> bool {
    #[cfg(test)]
    {
        false
    }
    #[cfg(not(test))]
    {
        is_startup_recovery_disabled_var(
            std::env::var_os(RALPHX_DISABLE_STARTUP_RECOVERY_ENV).as_deref(),
        )
    }
}

fn startup_job_step_started(step: &'static str) -> Instant {
    tracing::info!(step, "Startup job runner step starting");
    Instant::now()
}

fn startup_job_step_completed(step: &'static str, started_at: Instant) {
    tracing::info!(
        step,
        elapsed_ms = started_at.elapsed().as_millis(),
        "Startup job runner step completed"
    );
}

const ACCEPTED_PLAN_MODE_HANDOFF_SOURCE: &str = "accepted_plan_mode_proposal";

fn is_accepted_plan_mode_handoff_row(key: &QueueKey, message: &QueuedMessage) -> bool {
    if key.context_type != ChatContextType::Project {
        return false;
    }
    let Some(metadata) = message.metadata_override.as_deref() else {
        return false;
    };
    let Ok(metadata) = serde_json::from_str::<serde_json::Value>(metadata) else {
        return false;
    };
    let Some(request_id) = metadata
        .get("source_request_id")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return false;
    };

    message.id == format!("plan-mode-handoff:{request_id}")
        && metadata.get("source").and_then(|value| value.as_str())
            == Some(ACCEPTED_PLAN_MODE_HANDOFF_SOURCE)
        && metadata
            .get("required_workspace_mode")
            .and_then(|value| value.as_str())
            == Some("plan")
        && metadata
            .get("resume_in_place")
            .and_then(|value| value.as_bool())
            == Some(true)
        && metadata
            .get("persist_hidden_marker")
            .and_then(|value| value.as_bool())
            == Some(true)
}

/// Returns true if a task's metadata indicates it should be auto-recovered on startup.
///
/// Criteria: (`shutdown_interrupted == true` OR `last_agent_error` contains
/// "completed without calling") AND `startup_recovery_attempts < 1`.
fn should_auto_recover(meta: &serde_json::Value) -> bool {
    let shutdown_interrupted = meta
        .get("shutdown_interrupted")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let crash_indicator = meta
        .get("last_agent_error")
        .and_then(|v| v.as_str())
        .map(|e| e.contains("completed without calling"))
        .unwrap_or(false);

    let recovery_attempts = meta
        .get("startup_recovery_attempts")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    (shutdown_interrupted || crash_indicator) && recovery_attempts < 1
}

fn startup_resume_context_for_status(status: InternalStatus) -> Option<&'static str> {
    match status {
        InternalStatus::Executing
        | InternalStatus::ReExecuting
        | InternalStatus::QaRefining
        | InternalStatus::QaTesting => Some("execution"),
        InternalStatus::Reviewing => Some("review"),
        InternalStatus::PendingMerge | InternalStatus::Merging => Some("merge"),
        _ => None,
    }
}

fn startup_resume_chat_context_for_status(status: InternalStatus) -> Option<ChatContextType> {
    match status {
        InternalStatus::Executing
        | InternalStatus::ReExecuting
        | InternalStatus::QaRefining
        | InternalStatus::QaTesting => Some(ChatContextType::TaskExecution),
        InternalStatus::Reviewing => Some(ChatContextType::Review),
        InternalStatus::PendingMerge | InternalStatus::Merging => Some(ChatContextType::Merge),
        _ => None,
    }
}

/// Runs startup jobs, primarily task resumption.
///
/// Finds all tasks that were in agent-active states when the app shut down
/// and re-triggers their entry actions to respawn worker agents.
/// Also cleans up orphaned agent runs from previous sessions.
/// Phase 82: Supports optional project scoping via `active_project_state`.
/// When active project is set, only tasks from that project will be resumed.
/// When no active project is set, resumption is skipped entirely.
pub struct StartupJobRunner {
    task_repo: Arc<dyn TaskRepository>,
    task_dep_repo: Arc<dyn TaskDependencyRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    chat_conversation_repo: Arc<dyn ChatConversationRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    transition_service: Arc<TaskTransitionService>,
    execution_state: Arc<ExecutionState>,
    /// Phase 82: Active project state for per-project scoping
    active_project_state: Arc<ActiveProjectState>,
    /// Phase 90: App state repository for reading persisted active_project_id from DB
    app_state_repo: Arc<dyn AppStateRepository>,
    /// Execution settings repository for loading per-project max_concurrent quota
    execution_settings_repo: Arc<dyn ExecutionSettingsRepository>,
    /// Phase 105: Persisted agent registry for killing orphaned OS processes on restart
    running_agent_registry: Arc<dyn crate::domain::services::RunningAgentRegistry>,
    /// App boot cutoff used to distinguish previous-session rows from agents spawned
    /// by this process while earlier startup recovery phases are still running.
    previous_session_cutoff: chrono::DateTime<chrono::Utc>,
    /// Plan branch repository for resolving plan branch during deferred cleanup
    plan_branch_repo: Option<Arc<dyn crate::domain::repositories::PlanBranchRepository>>,
    reconciler: ReconciliationRunner,
    /// Optional task scheduler for auto-starting Ready tasks on startup.
    /// When provided, Ready tasks will be scheduled after resuming agent-active tasks.
    task_scheduler: Option<Arc<dyn TaskScheduler>>,
    /// Optional app handle for event emission
    app_handle: Option<AppHandle>,
    /// Optional review repository for adding audit-trail ReviewNotes on crash recovery
    review_repo: Option<Arc<dyn ReviewRepository>>,
    /// Optional chat service for Phase N+1 ideation recovery.
    /// When provided, orphaned ideation sessions are re-spawned via send_message().
    chat_service: Option<Arc<dyn ChatService>>,
    /// Ideation session repository for validating sessions before recovery.
    ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    /// Projects whose startup Git/GitHub preflight failed. Startup recovery for
    /// these projects is deferred so recovery does not immediately hit known-bad auth.
    git_startup_blocked_project_ids: Arc<HashSet<ProjectId>>,
    notification_repo: Option<Arc<dyn NotificationRepository>>,
    data_retention_service: Option<Arc<DataRetentionService>>,
    delegation_park_service:
        Option<Arc<crate::application::delegation_park::DelegationParkService>>,
}

struct StartupSafetyNet {
    task_repo: Arc<dyn TaskRepository>,
    task_dep_repo: Arc<dyn TaskDependencyRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    app_handle: Option<AppHandle>,
}

impl StartupSafetyNet {
    async fn run(self) {
        let step_started_at = startup_job_step_started("ready_task_unblock");
        StartupJobRunner::unblock_ready_tasks_for(
            Arc::clone(&self.task_repo),
            Arc::clone(&self.task_dep_repo),
            Arc::clone(&self.project_repo),
            self.app_handle.clone(),
        )
        .await;
        startup_job_step_completed("ready_task_unblock", step_started_at);

        let step_started_at = startup_job_step_started("dependency_violation_reconcile");
        StartupJobRunner::reconcile_dependency_violations_for(
            self.task_repo,
            self.task_dep_repo,
            self.project_repo,
            self.app_handle,
        )
        .await;
        startup_job_step_completed("dependency_violation_reconcile", step_started_at);
    }
}

impl StartupJobRunner {
    async fn persist_startup_status_change(
        &self,
        task: &crate::domain::entities::Task,
        from_status: InternalStatus,
        trigger: &str,
    ) -> AppResult<bool> {
        Self::persist_startup_status_change_with_repo(&self.task_repo, task, from_status, trigger)
            .await
    }

    async fn persist_startup_status_change_with_repo(
        task_repo: &Arc<dyn TaskRepository>,
        task: &crate::domain::entities::Task,
        from_status: InternalStatus,
        trigger: &str,
    ) -> AppResult<bool> {
        let updated = task_repo
            .update_with_expected_status(task, from_status)
            .await?;

        if updated {
            task_repo
                .persist_status_change(&task.id, from_status, task.internal_status, trigger)
                .await?;
        }

        Ok(updated)
    }

    /// Create a new StartupJobRunner with all required dependencies.
    /// Phase 82: Now requires active_project_state for per-project scoping.
    /// Phase 90: Now requires app_state_repo for reading persisted active project from DB.
    pub fn new(
        task_repo: Arc<dyn TaskRepository>,
        task_dep_repo: Arc<dyn TaskDependencyRepository>,
        project_repo: Arc<dyn ProjectRepository>,
        artifact_repo: Arc<dyn crate::domain::repositories::ArtifactRepository>,
        chat_conversation_repo: Arc<dyn ChatConversationRepository>,
        chat_message_repo: Arc<dyn crate::domain::repositories::ChatMessageRepository>,
        chat_attachment_repo: Arc<dyn crate::domain::repositories::ChatAttachmentRepository>,
        ideation_session_repo: Arc<dyn crate::domain::repositories::IdeationSessionRepository>,
        activity_event_repo: Arc<dyn crate::domain::repositories::ActivityEventRepository>,
        message_queue: Arc<crate::domain::services::MessageQueue>,
        running_agent_registry: Arc<dyn crate::domain::services::RunningAgentRegistry>,
        memory_event_repo: Arc<dyn crate::domain::repositories::MemoryEventRepository>,
        agent_run_repo: Arc<dyn AgentRunRepository>,
        transition_service: Arc<TaskTransitionService>,
        execution_state: Arc<ExecutionState>,
        active_project_state: Arc<ActiveProjectState>,
        app_state_repo: Arc<dyn AppStateRepository>,
        execution_settings_repo: Arc<dyn ExecutionSettingsRepository>,
        plan_branch_repo: Option<Arc<dyn crate::domain::repositories::PlanBranchRepository>>,
    ) -> Self {
        let mut reconciler = ReconciliationRunner::new(
            Arc::clone(&task_repo),
            Arc::clone(&task_dep_repo),
            Arc::clone(&project_repo),
            Arc::clone(&artifact_repo),
            Arc::clone(&chat_conversation_repo),
            Arc::clone(&chat_message_repo),
            Arc::clone(&chat_attachment_repo),
            Arc::clone(&ideation_session_repo),
            Arc::clone(&activity_event_repo),
            Arc::clone(&message_queue),
            Arc::clone(&running_agent_registry),
            Arc::clone(&memory_event_repo),
            Arc::clone(&agent_run_repo),
            Arc::clone(&transition_service),
            Arc::clone(&execution_state),
            None,
        )
        .with_execution_settings_repo(Arc::clone(&execution_settings_repo));
        if let Some(repo) = plan_branch_repo.as_ref() {
            reconciler = reconciler.with_plan_branch_repo(Arc::clone(repo));
        }

        Self {
            task_repo,
            task_dep_repo,
            project_repo,
            chat_conversation_repo,
            agent_run_repo,
            transition_service,
            execution_state,
            active_project_state,
            app_state_repo,
            execution_settings_repo,
            running_agent_registry,
            previous_session_cutoff: chrono::Utc::now(),
            plan_branch_repo,
            reconciler,
            task_scheduler: None,
            app_handle: None,
            review_repo: None,
            chat_service: None,
            ideation_session_repo,
            git_startup_blocked_project_ids: Arc::new(HashSet::new()),
            notification_repo: None,
            data_retention_service: None,
            delegation_park_service: None,
        }
    }

    async fn count_active_slot_consuming_contexts_for_project(
        &self,
        project_id: &ProjectId,
    ) -> Option<u32> {
        let registry_entries = self.running_agent_registry.list_all().await;
        let mut count = 0u32;

        for (key, info) in registry_entries {
            if info.pid == 0 {
                continue;
            }

            if key.context_type == "ideation" || key.context_type == "session" {
                let session_id = IdeationSessionId::from_string(key.context_id.clone());
                let session = match self.ideation_session_repo.get_by_id(&session_id).await {
                    Ok(Some(session)) => session,
                    Ok(None) => continue,
                    Err(error) => {
                        tracing::warn!(
                            project_id = project_id.as_str(),
                            error = %error,
                            "Failed to load ideation session while checking startup project capacity"
                        );
                        return None;
                    }
                };

                if session.project_id != *project_id {
                    continue;
                }

                count += 1;
                continue;
            }

            let context_type = match key.context_type.parse::<ChatContextType>() {
                Ok(value) => value,
                Err(_) => continue,
            };

            if !uses_execution_slot(context_type) {
                continue;
            }

            let task_id = crate::domain::entities::TaskId::from_string(key.context_id.clone());
            let task = match self.task_repo.get_by_id(&task_id).await {
                Ok(Some(task)) => task,
                Ok(None) => continue,
                Err(error) => {
                    tracing::warn!(
                        project_id = project_id.as_str(),
                        error = %error,
                        "Failed to load task while checking startup project capacity"
                    );
                    return None;
                }
            };

            if task.project_id != *project_id
                || !context_matches_running_status_for_gc(context_type, task.internal_status)
            {
                continue;
            }

            count += 1;
        }

        Some(count)
    }

    async fn project_has_execution_capacity(&self, project_id: &ProjectId) -> bool {
        let settings = match self
            .execution_settings_repo
            .get_settings(Some(project_id))
            .await
        {
            Ok(settings) => settings,
            Err(error) => {
                tracing::warn!(
                    project_id = project_id.as_str(),
                    error = %error,
                    "Failed to load execution settings while checking startup project capacity"
                );
                return true;
            }
        };

        let Some(running_project_total) = self
            .count_active_slot_consuming_contexts_for_project(project_id)
            .await
        else {
            return true;
        };

        self.execution_state
            .can_start_execution_context(running_project_total, settings.max_concurrent_tasks)
    }

    async fn prepare_active_task_startup_resume(
        &self,
        task: &crate::domain::entities::Task,
        status: InternalStatus,
        interrupted_contexts: &HashSet<RunningAgentKey>,
    ) -> Option<crate::domain::entities::Task> {
        let expected_context = startup_resume_context_for_status(status)?;
        let chat_context = startup_resume_chat_context_for_status(status)?;
        let mut meta: serde_json::Value = task
            .metadata
            .as_deref()
            .and_then(|raw| serde_json::from_str(raw).ok())
            .unwrap_or_else(|| serde_json::json!({}));

        let recovery_attempts = meta
            .get("startup_recovery_attempts")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        if recovery_attempts >= 1 {
            return None;
        }

        let actual_context = meta
            .get("last_agent_error_context")
            .and_then(|value| value.as_str());
        let metadata_claim = should_auto_recover(&meta)
            && actual_context
                .map(|context| context == expected_context)
                .unwrap_or(true);

        let registry_claim = interrupted_contexts.contains(&RunningAgentKey::new(
            chat_context.to_string(),
            task.id.as_str(),
        ));

        let orphaned_run_claim = self
            .latest_run_was_orphaned_on_startup(task, chat_context)
            .await;

        let recovery_source = if metadata_claim {
            "shutdown_interrupted_metadata"
        } else if registry_claim {
            "persisted_running_agent"
        } else if orphaned_run_claim {
            "orphaned_agent_run"
        } else {
            return None;
        };

        if let Some(obj) = meta.as_object_mut() {
            obj.insert(
                "startup_recovery_attempts".to_string(),
                serde_json::json!(recovery_attempts + 1),
            );
            obj.insert(
                "startup_recovery_source".to_string(),
                serde_json::json!(recovery_source),
            );
            obj.remove("shutdown_interrupted");
            obj.remove("is_timeout");
            obj.remove("failure_error");
        }

        let mut updated_task = task.clone();
        updated_task.metadata = Some(meta.to_string());
        updated_task.blocked_reason = None;
        updated_task.touch();

        if let Err(error) = self.task_repo.update(&updated_task).await {
            tracing::warn!(
                task_id = task.id.as_str(),
                status = status.as_str(),
                error = %error,
                "Failed to persist startup auto-resume metadata for active task"
            );
            return None;
        }

        tracing::info!(
            task_id = task.id.as_str(),
            status = status.as_str(),
            context = expected_context,
            "Startup recovery: resuming shutdown-interrupted active task without reconciliation timeout"
        );

        Some(updated_task)
    }

    async fn latest_run_was_orphaned_on_startup(
        &self,
        task: &crate::domain::entities::Task,
        chat_context: ChatContextType,
    ) -> bool {
        let conversation = match self
            .chat_conversation_repo
            .get_active_for_context(chat_context, task.id.as_str())
            .await
        {
            Ok(Some(conversation)) => conversation,
            Ok(None) => return false,
            Err(error) => {
                tracing::warn!(
                    task_id = task.id.as_str(),
                    context_type = %chat_context,
                    error = %error,
                    "Failed to load startup resume conversation"
                );
                return false;
            }
        };

        match self
            .agent_run_repo
            .get_latest_for_conversation(&conversation.id)
            .await
        {
            Ok(Some(run)) => {
                run.status == AgentRunStatus::Cancelled
                    && run.error_message.as_deref() == Some(ORPHANED_AGENT_RUN_ON_APP_RESTART)
            }
            Ok(None) => false,
            Err(error) => {
                tracing::warn!(
                    task_id = task.id.as_str(),
                    conversation_id = conversation.id.as_str(),
                    error = %error,
                    "Failed to load startup resume agent run"
                );
                false
            }
        }
    }

    /// Set the task scheduler for auto-starting Ready tasks (builder pattern).
    ///
    /// When set, the runner will call try_schedule_ready_tasks() after resuming
    /// agent-active tasks, allowing queued Ready tasks to start execution.
    pub fn with_task_scheduler(mut self, scheduler: Arc<dyn TaskScheduler>) -> Self {
        self.task_scheduler = Some(scheduler);
        self
    }

    /// Set the app handle for event emission (builder pattern).
    pub fn with_app_handle(mut self, app_handle: AppHandle) -> Self {
        self.app_handle = Some(app_handle.clone());
        self.reconciler = self.reconciler.with_app_handle(app_handle);
        self
    }

    /// Set the review repository for crash recovery audit notes (builder pattern).
    pub fn with_review_repo(mut self, review_repo: Arc<dyn ReviewRepository>) -> Self {
        self.review_repo = Some(review_repo);
        self
    }

    /// Set the app boot cutoff for previous-session cleanup.
    pub fn with_previous_session_cutoff(mut self, cutoff: chrono::DateTime<chrono::Utc>) -> Self {
        self.previous_session_cutoff = cutoff;
        self
    }

    /// Set the chat service for Phase N+1 ideation recovery (builder pattern).
    ///
    /// When set, orphaned ideation sessions captured in Phase 0 are re-spawned
    /// via `ChatService::send_message()` after all other startup phases complete.
    pub fn with_chat_service(mut self, chat_service: Arc<dyn ChatService>) -> Self {
        self.chat_service = Some(chat_service);
        self
    }

    /// Set projects whose Git/GitHub startup work should be deferred until repaired.
    pub fn with_git_startup_blocked_projects(
        mut self,
        project_ids: Arc<HashSet<ProjectId>>,
    ) -> Self {
        self.git_startup_blocked_project_ids = project_ids;
        self
    }

    pub fn with_notification_repo(
        mut self,
        notification_repo: Arc<dyn NotificationRepository>,
    ) -> Self {
        self.notification_repo = Some(notification_repo);
        self
    }

    /// Sets the retention service used by the startup payload-prune job.
    pub fn with_data_retention_db(mut self, db: DbConnection) -> Self {
        self.data_retention_service = Some(Arc::new(DataRetentionService::from_db(db)));
        self
    }

    /// Provide the durable delegation-park reconciler so coordinators parked across a restart
    /// are woken from durable state rather than stranded.
    pub fn with_delegation_park_service(
        mut self,
        service: Arc<crate::application::delegation_park::DelegationParkService>,
    ) -> Self {
        self.delegation_park_service = Some(service);
        self
    }

    pub fn spawn_post_ready_safety_net(&self, delay: Duration) {
        let runner = self.clone_for_safety_net();
        tauri::async_runtime::spawn(async move {
            if delay > Duration::ZERO {
                tokio::time::sleep(delay).await;
            }

            runner.run().await;
        });
    }

    fn clone_for_safety_net(&self) -> StartupSafetyNet {
        StartupSafetyNet {
            task_repo: Arc::clone(&self.task_repo),
            task_dep_repo: Arc::clone(&self.task_dep_repo),
            project_repo: Arc::clone(&self.project_repo),
            app_handle: self.app_handle.clone(),
        }
    }

    async fn run_post_ready_safety_net(&self) {
        self.clone_for_safety_net().run().await;
    }

    /// Run startup jobs, resuming tasks in agent-active states.
    ///
    /// Skips if execution is paused. Stops early if max_concurrent is reached.
    /// For each task in an agent-active state, re-executes entry actions to
    /// respawn the appropriate agent.
    pub fn run(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = HashSet<String>> + Send + '_>> {
        Box::pin(self.run_inner())
    }

    async fn run_inner(&self) -> HashSet<String> {
        debug!("StartupJobRunner::run() called");

        if let Some(notification_repo) = &self.notification_repo {
            let step_started_at = startup_job_step_started("notification_retention_prune");
            let (read_before, max_rows) =
                notification_retention_prune_args(stream_timeouts(), chrono::Utc::now());
            if let Err(error) = notification_repo.prune(read_before, max_rows).await {
                tracing::warn!(error = %error, "Failed to prune durable notifications at startup");
            }
            startup_job_step_completed("notification_retention_prune", step_started_at);
        }

        if let Some(park_service) = &self.delegation_park_service {
            let step_started_at = startup_job_step_started("delegation_park_reconcile");
            match park_service
                .reconcile_all(self.agent_run_repo.as_ref())
                .await
            {
                Ok(summary) => {
                    if summary.parks_examined > 0 {
                        info!(
                            parks_examined = summary.parks_examined,
                            "Reconciled durable delegation parks at startup"
                        );
                    }
                }
                Err(error) => tracing::warn!(
                    error = %error,
                    "Failed to reconcile delegation parks at startup; parked coordinators may wait until their deadline"
                ),
            }
            startup_job_step_completed("delegation_park_reconcile", step_started_at);
        }

        if let Some(data_retention_service) = &self.data_retention_service {
            let data_retention_service = Arc::clone(data_retention_service);
            // Stays detached: a first-run backlog drain can take many minutes, and inlining
            // it would stall every later startup job behind retention.
            tauri::async_runtime::spawn(async move {
                let step_started_at = startup_job_step_started("chat_payload_retention_prune");
                match data_retention_service.run_cycle(chrono::Utc::now()).await {
                    Ok(report) => tracing::info!(
                        pruned_rows = report.pruned_rows,
                        size_budget_active = report.size_budget_active,
                        size_budget_advised = report.size_budget_advised,
                        compaction_recommended = report.compaction_recommended,
                        "Completed startup chat payload retention cycle"
                    ),
                    Err(error) => {
                        tracing::warn!(error = %error, "Failed to prune chat payloads at startup");
                    }
                }
                startup_job_step_completed("chat_payload_retention_prune", step_started_at);
            });
        }

        if is_startup_recovery_disabled() {
            info!(
                env_var = RALPHX_DISABLE_STARTUP_RECOVERY_ENV,
                "Startup recovery disabled via environment; skipping startup jobs"
            );
            return HashSet::new();
        }

        // Phase 0: Snapshot ideation agents BEFORE previous-session cleanup clears old rows.
        // This captures orphaned ideation session PIDs for Phase N+1 recovery.
        // Must be called before cleanup — after that, old previous-session rows are gone.
        let step_started_at = startup_job_step_started("ideation_agent_snapshot");
        let ideation_snapshot = match self
            .running_agent_registry
            .list_by_context_type("ideation")
            .await
        {
            Ok(entries) => {
                if !entries.is_empty() {
                    info!(
                        count = entries.len(),
                        "Phase 0: Captured ideation agent snapshot for recovery"
                    );
                }
                entries
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "Phase 0: Failed to snapshot ideation agents — recovery will be skipped"
                );
                Vec::new()
            }
        };
        startup_job_step_completed("ideation_agent_snapshot", step_started_at);
        // Phase N+1: Ideation recovery — fire-and-forget tokio::spawn.
        // Runs after all existing phases so it doesn't interfere with task/review/merge recovery.
        // Triggered at the end of run(); the spawn is set up here early to capture the snapshot.
        let phase_n1_snapshot = ideation_snapshot;
        let phase_n1_chat_service = self.chat_service.clone();
        let phase_n1_session_repo = Arc::clone(&self.ideation_session_repo);
        let phase_n1_app_handle = self.app_handle.clone();

        // Phase 105: Kill orphaned agent OS processes from previous session.
        // The SQLite-backed registry persists PIDs across restarts, so we can
        // SIGTERM old processes before spawning new ones. Scope by app boot time:
        // delayed PR/workspace recovery must not stop agents spawned by this process.
        let step_started_at = startup_job_step_started("previous_session_agent_cleanup");
        let killed = self
            .running_agent_registry
            .stop_all_started_before(self.previous_session_cutoff)
            .await;
        startup_job_step_completed("previous_session_agent_cleanup", step_started_at);
        let interrupted_agent_contexts: HashSet<RunningAgentKey> = killed.iter().cloned().collect();
        if !killed.is_empty() {
            info!(
                count = killed.len(),
                cutoff = %self.previous_session_cutoff,
                "Killed orphaned agent processes from previous session"
            );
        }

        // Clean up orphaned agent runs from previous sessions
        // These are runs that were left in "running" status when the app was closed/crashed
        let step_started_at = startup_job_step_started("orphan_agent_run_cleanup");
        match self
            .agent_run_repo
            .cancel_running_started_before(self.previous_session_cutoff)
            .await
        {
            Ok(count) if count > 0 => {
                info!(
                    count = count,
                    cutoff = %self.previous_session_cutoff,
                    "Cancelled orphaned agent runs from previous session"
                );
            }
            Ok(_) => {
                // No orphaned runs, nothing to log
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to clean up orphaned agent runs");
            }
        }
        startup_job_step_completed("orphan_agent_run_cleanup", step_started_at);

        // Phase 4a.2: Sweep crash-orphaned Standalone workspace directories — the only
        // reclamation path for `standalone_workspaces/*` (archive never deletes them).
        // Fail-safe by construction: requires an app-owned data dir, and any entry the
        // sweep cannot positively prove is orphaned is left untouched.
        if let Some(app_data_dir) = self.app_handle.as_ref().and_then(|handle| {
            handle
                .try_state::<crate::application::AppState>()
                .map(|state| state.app_paths.app_data_dir().to_path_buf())
        }) {
            let step_started_at = startup_job_step_started("standalone_workspace_orphan_sweep");
            let sweep_summary =
                crate::application::standalone_workspace::sweep_orphaned_standalone_workspaces(
                    &app_data_dir,
                    Arc::clone(&self.chat_conversation_repo),
                )
                .await;
            if sweep_summary.removed > 0 || sweep_summary.skipped > 0 {
                info!(
                    removed = sweep_summary.removed,
                    retained = sweep_summary.retained,
                    skipped = sweep_summary.skipped,
                    "Standalone workspace crash-orphan sweep completed"
                );
            }
            startup_job_step_completed("standalone_workspace_orphan_sweep", step_started_at);
        }

        // Phase 90+: Read persisted app state (active project + halt mode) from DB.
        // No waiting needed — DB has the value from the previous session.
        debug!("Reading persisted app_state from DB...");
        let step_started_at = startup_job_step_started("app_state_load_and_quota");
        let app_settings = match self.app_state_repo.get().await {
            Ok(settings) => settings,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to read app_state from DB");
                Default::default()
            }
        };
        let active_project_id = app_settings.active_project_id.clone();
        if let Some(ref pid) = active_project_id {
            // Set in-memory state from DB value so other commands can use it immediately
            self.active_project_state.set(Some(pid.clone())).await;
            info!(project_id = pid.as_str(), "Active project loaded from DB");

            // Load execution settings for this project and sync runtime quota
            match self.execution_settings_repo.get_settings(Some(pid)).await {
                Ok(settings) => {
                    let old_max = self.execution_state.max_concurrent();
                    self.execution_state
                        .set_max_concurrent(settings.max_concurrent_tasks);
                    info!(
                        project_id = pid.as_str(),
                        old_max = old_max,
                        new_max = settings.max_concurrent_tasks,
                        "Updated max_concurrent from persisted project settings"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        project_id = pid.as_str(),
                        "Failed to load execution settings for active project, keeping current quota"
                    );
                }
            }
        }
        startup_job_step_completed("app_state_load_and_quota", step_started_at);

        self.run_post_ready_safety_net().await;

        match app_settings.execution_halt_mode {
            ExecutionHaltMode::Running => {}
            ExecutionHaltMode::Paused => {
                self.execution_state.pause();
                info!("Persisted pause barrier detected, skipping startup recovery");
                return HashSet::new();
            }
            ExecutionHaltMode::Stopped => {
                self.execution_state.pause();
                info!("Persisted stop barrier detected, skipping startup recovery");
                return HashSet::new();
            }
        }

        let step_started_at = startup_job_step_started("phase_n1_snapshot_refresh");
        self.refresh_phase_n1_snapshot_sessions(&phase_n1_snapshot)
            .await;
        startup_job_step_completed("phase_n1_snapshot_refresh", step_started_at);

        // Check if execution is paused - skip resumption if so
        if self.execution_state.is_paused() {
            info!("Execution paused, skipping task resumption");
            return HashSet::new();
        }
        debug!("Execution NOT paused, continuing...");

        // Recover exact durable Plan-mode handoffs after previous-session cleanup.
        // This runs before task/ideation recovery so it cannot compete with a
        // replacement Plan turn for the same project conversation.
        let step_started_at = startup_job_step_started("accepted_plan_mode_handoff_recovery");
        self.recover_accepted_plan_mode_handoffs().await;
        startup_job_step_completed("accepted_plan_mode_handoff_recovery", step_started_at);

        if active_project_id.is_none() {
            info!("No active project in DB, skipping task resumption");
            // Still try to schedule Ready tasks if scheduler is set
            if let Some(ref scheduler) = self.task_scheduler {
                let step_started_at =
                    startup_job_step_started("ready_task_scheduling_no_active_project");
                info!("Scheduling Ready tasks (no resumption)");
                scheduler.try_schedule_ready_tasks().await;
                startup_job_step_completed(
                    "ready_task_scheduling_no_active_project",
                    step_started_at,
                );
            }
            return HashSet::new();
        }

        // Get projects to process (scoped to active project in Phase 82)
        let step_started_at = startup_job_step_started("active_project_load");
        let projects = if let Some(ref active_pid) = active_project_id {
            // Scope to active project only
            match self.project_repo.get_by_id(active_pid).await {
                Ok(Some(project)) => vec![project],
                Ok(None) => {
                    tracing::warn!(
                        project_id = active_pid.as_str(),
                        "Active project not found, skipping resumption"
                    );
                    startup_job_step_completed("active_project_load", step_started_at);
                    return HashSet::new();
                }
                Err(e) => {
                    tracing::error!(error = %e, "Failed to get active project for startup resumption");
                    startup_job_step_completed("active_project_load", step_started_at);
                    return HashSet::new();
                }
            }
        } else {
            // Fallback to all projects (shouldn't reach here due to check above)
            match self.project_repo.get_all().await {
                Ok(projects) => projects,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to get projects for startup resumption");
                    startup_job_step_completed("active_project_load", step_started_at);
                    return HashSet::new();
                }
            }
        };
        startup_job_step_completed("active_project_load", step_started_at);

        if let Some(ref active_pid) = active_project_id {
            if self.git_startup_blocked_project_ids.contains(active_pid) {
                tracing::warn!(
                    project_id = active_pid.as_str(),
                    "Startup Git auth preflight blocked active-project task recovery; deferring task and merge resumption"
                );
                return HashSet::new();
            }
        }

        let mut resumed = 0u32;

        debug!(
            count = projects.len(),
            active_project = ?active_project_id.as_ref().map(|p| p.as_str()),
            "Found projects for startup resumption"
        );

        // Phase 0: Clean up stale git state before any task recovery
        let step_started_at = startup_job_step_started("stale_git_state_cleanup");
        self.cleanup_stale_git_state(&projects).await;
        startup_job_step_completed("stale_git_state_cleanup", step_started_at);

        // Phase 0.5: Resume deferred cleanup for tasks that were Merged but had
        // Phase 3 cleanup interrupted (app crash/restart). Runs before merge
        // recovery so worktrees and branches are cleaned before new merges.
        let step_started_at = startup_job_step_started("pending_cleanup_resume");
        self.resume_pending_cleanup(&projects).await;
        startup_job_step_completed("pending_cleanup_resume", step_started_at);

        // Phase 0.6: Repair stale non-merge worktree paths before spawning any agents.
        // Prevents task/review contexts from ever falling back to main repo checkout.
        let step_started_at = startup_job_step_started("non_merge_worktree_repair");
        self.repair_non_merge_task_worktrees(&projects).await;
        startup_job_step_completed("non_merge_worktree_repair", step_started_at);

        // Phase 0.8: Recover tasks escalated by app crash / unclean shutdown.
        // Finds Escalated tasks with crash metadata and transitions them back to their
        // pre-escalation states so Phase 1/2/3 can re-process them normally.
        let step_started_at = startup_job_step_started("crash_escalation_recovery");
        self.recover_crash_escalated_tasks(&projects).await;
        startup_job_step_completed("crash_escalation_recovery", step_started_at);

        // Phase 0.9: Remediate legacy MergeIncomplete rows that are actually commit-hook
        // rework failures. These should rejoin the normal RevisionNeeded -> ReExecuting flow,
        // not remain in merge recovery forever across restarts.
        let step_started_at = startup_job_step_started("commit_hook_merge_incomplete_remediation");
        self.remediate_commit_hook_merge_incomplete_tasks(&projects)
            .await;
        startup_job_step_completed("commit_hook_merge_incomplete_remediation", step_started_at);

        // Phase 1: Merge-first recovery — process PendingMerge, local Merging,
        // and PR-waiting tasks
        // before spawning other agents. This ensures main branch is in a clean state
        // before worker/reviewer agents start. PendingMerge first so fast-path
        // programmatic merges complete before agent-based merges.
        const MERGE_RECOVERY_STATES: &[InternalStatus] = &[
            InternalStatus::PendingMerge,
            InternalStatus::Merging,
            InternalStatus::WaitingOnPr,
        ];

        let step_started_at = startup_job_step_started("merge_first_recovery");
        let mut merge_recovery_tasks_seen = 0usize;
        let mut merge_recovery_resumed = 0u32;
        info!("Phase 1: Merge-first recovery — processing merge states before agent spawning");

        'merge_recovery: for project in &projects {
            for status in MERGE_RECOVERY_STATES {
                let tasks = match self.task_repo.get_by_status(&project.id, *status).await {
                    Ok(tasks) => tasks,
                    Err(e) => {
                        tracing::warn!(
                            project_id = project.id.as_str(),
                            status = ?status,
                            error = %e,
                            "Failed to get tasks by status for merge-first recovery"
                        );
                        continue;
                    }
                };
                merge_recovery_tasks_seen += tasks.len();

                debug!(
                    count = tasks.len(),
                    ?status,
                    "Phase 1: Found tasks in merge state"
                );
                for task in tasks {
                    if task.archived_at.is_some() {
                        debug!(task_id = task.id.as_str(), title = %task.title, "Skipping archived task");
                        continue;
                    }

                    // Skip tasks that already have main_merge_deferred=true — they need
                    // serialized one-at-a-time retry via try_retry_main_merges(), not
                    // direct execute_entry_actions() which could race if multiple siblings
                    // are all in PendingMerge with the flag set.
                    {
                        use crate::domain::state_machine::transition_handler::has_main_merge_deferred_metadata;
                        if has_main_merge_deferred_metadata(&task) {
                            debug!(
                                task_id = task.id.as_str(),
                                "Phase 1: skipping main_merge_deferred task — will be handled by try_retry_main_merges at startup end"
                            );
                            continue;
                        }
                    }

                    let task = if let Some(updated_task) = self
                        .prepare_active_task_startup_resume(
                            &task,
                            *status,
                            &interrupted_agent_contexts,
                        )
                        .await
                    {
                        updated_task
                    } else {
                        let reconciled = self.reconciler.reconcile_task(&task, *status).await;

                        if reconciled {
                            continue;
                        }

                        task
                    };

                    let requires_execution_capacity = *status != InternalStatus::WaitingOnPr;

                    if requires_execution_capacity
                        && !self.execution_state.can_start_any_execution_context()
                    {
                        info!(
                            global_max_concurrent = self.execution_state.global_max_concurrent(),
                            running_count = self.execution_state.running_count(),
                            "Phase 1: Global execution capacity reached, stopping merge-first recovery"
                        );
                        break 'merge_recovery;
                    }

                    if requires_execution_capacity
                        && !self.project_has_execution_capacity(&task.project_id).await
                    {
                        info!(
                            task_id = task.id.as_str(),
                            project_id = task.project_id.as_str(),
                            "Phase 1: skipping merge task because project execution capacity is full"
                        );
                        continue;
                    }

                    info!(
                        task_id = task.id.as_str(),
                        status = ?status,
                        "Phase 1: Resuming merge task"
                    );

                    self.transition_service
                        .execute_entry_actions(&task.id, &task, *status)
                        .await;

                    merge_recovery_resumed += 1;
                    resumed += 1;
                }
            }
        }
        info!(
            tasks_seen = merge_recovery_tasks_seen,
            tasks_resumed = merge_recovery_resumed,
            "Startup job runner merge-first recovery summary"
        );
        startup_job_step_completed("merge_first_recovery", step_started_at);

        // Iterate through projects and their tasks in agent-active states
        let step_started_at = startup_job_step_started("agent_active_recovery");
        let mut agent_active_tasks_seen = 0usize;
        let mut agent_active_resumed = 0u32;
        for project in &projects {
            debug!(
                project_id = project.id.as_str(),
                "Checking project for resumable tasks"
            );
            for status in AGENT_ACTIVE_STATUSES {
                // Skip merge states — already handled in Phase 1 merge-first recovery
                if *status == InternalStatus::Merging || *status == InternalStatus::PendingMerge {
                    continue;
                }

                // Get tasks in this status for this project
                let tasks = match self.task_repo.get_by_status(&project.id, *status).await {
                    Ok(tasks) => tasks,
                    Err(e) => {
                        tracing::warn!(
                            project_id = project.id.as_str(),
                            status = ?status,
                            error = %e,
                            "Failed to get tasks by status"
                        );
                        continue;
                    }
                };
                agent_active_tasks_seen += tasks.len();

                debug!(count = tasks.len(), ?status, "Found tasks in status");
                for task in tasks {
                    // Phase 106: Defense-in-depth — skip archived tasks even if query returns them
                    if task.archived_at.is_some() {
                        debug!(task_id = task.id.as_str(), title = %task.title, "Skipping archived task");
                        continue;
                    }

                    // Skip main-merge-deferred tasks when agents are still running.
                    // These are correctly deferred, not orphaned — reconciliation will retry when agents complete.
                    if Self::is_waiting_for_global_idle(&task, self.execution_state.running_count())
                    {
                        debug!(
                            task_id = task.id.as_str(),
                            running_count = self.execution_state.running_count(),
                            "Skipping main-merge-deferred task: agents still running"
                        );
                        continue;
                    }

                    let task = if let Some(updated_task) = self
                        .prepare_active_task_startup_resume(
                            &task,
                            *status,
                            &interrupted_agent_contexts,
                        )
                        .await
                    {
                        updated_task
                    } else {
                        let reconciled = self.reconciler.reconcile_task(&task, *status).await;

                        if reconciled {
                            continue;
                        }

                        task
                    };

                    // Check if we can start another task
                    if !self.execution_state.can_start_any_execution_context() {
                        info!(
                            global_max_concurrent = self.execution_state.global_max_concurrent(),
                            running_count = self.execution_state.running_count(),
                            "Global execution capacity reached, stopping resumption"
                        );
                        info!(count = resumed, "Task resumption complete (partial)");
                        info!(
                            tasks_seen = agent_active_tasks_seen,
                            tasks_resumed = agent_active_resumed,
                            "Startup job runner agent-active recovery summary"
                        );
                        startup_job_step_completed("agent_active_recovery", step_started_at);
                        return HashSet::new();
                    }

                    if !self.project_has_execution_capacity(&task.project_id).await {
                        info!(
                            task_id = task.id.as_str(),
                            project_id = task.project_id.as_str(),
                            "Skipping task resumption because project execution capacity is full"
                        );
                        continue;
                    }

                    info!(
                        task_id = task.id.as_str(),
                        status = ?status,
                        "Resuming task"
                    );

                    // Re-execute entry actions to respawn the agent
                    self.transition_service
                        .execute_entry_actions(&task.id, &task, *status)
                        .await;

                    agent_active_resumed += 1;
                    resumed += 1;
                }
            }
        }
        info!(
            tasks_seen = agent_active_tasks_seen,
            tasks_resumed = agent_active_resumed,
            "Startup job runner agent-active recovery summary"
        );
        startup_job_step_completed("agent_active_recovery", step_started_at);

        info!(count = resumed, "Task resumption complete");

        // Re-trigger auto-transition states that may have been interrupted mid-transition
        // These states have on_enter side effects that trigger auto-transitions to spawn agents
        let step_started_at = startup_job_step_started("auto_transition_recovery");
        let mut auto_transition_tasks_seen = 0usize;
        let mut auto_transition_retriggered = 0u32;
        for project in &projects {
            for status in AUTO_TRANSITION_STATES {
                // Skip PendingMerge — already handled in Phase 1 merge-first recovery
                if *status == InternalStatus::PendingMerge {
                    continue;
                }

                let tasks = match self.task_repo.get_by_status(&project.id, *status).await {
                    Ok(tasks) => tasks,
                    Err(e) => {
                        tracing::warn!(
                            project_id = project.id.as_str(),
                            status = ?status,
                            error = %e,
                            "Failed to get tasks by status for auto-transition"
                        );
                        continue;
                    }
                };
                auto_transition_tasks_seen += tasks.len();

                debug!(
                    count = tasks.len(),
                    ?status,
                    "Found tasks in auto-transition status"
                );
                for task in tasks {
                    // Phase 106: Defense-in-depth — skip archived tasks even if query returns them
                    if task.archived_at.is_some() {
                        debug!(task_id = task.id.as_str(), title = %task.title, "Skipping archived task");
                        continue;
                    }

                    // Skip main-merge-deferred tasks when agents are still running.
                    // These tasks are correctly deferred and will be retried when all agents
                    // complete (via try_retry_main_merges on global idle).
                    if Self::is_waiting_for_global_idle(&task, self.execution_state.running_count())
                    {
                        debug!(
                            task_id = task.id.as_str(),
                            running_count = self.execution_state.running_count(),
                            "Skipping main-merge-deferred task in auto-transition recovery: agents still running"
                        );
                        continue;
                    }

                    // Check max_concurrent before triggering (auto-transitions may spawn agents)
                    if !self.execution_state.can_start_any_execution_context() {
                        info!(
                            global_max_concurrent = self.execution_state.global_max_concurrent(),
                            running_count = self.execution_state.running_count(),
                            "Global execution capacity reached, stopping auto-transition recovery"
                        );
                        info!(
                            tasks_seen = auto_transition_tasks_seen,
                            tasks_retriggered = auto_transition_retriggered,
                            "Startup job runner auto-transition recovery summary"
                        );
                        startup_job_step_completed("auto_transition_recovery", step_started_at);
                        return HashSet::new();
                    }

                    if !self.project_has_execution_capacity(&task.project_id).await {
                        info!(
                            task_id = task.id.as_str(),
                            project_id = task.project_id.as_str(),
                            "Skipping auto-transition recovery because project execution capacity is full"
                        );
                        continue;
                    }

                    info!(
                        task_id = task.id.as_str(),
                        status = ?status,
                        "Re-triggering auto-transition for stuck task"
                    );

                    // Re-execute entry actions - this will trigger check_auto_transition()
                    self.transition_service
                        .execute_entry_actions(&task.id, &task, *status)
                        .await;
                    auto_transition_retriggered += 1;
                }
            }
        }
        info!(
            tasks_seen = auto_transition_tasks_seen,
            tasks_retriggered = auto_transition_retriggered,
            "Startup job runner auto-transition recovery summary"
        );
        startup_job_step_completed("auto_transition_recovery", step_started_at);

        // After resuming agent-active tasks, try to schedule any Ready tasks
        // that may be waiting in the queue (if scheduler is configured)
        if let Some(ref scheduler) = self.task_scheduler {
            let step_started_at = startup_job_step_started("ready_task_scheduling");
            info!("Scheduling Ready tasks after resumption");
            scheduler.try_schedule_ready_tasks().await;
            startup_job_step_completed("ready_task_scheduling", step_started_at);
        }

        // Boot recovery: if no agents were spawned during startup (quiescent boot),
        // invoke try_retry_main_merges() now. Normally this is called from the
        // agent-exit path in transition_handler when running_count transitions to 0,
        // but at boot there may be no agents to exit and thus no future trigger.
        // Without this call, PendingMerge tasks with main_merge_deferred=true would
        // sit stuck indefinitely after a reboot.
        if self.execution_state.running_count() == 0 {
            if let Some(ref scheduler) = self.task_scheduler {
                let step_started_at = startup_job_step_started("boot_main_merge_retry");
                info!("Boot recovery: invoking try_retry_main_merges for deferred main-branch merges (running_count == 0)");
                scheduler.try_retry_main_merges().await;
                startup_job_step_completed("boot_main_merge_retry", step_started_at);
            }
        }

        // Phase 4: Startup recovery for pending ideation sessions.
        // Drains sessions that were deferred at creation time due to capacity limits.
        // Runs after main startup recovery to avoid competing with orphaned-agent re-spawning.
        let step_started_at = startup_job_step_started("pending_ideation_drain");
        self.drain_pending_ideation_sessions().await;
        startup_job_step_completed("pending_ideation_drain", step_started_at);

        // Phase N+1: Ideation agent recovery — fire-and-forget.
        // Processes the snapshot captured in Phase 0, after all other startup phases complete.
        // Does NOT block startup; app becomes responsive before recovery finishes.
        let mut phase_n1_claimed_sessions = HashSet::new();
        if !phase_n1_snapshot.is_empty() {
            if let Some(chat_service) = phase_n1_chat_service {
                phase_n1_claimed_sessions =
                    Self::collect_phase_n1_snapshot_session_ids(&phase_n1_snapshot);
                let mut recovery_queue = RecoveryQueue::new(std::time::Duration::from_secs(2));
                for (key, info) in phase_n1_snapshot {
                    let item = RecoveryItem {
                        context_type: key.context_type,
                        context_id: key.context_id,
                        conversation_id: info.conversation_id,
                        priority: RecoveryPriority::Ideation,
                        started_at: info.started_at,
                    };
                    recovery_queue.enqueue(item);
                }
                let queue_len = recovery_queue.len();
                info!(count = queue_len, "Phase N+1: Starting ideation recovery");

                tokio::spawn(async move {
                    let summary = recovery_queue
                        .process(|item| {
                            let chat_service = Arc::clone(&chat_service);
                            let session_repo = Arc::clone(&phase_n1_session_repo);
                            let app_handle = phase_n1_app_handle.clone();
                            async move {
                                Self::recover_ideation_session(
                                    item,
                                    chat_service.as_ref(),
                                    session_repo.as_ref(),
                                    app_handle.as_ref(),
                                )
                                .await
                            }
                        })
                        .await;
                    info!(
                        recovered = summary.recovered,
                        total = queue_len,
                        skipped = summary.skipped,
                        failed = summary.failed,
                        "Phase N+1: Ideation recovery complete: recovered {}/{} sessions ({} skipped, {} failed)",
                        summary.recovered,
                        queue_len,
                        summary.skipped,
                        summary.failed,
                    );
                });
            } else {
                info!("Phase N+1: No chat service configured, skipping ideation recovery");
            }
        }

        phase_n1_claimed_sessions
    }

    fn collect_phase_n1_snapshot_session_ids(
        phase_n1_snapshot: &[(
            crate::domain::services::RunningAgentKey,
            crate::domain::services::RunningAgentInfo,
        )],
    ) -> HashSet<String> {
        phase_n1_snapshot
            .iter()
            .map(|(key, _)| key.context_id.clone())
            .collect()
    }

    async fn refresh_phase_n1_snapshot_sessions(
        &self,
        phase_n1_snapshot: &[(
            crate::domain::services::RunningAgentKey,
            crate::domain::services::RunningAgentInfo,
        )],
    ) {
        if phase_n1_snapshot.is_empty() || self.chat_service.is_none() {
            return;
        }

        let mut refreshed = 0u32;
        let mut seen_sessions = HashSet::new();

        for (key, _) in phase_n1_snapshot {
            if !seen_sessions.insert(key.context_id.clone()) {
                continue;
            }

            match self
                .ideation_session_repo
                .touch_updated_at(&key.context_id)
                .await
            {
                Ok(()) => {
                    refreshed += 1;
                }
                Err(error) => {
                    tracing::warn!(
                        session_id = %key.context_id,
                        error = %error,
                        "Phase N+1: Failed to refresh ideation session before startup recovery"
                    );
                }
            }
        }

        if refreshed > 0 {
            info!(
                count = refreshed,
                "Phase N+1: Refreshed ideation sessions before startup recovery"
            );
        }
    }

    /// Phase N+1: Recover a single orphaned ideation session.
    ///
    /// 1. Validates the session exists and is still `Active` (skips otherwise).
    /// 2. Calls `ChatService::send_message()` with a synthetic restart prompt.
    ///    `send_message()` finds the existing conversation, loads message history,
    ///    and spawns a fresh agent with history replayed (NOT `--resume`).
    /// 3. Emits `agent:session_recovered` event after each successful spawn.
    ///
    /// Returns `Ok(())` for intentional skips (session gone/inactive) as well as
    /// successful recoveries — only unexpected errors are returned as `Err`.
    async fn recover_ideation_session(
        item: crate::application::recovery_queue::RecoveryItem,
        chat_service: &dyn ChatService,
        session_repo: &dyn IdeationSessionRepository,
        app_handle: Option<&AppHandle>,
    ) -> Result<(), String> {
        let session_id = IdeationSessionId::from_string(item.context_id.clone());

        // Validate session: must exist and be Active.
        match session_repo.get_by_id(&session_id).await {
            Ok(Some(session)) if session.status == IdeationSessionStatus::Active => {
                // Active — proceed with recovery.
            }
            Ok(Some(session)) => {
                info!(
                    session_id = %item.context_id,
                    status = %session.status,
                    "Phase N+1: Skipping recovery — session is not active"
                );
                return Ok(());
            }
            Ok(None) => {
                info!(
                    session_id = %item.context_id,
                    "Phase N+1: Skipping recovery — session not found"
                );
                return Ok(());
            }
            Err(e) => {
                return Err(format!(
                    "Failed to validate session {}: {}",
                    item.context_id, e
                ));
            }
        }

        // Send recovery message. send_message() finds the existing conversation,
        // loads message history, and spawns a fresh agent with history replayed.
        match chat_service
            .send_message(
                ChatContextType::Ideation,
                &item.context_id,
                "[System] App restarted. Assess the current session state and continue where you left off.",
                SendMessageOptions {
                    metadata: None,
                    created_at: None,
                    harness_override: None,
                    is_external_mcp: false,
                    caller_context: SendCallerContext::UserInitiated,
                    ..Default::default()
                },
            )
            .await
        {
            Ok(_) => {
                info!(
                    session_id = %item.context_id,
                    conversation_id = %item.conversation_id,
                    "Phase N+1: Recovered ideation session"
                );
                if let Some(handle) = app_handle {
                    let _ = handle.emit("agent:session_recovered", &item.context_id);
                }
                Ok(())
            }
            Err(e) => Err(format!(
                "send_message failed for session {}: {}",
                item.context_id, e
            )),
        }
    }

    /// Phase 0.8: Recover tasks that were escalated due to app crash or unclean shutdown.
    ///
    /// Scans Escalated tasks across the given projects for auto-recovery criteria:
    /// - `shutdown_interrupted == true` in metadata (set by L1 shutdown handler), OR
    /// - `last_agent_error` contains "completed without calling" (crash indicator);
    ///   AND `startup_recovery_attempts < 1` (prevent infinite recovery loops).
    ///
    /// Matching tasks are transitioned back to their pre-escalation state based on
    /// `last_agent_error_context`: "review" → PendingReview, "execution" → Ready,
    /// "merge" → PendingMerge. Phase 1/2/3 then pick them up for normal re-processing.
    async fn recover_crash_escalated_tasks(
        &self,
        projects: &[crate::domain::entities::Project],
    ) -> u32 {
        let mut recovered = 0u32;

        for project in projects {
            let escalated_tasks = match self
                .task_repo
                .get_by_status(&project.id, InternalStatus::Escalated)
                .await
            {
                Ok(tasks) => tasks,
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        project_id = project.id.as_str(),
                        "Phase 0.8: Failed to query Escalated tasks for crash recovery"
                    );
                    continue;
                }
            };

            for mut task in escalated_tasks {
                if task.archived_at.is_some() {
                    debug!(
                        task_id = task.id.as_str(),
                        "Phase 0.8: Skipping archived task"
                    );
                    continue;
                }

                // Parse metadata JSON (task.metadata is Option<String>)
                let mut meta: serde_json::Value = task
                    .metadata
                    .as_deref()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or_else(|| serde_json::json!({}));

                if !should_auto_recover(&meta) {
                    continue;
                }

                // Determine recovery target from last_agent_error_context
                let recovery_target = match meta
                    .get("last_agent_error_context")
                    .and_then(|v| v.as_str())
                {
                    Some("review") => InternalStatus::PendingReview,
                    Some("execution") => InternalStatus::Ready,
                    Some("merge") => InternalStatus::PendingMerge,
                    other => {
                        debug!(
                            task_id = task.id.as_str(),
                            context = ?other,
                            "Phase 0.8: Unknown or missing last_agent_error_context, skipping"
                        );
                        continue;
                    }
                };

                let from_status = task.internal_status;

                // Update recovery metadata: increment counter, clear shutdown flag
                if let Some(obj) = meta.as_object_mut() {
                    let attempts = obj
                        .get("startup_recovery_attempts")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    obj.insert(
                        "startup_recovery_attempts".to_string(),
                        serde_json::json!(attempts + 1),
                    );
                    obj.remove("shutdown_interrupted");
                }

                task.metadata = Some(meta.to_string());
                task.internal_status = recovery_target;
                task.touch();

                match self
                    .persist_startup_status_change(&task, from_status, "crash_recovery")
                    .await
                {
                    Ok(true) => {}
                    Ok(false) => {
                        tracing::info!(
                            task_id = task.id.as_str(),
                            from = from_status.as_str(),
                            to = recovery_target.as_str(),
                            "Phase 0.8: Crash recovery skipped — task already transitioned"
                        );
                        continue;
                    }
                    Err(e) => {
                        tracing::error!(
                            error = %e,
                            task_id = task.id.as_str(),
                            "Phase 0.8: Failed to persist crash recovery transition"
                        );
                        continue;
                    }
                }

                // Add ReviewNote so the frontend can display the auto-recovery reason
                if let Some(ref repo) = self.review_repo {
                    let note = ReviewNote::with_notes(
                        task.id.clone(),
                        ReviewerType::System,
                        ReviewOutcome::Approved,
                        format!(
                            "Auto-recovered by startup crash recovery: task was escalated due to \
                             app shutdown/crash and automatically transitioned to {:?} for re-processing.",
                            recovery_target
                        ),
                    );
                    if let Err(e) = repo.add_note(&note).await {
                        tracing::warn!(
                            error = %e,
                            task_id = task.id.as_str(),
                            "Phase 0.8: Failed to add crash recovery ReviewNote"
                        );
                    }
                }

                info!(
                    task_id = task.id.as_str(),
                    from = from_status.as_str(),
                    to = recovery_target.as_str(),
                    "Phase 0.8: Auto-recovered crash-escalated task"
                );

                recovered += 1;
            }
        }

        if recovered > 0 {
            info!(
                count = recovered,
                "Phase 0.8: Crash-escalated task recovery complete"
            );
        } else {
            debug!("Phase 0.8: No crash-escalated tasks found for auto-recovery");
        }

        recovered
    }

    /// Unblock tasks whose blockers are all complete.
    ///
    /// This is a safety net for cases where the app crashed before real-time
    /// unblocking could run (e.g., when a task merged but the app closed before
    /// dependent tasks were unblocked).
    ///
    /// Scans all Blocked tasks across all projects and transitions those
    /// whose blockers satisfy the shared dependency classifier to Ready status.
    async fn unblock_ready_tasks_for(
        task_repo: Arc<dyn TaskRepository>,
        task_dep_repo: Arc<dyn TaskDependencyRepository>,
        project_repo: Arc<dyn ProjectRepository>,
        app_handle: Option<AppHandle>,
    ) {
        // Get all projects to scan for blocked tasks
        let projects = match project_repo.get_all().await {
            Ok(projects) => projects,
            Err(e) => {
                tracing::error!(error = %e, "Failed to fetch projects for startup unblock");
                return;
            }
        };

        let mut unblocked_count = 0u32;

        for project in projects {
            // Get all blocked tasks for this project
            let blocked_tasks = match task_repo
                .get_by_status(&project.id, InternalStatus::Blocked)
                .await
            {
                Ok(tasks) => tasks,
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        project_id = project.id.as_str(),
                        "Failed to fetch blocked tasks for project"
                    );
                    continue;
                }
            };

            if blocked_tasks.is_empty() {
                continue;
            }

            tracing::debug!(
                project_id = project.id.as_str(),
                count = blocked_tasks.len(),
                "Checking blocked tasks for startup unblock"
            );

            for mut task in blocked_tasks {
                // Phase 106: Defense-in-depth — skip archived tasks even if query returns them
                if task.archived_at.is_some() {
                    debug!(task_id = task.id.as_str(), title = %task.title, "Skipping archived task in unblock check");
                    continue;
                }

                // Get blockers for this task
                let blockers = match task_dep_repo.get_blockers(&task.id).await {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            task_id = task.id.as_str(),
                            "Failed to get blockers for task"
                        );
                        continue;
                    }
                };

                // Check if all blockers are complete
                if !Self::all_blockers_complete_with_repo(&task_repo, &blockers).await {
                    continue;
                }

                // All blockers complete - transition to Ready
                task.internal_status = InternalStatus::Ready;
                task.blocked_reason = None;
                task.touch();

                match Self::persist_startup_status_change_with_repo(
                    &task_repo,
                    &task,
                    InternalStatus::Blocked,
                    "startup_unblock",
                )
                .await
                {
                    Ok(true) => {}
                    Ok(false) => {
                        tracing::info!(
                            task_id = task.id.as_str(),
                            "Startup unblock skipped — task already transitioned"
                        );
                        continue;
                    }
                    Err(e) => {
                        tracing::error!(
                            error = %e,
                            task_id = task.id.as_str(),
                            "Failed to unblock task on startup"
                        );
                        continue;
                    }
                }

                tracing::info!(
                    task_id = task.id.as_str(),
                    task_title = %task.title,
                    "Task unblocked on startup - all blockers complete"
                );

                if let Some(ref handle) = app_handle {
                    let _ = handle.emit(
                        "task:unblocked",
                        serde_json::json!({
                            "taskId": task.id.as_str(),
                            "taskTitle": task.title,
                            "timestamp": chrono::Utc::now().to_rfc3339(),
                        }),
                    );
                }

                unblocked_count += 1;
            }
        }

        if unblocked_count > 0 {
            info!(count = unblocked_count, "Unblocked tasks on startup");
        } else {
            tracing::debug!("No blocked tasks needed unblocking on startup");
        }
    }

    /// Check if all blocker tasks satisfy the dependency (allow unblocking).
    /// Delegates to the shared dependency blocker classifier.
    /// MergeIncomplete, Failed, and Stopped are terminal but do NOT satisfy dependencies.
    /// If a blocker doesn't exist (was deleted), it's considered satisfied.
    async fn all_blockers_complete_with_repo(
        task_repo: &Arc<dyn TaskRepository>,
        blocker_ids: &[crate::domain::entities::TaskId],
    ) -> bool {
        for blocker_id in blocker_ids {
            match task_repo.get_by_id(blocker_id).await {
                Ok(Some(task)) => {
                    if task.internal_status.is_active_dependency_blocker() {
                        return false;
                    }
                }
                Ok(None) => {
                    // Blocker was deleted - consider it complete (not blocking)
                    continue;
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        blocker_id = blocker_id.as_str(),
                        "Failed to fetch blocker task, assuming incomplete"
                    );
                    return false;
                }
            }
        }
        true
    }

    /// Detect tasks in non-Blocked states that have unsatisfied dependencies and
    /// move them back to Blocked (or Stopped for states where Blocked is invalid).
    ///
    /// This is the reverse of `unblock_ready_tasks()`: it catches tasks that ended
    /// up in Ready/Executing/Reviewing/etc. despite having a Failed or otherwise
    /// unsatisfied blocker. This can happen when a blocker fails while a dependent
    /// task is already past the Blocked state, and the app crashes before the
    /// dependency manager can react.
    ///
    /// State machine mapping:
    /// - Ready/Executing/ReExecuting → Blocked (valid transition)
    /// - QaRefining/QaTesting/Reviewing → Stopped (Blocked not valid from these)
    pub async fn reconcile_dependency_violations(&self) {
        Self::reconcile_dependency_violations_for(
            Arc::clone(&self.task_repo),
            Arc::clone(&self.task_dep_repo),
            Arc::clone(&self.project_repo),
            self.app_handle.clone(),
        )
        .await;
    }

    async fn reconcile_dependency_violations_for(
        task_repo: Arc<dyn TaskRepository>,
        task_dep_repo: Arc<dyn TaskDependencyRepository>,
        project_repo: Arc<dyn ProjectRepository>,
        app_handle: Option<AppHandle>,
    ) {
        let projects = match project_repo.get_all().await {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(error = %e, "Failed to fetch projects for dependency reconciliation");
                return;
            }
        };

        /// States to scan for dependency violations, mapped to their recovery target.
        /// Returns Some(target) if the state should be checked, None otherwise.
        fn violation_target(status: InternalStatus) -> Option<InternalStatus> {
            match status {
                InternalStatus::Ready | InternalStatus::Executing | InternalStatus::ReExecuting => {
                    Some(InternalStatus::Blocked)
                }
                InternalStatus::QaRefining
                | InternalStatus::QaTesting
                | InternalStatus::Reviewing => Some(InternalStatus::Stopped),
                _ => None,
            }
        }

        const SCAN_STATUSES: &[InternalStatus] = &[
            InternalStatus::Ready,
            InternalStatus::Executing,
            InternalStatus::ReExecuting,
            InternalStatus::QaRefining,
            InternalStatus::QaTesting,
            InternalStatus::Reviewing,
        ];

        let mut reblocked = 0u32;
        let mut stopped = 0u32;

        for project in &projects {
            for &status in SCAN_STATUSES {
                let tasks = match task_repo.get_by_status(&project.id, status).await {
                    Ok(t) => t,
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            project_id = project.id.as_str(),
                            ?status,
                            "Failed to fetch tasks for dependency reconciliation"
                        );
                        continue;
                    }
                };

                for mut task in tasks {
                    if task.archived_at.is_some() {
                        continue;
                    }

                    // Get blockers for this task
                    let blockers = match task_dep_repo.get_blockers(&task.id).await {
                        Ok(b) => b,
                        Err(_) => continue,
                    };

                    if blockers.is_empty() {
                        continue;
                    }

                    // Check if any blocker is unsatisfied
                    let mut unsatisfied_names: Vec<String> = Vec::new();
                    for blocker_id in &blockers {
                        match task_repo.get_by_id(blocker_id).await {
                            Ok(Some(blocker)) => {
                                if blocker.internal_status.is_active_dependency_blocker() {
                                    let label = if blocker.internal_status == InternalStatus::Failed
                                    {
                                        format!("\"{}\" (failed)", blocker.title)
                                    } else {
                                        format!(
                                            "\"{}\" ({})",
                                            blocker.title, blocker.internal_status
                                        )
                                    };
                                    unsatisfied_names.push(label);
                                }
                            }
                            Ok(None) => {} // deleted blocker = satisfied
                            Err(_) => {}   // fail-open on repo errors
                        }
                    }

                    if unsatisfied_names.is_empty() {
                        continue;
                    }

                    let Some(target) = violation_target(status) else {
                        continue;
                    };

                    let reason = format!("Waiting for: {}", unsatisfied_names.join(", "));
                    let from_status = task.internal_status;
                    task.internal_status = target;
                    task.blocked_reason = Some(reason);
                    task.touch();

                    match Self::persist_startup_status_change_with_repo(
                        &task_repo,
                        &task,
                        from_status,
                        "dep_reconciliation",
                    )
                    .await
                    {
                        Ok(true) => {}
                        Ok(false) => {
                            tracing::info!(
                                task_id = task.id.as_str(),
                                from = from_status.as_str(),
                                to = target.as_str(),
                                "Dependency reconciliation skipped — task already transitioned"
                            );
                            continue;
                        }
                        Err(e) => {
                            tracing::error!(
                                error = %e,
                                task_id = task.id.as_str(),
                                "Failed to reconcile dependency violation"
                            );
                            continue;
                        }
                    }

                    if let Some(ref handle) = app_handle {
                        let _ = handle.emit(
                            "task:event",
                            serde_json::json!({
                                "type": "status_changed",
                                "taskId": task.id.as_str(),
                                "from": from_status.as_str(),
                                "to": target.as_str(),
                                "changedBy": "dep_reconciliation",
                            }),
                        );
                    }

                    match target {
                        InternalStatus::Blocked => {
                            reblocked += 1;
                            info!(
                                task_id = task.id.as_str(),
                                from = from_status.as_str(),
                                "Re-blocked task with unsatisfied dependencies"
                            );
                        }
                        InternalStatus::Stopped => {
                            stopped += 1;
                            info!(
                                task_id = task.id.as_str(),
                                from = from_status.as_str(),
                                "Stopped task with unsatisfied dependencies (Blocked not valid)"
                            );
                        }
                        _ => {}
                    }
                }
            }
        }

        if reblocked > 0 || stopped > 0 {
            info!(
                reblocked,
                stopped, "Startup dependency reconciliation complete"
            );
        } else {
            debug!("No dependency violations found on startup");
        }
    }

    /// Abort stale rebase/merge operations on project repos.
    /// Called before any task recovery to ensure clean git state.
    #[doc(hidden)]
    pub async fn cleanup_stale_git_state(&self, projects: &[crate::domain::entities::Project]) {
        for project in projects {
            let repo_path = Path::new(&project.working_directory);
            if !crate::utils::path_safety::checked_exists(repo_path, "project repository")
                .unwrap_or(false)
            {
                continue;
            }
            if GitService::is_rebase_in_progress(repo_path) {
                info!(
                    project_id = project.id.as_str(),
                    "Phase 0: Aborting stale rebase on main repo before startup recovery"
                );
                let _ = GitService::abort_rebase(repo_path).await;
            }
            if GitService::is_merge_in_progress(repo_path) {
                info!(
                    project_id = project.id.as_str(),
                    "Phase 0: Aborting stale merge on main repo before startup recovery"
                );
                let _ = GitService::abort_merge(repo_path).await;
            }
        }
    }

    /// Resume deferred cleanup for tasks that completed merge (Phase 2) but had
    /// Phase 3 cleanup interrupted (e.g., app crash/restart).
    ///
    /// Scans Merged tasks across all given projects for the `pending_cleanup`
    /// metadata flag. For each, runs the deferred cleanup (worktree removal,
    /// branch deletion, metadata clearing).
    async fn resume_pending_cleanup(&self, projects: &[crate::domain::entities::Project]) {
        use crate::domain::state_machine::transition_handler::deferred_merge_cleanup;

        let started_at = Instant::now();
        let mut cleanup_query_tasks_seen = 0usize;
        let mut pending_cleanup_tasks = 0usize;
        let mut task_scan_elapsed_ms = 0u128;
        let mut plan_branch_lookup_elapsed_ms = 0u128;
        let mut resumed = 0u32;

        for project in projects {
            let task_scan_started_at = Instant::now();
            let pending_tasks = match self
                .task_repo
                .get_by_status_with_metadata_bool(
                    &project.id,
                    InternalStatus::Merged,
                    "pending_cleanup",
                )
                .await
            {
                Ok(tasks) => tasks,
                Err(e) => {
                    task_scan_elapsed_ms += task_scan_started_at.elapsed().as_millis();
                    tracing::warn!(
                        error = %e,
                        project_id = project.id.as_str(),
                        "Failed to fetch pending-cleanup Merged tasks for startup cleanup resumption"
                    );
                    continue;
                }
            };
            task_scan_elapsed_ms += task_scan_started_at.elapsed().as_millis();
            cleanup_query_tasks_seen += pending_tasks.len();

            for task in pending_tasks {
                pending_cleanup_tasks += 1;

                info!(
                    task_id = task.id.as_str(),
                    task_branch = ?task.task_branch,
                    worktree_path = ?task.worktree_path,
                    "Phase 0.5: Resuming deferred cleanup for Merged task"
                );

                // Fire-and-forget: don't block startup on cleanup
                let task_repo = Arc::clone(&self.task_repo);
                let working_dir = project.working_directory.clone();
                let task_id = task.id.clone();
                let task_branch = task.task_branch.clone();
                let worktree_path = task.worktree_path.clone();
                // Resolve plan branch for ancestor guard
                let plan_branch_lookup_started_at = Instant::now();
                let plan_branch = if let (Some(ref exec_plan_id), Some(ref pb_repo)) =
                    (&task.execution_plan_id, &self.plan_branch_repo)
                {
                    pb_repo
                        .get_by_execution_plan_id(exec_plan_id)
                        .await
                        .ok()
                        .flatten()
                        .map(|pb| pb.branch_name)
                } else {
                    None
                };
                plan_branch_lookup_elapsed_ms +=
                    plan_branch_lookup_started_at.elapsed().as_millis();
                tokio::spawn(async move {
                    deferred_merge_cleanup(
                        task_id,
                        task_repo,
                        working_dir,
                        task_branch,
                        worktree_path,
                        plan_branch,
                    )
                    .await;
                });

                resumed += 1;
            }
        }

        info!(
            projects_seen = projects.len(),
            cleanup_query_tasks_seen,
            pending_cleanup_tasks,
            cleanup_jobs_spawned = resumed,
            task_scan_elapsed_ms,
            plan_branch_lookup_elapsed_ms,
            elapsed_ms = started_at.elapsed().as_millis(),
            "Phase 0.5: Pending cleanup startup scan completed"
        );
    }

    /// Repair task worktree paths for non-merge statuses on startup.
    ///
    /// In Worktree mode, task/review agents must never run in `project.working_directory`.
    /// If `worktree_path` is stale/missing/mis-pointed, recreate expected task worktree path.
    async fn repair_non_merge_task_worktrees(&self, projects: &[crate::domain::entities::Project]) {
        const REPAIR_STATUSES: &[InternalStatus] = &[
            InternalStatus::Executing,
            InternalStatus::QaRefining,
            InternalStatus::QaTesting,
            InternalStatus::Reviewing,
            InternalStatus::ReExecuting,
            InternalStatus::PendingReview,
            InternalStatus::RevisionNeeded,
        ];

        for project in projects {
            if !project.is_worktree() {
                continue;
            }

            let repo_path = Path::new(&project.working_directory);
            if !crate::utils::path_safety::checked_exists(repo_path, "project repository")
                .unwrap_or(false)
            {
                continue;
            }

            for status in REPAIR_STATUSES {
                let tasks = match self.task_repo.get_by_status(&project.id, *status).await {
                    Ok(tasks) => tasks,
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            project_id = project.id.as_str(),
                            status = ?status,
                            "Failed to load tasks for startup worktree repair"
                        );
                        continue;
                    }
                };

                for mut task in tasks {
                    let Some(task_branch) = task.task_branch.clone() else {
                        continue;
                    };

                    let expected = project.task_worktree_path(task.id.as_str());
                    let expected_exists = crate::utils::path_safety::checked_exists(
                        &expected,
                        "expected task worktree",
                    )
                    .unwrap_or(false);

                    let current = task.worktree_path.as_ref().map(std::path::PathBuf::from);
                    let current_exists = current
                        .as_ref()
                        .map(|p| {
                            crate::utils::path_safety::checked_exists(p, "stored task worktree")
                                .unwrap_or(false)
                        })
                        .unwrap_or(false);

                    let current_merge_like = current
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .and_then(|n| n.to_str())
                        .map(|name| {
                            name.starts_with("merge-")
                                || name.starts_with("rebase-")
                                || name.starts_with("plan-update-")
                                || name.starts_with("source-update-")
                        })
                        .unwrap_or(false);

                    let needs_reset = current.is_none()
                        || !current_exists
                        || current_merge_like
                        || current.as_ref().map(|p| p != &expected).unwrap_or(true);

                    if !needs_reset {
                        continue;
                    }

                    if !expected_exists {
                        if let Err(e) = GitService::checkout_existing_branch_worktree(
                            repo_path,
                            &expected,
                            &task_branch,
                        )
                        .await
                        {
                            tracing::warn!(
                                task_id = task.id.as_str(),
                                status = ?task.internal_status,
                                branch = task_branch.as_str(),
                                expected_worktree = %expected.display(),
                                error = %e,
                                "Startup worktree repair failed; task will remain blocked from unsafe main-repo fallback"
                            );
                            task.worktree_path = None;
                            task.touch();
                            if let Err(update_err) = self.task_repo.update(&task).await {
                                tracing::warn!(
                                    task_id = task.id.as_str(),
                                    error = %update_err,
                                    "Failed to persist cleared worktree_path after repair failure"
                                );
                            }
                            continue;
                        }
                    }

                    task.worktree_path = Some(expected.to_string_lossy().to_string());
                    task.touch();
                    if let Err(e) = self.task_repo.update(&task).await {
                        tracing::warn!(
                            task_id = task.id.as_str(),
                            expected_worktree = %expected.display(),
                            error = %e,
                            "Failed to persist repaired task worktree_path"
                        );
                    } else {
                        tracing::info!(
                            task_id = task.id.as_str(),
                            status = ?task.internal_status,
                            worktree_path = %expected.display(),
                            "Startup repaired non-merge task worktree_path"
                        );
                    }
                }
            }
        }
    }

    async fn remediate_commit_hook_merge_incomplete_tasks(
        &self,
        projects: &[crate::domain::entities::Project],
    ) {
        for project in projects {
            let tasks = match self
                .task_repo
                .get_by_status(&project.id, InternalStatus::MergeIncomplete)
                .await
            {
                Ok(tasks) => tasks,
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        project_id = project.id.as_str(),
                        "Failed to load MergeIncomplete tasks for commit-hook startup remediation"
                    );
                    continue;
                }
            };

            for task in tasks {
                if task.archived_at.is_some()
                    || !crate::domain::state_machine::transition_handler::task_has_commit_hook_merge_failure(&task)
                {
                    continue;
                }

                tracing::info!(
                    task_id = task.id.as_str(),
                    "Phase 0.9: Inspecting legacy commit-hook MergeIncomplete task"
                );

                if let Some(error) =
                    crate::domain::state_machine::transition_handler::extract_commit_hook_merge_error(
                        &task,
                    )
                {
                    let kind =
                        crate::domain::state_machine::transition_handler::classify_commit_hook_failure_text(
                            &error,
                        );
                    let fingerprint =
                        crate::domain::state_machine::transition_handler::commit_hook_failure_fingerprint(
                            &error,
                        );
                    let repeated =
                        crate::domain::state_machine::transition_handler::is_repeated_commit_hook_failure(
                            &task,
                            &fingerprint,
                        );

                    if matches!(
                        kind,
                        crate::domain::state_machine::transition_handler::CommitHookFailureKind::EnvironmentFailure
                    ) || repeated
                    {
                        tracing::info!(
                            task_id = task.id.as_str(),
                            kind = kind.as_str(),
                            repeated,
                            "Phase 0.9: Marking commit-hook MergeIncomplete task as blocked"
                        );
                        if let Err(e) = self
                            .transition_service
                            .mark_commit_hook_merge_failure_blocked(
                                &task.id,
                                Some(error),
                                "startup_recovery",
                            )
                            .await
                        {
                            tracing::warn!(
                                task_id = task.id.as_str(),
                                error = %e,
                                "Phase 0.9: Failed to mark commit-hook MergeIncomplete task as blocked"
                            );
                        }
                        continue;
                    }
                }

                tracing::info!(
                    task_id = task.id.as_str(),
                    "Phase 0.9: Rerouting legacy commit-hook MergeIncomplete task into revision flow"
                );

                if let Err(e) = self
                    .transition_service
                    .reroute_commit_hook_merge_failure(&task.id, None, false, "startup_recovery")
                    .await
                {
                    tracing::warn!(
                        task_id = task.id.as_str(),
                        error = %e,
                        "Phase 0.9: Failed to reroute commit-hook MergeIncomplete task"
                    );
                }
            }
        }
    }

    /// Check if a task is waiting for global idle (no agents running) before retrying.
    ///
    /// Used for main-merge-deferred tasks that should not be resumed on startup
    /// when agents are still running. Returns true only if:
    /// - Task has `main_merge_deferred` metadata flag set
    /// - There are agents currently running (running_count > 0)
    #[doc(hidden)]
    pub fn is_waiting_for_global_idle(
        task: &crate::domain::entities::Task,
        running_count: u32,
    ) -> bool {
        use crate::domain::state_machine::transition_handler::has_main_merge_deferred_metadata;

        has_main_merge_deferred_metadata(task) && running_count > 0
    }

    /// Phase 4: Drain deferred pending ideation sessions on startup.
    ///
    /// Queries all projects with sessions that have `pending_initial_prompt` set
    /// (deferred due to capacity limits at session creation time) and attempts to
    /// launch them now that the app is restarting with fresh capacity.
    ///
    /// Outcome of each drain attempt (spawned / skipped / re-persisted) is logged
    /// by `PendingSessionDrainService` internally.
    async fn drain_pending_ideation_sessions(&self) {
        let chat_service = match self.chat_service.as_ref() {
            Some(cs) => Arc::clone(cs),
            None => {
                debug!("Startup pending drain: no chat service configured, skipping");
                return;
            }
        };

        let project_ids = match self
            .ideation_session_repo
            .list_projects_with_pending_sessions()
            .await
        {
            Ok(ids) => ids,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "Startup pending drain: failed to query projects with pending sessions"
                );
                return;
            }
        };

        if project_ids.is_empty() {
            debug!("Startup pending drain: no projects with pending sessions found");
            return;
        }

        info!(
            count = project_ids.len(),
            "Startup pending drain: found projects with pending sessions"
        );

        let drain_service =
            crate::application::pending_session_drain::PendingSessionDrainService::new(
                Arc::clone(&self.ideation_session_repo),
                Arc::clone(&self.project_repo),
                Arc::clone(&self.task_repo),
                Arc::clone(&self.chat_conversation_repo),
                Arc::clone(&self.execution_settings_repo),
                Arc::clone(&self.execution_state),
                Arc::clone(&self.running_agent_registry),
                Arc::clone(&self.reconciler.message_queue),
                chat_service,
            );

        for project_id in &project_ids {
            info!(
                project_id = %project_id,
                "Startup pending drain: draining pending sessions for project"
            );
            drain_service
                .try_drain_pending_for_project(project_id)
                .await;
        }

        info!(
            count = project_ids.len(),
            "Startup pending drain: drain complete"
        );
    }

    /// Restart only exact accepted Plan-mode continuations through ChatService's
    /// canonical queued-send seam. Malformed durable rows and uncertain ownership
    /// are intentionally ignored so startup cannot launch arbitrary messages.
    async fn recover_accepted_plan_mode_handoffs(&self) {
        let Some(app_handle) = self.app_handle.as_ref() else {
            debug!("Startup Plan-mode handoff recovery: no app handle configured");
            return;
        };
        let Some(state) = app_handle.try_state::<crate::application::AppState>() else {
            tracing::warn!("Startup Plan-mode handoff recovery: AppState is unavailable");
            return;
        };
        self.recover_accepted_plan_mode_handoffs_for_state(state.inner())
            .await;
    }

    async fn recover_accepted_plan_mode_handoffs_for_state(
        &self,
        state: &crate::application::AppState,
    ) {
        let Some(chat_service) = self.chat_service.as_ref().cloned() else {
            debug!("Startup Plan-mode handoff recovery: no chat service configured");
            return;
        };
        let queued_message_repo = Arc::clone(&state.queued_message_repo);
        let chat_conversation_repo = Arc::clone(&state.chat_conversation_repo);
        let workspace_repo = Arc::clone(&state.agent_conversation_workspace_repo);
        let ideation_session_repo = Arc::clone(&state.ideation_session_repo);
        let running_agent_registry = Arc::clone(&state.running_agent_registry);
        let interactive_process_registry = Arc::clone(&state.interactive_process_registry);

        let running_project_owners = match running_agent_registry
            .list_by_context_type(&ChatContextType::Project.to_string())
            .await
        {
            Ok(owners) => owners,
            Err(error) => {
                tracing::warn!(error = %error, "Startup Plan-mode handoff recovery: running-registry read failed; recovery skipped");
                return;
            }
        };

        let keys = match queued_message_repo.list_keys().await {
            Ok(keys) => keys,
            Err(error) => {
                tracing::warn!(error = %error, "Startup Plan-mode handoff recovery: queue-key read failed");
                return;
            }
        };

        for key in keys
            .into_iter()
            .filter(|key| key.context_type == ChatContextType::Project)
        {
            let messages = match queued_message_repo.list(&key).await {
                Ok(messages) => messages,
                Err(error) => {
                    tracing::warn!(context_id = %key.context_id, error = %error, "Startup Plan-mode handoff recovery: queue-row read failed");
                    continue;
                }
            };
            let conversation_id = ChatConversationId::from_string(key.context_id.clone());

            for message in messages
                .iter()
                .filter(|message| is_accepted_plan_mode_handoff_row(&key, message))
            {
                match chat_conversation_repo.get_by_id(&conversation_id).await {
                    Ok(Some(conversation))
                        if conversation.context_type == ChatContextType::Project
                            && conversation.agent_mode
                                == Some(AgentConversationWorkspaceMode::Plan) => {}
                    Ok(_) => continue,
                    Err(error) => {
                        tracing::warn!(conversation_id = %conversation_id, error = %error, "Startup Plan-mode handoff recovery: conversation read failed");
                        continue;
                    }
                }
                let workspace = match workspace_repo
                    .get_by_conversation_id(&conversation_id)
                    .await
                {
                    Ok(Some(workspace))
                        if workspace.mode == AgentConversationWorkspaceMode::Plan =>
                    {
                        workspace
                    }
                    Ok(_) => continue,
                    Err(error) => {
                        tracing::warn!(conversation_id = %conversation_id, error = %error, "Startup Plan-mode handoff recovery: workspace read failed");
                        continue;
                    }
                };
                let Some(session_id) = workspace.linked_ideation_session_id else {
                    continue;
                };
                match ideation_session_repo.get_by_id(&session_id).await {
                    Ok(Some(session))
                        if session.session_flow == IdeationSessionFlow::Planning
                            && session.status == IdeationSessionStatus::Active => {}
                    Ok(_) => continue,
                    Err(error) => {
                        tracing::warn!(conversation_id = %conversation_id, error = %error, "Startup Plan-mode handoff recovery: linked session read failed");
                        continue;
                    }
                }

                let running_key = RunningAgentKey::new(
                    ChatContextType::Project.to_string(),
                    conversation_id.as_str(),
                );
                if running_project_owners
                    .iter()
                    .any(|(key, _)| key == &running_key)
                    || interactive_process_registry
                        .capture_owner(&crate::application::interactive_process_registry::InteractiveProcessKey::new(
                            ChatContextType::Project.to_string(),
                            conversation_id.as_str(),
                        ))
                        .await
                        .is_some()
                {
                    continue;
                }

                let outcome = chat_service
                    .kick_runtime_handoff(&conversation_id, &message.id)
                    .await;
                tracing::info!(
                    conversation_id = %conversation_id,
                    queued_message_id = %message.id,
                    outcome = ?outcome,
                    "Startup Plan-mode handoff recovery attempted canonical queue kick"
                );
            }
        }
    }
}

#[cfg(test)]
#[path = "startup_jobs_tests.rs"]
mod tests;
