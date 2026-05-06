// Task Transition Service
//
// Orchestrates task status transitions with proper state machine entry/exit actions.
// This service bridges the gap between simple status updates and the full state machine
// that triggers side effects like spawning worker agents.
//
// Key responsibilities:
// - Build TaskServices from AppState dependencies
// - Handle status transitions with proper entry actions
// - Spawn workers when moving to Executing state
// - Emit events for UI updates

use async_trait::async_trait;
use std::collections::HashSet;
use std::future::Future;
use std::panic::Location;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::application::agent_client_bundle::{AgentClientBundle, AgentClientFactoryBundle};
use crate::application::runtime_factory::{
    build_chat_service_with_fallback, ChatRuntimeFactoryDeps,
};
use crate::application::{
    AppChatService, AppState, ChatService, GitService, InteractiveProcessRegistry,
};
use crate::commands::ExecutionState;
use crate::domain::agents::{AgentHarnessKind, AgenticClient};
use crate::domain::entities::task_metadata::GIT_ISOLATION_ERROR_PREFIX;
use crate::domain::entities::{
    ExecutionFailureSource, ExecutionRecoveryEvent, ExecutionRecoveryEventKind,
    ExecutionRecoveryMetadata, ExecutionRecoveryReasonCode, ExecutionRecoverySource,
    InternalStatus, ReviewNote, ReviewOutcome, ReviewerType, Task, TaskCategory, TaskId,
};
use crate::domain::repositories::{
    ActivityEventRepository, AgentLaneSettingsRepository, AgentRunRepository, ArtifactRepository,
    ChatAttachmentRepository, ChatConversationRepository, ChatMessageRepository,
    ExecutionSettingsRepository, ExternalEventsRepository, IdeationSessionRepository,
    MemoryEventRepository, PlanBranchRepository, ProjectRepository, ReviewRepository,
    TaskDependencyRepository, TaskRepository, TaskStepRepository,
};
use crate::domain::services::{
    github_service::{
        PrMergeStateStatus, PrMergeableState, PrReviewFeedback, PrStatus, PrSyncState,
    },
    payload_enrichment::{PresentationKind, WebhookPresentationContext},
    MessageQueue, PlanPrPublisher, PrReviewState, RunningAgentRegistry,
};
use crate::domain::state_machine::services::{
    AgentSpawner, DependencyManager, EventEmitter, Notifier, ReviewStartResult, ReviewStarter,
    TaskScheduler, WebhookPublisher,
};
use crate::domain::state_machine::transition_handler::metadata_builder::{
    build_stop_metadata, build_trigger_origin_metadata, MetadataUpdate,
};
use crate::domain::state_machine::transition_handler::set_trigger_origin;
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::spawner::AgenticClientSpawner;
use ralphx_domain::entities::EventType;

#[allow(clippy::too_many_arguments)]
fn build_transition_chat_service_fallback<R: Runtime>(
    chat_message_repo: Arc<dyn ChatMessageRepository>,
    chat_attachment_repo: Arc<dyn ChatAttachmentRepository>,
    conversation_repo: Arc<dyn ChatConversationRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    task_repo: Arc<dyn TaskRepository>,
    task_dep_repo: Arc<dyn TaskDependencyRepository>,
    ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    activity_event_repo: Arc<dyn ActivityEventRepository>,
    message_queue: Arc<MessageQueue>,
    running_agent_registry: Arc<dyn RunningAgentRegistry>,
    memory_event_repo: Arc<dyn MemoryEventRepository>,
    execution_state: Arc<ExecutionState>,
    app_handle: Option<AppHandle<R>>,
) -> AppChatService<R> {
    let deps = ChatRuntimeFactoryDeps::from_core(
        chat_message_repo,
        chat_attachment_repo,
        Arc::new(crate::infrastructure::memory::MemoryArtifactRepository::new()),
        conversation_repo,
        agent_run_repo,
        project_repo,
        task_repo,
        task_dep_repo,
        ideation_session_repo,
        activity_event_repo,
        message_queue,
        running_agent_registry,
        memory_event_repo,
    );

    build_chat_service_with_fallback(&app_handle, Some(execution_state), &deps)
}

fn github_pr_review_feedback_title(pr_number: i64) -> String {
    format!("Address GitHub PR #{pr_number} review feedback")
}

fn format_github_pr_review_feedback(pr_number: i64, feedback: &PrReviewFeedback) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "GitHub PR #{pr_number} requested changes from @{}.\n",
        feedback.author
    ));
    if let Some(submitted_at) = feedback.submitted_at.as_deref() {
        out.push_str(&format!("Submitted: {submitted_at}\n"));
    }
    out.push_str(&format!("GitHub review id: {}\n", feedback.review_id));

    if let Some(body) = feedback
        .body
        .as_deref()
        .filter(|body| !body.trim().is_empty())
    {
        out.push_str("\nReview body:\n");
        out.push_str(body.trim());
        out.push('\n');
    }

    if !feedback.comments.is_empty() {
        out.push_str("\nInline comments:\n");
        for comment in &feedback.comments {
            let location = match (comment.path.as_deref(), comment.line) {
                (Some(path), Some(line)) => format!("{path}:{line}"),
                (Some(path), None) => path.to_string(),
                (None, Some(line)) => format!("line {line}"),
                (None, None) => "inline comment".to_string(),
            };
            out.push_str(&format!(
                "- @{} on {}: {}\n",
                comment.author,
                location,
                comment.body.trim()
            ));
        }
    }

    out
}

fn task_metadata_value(task: &Task) -> Option<serde_json::Value> {
    task.metadata
        .as_deref()
        .and_then(|metadata| serde_json::from_str::<serde_json::Value>(metadata).ok())
}

fn is_github_pr_review_correction_task(
    task: &Task,
    merge_task_id: &TaskId,
    review_id: &str,
) -> bool {
    let Some(metadata) = task_metadata_value(task) else {
        return false;
    };

    let correction_kind_matches = metadata
        .get("github_pr_review_correction")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let source_task_matches = metadata
        .get("github_pr_correction_for_task_id")
        .and_then(|value| value.as_str())
        == Some(merge_task_id.as_str());
    let review_matches = metadata
        .get("github_pr_review_id")
        .and_then(|value| value.as_str())
        == Some(review_id);

    correction_kind_matches && source_task_matches && review_matches
}

// ============================================================================
// No-op service implementations (for services not yet fully implemented)
// ============================================================================

/// EventEmitter - emits events to Tauri app handle when available.
///
/// Dual-emits: fires the Tauri frontend event AND writes to the `external_events`
/// table so external consumers (poll/SSE) can observe all state transitions.
pub struct TauriEventEmitter<R: Runtime = tauri::Wry> {
    app_handle: Option<AppHandle<R>>,
    /// Optional external events repo for dual-emit to DB.
    external_events_repo: Option<Arc<dyn ExternalEventsRepository>>,
    /// Optional task repo for resolving project_id and task details during enrichment.
    task_repo_for_emit: Option<Arc<dyn TaskRepository>>,
    /// Optional project repo for resolving project_name during enrichment.
    project_repo_for_emit: Option<Arc<dyn ProjectRepository>>,
    /// Optional ideation session repo for resolving session_title during enrichment.
    ideation_session_repo_for_emit: Option<Arc<dyn IdeationSessionRepository>>,
    /// Optional webhook publisher for triple-emit (Tauri + DB + webhooks).
    webhook_publisher: Option<Arc<dyn WebhookPublisher>>,
}

impl<R: Runtime> TauriEventEmitter<R> {
    pub fn new(app_handle: Option<AppHandle<R>>) -> Self {
        Self {
            app_handle,
            external_events_repo: None,
            task_repo_for_emit: None,
            project_repo_for_emit: None,
            ideation_session_repo_for_emit: None,
            webhook_publisher: None,
        }
    }

    /// Attach external events repository and repos for enriched dual-emit.
    pub fn with_external_events(
        mut self,
        external_events_repo: Arc<dyn ExternalEventsRepository>,
        task_repo: Arc<dyn TaskRepository>,
        project_repo: Arc<dyn ProjectRepository>,
        ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    ) -> Self {
        self.external_events_repo = Some(external_events_repo);
        self = self.with_enrichment_repos(task_repo, project_repo, ideation_session_repo);
        self
    }

    /// Attach lookup repos used to enrich UI status-change payloads.
    ///
    /// This must work even when external event persistence is not configured; otherwise
    /// live Tauri status-change emits can be skipped and the Kanban board can retain
    /// stale task statuses.
    pub fn with_enrichment_repos(
        mut self,
        task_repo: Arc<dyn TaskRepository>,
        project_repo: Arc<dyn ProjectRepository>,
        ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    ) -> Self {
        self.task_repo_for_emit = Some(task_repo);
        self.project_repo_for_emit = Some(project_repo);
        self.ideation_session_repo_for_emit = Some(ideation_session_repo);
        self
    }

    /// Attach webhook publisher for triple-emit (Tauri + DB + webhooks).
    pub fn with_webhook_publisher(mut self, publisher: Arc<dyn WebhookPublisher>) -> Self {
        self.webhook_publisher = Some(publisher);
        self
    }

    /// Build a fully enriched status-change payload by resolving task → project → session.
    ///
    /// Returns `Some((project_id, payload))` with `project_name`, `session_title`, `task_title`,
    /// and `presentation_kind` injected when task/project/session lookups succeed.
    /// Returns `None` (with a `warn!` log) when the task cannot be found — callers must skip
    /// all sinks in that case. Project and session lookup failures degrade gracefully to `None`
    /// fields in the payload (non-fatal).
    ///
    /// Used by `emit_status_change` to build a single enriched payload for all three sinks.
    pub(crate) async fn build_enriched_payload(
        &self,
        task_id: &str,
        old_status: &str,
        new_status: &str,
    ) -> Option<(String, serde_json::Value)> {
        let task_repo = self.task_repo_for_emit.as_ref()?;
        let tid = crate::domain::entities::TaskId::from_string(task_id.to_string());
        let task = match task_repo.get_by_id(&tid).await.ok().flatten() {
            Some(t) => t,
            None => {
                tracing::warn!(
                    task_id = task_id,
                    "build_enriched_payload: task not found — skipping enrichment"
                );
                return None;
            }
        };

        let project_id = task.project_id.to_string();
        let task_title = Some(task.title.clone());

        let project_name = if let Some(repo) = &self.project_repo_for_emit {
            let result = repo
                .get_by_id(&task.project_id)
                .await
                .ok()
                .flatten()
                .map(|p| p.name);
            if result.is_none() {
                tracing::debug!(
                    project_id = %task.project_id,
                    "build_enriched_payload: project not found — project_name omitted"
                );
            }
            result
        } else {
            None
        };

        let session_title = if let (Some(sid), Some(repo)) = (
            task.ideation_session_id.as_ref(),
            self.ideation_session_repo_for_emit.as_ref(),
        ) {
            let result = repo
                .get_by_id(sid)
                .await
                .ok()
                .flatten()
                .and_then(|s| s.title);
            if result.is_none() {
                tracing::debug!(
                    "build_enriched_payload: session not found or title None — session_title omitted"
                );
            }
            result
        } else {
            None
        };

        let ctx = WebhookPresentationContext {
            project_name,
            session_title,
            task_title,
            presentation_kind: Some(PresentationKind::TaskStatusChanged),
        };

        let mut payload = serde_json::json!({
            "task_id": task_id,
            "project_id": project_id,
            "old_status": old_status,
            "new_status": new_status,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        ctx.inject_into(&mut payload);

        Some((project_id, payload))
    }

    /// Write a pre-built status-change event payload to the external_events table.
    async fn write_external_event(&self, project_id: &str, payload: serde_json::Value) {
        let Some(ref ext_repo) = self.external_events_repo else {
            return;
        };

        let payload_str = payload.to_string();
        if let Err(e) = ext_repo
            .insert_event("task:status_changed", project_id, &payload_str)
            .await
        {
            tracing::warn!(
                error = %e,
                project_id = project_id,
                "write_external_event: failed to insert into external_events (non-fatal)"
            );
        }
    }
}

#[async_trait]
impl<R: Runtime> EventEmitter for TauriEventEmitter<R> {
    async fn emit(&self, event_type: &str, task_id: &str) {
        if let Some(ref handle) = self.app_handle {
            let payload = serde_json::json!({
                "taskId": task_id,
                "timestamp": chrono::Utc::now().to_rfc3339(),
            });
            if crate::application::ThrottledEmitter::<R>::is_batchable(event_type) {
                if let Some(throttled) =
                    handle.try_state::<std::sync::Arc<crate::application::ThrottledEmitter>>()
                {
                    throttled.emit(event_type, payload);
                    return;
                }
            }
            let _ = handle.emit(event_type, payload);
        }
    }

    async fn emit_with_payload(&self, event_type: &str, task_id: &str, payload: &str) {
        if let Some(ref handle) = self.app_handle {
            let _ = handle.emit(
                event_type,
                serde_json::json!({
                    "taskId": task_id,
                    "payload": payload,
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                }),
            );
        }
    }

    async fn emit_status_change(&self, task_id: &str, old_status: &str, new_status: &str) {
        // Build enriched payload once; skip all sinks if task not found.
        let (project_id, payload) = match self
            .build_enriched_payload(task_id, old_status, new_status)
            .await
        {
            Some(pair) => pair,
            None => {
                tracing::warn!(
                    task_id = task_id,
                    "emit_status_change: build_enriched_payload returned None — skipping all sinks"
                );
                return;
            }
        };

        // Sink 1: Tauri UI event.
        if let Some(ref handle) = self.app_handle {
            if let Some(throttled) =
                handle.try_state::<std::sync::Arc<crate::application::ThrottledEmitter>>()
            {
                throttled.emit("task:status_changed", payload.clone());
            } else {
                let _ = handle.emit("task:status_changed", payload.clone());
            }
        }

        // Sink 2: external_events DB table.
        self.write_external_event(&project_id, payload.clone())
            .await;

        // Sink 3: webhook publisher.
        if let Some(ref publisher) = self.webhook_publisher {
            publisher
                .publish(EventType::TaskStatusChanged, &project_id, payload)
                .await;
        }
    }
}

/// LoggingNotifier - logs notifications for debugging
pub struct LoggingNotifier;

#[async_trait]
impl Notifier for LoggingNotifier {
    async fn notify(&self, notification_type: &str, task_id: &str) {
        tracing::info!(
            task_id = task_id,
            notification_type = notification_type,
            "Notification"
        );
    }

    async fn notify_with_message(&self, notification_type: &str, task_id: &str, message: &str) {
        tracing::info!(
            task_id = task_id,
            notification_type = notification_type,
            message = message,
            "Notification with message"
        );
    }
}

/// Repository-backed DependencyManager for automatic task blocking/unblocking
///
/// When a task completes (enters Approved state), this manager:
/// 1. Finds all tasks that were blocked by the completed task
/// 2. For each blocked task, checks if ALL its blockers are now complete
/// 3. If all blockers complete, transitions the task from Blocked to Ready
/// 4. Emits task:unblocked event for UI updates
pub struct RepoBackedDependencyManager<R: Runtime = tauri::Wry> {
    task_dep_repo: Arc<dyn TaskDependencyRepository>,
    task_repo: Arc<dyn TaskRepository>,
    app_handle: Option<AppHandle<R>>,
}

impl<R: Runtime> RepoBackedDependencyManager<R> {
    pub fn new(
        task_dep_repo: Arc<dyn TaskDependencyRepository>,
        task_repo: Arc<dyn TaskRepository>,
        app_handle: Option<AppHandle<R>>,
    ) -> Self {
        Self {
            task_dep_repo,
            task_repo,
            app_handle,
        }
    }

    /// Check if a blocking task satisfies the dependency (no longer blocking dependents).
    /// Delegates to InternalStatus::is_dependency_satisfied() as the single source of truth.
    /// If task doesn't exist (deleted), consider it satisfied (not blocking).
    async fn is_blocker_complete(&self, blocker_id: &TaskId) -> bool {
        if let Ok(Some(task)) = self.task_repo.get_by_id(blocker_id).await {
            task.internal_status.is_dependency_satisfied()
        } else {
            // If task doesn't exist, consider it "complete" (not blocking)
            true
        }
    }

    /// Get names of incomplete blockers for a task (for blocked_reason message).
    /// Returns (waiting_names, failed_names) so callers can produce specific messages.
    async fn get_incomplete_blocker_names(&self, task_id: &TaskId) -> (Vec<String>, Vec<String>) {
        let blockers = match self.task_dep_repo.get_blockers(task_id).await {
            Ok(b) => b,
            Err(_) => return (Vec::new(), Vec::new()),
        };

        let mut waiting_names = Vec::new();
        let mut failed_names = Vec::new();
        for blocker_id in blockers {
            if let Ok(Some(task)) = self.task_repo.get_by_id(&blocker_id).await {
                match task.internal_status {
                    InternalStatus::Merged
                    | InternalStatus::Cancelled
                    | InternalStatus::Stopped
                    | InternalStatus::MergeIncomplete => {
                        // complete — not included
                    }
                    InternalStatus::Failed => {
                        failed_names.push(task.title);
                    }
                    _ => {
                        waiting_names.push(task.title);
                    }
                }
            }
        }
        (waiting_names, failed_names)
    }
}

#[async_trait]
impl<R: Runtime> DependencyManager for RepoBackedDependencyManager<R> {
    async fn unblock_dependents(&self, completed_task_id: &str) {
        let task_id = TaskId::from_string(completed_task_id.to_string());

        // Find all tasks that depend on the completed task
        let dependents = match self.task_dep_repo.get_blocked_by(&task_id).await {
            Ok(deps) => deps,
            Err(e) => {
                tracing::error!(error = %e, task_id = completed_task_id, "Failed to get dependents");
                return;
            }
        };

        tracing::info!(
            completed_task_id = completed_task_id,
            dependent_count = dependents.len(),
            "Checking dependents for unblocking"
        );

        for dependent_id in dependents {
            // Get all blockers for this dependent task
            let blockers = match self.task_dep_repo.get_blockers(&dependent_id).await {
                Ok(b) => b,
                Err(_) => continue,
            };

            // Check if ALL blockers are now complete
            let mut all_complete = true;
            for blocker_id in &blockers {
                if !self.is_blocker_complete(blocker_id).await {
                    all_complete = false;
                    break;
                }
            }

            // Get the dependent task
            let mut dependent_task = match self.task_repo.get_by_id(&dependent_id).await {
                Ok(Some(t)) => t,
                _ => continue,
            };

            if all_complete {
                // All blockers complete — transition Blocked → Ready using a direct DB update
                // (not TransitionHandler) to avoid recursive re-entry: we are already inside
                // on_enter(Merged) → unblock_dependents(). TransitionHandler would call
                // on_enter(Ready) which could trigger try_schedule_ready_tasks() mid-loop and
                // interact with the scheduler before all dependents are processed.
                //
                // Scheduling guarantee: on_enter(Merged) calls try_schedule_ready_tasks() via
                // tokio::spawn (600ms delay) AFTER this unblock_dependents() call completes,
                // so every task set to Ready here is guaranteed to be picked up by the
                // scheduler. See on_enter_states.rs State::Merged branch.
                if dependent_task.internal_status == InternalStatus::Blocked {
                    dependent_task.internal_status = InternalStatus::Ready;
                    dependent_task.blocked_reason = None;
                    dependent_task.touch();

                    if let Err(e) = self.task_repo.update(&dependent_task).await {
                        tracing::error!(error = %e, task_id = %dependent_id, "Failed to unblock task");
                        continue;
                    }

                    // Record state transition history for timeline visibility
                    if let Err(e) = self
                        .task_repo
                        .persist_status_change(
                            &dependent_id,
                            InternalStatus::Blocked,
                            InternalStatus::Ready,
                            "blockers_resolved",
                        )
                        .await
                    {
                        tracing::warn!(error = %e, task_id = %dependent_id, "Failed to record unblock transition (non-fatal)");
                    }

                    tracing::info!(
                        task_id = %dependent_id,
                        task_title = %dependent_task.title,
                        "Task unblocked - all blockers complete"
                    );

                    // Emit task:unblocked event for UI update
                    if let Some(ref handle) = self.app_handle {
                        let _ = handle.emit(
                            "task:unblocked",
                            serde_json::json!({
                                "taskId": dependent_id.as_str(),
                                "taskTitle": dependent_task.title,
                                "timestamp": chrono::Utc::now().to_rfc3339(),
                            }),
                        );
                    }
                }
            } else {
                // Some blockers still incomplete - update blocked_reason with remaining names
                let (waiting_names, failed_names) =
                    self.get_incomplete_blocker_names(&dependent_id).await;
                let new_reason = if !failed_names.is_empty() && waiting_names.is_empty() {
                    // All remaining blockers have failed
                    let names = failed_names
                        .iter()
                        .map(|n| format!("\"{}\"", n))
                        .collect::<Vec<_>>()
                        .join(", ");
                    if failed_names.len() == 1 {
                        format!("Dependency {} failed", names)
                    } else {
                        format!("Dependencies {} failed", names)
                    }
                } else if !failed_names.is_empty() {
                    // Mix of failed and still-running blockers
                    let failed = failed_names
                        .iter()
                        .map(|n| format!("\"{}\" (failed)", n))
                        .collect::<Vec<_>>();
                    let waiting = waiting_names
                        .iter()
                        .map(|n| format!("\"{}\"", n))
                        .collect::<Vec<_>>();
                    let all: Vec<String> = failed.into_iter().chain(waiting).collect();
                    format!("Waiting for: {}", all.join(", "))
                } else if !waiting_names.is_empty() {
                    format!("Waiting for: {}", waiting_names.join(", "))
                } else {
                    return;
                };
                if dependent_task.blocked_reason.as_ref() != Some(&new_reason) {
                    dependent_task.blocked_reason = Some(new_reason);
                    dependent_task.touch();
                    let _ = self.task_repo.update(&dependent_task).await;
                }
            }
        }
    }

    async fn has_unresolved_blockers(&self, task_id: &str) -> bool {
        let task_id = TaskId::from_string(task_id.to_string());
        let blockers = match self.task_dep_repo.get_blockers(&task_id).await {
            Ok(b) => b,
            Err(_) => return false,
        };

        for blocker_id in blockers {
            if !self.is_blocker_complete(&blocker_id).await {
                return true;
            }
        }
        false
    }

    async fn get_blocking_tasks(&self, task_id: &str) -> Vec<String> {
        let task_id = TaskId::from_string(task_id.to_string());
        match self.task_dep_repo.get_blockers(&task_id).await {
            Ok(blockers) => blockers
                .into_iter()
                .map(|id| id.as_str().to_string())
                .collect(),
            Err(_) => Vec::new(),
        }
    }
}

/// No-op ReviewStarter - placeholder until review system is wired
pub struct NoOpReviewStarter;

#[async_trait]
impl ReviewStarter for NoOpReviewStarter {
    async fn start_ai_review(&self, task_id: &str, _project_id: &str) -> ReviewStartResult {
        tracing::info!(task_id = task_id, "AI review would start here");
        // Return disabled for now - review system not fully wired
        ReviewStartResult::Disabled
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Convert InternalStatus to state machine State.
/// Used by execute_entry_actions and execute_exit_actions.
fn internal_status_to_state(
    status: InternalStatus,
) -> crate::domain::state_machine::machine::State {
    use crate::domain::state_machine::machine::State;
    match status {
        InternalStatus::Backlog => State::Backlog,
        InternalStatus::Ready => State::Ready,
        InternalStatus::Blocked => State::Blocked,
        InternalStatus::Executing => State::Executing,
        InternalStatus::QaRefining => State::QaRefining,
        InternalStatus::QaTesting => State::QaTesting,
        InternalStatus::QaPassed => State::QaPassed,
        InternalStatus::QaFailed => State::QaFailed(Default::default()),
        InternalStatus::PendingReview => State::PendingReview,
        InternalStatus::Reviewing => State::Reviewing,
        InternalStatus::ReviewPassed => State::ReviewPassed,
        InternalStatus::Escalated => State::Escalated,
        InternalStatus::RevisionNeeded => State::RevisionNeeded,
        InternalStatus::ReExecuting => State::ReExecuting,
        InternalStatus::Approved => State::Approved,
        InternalStatus::PendingMerge => State::PendingMerge,
        InternalStatus::Merging => State::Merging,
        InternalStatus::WaitingOnPr => State::WaitingOnPr,
        InternalStatus::MergeIncomplete => State::MergeIncomplete,
        InternalStatus::MergeConflict => State::MergeConflict,
        InternalStatus::Merged => State::Merged,
        InternalStatus::Failed => State::Failed(Default::default()),
        InternalStatus::Cancelled => State::Cancelled,
        InternalStatus::Paused => State::Paused,
        InternalStatus::Stopped => State::Stopped,
    }
}

/// Convert state machine State to InternalStatus.
/// Used for persisting auto-transitions to the database.
fn state_to_internal_status(
    state: &crate::domain::state_machine::machine::State,
) -> InternalStatus {
    use crate::domain::state_machine::machine::State;
    match state {
        State::Backlog => InternalStatus::Backlog,
        State::Ready => InternalStatus::Ready,
        State::Blocked => InternalStatus::Blocked,
        State::Executing => InternalStatus::Executing,
        State::QaRefining => InternalStatus::QaRefining,
        State::QaTesting => InternalStatus::QaTesting,
        State::QaPassed => InternalStatus::QaPassed,
        State::QaFailed(_) => InternalStatus::QaFailed,
        State::PendingReview => InternalStatus::PendingReview,
        State::Reviewing => InternalStatus::Reviewing,
        State::ReviewPassed => InternalStatus::ReviewPassed,
        State::Escalated => InternalStatus::Escalated,
        State::RevisionNeeded => InternalStatus::RevisionNeeded,
        State::ReExecuting => InternalStatus::ReExecuting,
        State::Approved => InternalStatus::Approved,
        State::PendingMerge => InternalStatus::PendingMerge,
        State::Merging => InternalStatus::Merging,
        State::WaitingOnPr => InternalStatus::WaitingOnPr,
        State::MergeIncomplete => InternalStatus::MergeIncomplete,
        State::MergeConflict => InternalStatus::MergeConflict,
        State::Merged => InternalStatus::Merged,
        State::Failed(_) => InternalStatus::Failed,
        State::Cancelled => InternalStatus::Cancelled,
        State::Paused => InternalStatus::Paused,
        State::Stopped => InternalStatus::Stopped,
    }
}

// ============================================================================
// Corrective transition helper types
// ============================================================================

/// Result returned by [`TaskTransitionService::apply_corrective_transition`].
pub(crate) struct CorrectionResult {
    /// The task in its post-update state (internal_status = target_status).
    pub task: Task,
    /// The status the task was in before the correction (captured from re-fetched task).
    pub from_status: InternalStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PrBranchFreshnessOutcome {
    NotApplicable,
    UpToDate,
    Updated,
    ConflictRouted,
}

fn pr_branch_freshness_task_eligible(task: &Task) -> bool {
    task.category == TaskCategory::PlanMerge
        && task.archived_at.is_none()
        && task.internal_status == InternalStatus::WaitingOnPr
        && !task.is_terminal()
}

fn pr_sync_state_requires_update(sync_state: &PrSyncState) -> bool {
    matches!(
        sync_state.merge_state_status,
        Some(PrMergeStateStatus::Behind)
    ) && !matches!(sync_state.mergeable, Some(PrMergeableState::Conflicting))
}

fn pr_sync_state_requires_conflict_resolution(sync_state: &PrSyncState) -> bool {
    matches!(
        sync_state.merge_state_status,
        Some(PrMergeStateStatus::Dirty)
    ) || matches!(sync_state.mergeable, Some(PrMergeableState::Conflicting))
}

fn remote_tracking_ref(base_branch: &str) -> String {
    if base_branch.starts_with("origin/") {
        base_branch.to_string()
    } else {
        format!("origin/{base_branch}")
    }
}

fn task_metadata_bool(task: &Task, key: &str) -> bool {
    task.metadata
        .as_deref()
        .and_then(|metadata| serde_json::from_str::<serde_json::Value>(metadata).ok())
        .and_then(|value| value.get(key)?.as_bool())
        .unwrap_or(false)
}

// ============================================================================
// TaskTransitionService
// ============================================================================

/// Service for orchestrating task status transitions with proper entry actions.
///
/// This service ensures that when a task's status changes (e.g., via Kanban drag-drop),
/// the appropriate side effects are triggered (e.g., spawning worker agents).
pub struct TaskTransitionService<R: Runtime = tauri::Wry> {
    task_repo: Arc<dyn TaskRepository>,
    task_dependency_repo: Arc<dyn TaskDependencyRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    chat_message_repo: Arc<dyn ChatMessageRepository>,
    chat_attachment_repo: Arc<dyn ChatAttachmentRepository>,
    conversation_repo: Arc<dyn ChatConversationRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    agent_spawner: Arc<dyn AgentSpawner>,
    agent_client_factories: AgentClientFactoryBundle,
    event_emitter: Arc<dyn EventEmitter>,
    notifier: Arc<dyn Notifier>,
    dependency_manager: Arc<dyn DependencyManager>,
    review_starter: Arc<dyn ReviewStarter>,
    chat_service: Arc<dyn ChatService>,
    message_queue: Arc<MessageQueue>,
    memory_event_repo: Arc<dyn MemoryEventRepository>,
    execution_state: Arc<ExecutionState>,
    _app_handle: Option<AppHandle<R>>,
    /// Task scheduler for auto-scheduling Ready tasks when slots are available.
    /// Passed to TaskServices so TransitionHandler can trigger scheduling on
    /// state exits and Ready state entry.
    task_scheduler: Option<Arc<dyn TaskScheduler>>,
    /// Plan branch repository for resolving feature branch targets.
    /// Passed to TaskServices so TransitionHandler can override merge targets.
    plan_branch_repo: Option<Arc<dyn PlanBranchRepository>>,

    /// Task step repository for updating step statuses.
    /// Passed to TaskServices so TransitionHandler can fail in-progress steps.
    step_repo: Option<Arc<dyn TaskStepRepository>>,

    /// Ideation session repository for fetching live session titles.
    /// Passed to TaskServices so TransitionHandler can build descriptive plan merge commit messages.
    ideation_session_repo: Option<Arc<dyn IdeationSessionRepository>>,
    /// Artifact repository for reading plan artifact markdown during PR creation/update.
    artifact_repo: Option<Arc<dyn ArtifactRepository>>,
    execution_settings_repo: Option<Arc<dyn ExecutionSettingsRepository>>,
    agent_lane_settings_repo: Option<Arc<dyn AgentLaneSettingsRepository>>,
    review_repo: Option<Arc<dyn ReviewRepository>>,

    /// Activity event repository for emitting merge pipeline audit events.
    /// Cloned before being passed to the app chat service so the transition handler also has access.
    activity_event_repo: Arc<dyn ActivityEventRepository>,

    /// Per-task team mode override. When `Some(true)`, the chat service uses
    /// team-mode agent names (e.g., orchestrator-execution instead of worker).
    /// `Some(false)` means solo was explicitly chosen (skip metadata fallback).
    /// `None` means unset — fall back to task metadata `agent_variant`.
    team_mode: Option<bool>,

    /// Shared InteractiveProcessRegistry from AppState.
    /// When set via `with_interactive_process_registry`, injected into the chat
    /// service so state-machine-spawned agents (execution/review/merge) register
    /// their stdin in the same registry that `send_agent_message` checks.
    /// `None` means the chat service uses its own private registry (test default).
    interactive_process_registry: Option<Arc<InteractiveProcessRegistry>>,

    /// Shared tokio mutex for the concurrent merge guard critical section.
    /// Serializes the check-and-set in the worktree-mode merge guard so two tasks
    /// cannot both read "no blocker" simultaneously (eliminates TOCTOU race).
    /// Shared across all `execute_entry_actions` calls from this service instance.
    merge_lock: Arc<tokio::sync::Mutex<()>>,

    /// Shared set of task IDs currently undergoing `attempt_programmatic_merge`.
    /// Prevents double-click / duplicate reconciliation from spawning two concurrent
    /// merge attempts for the same task (self-dedup).
    merges_in_flight: Arc<std::sync::Mutex<HashSet<String>>>,

    /// Task-keyed CancellationTokens for in-flight post-merge validations.
    /// Shared across all TaskServices instances so pre_merge_cleanup can cancel
    /// a running validation when a new merge attempt starts for the same task.
    validation_tokens: Arc<dashmap::DashMap<String, tokio_util::sync::CancellationToken>>,

    /// Running agent registry for rebuilding the internal spawner with runtime admission context.
    running_agent_registry: Arc<dyn RunningAgentRegistry>,

    /// External events repository for dual-emit of state changes.
    /// When set, every status change is also written to the external_events DB table
    /// so external consumers (poll/SSE) can observe transitions.
    external_events_repo: Option<Arc<dyn ExternalEventsRepository>>,

    /// PR poller registry for GitHub PR polling (AD18).
    /// Passed to TaskServices so state machine actions can start/stop polling.
    /// None disables PR integration.
    pr_poller_registry: Option<Arc<crate::application::PrPollerRegistry>>,

    /// GitHub service for PR operations (AD17).
    /// Passed to TaskServices so state machine actions can push branches and mark PRs ready.
    /// None disables PR-mode merge path.
    github_service: Option<Arc<dyn crate::domain::services::GithubServiceTrait>>,

    /// Webhook publisher for triple-emit (Tauri + DB + webhooks).
    /// Set via with_webhook_publisher_for_emitter(). Propagated to TaskServices
    /// via build_task_services_common() and to TauriEventEmitter via with_external_events_repo().
    webhook_publisher: Option<Arc<dyn WebhookPublisher>>,

    /// Shared per-session mutex map for serializing concurrent plan:delivered checks.
    /// Shared across all TaskServices instances produced by this service.
    /// ONE Arc shared between both Tauri IPC and HTTP server AppState paths.
    session_merge_locks: Arc<dashmap::DashMap<String, Arc<tokio::sync::Mutex<()>>>>,

    /// Self-referential Arc for passing to TaskServices (PR merge poller pattern).
    /// Set via `set_self_arc()` after Arc-wrapping. Uses Mutex + Any for runtime-generic storage.
    /// Used so `on_enter(Merging)` can pass `Arc<TaskTransitionService<Wry>>` to start_polling.
    self_arc: std::sync::Mutex<Option<Arc<dyn std::any::Any + Send + Sync>>>,
}

impl<R: Runtime> TaskTransitionService<R> {
    fn default_agent_client_factories() -> AgentClientFactoryBundle {
        AgentClientFactoryBundle::standard_production_runtime_factories()
    }

    fn rebuild_agent_spawner(&mut self) {
        self.agent_spawner = Self::build_agent_spawner(
            &self.agent_client_factories,
            Arc::clone(&self.task_repo),
            Arc::clone(&self.project_repo),
            Arc::clone(&self.execution_state),
            self.execution_settings_repo.as_ref().map(Arc::clone),
            self.agent_lane_settings_repo.as_ref().map(Arc::clone),
            Arc::clone(
                self.ideation_session_repo
                    .as_ref()
                    .expect("ideation_session_repo set in new"),
            ),
            Arc::clone(&self.running_agent_registry),
        );
    }

    fn build_agent_spawner(
        agent_client_factories: &AgentClientFactoryBundle,
        task_repo: Arc<dyn TaskRepository>,
        project_repo: Arc<dyn ProjectRepository>,
        execution_state: Arc<ExecutionState>,
        execution_settings_repo: Option<Arc<dyn ExecutionSettingsRepository>>,
        agent_lane_settings_repo: Option<Arc<dyn AgentLaneSettingsRepository>>,
        ideation_session_repo: Arc<dyn IdeationSessionRepository>,
        running_agent_registry: Arc<dyn RunningAgentRegistry>,
    ) -> Arc<dyn AgentSpawner> {
        let agent_clients = agent_client_factories.instantiate();
        let agent_client = Arc::clone(&agent_clients.default_client);
        let spawner = AgenticClientSpawner::new(agent_client)
            .with_default_harness(agent_clients.default_harness)
            .with_harness_clients(agent_clients.iter_explicit_harness_clients())
            .with_repos(Arc::clone(&task_repo), Arc::clone(&project_repo))
            .with_execution_state(Arc::clone(&execution_state));
        let spawner = if let (Some(execution_repo), Some(agent_lane_repo)) =
            (execution_settings_repo, agent_lane_settings_repo)
        {
            spawner.with_runtime_admission_context(
                execution_repo,
                agent_lane_repo,
                ideation_session_repo,
                running_agent_registry,
            )
        } else {
            spawner
        };
        Arc::new(spawner)
    }

    fn validate_status_transition(
        &self,
        task_id: &TaskId,
        from: InternalStatus,
        to: InternalStatus,
        caller: &'static Location<'static>,
    ) -> AppResult<()> {
        if from.can_transition_to(to) {
            return Ok(());
        }

        tracing::warn!(
            task_id = task_id.as_str(),
            from = from.as_str(),
            to = to.as_str(),
            caller_file = caller.file(),
            caller_line = caller.line(),
            caller_column = caller.column(),
            "Rejected invalid task status transition"
        );

        Err(AppError::InvalidTransition {
            from: from.as_str().to_string(),
            to: to.as_str().to_string(),
        })
    }

    fn rebuild_chat_service(&mut self) {
        if let Some(handle) = self._app_handle.as_ref() {
            if let Some(app_state) = handle.try_state::<AppState>() {
                self.chat_service = Arc::new(app_state.build_chat_service_for_runtime(
                    Some(Arc::clone(&self.execution_state)),
                    self._app_handle.clone(),
                ));
                return;
            }
        }

        let mut service = build_transition_chat_service_fallback(
            Arc::clone(&self.chat_message_repo),
            Arc::clone(&self.chat_attachment_repo),
            Arc::clone(&self.conversation_repo),
            Arc::clone(&self.agent_run_repo),
            Arc::clone(&self.project_repo),
            Arc::clone(&self.task_repo),
            Arc::clone(&self.task_dependency_repo),
            Arc::clone(
                self.ideation_session_repo
                    .as_ref()
                    .expect("ideation_session_repo set in new"),
            ),
            Arc::clone(&self.activity_event_repo),
            Arc::clone(&self.message_queue),
            Arc::clone(&self.running_agent_registry),
            Arc::clone(&self.memory_event_repo),
            Arc::clone(&self.execution_state),
            self._app_handle.clone(),
        );

        if let Some(repo) = self.execution_settings_repo.as_ref() {
            service = service.with_execution_settings_repo(Arc::clone(repo));
        }
        if let Some(repo) = self.agent_lane_settings_repo.as_ref() {
            service = service.with_agent_lane_settings_repo(Arc::clone(repo));
        }
        if let Some(repo) = self.plan_branch_repo.as_ref() {
            service = service.with_plan_branch_repo(Arc::clone(repo));
        }
        if let Some(ipr) = self.interactive_process_registry.as_ref() {
            service = service.with_interactive_process_registry(Arc::clone(ipr));
        }
        match self.team_mode {
            Some(explicit) => {
                service = service.with_team_mode(explicit);
            }
            None => {
                use crate::infrastructure::agents::claude::env_variant_override;
                if env_variant_override("execution").as_deref() == Some("team") {
                    service = service.with_team_mode(true);
                }
            }
        }

        self.chat_service = Arc::new(service);
    }

    /// Create a new TaskTransitionService with all required dependencies.
    pub fn new(
        task_repo: Arc<dyn TaskRepository>,
        task_dep_repo: Arc<dyn TaskDependencyRepository>,
        project_repo: Arc<dyn ProjectRepository>,
        chat_message_repo: Arc<dyn ChatMessageRepository>,
        chat_attachment_repo: Arc<dyn ChatAttachmentRepository>,
        conversation_repo: Arc<dyn ChatConversationRepository>,
        agent_run_repo: Arc<dyn AgentRunRepository>,
        ideation_session_repo: Arc<dyn IdeationSessionRepository>,
        activity_event_repo: Arc<dyn ActivityEventRepository>,
        message_queue: Arc<MessageQueue>,
        running_agent_registry: Arc<dyn RunningAgentRegistry>,
        execution_state: Arc<ExecutionState>,
        app_handle: Option<AppHandle<R>>,
        memory_event_repo: Arc<dyn MemoryEventRepository>,
    ) -> Self {
        let agent_client_factories = Self::default_agent_client_factories();
        let agent_spawner = Self::build_agent_spawner(
            &agent_client_factories,
            Arc::clone(&task_repo),
            Arc::clone(&project_repo),
            Arc::clone(&execution_state),
            None,
            None,
            Arc::clone(&ideation_session_repo),
            Arc::clone(&running_agent_registry),
        );

        // Clone activity_event_repo before consuming it in the chat service
        // so the transition handler can also use it for merge pipeline audit events.
        let activity_event_repo_for_services = Arc::clone(&activity_event_repo);

        // Create the unified chat service for worker spawning
        let chat_service: Arc<dyn ChatService> = {
            let mut service = if let Some(ref handle) = app_handle {
                if let Some(app_state) = handle.try_state::<AppState>() {
                    app_state.build_chat_service_for_runtime(
                        Some(Arc::clone(&execution_state)),
                        app_handle.clone(),
                    )
                } else {
                    build_transition_chat_service_fallback(
                        Arc::clone(&chat_message_repo),
                        Arc::clone(&chat_attachment_repo),
                        Arc::clone(&conversation_repo),
                        Arc::clone(&agent_run_repo),
                        Arc::clone(&project_repo),
                        Arc::clone(&task_repo),
                        Arc::clone(&task_dep_repo),
                        Arc::clone(&ideation_session_repo),
                        Arc::clone(&activity_event_repo),
                        Arc::clone(&message_queue),
                        Arc::clone(&running_agent_registry),
                        Arc::clone(&memory_event_repo),
                        Arc::clone(&execution_state),
                        Some(handle.clone()),
                    )
                }
            } else {
                build_transition_chat_service_fallback(
                    Arc::clone(&chat_message_repo),
                    Arc::clone(&chat_attachment_repo),
                    Arc::clone(&conversation_repo),
                    Arc::clone(&agent_run_repo),
                    Arc::clone(&project_repo),
                    Arc::clone(&task_repo),
                    Arc::clone(&task_dep_repo),
                    Arc::clone(&ideation_session_repo),
                    Arc::clone(&activity_event_repo),
                    Arc::clone(&message_queue),
                    Arc::clone(&running_agent_registry),
                    Arc::clone(&memory_event_repo),
                    Arc::clone(&execution_state),
                    None,
                )
            };
            // Global env var override: RALPHX_PROCESS_VARIANT_EXECUTION=team
            use crate::infrastructure::agents::claude::env_variant_override;
            if env_variant_override("execution").as_deref() == Some("team") {
                service = service.with_team_mode(true);
            }
            Arc::new(service)
        };

        // Create other services
        let event_emitter: Arc<dyn EventEmitter> = Arc::new(
            TauriEventEmitter::new(app_handle.clone()).with_enrichment_repos(
                Arc::clone(&task_repo),
                Arc::clone(&project_repo),
                Arc::clone(&ideation_session_repo),
            ),
        );
        let notifier: Arc<dyn Notifier> = Arc::new(LoggingNotifier);
        // Use real dependency manager for automatic blocking/unblocking based on dependency graph
        let dependency_manager: Arc<dyn DependencyManager> =
            Arc::new(RepoBackedDependencyManager::new(
                Arc::clone(&task_dep_repo),
                Arc::clone(&task_repo),
                app_handle.clone(),
            ));
        let review_starter: Arc<dyn ReviewStarter> = Arc::new(NoOpReviewStarter);

        Self {
            task_repo,
            task_dependency_repo: task_dep_repo,
            project_repo,
            chat_message_repo,
            chat_attachment_repo,
            conversation_repo,
            agent_run_repo,
            agent_spawner,
            agent_client_factories,
            event_emitter,
            notifier,
            dependency_manager,
            review_starter,
            chat_service,
            message_queue,
            memory_event_repo,
            execution_state,
            _app_handle: app_handle,
            task_scheduler: None,
            plan_branch_repo: None,
            step_repo: None,
            ideation_session_repo: Some(ideation_session_repo),
            artifact_repo: None,
            execution_settings_repo: None,
            agent_lane_settings_repo: None,
            review_repo: None,
            activity_event_repo: activity_event_repo_for_services,
            team_mode: None,
            interactive_process_registry: None,
            merge_lock: Arc::new(tokio::sync::Mutex::new(())),
            merges_in_flight: Arc::new(std::sync::Mutex::new(HashSet::new())),
            external_events_repo: None,
            validation_tokens: Arc::new(dashmap::DashMap::new()),
            running_agent_registry,
            pr_poller_registry: None,
            github_service: None,
            webhook_publisher: None,
            session_merge_locks: Arc::new(dashmap::DashMap::new()),
            self_arc: std::sync::Mutex::new(None),
        }
    }

    /// Set the self-arc for passing to TaskServices (PR merge poller — AD17).
    ///
    /// Call this immediately after `Arc::new(transition_service)`:
    /// ```ignore
    /// let svc = Arc::new(TaskTransitionService::new(...));
    /// svc.set_self_arc(Arc::clone(&svc));
    /// ```
    /// Only has effect when R: 'static (i.e., in production with Wry runtime).
    pub fn set_self_arc(&self, arc: Arc<TaskTransitionService<R>>)
    where
        R: 'static,
    {
        *self.self_arc.lock().unwrap() = Some(arc as Arc<dyn std::any::Any + Send + Sync>);
    }

    /// Wrap the service in an Arc and wire self_arc so PR-mode pollers can call back into it.
    pub fn into_arc(self) -> Arc<TaskTransitionService<R>>
    where
        R: 'static,
    {
        let arc = Arc::new(self);
        arc.set_self_arc(Arc::clone(&arc));
        arc
    }

    /// Set the task scheduler for auto-scheduling Ready tasks (builder pattern).
    ///
    /// When set, the scheduler is passed to TaskServices so that TransitionHandler
    /// can trigger scheduling when tasks exit agent-active states or enter Ready state.
    pub fn with_task_scheduler(mut self, scheduler: Arc<dyn TaskScheduler>) -> Self {
        self.task_scheduler = Some(scheduler);
        self
    }

    /// Set the plan branch repository for feature branch resolution (builder pattern).
    pub fn with_plan_branch_repo(mut self, repo: Arc<dyn PlanBranchRepository>) -> Self {
        self.chat_service.set_plan_branch_repo(Arc::clone(&repo));
        self.plan_branch_repo = Some(repo);
        self
    }

    /// Set the task step repository (builder pattern).
    pub fn with_step_repo(mut self, repo: Arc<dyn TaskStepRepository>) -> Self {
        self.step_repo = Some(repo);
        self
    }

    /// Set the artifact repository for richer PR metadata (builder pattern).
    pub fn with_artifact_repo(mut self, repo: Arc<dyn ArtifactRepository>) -> Self {
        self.artifact_repo = Some(repo);
        self
    }

    /// Set the review repository for system review-note persistence (builder pattern).
    pub fn with_review_repo(mut self, repo: Arc<dyn ReviewRepository>) -> Self {
        self.review_repo = Some(repo);
        self
    }

    /// Inject execution settings so the internal task-agent spawner can honor project caps.
    pub fn with_execution_settings_repo(
        mut self,
        repo: Arc<dyn ExecutionSettingsRepository>,
    ) -> Self {
        let app_agent_lane_settings_repo = self
            ._app_handle
            .as_ref()
            .and_then(|handle| handle.try_state::<AppState>())
            .map(|app_state| Arc::clone(&app_state.agent_lane_settings_repo));
        if let Some(agent_lane_settings_repo) = app_agent_lane_settings_repo.as_ref() {
            self.agent_lane_settings_repo = Some(Arc::clone(agent_lane_settings_repo));
        }

        self.execution_settings_repo = Some(Arc::clone(&repo));
        self.rebuild_chat_service();
        self.rebuild_agent_spawner();
        self
    }

    /// Inject provider-neutral lane settings so the transition chat/spawn paths can
    /// resolve Codex-vs-Claude behavior even when no AppState is available.
    pub fn with_agent_lane_settings_repo(
        mut self,
        repo: Arc<dyn AgentLaneSettingsRepository>,
    ) -> Self {
        self.agent_lane_settings_repo = Some(Arc::clone(&repo));
        self.rebuild_chat_service();
        self.rebuild_agent_spawner();
        self
    }

    /// Override the agentic client factory used by the state-machine spawner.
    ///
    /// Defaults to `ClaudeCodeClient`; tests and future harness integrations can inject
    /// another provider without changing the transition-service callsites.
    pub fn with_agentic_client_factory<F>(mut self, factory: F) -> Self
    where
        F: Fn() -> Arc<dyn AgenticClient> + Send + Sync + 'static,
    {
        self.agent_client_factories = self
            .agent_client_factories
            .clone()
            .with_default_factory(Arc::new(factory));
        self.rebuild_agent_spawner();
        self
    }

    /// Override the state-machine spawner with a concrete agentic client instance.
    ///
    /// This is a convenience wrapper for callers that already hold the client in AppState.
    pub fn with_agentic_client(self, client: Arc<dyn AgenticClient>) -> Self {
        self.with_agentic_client_factory(move || Arc::clone(&client))
    }

    /// Override the full harness client bundle used by the state-machine spawner.
    pub fn with_agent_clients(mut self, clients: AgentClientBundle) -> Self {
        self.agent_client_factories = AgentClientFactoryBundle::from_client_bundle(&clients);
        self.rebuild_agent_spawner();
        self
    }

    /// Override the agentic client factory used for a specific harness.
    pub fn with_harness_agentic_client_factory<F>(
        mut self,
        harness: AgentHarnessKind,
        factory: F,
    ) -> Self
    where
        F: Fn() -> Arc<dyn AgenticClient> + Send + Sync + 'static,
    {
        self.agent_client_factories = self
            .agent_client_factories
            .clone()
            .with_harness_factory(harness, Arc::new(factory));
        self.rebuild_agent_spawner();
        self
    }

    /// Override the state-machine spawner client with a concrete harness-specific instance.
    pub fn with_harness_agentic_client(
        self,
        harness: AgentHarnessKind,
        client: Arc<dyn AgenticClient>,
    ) -> Self {
        self.with_harness_agentic_client_factory(harness, move || Arc::clone(&client))
    }

    /// Enable team mode for agent spawning (builder pattern).
    ///
    /// When enabled, the chat service resolves to team-mode agent names
    /// (e.g., orchestrator-execution instead of worker).
    pub fn with_team_mode(mut self, team_mode: bool) -> Self {
        self.team_mode = Some(team_mode);
        self
    }

    /// Inject the shared AppState InteractiveProcessRegistry (builder pattern).
    ///
    /// When set, state-machine-spawned agents (execution/review/merge) register their
    /// stdin in this registry so that `send_agent_message` can deliver messages via
    /// the fast stdin path (Gate 1) instead of queuing them.
    pub fn with_interactive_process_registry(self, ipr: Arc<InteractiveProcessRegistry>) -> Self {
        self.chat_service
            .set_interactive_process_registry(Arc::clone(&ipr));
        // Store for downstream builders (e.g. TaskSchedulerService) to propagate
        let mut s = self;
        s.interactive_process_registry = Some(ipr);
        s
    }

    /// Attach an external events repository so every state transition is also written
    /// to the `external_events` DB table (dual-emit for poll/SSE consumers).
    pub fn with_external_events_repo(mut self, repo: Arc<dyn ExternalEventsRepository>) -> Self {
        // Rebuild the event emitter with external events + enrichment repos + optional webhook publisher.
        let ideation_session_repo = self
            .ideation_session_repo
            .as_ref()
            .expect("ideation_session_repo set in new()")
            .clone();
        let emitter = TauriEventEmitter::new(self._app_handle.clone()).with_external_events(
            Arc::clone(&repo),
            Arc::clone(&self.task_repo),
            Arc::clone(&self.project_repo),
            ideation_session_repo,
        );
        let emitter = if let Some(ref pub_) = self.webhook_publisher {
            emitter.with_webhook_publisher(Arc::clone(pub_))
        } else {
            emitter
        };
        self.event_emitter = Arc::new(emitter);
        self.external_events_repo = Some(repo);
        self
    }

    /// Attach PR poller registry for GitHub PR polling (builder pattern).
    pub fn with_pr_poller_registry(
        mut self,
        registry: Arc<crate::application::PrPollerRegistry>,
    ) -> Self {
        self.pr_poller_registry = Some(registry);
        self
    }

    /// Attach GitHub service for PR operations (builder pattern, AD17).
    pub fn with_github_service(
        mut self,
        svc: Arc<dyn crate::domain::services::GithubServiceTrait>,
    ) -> Self {
        self.github_service = Some(svc);
        self
    }

    /// Attach webhook publisher for triple-emit (builder pattern).
    ///
    /// Must be called BEFORE `with_external_events_repo()` — that method reads
    /// this field when rebuilding the event emitter.
    pub fn with_webhook_publisher_for_emitter(
        mut self,
        publisher: Arc<dyn WebhookPublisher>,
    ) -> Self {
        self.webhook_publisher = Some(publisher);
        self
    }

    /// Inject a shared session-merge-locks DashMap (builder pattern).
    ///
    /// Caller must share ONE Arc between both AppState instances (Tauri IPC + HTTP server)
    /// so concurrent plan:delivered checks from either path use the same per-session mutex.
    pub fn with_session_merge_locks(
        mut self,
        locks: Arc<dashmap::DashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    ) -> Self {
        self.session_merge_locks = locks;
        self
    }

    /// Transition a task to a new status, triggering appropriate entry actions.
    ///
    /// This is a backward-compatible wrapper around transition_task_with_metadata
    /// that passes None for the metadata parameter.
    ///
    /// # Arguments
    /// * `task_id` - The ID of the task to transition
    /// * `new_status` - The target status
    ///
    /// # Returns
    /// * `Ok(Task)` - The updated task with new status
    /// * `Err(AppError)` - If the task is not found or transition is invalid
    #[track_caller]
    pub fn transition_task<'a>(
        &'a self,
        task_id: &'a TaskId,
        new_status: InternalStatus,
    ) -> impl Future<Output = AppResult<Task>> + 'a {
        let caller = Location::caller();
        async move {
            self.transition_task_with_metadata_from_caller(task_id, new_status, None, caller)
                .await
        }
    }

    /// Transition a task to a new status with optional metadata update.
    ///
    /// This is the main entry point for status changes that should trigger side effects
    /// like spawning worker agents. Metadata updates are merged atomically with the
    /// status change.
    ///
    /// # Arguments
    /// * `task_id` - The ID of the task to transition
    /// * `new_status` - The target status
    /// * `metadata_update` - Optional metadata to merge into task.metadata
    ///
    /// # Returns
    /// * `Ok(Task)` - The updated task with new status and merged metadata
    /// * `Err(AppError)` - If the task is not found or transition is invalid
    #[track_caller]
    pub fn transition_task_with_metadata<'a>(
        &'a self,
        task_id: &'a TaskId,
        new_status: InternalStatus,
        metadata_update: Option<MetadataUpdate>,
    ) -> impl Future<Output = AppResult<Task>> + 'a {
        let caller = Location::caller();
        async move {
            self.transition_task_with_metadata_from_caller(
                task_id,
                new_status,
                metadata_update,
                caller,
            )
            .await
        }
    }

    async fn transition_task_with_metadata_from_caller(
        &self,
        task_id: &TaskId,
        new_status: InternalStatus,
        metadata_update: Option<MetadataUpdate>,
        caller: &'static Location<'static>,
    ) -> AppResult<Task> {
        tracing::debug!(
            task_id = task_id.as_str(),
            new_status = new_status.as_str(),
            caller_file = caller.file(),
            caller_line = caller.line(),
            caller_column = caller.column(),
            "Starting task transition"
        );

        // 1. Fetch the task
        let mut task =
            self.task_repo.get_by_id(task_id).await?.ok_or_else(|| {
                AppError::NotFound(format!("Task not found: {}", task_id.as_str()))
            })?;

        if task.archived_at.is_some() {
            return Err(AppError::Validation(format!(
                "Cannot transition archived task: {}",
                task_id.as_str()
            )));
        }

        let old_status = task.internal_status;
        tracing::debug!(
            old_status = old_status.as_str(),
            "Found task with current status"
        );

        // 2. If status is the same, no transition needed
        if old_status == new_status {
            tracing::debug!("Status unchanged, skipping transition");
            return Ok(task);
        }

        tracing::debug!(
            from = old_status.as_str(),
            to = new_status.as_str(),
            caller_file = caller.file(),
            caller_line = caller.line(),
            caller_column = caller.column(),
            "Transitioning task status"
        );

        self.validate_status_transition(task_id, old_status, new_status, caller)?;

        // 3. Update the task status
        task.internal_status = new_status;
        task.touch();

        // 3.1. Compute auto-metadata for QA transitions
        let auto_metadata = auto_metadata_for_status(new_status);

        // 3.2. Merge metadata updates (auto + explicit)
        if metadata_update.is_some() || auto_metadata.is_some() {
            // Prioritize explicit update, fallback to auto
            let final_update = metadata_update.or(auto_metadata);
            if let Some(update) = final_update {
                task.metadata = Some(update.merge_into(task.metadata.as_deref()));
            }
        }

        // 4. Persist the update with optimistic locking (WHERE id = ? AND status = old_status)
        //    If another caller already transitioned this task, rows_affected = 0 → no-op.
        let updated = self
            .task_repo
            .update_with_expected_status(&task, old_status)
            .await?;

        if !updated {
            tracing::info!(
                task_id = task_id.as_str(),
                from = old_status.as_str(),
                to = new_status.as_str(),
                caller_file = caller.file(),
                caller_line = caller.line(),
                caller_column = caller.column(),
                "Optimistic lock: task already transitioned by another caller, skipping"
            );
            // Re-fetch to return current state
            let current = self.task_repo.get_by_id(task_id).await?.ok_or_else(|| {
                AppError::NotFound(format!("Task not found: {}", task_id.as_str()))
            })?;
            return Ok(current);
        }

        // 4.1 Record state transition history for time-travel feature
        if let Err(e) = self
            .task_repo
            .persist_status_change(task_id, old_status, new_status, "system")
            .await
        {
            tracing::warn!(error = %e, "Failed to record state history (non-fatal)");
        }
        tracing::debug!("Task status persisted to database");

        // Log every confirmed state change at INFO level so the full state history is
        // visible in logs regardless of trigger type. Uses a distinct message from the
        // auto-transition log (~line 995) so grep can tell event-driven from timer-driven.
        tracing::info!(
            task_id = task_id.as_str(),
            from = old_status.as_str(),
            to = new_status.as_str(),
            caller_file = caller.file(),
            caller_line = caller.line(),
            caller_column = caller.column(),
            "Status transition confirmed"
        );

        // 5. Emit event for UI update
        if let Some(ref handle) = self._app_handle {
            let _ = handle.emit(
                "task:event",
                serde_json::json!({
                    "type": "status_changed",
                    "taskId": task_id.as_str(),
                    "from": old_status.as_str(),
                    "to": new_status.as_str(),
                    "changedBy": "user",
                }),
            );
            tracing::debug!("Emitted task:event status_changed");
        }
        self.event_emitter
            .emit_status_change(task_id.as_str(), old_status.as_str(), new_status.as_str())
            .await;

        // 6. Execute exit actions for the old status (e.g., decrement running count)
        tracing::debug!(
            old_status = old_status.as_str(),
            "Executing exit actions for old status"
        );
        self.execute_exit_actions(task_id, &task, old_status, new_status)
            .await;

        // 7. Execute entry actions for the new status
        tracing::debug!(
            new_status = new_status.as_str(),
            "Executing entry actions for new status"
        );
        self.execute_entry_actions(task_id, &task, new_status).await;

        tracing::debug!("Task transition complete");

        Ok(task)
    }

    /// Transition a task to Stopped status with context capture for smart resume.
    ///
    /// This method is specifically for stopping tasks mid-execution. It captures
    /// the current status and optional reason in the task's metadata, enabling
    /// the "smart resume" feature to restore context when the task is restarted.
    ///
    /// # Arguments
    /// * `task_id` - The ID of the task to stop
    /// * `from_status` - The status the task was in when stopped (captured for resume)
    /// * `reason` - Optional reason for stopping (captured for resume)
    ///
    /// # Returns
    /// * `Ok(Task)` - The stopped task with stop metadata
    /// * `Err(AppError)` - If the task is not found or transition is invalid
    #[track_caller]
    pub fn transition_to_stopped_with_context<'a>(
        &'a self,
        task_id: &'a TaskId,
        from_status: InternalStatus,
        reason: Option<String>,
    ) -> impl Future<Output = AppResult<Task>> + 'a {
        let caller = Location::caller();
        tracing::info!(
            task_id = task_id.as_str(),
            from_status = from_status.as_str(),
            reason = ?reason,
            "Stopping task with context capture"
        );

        // Build stop metadata
        let stop_metadata = build_stop_metadata(from_status, reason);

        // Transition to Stopped with metadata
        async move {
            self.transition_task_with_metadata_from_caller(
                task_id,
                InternalStatus::Stopped,
                Some(stop_metadata),
                caller,
            )
            .await
        }
    }

    async fn find_existing_github_pr_review_correction(
        &self,
        merge_task: &Task,
        review_id: &str,
    ) -> AppResult<Option<Task>> {
        let candidates = if let Some(execution_plan_id) = merge_task.execution_plan_id.as_ref() {
            self.task_repo
                .list_paginated(
                    &merge_task.project_id,
                    None,
                    0,
                    1000,
                    false,
                    None,
                    Some(execution_plan_id.as_str()),
                    None,
                )
                .await?
        } else if let Some(session_id) = merge_task.ideation_session_id.as_ref() {
            self.task_repo.get_by_ideation_session(session_id).await?
        } else {
            self.task_repo
                .get_by_project_filtered(&merge_task.project_id, false)
                .await?
        };

        Ok(candidates.into_iter().find(|candidate| {
            candidate.archived_at.is_none()
                && is_github_pr_review_correction_task(candidate, &merge_task.id, review_id)
        }))
    }

    async fn persist_github_pr_review_note_once(
        &self,
        correction_task: &Task,
        pr_number: i64,
        feedback: &PrReviewFeedback,
    ) {
        let Some(review_repo) = self.review_repo.as_ref() else {
            return;
        };

        match review_repo.get_notes_by_task_id(&correction_task.id).await {
            Ok(notes)
                if notes.iter().any(|note| {
                    note.notes
                        .as_deref()
                        .map(|notes| notes.contains(&feedback.review_id))
                        .unwrap_or(false)
                }) =>
            {
                return;
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(
                    task_id = correction_task.id.as_str(),
                    error = %error,
                    "Failed to inspect existing GitHub PR review notes"
                );
            }
        }

        let note_body = format_github_pr_review_feedback(pr_number, feedback);
        let note = ReviewNote::with_content(
            correction_task.id.clone(),
            ReviewerType::Human,
            ReviewOutcome::ChangesRequested,
            Some(format!(
                "GitHub PR #{pr_number} requested changes from @{}",
                feedback.author
            )),
            Some(note_body),
            None,
        );

        if let Err(error) = review_repo.add_note(&note).await {
            tracing::warn!(
                task_id = correction_task.id.as_str(),
                error = %error,
                "Failed to persist GitHub PR changes_requested review note"
            );
        }
    }

    /// Convert an actionable GitHub requested-changes review into normal plan work.
    ///
    /// The final plan merge task is system-managed, so it should not be re-executed as
    /// an implementation worker. Instead, RalphX creates one regular correction task on
    /// the same execution plan, blocks the final merge behind it, and lets the existing
    /// worker/review/merge pipeline bring the plan PR back to a reviewable state.
    #[track_caller]
    #[allow(clippy::manual_async_fn)]
    pub fn route_github_pr_changes_requested<'a>(
        &'a self,
        task_id: &'a TaskId,
        pr_number: i64,
        feedback: PrReviewFeedback,
        history_actor: &'a str,
    ) -> impl Future<Output = AppResult<Task>> + 'a {
        async move {
            let mut merge_task = self
                .task_repo
                .get_by_id(task_id)
                .await?
                .ok_or_else(|| AppError::TaskNotFound(task_id.as_str().to_string()))?;

            if merge_task.archived_at.is_some() {
                return Err(AppError::Validation(format!(
                    "Cannot route GitHub PR review for archived task {}",
                    task_id.as_str()
                )));
            }

            if merge_task.category != TaskCategory::PlanMerge {
                return Err(AppError::Conflict(format!(
                    "Task {} is not a plan merge task",
                    task_id.as_str()
                )));
            }

            let correction_task = match self
                .find_existing_github_pr_review_correction(&merge_task, &feedback.review_id)
                .await?
            {
                Some(existing) => existing,
                None => {
                    let mut correction = Task::new(
                        merge_task.project_id.clone(),
                        github_pr_review_feedback_title(pr_number),
                    );
                    correction.internal_status = InternalStatus::Ready;
                    correction.priority = merge_task.priority.saturating_add(1);
                    correction.description =
                        Some(format_github_pr_review_feedback(pr_number, &feedback));
                    correction.plan_artifact_id = merge_task.plan_artifact_id.clone();
                    correction.ideation_session_id = merge_task.ideation_session_id.clone();
                    correction.execution_plan_id = merge_task.execution_plan_id.clone();
                    correction.metadata = Some(
                        serde_json::json!({
                            "github_pr_review_correction": true,
                            "github_pr_number": pr_number,
                            "github_pr_review_id": feedback.review_id,
                            "github_pr_review_author": feedback.author,
                            "github_pr_review_submitted_at": feedback.submitted_at,
                            "github_pr_correction_for_task_id": merge_task.id.as_str(),
                        })
                        .to_string(),
                    );
                    self.task_repo.create(correction).await?
                }
            };

            if !self
                .task_dependency_repo
                .has_dependency(&merge_task.id, &correction_task.id)
                .await?
            {
                self.task_dependency_repo
                    .add_dependency(&merge_task.id, &correction_task.id)
                    .await?;
            }

            self.persist_github_pr_review_note_once(&correction_task, pr_number, &feedback)
                .await;

            crate::domain::state_machine::transition_handler::merge_metadata_into(
                &mut merge_task,
                &serde_json::json!({
                    "github_pr_review_id": feedback.review_id,
                    "github_pr_review_author": feedback.author,
                    "github_pr_review_pr_number": pr_number,
                    "github_pr_review_correction_task_id": correction_task.id.as_str(),
                    "github_pr_review_routed_at": chrono::Utc::now().to_rfc3339(),
                }),
            );
            self.task_repo.update(&merge_task).await?;

            if let Some(plan_branch_repo) = self.plan_branch_repo.as_ref() {
                if let Err(error) = plan_branch_repo.clear_polling_active_by_task(task_id).await {
                    tracing::warn!(
                        task_id = task_id.as_str(),
                        error = %error,
                        "Failed to clear PR polling after GitHub requested changes"
                    );
                }
            }

            let blocked_reason = format!(
                "GitHub PR #{pr_number} requested changes; waiting for correction task `{}`",
                correction_task.title
            );

            let updated = if merge_task.internal_status == InternalStatus::Blocked {
                merge_task.blocked_reason = Some(blocked_reason);
                merge_task.touch();
                self.task_repo.update(&merge_task).await?;
                merge_task
            } else {
                self.transition_task_corrective_with_exit(
                    task_id,
                    InternalStatus::Blocked,
                    Some(blocked_reason),
                    history_actor,
                )
                .await?
            };

            if let Some(scheduler) = self.task_scheduler.as_ref() {
                scheduler.try_schedule_ready_tasks().await;
            }

            Ok(updated)
        }
    }

    /// Keep an open PR-mode plan PR branch current with its GitHub base branch.
    ///
    /// Simple behind-but-mergeable cases are updated programmatically and pushed.
    /// Conflict cases are routed to the merger agent, but completion returns to
    /// `WaitingOnPr`; GitHub remains the authority for the final plan merge.
    #[track_caller]
    #[allow(clippy::manual_async_fn)]
    pub(crate) fn reconcile_pr_branch_freshness<'a>(
        &'a self,
        task_id: &'a TaskId,
        expected_plan_branch_id: &'a crate::domain::entities::PlanBranchId,
        pr_number: i64,
        source: &'a str,
    ) -> impl Future<Output = AppResult<PrBranchFreshnessOutcome>> + 'a {
        async move {
            let Some(plan_branch_repo) = self.plan_branch_repo.as_ref() else {
                return Ok(PrBranchFreshnessOutcome::NotApplicable);
            };
            let Some(github_service) = self.github_service.as_ref() else {
                return Ok(PrBranchFreshnessOutcome::NotApplicable);
            };

            let task = self
                .task_repo
                .get_by_id(task_id)
                .await?
                .ok_or_else(|| AppError::TaskNotFound(task_id.as_str().to_string()))?;
            if !pr_branch_freshness_task_eligible(&task) {
                return Ok(PrBranchFreshnessOutcome::NotApplicable);
            }

            let plan_branch = plan_branch_repo
                .get_by_merge_task_id(task_id)
                .await?
                .ok_or_else(|| {
                    AppError::NotFound(format!(
                        "No plan branch found for merge task {}",
                        task_id.as_str()
                    ))
                })?;
            if plan_branch.id != *expected_plan_branch_id
                || !plan_branch.pr_eligible
                || plan_branch.status != crate::domain::entities::PlanBranchStatus::Active
                || plan_branch.pr_number != Some(pr_number)
            {
                return Ok(PrBranchFreshnessOutcome::NotApplicable);
            }

            let project = self
                .project_repo
                .get_by_id(&task.project_id)
                .await?
                .ok_or_else(|| AppError::ProjectNotFound(task.project_id.as_str().to_string()))?;
            let repo_path = Path::new(&project.working_directory);
            let sync_state = github_service
                .check_pr_sync_state(repo_path, pr_number)
                .await?;

            if sync_state.status != PrStatus::Open {
                return Ok(PrBranchFreshnessOutcome::NotApplicable);
            }
            if sync_state.head_ref_name != plan_branch.branch_name {
                tracing::warn!(
                    task_id = task_id.as_str(),
                    pr_number,
                    expected = %plan_branch.branch_name,
                    actual = %sync_state.head_ref_name,
                    "PR freshness: GitHub head branch does not match RalphX plan branch; skipping"
                );
                return Ok(PrBranchFreshnessOutcome::NotApplicable);
            }

            if pr_sync_state_requires_conflict_resolution(&sync_state) {
                GitService::fetch_origin(repo_path).await?;
                return self
                    .route_pr_branch_update_conflict(
                        task,
                        &project,
                        &plan_branch,
                        &sync_state,
                        pr_number,
                        source,
                        Vec::new(),
                    )
                    .await;
            }

            if !pr_sync_state_requires_update(&sync_state) {
                return Ok(PrBranchFreshnessOutcome::UpToDate);
            }

            GitService::fetch_origin(repo_path).await?;
            let remote_base = remote_tracking_ref(&sync_state.base_ref_name);
            let update_result =
                crate::domain::state_machine::transition_handler::update_plan_from_main_isolated(
                    repo_path,
                    &plan_branch.branch_name,
                    &remote_base,
                    &project,
                    task_id.as_str(),
                    None,
                )
                .await;

            use crate::domain::state_machine::transition_handler::PlanUpdateResult;
            match update_result {
                PlanUpdateResult::Updated | PlanUpdateResult::AlreadyUpToDate => {
                    self.push_and_refresh_pr_branch(&task, &project, &plan_branch)
                        .await?;
                    Ok(PrBranchFreshnessOutcome::Updated)
                }
                PlanUpdateResult::NotPlanBranch => Ok(PrBranchFreshnessOutcome::NotApplicable),
                PlanUpdateResult::Conflicts { conflict_files } => {
                    self.route_pr_branch_update_conflict(
                        task,
                        &project,
                        &plan_branch,
                        &sync_state,
                        pr_number,
                        source,
                        conflict_files,
                    )
                    .await
                }
                PlanUpdateResult::Error(error) => Err(AppError::GitOperation(format!(
                    "PR branch freshness update failed: {error}"
                ))),
            }
        }
    }

    async fn push_and_refresh_pr_branch(
        &self,
        task: &Task,
        project: &crate::domain::entities::Project,
        plan_branch: &crate::domain::entities::PlanBranch,
    ) -> AppResult<()> {
        let Some(plan_branch_repo) = self.plan_branch_repo.as_ref() else {
            return Ok(());
        };
        let Some(github_service) = self.github_service.as_ref() else {
            return Ok(());
        };

        plan_branch_repo
            .update_pr_push_status(
                &plan_branch.id,
                crate::domain::entities::plan_branch::PrPushStatus::Pending,
            )
            .await?;
        let mut pending_plan_branch = plan_branch.clone();
        pending_plan_branch.pr_push_status =
            crate::domain::entities::plan_branch::PrPushStatus::Pending;
        crate::domain::state_machine::transition_handler::sync_plan_branch_pr_if_needed(
            project,
            &pending_plan_branch,
            github_service,
            plan_branch_repo,
        )
        .await;

        let refreshed_plan_branch = plan_branch_repo
            .get_by_id(&plan_branch.id)
            .await?
            .unwrap_or_else(|| plan_branch.clone());
        let publisher = PlanPrPublisher::new(
            github_service,
            self.ideation_session_repo.as_ref(),
            self.artifact_repo.as_ref(),
        );
        publisher
            .sync_existing_pr(task, project, &refreshed_plan_branch, PrReviewState::Ready)
            .await
    }

    async fn route_pr_branch_update_conflict(
        &self,
        mut task: Task,
        project: &crate::domain::entities::Project,
        plan_branch: &crate::domain::entities::PlanBranch,
        sync_state: &PrSyncState,
        pr_number: i64,
        source: &str,
        conflict_files: Vec<PathBuf>,
    ) -> AppResult<PrBranchFreshnessOutcome> {
        if task.internal_status == InternalStatus::Merging
            && task_metadata_bool(&task, "pr_branch_update_conflict")
        {
            return Ok(PrBranchFreshnessOutcome::ConflictRouted);
        }

        let repo_path = Path::new(&project.working_directory);
        let merge_worktree =
            crate::domain::state_machine::transition_handler::compute_merge_worktree_path(
                project,
                task.id.as_str(),
            );
        let merge_worktree_path = PathBuf::from(&merge_worktree);
        if merge_worktree_path.exists() {
            let _ = GitService::delete_worktree(repo_path, &merge_worktree_path).await;
        }
        GitService::checkout_existing_branch_worktree(
            repo_path,
            &merge_worktree_path,
            &plan_branch.branch_name,
        )
        .await?;

        let remote_base = remote_tracking_ref(&sync_state.base_ref_name);
        let conflict_file_strings = conflict_files
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>();
        crate::domain::state_machine::transition_handler::merge_metadata_into(
            &mut task,
            &serde_json::json!({
                "error": "The GitHub PR branch is stale and needs conflict resolution before review can continue.",
                "branch_freshness_conflict": true,
                "freshness_origin_state": "waiting_on_pr",
                "plan_update_conflict": true,
                "pr_branch_update_conflict": true,
                "github_pr_number": pr_number,
                "pr_branch_update_source": source,
                "source_branch": remote_base,
                "target_branch": plan_branch.branch_name,
                "base_branch": remote_base,
                "conflict_files": conflict_file_strings,
            }),
        );
        task.worktree_path = Some(merge_worktree);
        task.touch();
        self.task_repo.update(&task).await?;

        if let Some(plan_branch_repo) = self.plan_branch_repo.as_ref() {
            let _ = plan_branch_repo
                .clear_polling_active_by_task(&task.id)
                .await;
            let _ = plan_branch_repo
                .update_pr_push_status(
                    &plan_branch.id,
                    crate::domain::entities::plan_branch::PrPushStatus::Pending,
                )
                .await;
        }

        let updated = self
            .transition_task_corrective_with_exit(
                &task.id,
                InternalStatus::Merging,
                Some(format!(
                    "PR #{pr_number} branch needs conflict resolution before review can continue"
                )),
                "pr_branch_freshness",
            )
            .await?;
        self.execute_entry_actions(&task.id, &updated, InternalStatus::Merging)
            .await;
        Ok(PrBranchFreshnessOutcome::ConflictRouted)
    }

    /// Reroute a merge failure caused by repository commit hooks back into revision flow.
    ///
    /// This is the shared repair path for:
    /// - live merge outcome handling
    /// - manual retry actions on hook-blocked `MergeIncomplete` tasks
    /// - reconciliation/startup remediation of legacy hook-blocked merge rows
    ///
    /// When `execute_now` is true, this method also replays `RevisionNeeded` entry actions so
    /// the normal `RevisionNeeded -> ReExecuting` auto-transition runs immediately.
    #[track_caller]
    #[allow(clippy::manual_async_fn)]
    pub fn reroute_commit_hook_merge_failure<'a>(
        &'a self,
        task_id: &'a TaskId,
        explicit_error: Option<String>,
        execute_now: bool,
        history_actor: &'a str,
    ) -> impl Future<Output = AppResult<Task>> + 'a {
        async move {
            let mut task = self
                .task_repo
                .get_by_id(task_id)
                .await?
                .ok_or_else(|| AppError::TaskNotFound(task_id.as_str().to_string()))?;

            if task.internal_status == InternalStatus::ReExecuting {
                return Ok(task);
            }

            let full_error = explicit_error
                .or_else(|| {
                    crate::domain::state_machine::transition_handler::extract_commit_hook_merge_error(
                        &task,
                    )
                })
                .ok_or_else(|| {
                    AppError::Conflict(format!(
                        "Task {} is not a commit-hook merge failure",
                        task_id.as_str()
                    ))
                })?;
            let failure_kind =
                crate::domain::state_machine::transition_handler::classify_commit_hook_failure_text(
                    &full_error,
                );
            if matches!(
                failure_kind,
                crate::domain::state_machine::transition_handler::CommitHookFailureKind::EnvironmentFailure
            ) {
                return Err(AppError::Conflict(format!(
                    "Task {} has a commit-hook environment failure, not code feedback",
                    task_id.as_str()
                )));
            }
            let fingerprint =
                crate::domain::state_machine::transition_handler::commit_hook_failure_fingerprint(
                    &full_error,
                );
            if crate::domain::state_machine::transition_handler::is_repeated_commit_hook_failure(
                &task,
                &fingerprint,
            ) {
                return Err(AppError::Conflict(format!(
                    "Task {} has repeated the same commit-hook failure after re-execution",
                    task_id.as_str()
                )));
            }
            let feedback =
                crate::domain::state_machine::transition_handler::build_commit_hook_revision_feedback(
                    &full_error,
                );
            let review_note_body =
                crate::domain::state_machine::transition_handler::build_commit_hook_review_note_body(
                    &full_error,
                );

            if let Some(review_repo) = self.review_repo.as_ref() {
                let note = ReviewNote::with_content(
                    task.id.clone(),
                    ReviewerType::System,
                    ReviewOutcome::ChangesRequested,
                    Some(feedback.clone()),
                    Some(review_note_body),
                    None,
                );
                if let Err(error) = review_repo.add_note(&note).await {
                    tracing::warn!(
                        task_id = task_id.as_str(),
                        error = %error,
                        "Failed to persist system changes_requested note for commit-hook reroute"
                    );
                }
            }

            crate::domain::state_machine::transition_handler::merge_metadata_into(
                &mut task,
                &serde_json::json!({
                    "merge_revision_feedback": feedback,
                    "merge_revision_error": full_error,
                    "merge_hook_failure_kind": failure_kind.as_str(),
                    "merge_hook_failure_fingerprint": fingerprint,
                    "merge_hook_failure_repeat_count": 0,
                    "merge_hook_reexecution_requested": true,
                }),
            );
            task.touch();
            self.task_repo.update(&task).await?;

            let updated = if task.internal_status == InternalStatus::RevisionNeeded {
                task
            } else {
                self.transition_task_corrective_with_exit(
                    task_id,
                    InternalStatus::RevisionNeeded,
                    None,
                    history_actor,
                )
                .await?
            };

            if execute_now {
                self.execute_entry_actions(task_id, &updated, InternalStatus::RevisionNeeded)
                    .await;
                return self
                    .task_repo
                    .get_by_id(task_id)
                    .await?
                    .ok_or_else(|| AppError::TaskNotFound(task_id.as_str().to_string()));
            }

            Ok(updated)
        }
    }

    /// Reroute merge scope-drift guard failures back into revision flow.
    ///
    /// This is the shared repair path for merge entry actions that detect
    /// unclassified out-of-scope files after review. It intentionally uses a
    /// corrective transition because `PendingMerge -> RevisionNeeded` is not a
    /// normal user workflow transition.
    #[track_caller]
    #[allow(clippy::manual_async_fn)]
    pub fn reroute_merge_scope_drift_to_revision<'a>(
        &'a self,
        task_id: &'a TaskId,
        metadata: serde_json::Value,
        execute_now: bool,
        history_actor: &'a str,
    ) -> impl Future<Output = AppResult<Task>> + 'a {
        async move {
            let mut task = self
                .task_repo
                .get_by_id(task_id)
                .await?
                .ok_or_else(|| AppError::TaskNotFound(task_id.as_str().to_string()))?;

            if task.internal_status == InternalStatus::ReExecuting {
                return Ok(task);
            }

            crate::domain::state_machine::transition_handler::merge_metadata_into(
                &mut task, &metadata,
            );
            task.touch();
            self.task_repo.update(&task).await?;

            let updated = if task.internal_status == InternalStatus::RevisionNeeded {
                task
            } else {
                self.transition_task_corrective_with_exit(
                    task_id,
                    InternalStatus::RevisionNeeded,
                    None,
                    history_actor,
                )
                .await?
            };

            if execute_now {
                self.execute_entry_actions(task_id, &updated, InternalStatus::RevisionNeeded)
                    .await;
                return self
                    .task_repo
                    .get_by_id(task_id)
                    .await?
                    .ok_or_else(|| AppError::TaskNotFound(task_id.as_str().to_string()));
            }

            Ok(updated)
        }
    }

    /// Mark a repository hook failure as merge-blocking infrastructure/repeat state.
    ///
    /// This intentionally does not create review notes or run RevisionNeeded entry actions:
    /// the hook did not produce trustworthy code feedback, or the same feedback already
    /// repeated after re-execution.
    #[track_caller]
    #[allow(clippy::manual_async_fn)]
    pub fn mark_commit_hook_merge_failure_blocked<'a>(
        &'a self,
        task_id: &'a TaskId,
        explicit_error: Option<String>,
        history_actor: &'a str,
    ) -> impl Future<Output = AppResult<Task>> + 'a {
        async move {
            let mut task = self
                .task_repo
                .get_by_id(task_id)
                .await?
                .ok_or_else(|| AppError::TaskNotFound(task_id.as_str().to_string()))?;

            let full_error = explicit_error
                .or_else(|| {
                    crate::domain::state_machine::transition_handler::extract_commit_hook_merge_error(
                        &task,
                    )
                })
                .ok_or_else(|| {
                    AppError::Conflict(format!(
                        "Task {} is not a commit-hook merge failure",
                        task_id.as_str()
                    ))
                })?;
            let failure_kind =
                crate::domain::state_machine::transition_handler::classify_commit_hook_failure_text(
                    &full_error,
                );
            let fingerprint =
                crate::domain::state_machine::transition_handler::commit_hook_failure_fingerprint(
                    &full_error,
                );
            let repeated =
                crate::domain::state_machine::transition_handler::is_repeated_commit_hook_failure(
                    &task,
                    &fingerprint,
                );

            let blocked_reason = if repeated {
                "repeated_hook_failure"
            } else if matches!(
                failure_kind,
                crate::domain::state_machine::transition_handler::CommitHookFailureKind::EnvironmentFailure
            ) {
                "hook_environment_failure"
            } else {
                "hook_failure_blocked"
            };
            let repeat_count = if repeated {
                crate::domain::state_machine::transition_handler::commit_hook_repeat_count(
                    &task,
                    &fingerprint,
                ) + 1
            } else {
                crate::domain::state_machine::transition_handler::commit_hook_repeat_count(
                    &task,
                    &fingerprint,
                )
            };

            let mut metadata = serde_json::json!({
                "error": full_error,
                "merge_revision_error": full_error,
                "merge_hook_failure_kind": failure_kind.as_str(),
                "merge_hook_failure_fingerprint": fingerprint,
                "merge_hook_failure_repeat_count": repeat_count,
                "merge_hook_blocked_reason": blocked_reason,
                "merge_hook_reexecution_requested": false,
            });
            if let Some(obj) = metadata.as_object_mut() {
                if repeated {
                    obj.insert(
                        "merge_hook_repeated_error".to_string(),
                        serde_json::json!(full_error),
                    );
                } else if matches!(
                    failure_kind,
                    crate::domain::state_machine::transition_handler::CommitHookFailureKind::EnvironmentFailure
                ) {
                    obj.insert(
                        "merge_hook_environment_error".to_string(),
                        serde_json::json!(full_error),
                    );
                }
            }

            crate::domain::state_machine::transition_handler::merge_metadata_into(
                &mut task, &metadata,
            );
            task.touch();
            self.task_repo.update(&task).await?;

            if task.internal_status == InternalStatus::MergeIncomplete {
                return Ok(task);
            }

            self.transition_task_corrective_with_exit(
                task_id,
                InternalStatus::MergeIncomplete,
                None,
                history_actor,
            )
            .await
        }
    }

    /// Apply an explicit corrective transition for nonstandard repair flows.
    ///
    /// Unlike `transition_task*`, this path intentionally bypasses the normal state-machine
    /// legality guard and should be reserved for recovery/corrective callers that need to
    /// persist a terminal or repair status not representable as a legal workflow transition.
    ///
    /// This wrapper still records status history and uses the same optimistic-lock discipline
    /// as the internal corrective path, but it deliberately skips the normal entry/exit actions
    /// and event emission that belong to workflow transitions.
    #[track_caller]
    pub fn transition_task_corrective<'a>(
        &'a self,
        task_id: &'a TaskId,
        target_status: InternalStatus,
        blocked_reason: Option<String>,
        history_actor: &'a str,
    ) -> impl Future<Output = AppResult<Task>> + 'a {
        let caller = Location::caller();
        async move {
            let existing = self
                .task_repo
                .get_by_id(task_id)
                .await?
                .ok_or_else(|| AppError::TaskNotFound(task_id.as_str().to_string()))?;

            if existing.internal_status == target_status {
                return Ok(existing);
            }

            match self
                .apply_corrective_transition(task_id, target_status, blocked_reason, history_actor)
                .await
            {
                Some(result) => Ok(result.task),
                None => {
                    let current = self
                        .task_repo
                        .get_by_id(task_id)
                        .await?
                        .ok_or_else(|| AppError::TaskNotFound(task_id.as_str().to_string()))?;

                    tracing::warn!(
                        task_id = task_id.as_str(),
                        target_status = target_status.as_str(),
                        current_status = current.internal_status.as_str(),
                        caller_file = caller.file(),
                        caller_line = caller.line(),
                        caller_column = caller.column(),
                        "Corrective transition did not persist"
                    );

                    Err(AppError::Conflict(format!(
                        "Corrective transition to {} did not persist; current status is {}",
                        target_status.as_str(),
                        current.internal_status.as_str()
                    )))
                }
            }
        }
    }

    /// Build the common TaskServices shared by both entry and exit action handlers.
    ///
    /// Caller-specific fields (merge_lock / merges_in_flight / validation_tokens /
    /// self_arc for entry; activity_event_repo for exit) must be added by the
    /// caller after this returns.
    fn build_task_services_common(&self) -> crate::domain::state_machine::context::TaskServices {
        use crate::domain::state_machine::context::TaskServices;

        let mut services = TaskServices::new(
            Arc::clone(&self.agent_spawner),
            Arc::clone(&self.event_emitter),
            Arc::clone(&self.notifier),
            Arc::clone(&self.dependency_manager),
            Arc::clone(&self.review_starter),
            Arc::clone(&self.chat_service),
        )
        .with_execution_state(Arc::clone(&self.execution_state))
        .with_task_repo(Arc::clone(&self.task_repo))
        .with_project_repo(Arc::clone(&self.project_repo));

        if let Some(ref handle) = self._app_handle {
            services = services.try_with_app_handle(handle.clone());
        }
        if let Some(ref scheduler) = self.task_scheduler {
            services = services.with_task_scheduler(Arc::clone(scheduler));
        }
        if let Some(ref repo) = self.plan_branch_repo {
            services = services.with_plan_branch_repo(Arc::clone(repo));
        }
        if let Some(ref repo) = self.step_repo {
            services = services.with_step_repo(Arc::clone(repo));
        }
        if let Some(ref repo) = self.artifact_repo {
            services = services.with_artifact_repo(Arc::clone(repo));
        }
        if let Some(ref repo) = self.ideation_session_repo {
            services = services.with_ideation_session_repo(Arc::clone(repo));
        }
        if let Some(ref registry) = self.pr_poller_registry {
            services = services
                .with_pr_creation_guard(Arc::clone(&registry.pr_creation_guard))
                .with_pr_poller_registry(Arc::clone(registry));
        }
        if let Some(ref svc) = self.github_service {
            services = services.with_github_service(Arc::clone(svc));
        }
        if let Some(ref publisher) = self.webhook_publisher {
            services = services.with_webhook_publisher(Arc::clone(publisher));
        }
        if let Some(ref repo) = self.external_events_repo {
            services = services.with_external_events_repo(Arc::clone(repo));
        }
        services = services.with_session_merge_locks(Arc::clone(&self.session_merge_locks));
        services
    }

    /// Apply a corrective status transition after an error in `on_enter`.
    ///
    /// Encapsulates the shared boilerplate across all three error-correction handlers:
    /// re-fetch task → set target status (+ optional `blocked_reason`) →
    /// optimistic-lock update (`update_with_expected_status`) → persist transition history.
    ///
    /// Does **not** emit events — the caller is responsible for event emission and any
    /// async post-actions (e.g., spawning a merger agent).
    ///
    /// # Returns
    /// - `Some(CorrectionResult)` on success (lock acquired, DB updated, history persisted).
    /// - `None` when the optimistic lock fails (another caller already transitioned the
    ///   task) or the task is not found; logs an appropriate message in each case.
    async fn apply_corrective_transition(
        &self,
        task_id: &TaskId,
        target_status: InternalStatus,
        blocked_reason: Option<String>,
        history_actor: &str,
    ) -> Option<CorrectionResult> {
        let Ok(Some(mut task)) = self.task_repo.get_by_id(task_id).await else {
            tracing::warn!(
                task_id = task_id.as_str(),
                "apply_corrective_transition: task not found — skipping"
            );
            return None;
        };
        let from_status = task.internal_status;
        task.internal_status = target_status;
        if let Some(br) = blocked_reason {
            task.blocked_reason = Some(br);
        }
        task.touch();

        match self
            .task_repo
            .update_with_expected_status(&task, from_status)
            .await
        {
            Ok(false) => {
                tracing::info!(
                    task_id = task_id.as_str(),
                    from = from_status.as_str(),
                    to = target_status.as_str(),
                    "apply_corrective_transition: task already transitioned by another caller, skipping"
                );
                None
            }
            Err(update_err) => {
                tracing::error!(
                    error = %update_err,
                    to = target_status.as_str(),
                    "apply_corrective_transition: failed to persist corrective status"
                );
                None
            }
            Ok(true) => {
                let _ = self
                    .task_repo
                    .persist_status_change(task_id, from_status, target_status, history_actor)
                    .await;
                Some(CorrectionResult { task, from_status })
            }
        }
    }

    /// Apply a corrective transition while still honoring exit actions and status-change emission.
    ///
    /// Use this for repair flows that must bypass the normal legality guard but still need
    /// side effects tied to leaving the current state, such as decrementing `running_count`
    /// when a task exits an agent-active state.
    #[track_caller]
    pub fn transition_task_corrective_with_exit<'a>(
        &'a self,
        task_id: &'a TaskId,
        target_status: InternalStatus,
        blocked_reason: Option<String>,
        history_actor: &'a str,
    ) -> impl Future<Output = AppResult<Task>> + 'a {
        let caller = Location::caller();
        async move {
            let existing = self
                .task_repo
                .get_by_id(task_id)
                .await?
                .ok_or_else(|| AppError::TaskNotFound(task_id.as_str().to_string()))?;

            if existing.internal_status == target_status {
                return Ok(existing);
            }

            self.execute_exit_actions(task_id, &existing, existing.internal_status, target_status)
                .await;

            match self
                .apply_corrective_transition(task_id, target_status, blocked_reason, history_actor)
                .await
            {
                Some(result) => {
                    if let Some(ref handle) = self._app_handle {
                        let _ = handle.emit(
                            "task:event",
                            serde_json::json!({
                                "type": "status_changed",
                                "taskId": task_id.as_str(),
                                "from": result.from_status.as_str(),
                                "to": target_status.as_str(),
                                "changedBy": history_actor,
                            }),
                        );
                    }
                    self.event_emitter
                        .emit_status_change(
                            task_id.as_str(),
                            result.from_status.as_str(),
                            target_status.as_str(),
                        )
                        .await;
                    Ok(result.task)
                }
                None => {
                    let current = self
                        .task_repo
                        .get_by_id(task_id)
                        .await?
                        .ok_or_else(|| AppError::TaskNotFound(task_id.as_str().to_string()))?;

                    tracing::warn!(
                        task_id = task_id.as_str(),
                        target_status = target_status.as_str(),
                        current_status = current.internal_status.as_str(),
                        caller_file = caller.file(),
                        caller_line = caller.line(),
                        caller_column = caller.column(),
                        "Corrective transition with exit actions did not persist"
                    );

                    Err(AppError::Conflict(format!(
                        "Corrective transition to {} did not persist; current status is {}",
                        target_status.as_str(),
                        current.internal_status.as_str(),
                    )))
                }
            }
        }
    }

    /// Execute entry actions for a given status, including auto-transitions.
    ///
    /// This method delegates to TransitionHandler::on_enter() to ensure we use
    /// the canonical entry action logic defined in the state machine module.
    /// It also handles auto-transitions (e.g., PendingReview → Reviewing).
    ///
    /// Public so that StartupJobRunner can re-trigger entry actions on app restart
    /// for tasks that were in agent-active states when the app shut down.
    pub async fn execute_entry_actions(
        &self,
        task_id: &TaskId,
        task: &Task,
        status: InternalStatus,
    ) {
        use crate::domain::state_machine::{
            context::TaskContext, machine::TaskStateMachine, transition_handler::TransitionHandler,
        };

        let state = internal_status_to_state(status);

        // Per-task team_mode override: check builder flag OR task metadata.
        // Some(true/false) = explicitly set by caller → use directly, skip metadata.
        // None = unset → fall back to task metadata agent_variant.
        match self.team_mode {
            Some(explicit) => {
                self.chat_service.set_team_mode(explicit);
            }
            None => {
                // No explicit choice — fall back to task metadata.
                // Always set team_mode explicitly to prevent AtomicBool contamination
                // from previous tasks sharing the same Arc<ChatService>.
                let is_team = task
                    .metadata
                    .as_ref()
                    .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
                    .and_then(|meta| {
                        meta.get("agent_variant")
                            .and_then(|v| v.as_str())
                            .map(|s| s == "team")
                    })
                    .unwrap_or(false);
                self.chat_service.set_team_mode(is_team);
            }
        }

        // Build common TaskServices, then add entry-specific fields.
        let mut services = self.build_task_services_common();

        // Pass shared merge lock for TOCTOU-safe concurrent merge guard
        services = services.with_merge_lock(Arc::clone(&self.merge_lock));

        // Pass shared merges_in_flight set for self-dedup across concurrent calls
        services = services.with_merges_in_flight(Arc::clone(&self.merges_in_flight));

        // Pass shared validation_tokens DashMap for cancelling in-flight validations
        services = services.with_validation_tokens(Arc::clone(&self.validation_tokens));

        // Pass self-arc as transition_service for PR merge poller (AD17).
        // Downcast from Arc<dyn Any> → Arc<TaskTransitionService<Wry>> (only succeeds for Wry runtime).
        {
            let locked = self.self_arc.lock().unwrap();
            if let Some(ref any_arc) = *locked {
                if let Ok(ts_wry) =
                    Arc::clone(any_arc).downcast::<TaskTransitionService<tauri::Wry>>()
                {
                    services = services.with_transition_service(ts_wry);
                }
            }
        }

        // Create TaskContext
        let context = TaskContext::new(task_id.as_str(), task.project_id.as_str(), services);

        // Create state machine and handler
        let mut machine = TaskStateMachine::new(context);
        let handler = TransitionHandler::new(&mut machine);

        // Execute entry action via TransitionHandler
        tracing::debug!(?state, "Calling TransitionHandler::on_enter");
        if let Err(e) = handler.on_enter(&state).await {
            tracing::error!(error = %e, "on_enter failed");

            // If execution was blocked (e.g., git isolation failure), transition task to Failed.
            if let AppError::ExecutionBlocked(ref blocked_reason) = e {
                tracing::warn!(
                    task_id = task_id.as_str(),
                    error = %e,
                    "ExecutionBlocked during on_enter — transitioning task to Failed"
                );
                if let Some(result) = self
                    .apply_corrective_transition(
                        task_id,
                        InternalStatus::Failed,
                        Some(e.to_string()),
                        "system",
                    )
                    .await
                {
                    // Emit event for UI
                    if let Some(ref handle) = self._app_handle {
                        let _ = handle.emit(
                            "task:event",
                            serde_json::json!({
                                "type": "status_changed",
                                "taskId": task_id.as_str(),
                                "from": result.from_status.as_str(),
                                "to": "failed",
                                "changedBy": "system",
                                "reason": e.to_string(),
                            }),
                        );
                    }
                    self.event_emitter
                        .emit_status_change(task_id.as_str(), result.from_status.as_str(), "failed")
                        .await;
                    // Create ExecutionRecoveryMetadata for git isolation failures.
                    // Written ONLY after the optimistic lock succeeded (inside apply_corrective_transition
                    // Ok(true) branch), so the task IS in Failed state. This prevents orphaned metadata
                    // on tasks that another caller already transitioned.
                    if let Some(recovery_json) = create_git_isolation_recovery_metadata_json(
                        blocked_reason,
                        result.task.metadata.as_deref(),
                    ) {
                        if let Err(meta_err) = self
                            .task_repo
                            .update_metadata(task_id, Some(recovery_json))
                            .await
                        {
                            tracing::error!(
                                error = %meta_err,
                                "Failed to persist ExecutionRecoveryMetadata after git isolation ExecutionBlocked"
                            );
                        } else {
                            tracing::info!(
                                task_id = task_id.as_str(),
                                "ExecutionRecoveryMetadata persisted for git isolation failure — task eligible for auto-recovery"
                            );
                        }
                    }
                }
            } else if matches!(&e, AppError::BranchFreshnessConflict) {
                tracing::warn!(
                    task_id = task_id.as_str(),
                    "BranchFreshnessConflict during initial on_enter — delegating to corrective handler"
                );
                self.handle_branch_freshness_conflict(&handler, task_id, &state)
                    .await;
            } else if matches!(&e, AppError::ReviewWorktreeMissing) {
                use crate::domain::state_machine::machine::State as MState;
                tracing::warn!(
                    task_id = task_id.as_str(),
                    "ReviewWorktreeMissing during initial on_enter — routing to Escalated"
                );
                handler.on_exit(&state, &MState::Escalated).await;
                if let Some(result) = self
                    .apply_corrective_transition(task_id, InternalStatus::Escalated, None, "system")
                    .await
                {
                    if let Some(ref handle) = self._app_handle {
                        let _ = handle.emit(
                            "task:event",
                            serde_json::json!({
                                "type": "status_changed",
                                "taskId": task_id.as_str(),
                                "from": result.from_status.as_str(),
                                "to": "escalated",
                                "changedBy": "system",
                                "reason": "ReviewWorktreeMissing during initial on_enter",
                            }),
                        );
                    }
                    self.event_emitter
                        .emit_status_change(
                            task_id.as_str(),
                            result.from_status.as_str(),
                            "escalated",
                        )
                        .await;
                    // Dual-channel emit: persist to external_events table and fire webhook.
                    // The corrective path bypasses on_enter(Escalated), so we emit explicitly here.
                    let escalated_payload = serde_json::json!({
                        "task_id": task_id.as_str(),
                        "project_id": result.task.project_id.as_str(),
                        "timestamp": chrono::Utc::now().to_rfc3339(),
                    });
                    if let Some(ref repo) = self.external_events_repo {
                        let _ = repo
                            .insert_event(
                                &EventType::ReviewEscalated.to_string(),
                                result.task.project_id.as_str(),
                                &escalated_payload.to_string(),
                            )
                            .await;
                    }
                    if let Some(ref publisher) = self.webhook_publisher {
                        publisher
                            .publish(
                                EventType::ReviewEscalated,
                                result.task.project_id.as_str(),
                                escalated_payload,
                            )
                            .await;
                    }
                }
            }
        }
        tracing::debug!("TransitionHandler::on_enter complete");

        // === AUTO-TRANSITION LOOP ===
        // Check for auto-transitions (e.g., PendingReview → Reviewing, RevisionNeeded → ReExecuting)
        // This is critical for states that should immediately transition to spawn an agent.
        // Currently runs at most once per call (single auto-transition step); the loop structure
        // enables Wave 2A's `continue` semantics for PendingReview re-entry after corrective routing.
        let mut current_state = state;
        loop {
            let mut auto_state = match handler.check_auto_transition(&current_state) {
                Some(s) => s,
                None => break,
            };

            if matches!(current_state, crate::domain::state_machine::State::Approved)
                && matches!(
                    auto_state,
                    crate::domain::state_machine::State::PendingMerge
                )
            {
                if let Ok(Some(task)) = self.task_repo.get_by_id(task_id).await {
                    let is_branchless = task.task_branch.is_none();
                    let has_no_changes = crate::domain::state_machine::transition_handler::has_no_code_changes_metadata(&task);

                    if is_branchless || has_no_changes {
                        let reason = if has_no_changes {
                            "no_code_changes metadata"
                        } else {
                            "no task branch"
                        };
                        tracing::info!(
                            task_id = task_id.as_str(),
                            reason = reason,
                            "Skipping merge pipeline during auto-transition"
                        );
                        auto_state = crate::domain::state_machine::State::Merged;
                    }
                }
            }

            let current_status = state_to_internal_status(&current_state);
            let auto_status = state_to_internal_status(&auto_state);
            tracing::info!(
                from = current_status.as_str(),
                to = auto_status.as_str(),
                "Auto-transition triggered"
            );

            // Execute on_exit for the intermediate state
            handler.on_exit(&current_state, &auto_state).await;

            // Persist the auto-transition to the database
            if let Ok(Some(mut updated_task)) = self.task_repo.get_by_id(task_id).await {
                let from_status = updated_task.internal_status;
                updated_task.internal_status = auto_status;

                // Set trigger_origin for RevisionNeeded → ReExecuting transition
                if from_status == InternalStatus::RevisionNeeded
                    && auto_status == InternalStatus::ReExecuting
                {
                    set_trigger_origin(&mut updated_task, "revision");
                }

                updated_task.touch();
                if let Err(e) = self.task_repo.update(&updated_task).await {
                    tracing::error!(error = %e, "Failed to persist auto-transition");
                }
                // Record auto-transition in history
                if let Err(e) = self
                    .task_repo
                    .persist_status_change(task_id, from_status, auto_status, "auto")
                    .await
                {
                    tracing::warn!(error = %e, "Failed to record auto-transition history (non-fatal)");
                }
            }

            // Emit task:event for auto-transition so UI updates in real time
            if let Some(ref handle) = self._app_handle {
                let _ = handle.emit(
                    "task:event",
                    serde_json::json!({
                        "type": "status_changed",
                        "taskId": task_id.as_str(),
                        "from": current_status.as_str(),
                        "to": auto_status.as_str(),
                        "changedBy": "auto",
                    }),
                );
                tracing::debug!("Emitted task:event for auto-transition");
            }
            self.event_emitter
                .emit_status_change(
                    task_id.as_str(),
                    current_status.as_str(),
                    auto_status.as_str(),
                )
                .await;

            // Execute on_enter for the auto-transition target state
            if let Err(e) = handler.on_enter(&auto_state).await {
                tracing::error!(error = %e, "on_enter failed for auto-transition state {:?}", auto_state);

                // BranchFreshnessConflict during auto-transition: route based on where the
                // conflict originated. "reviewing" origin → PendingReview (re-queue for review,
                // preserving review requirement). "executing"/"re_executing"/absent → Merging
                // (existing behavior for execution-phase conflicts).
                if matches!(&e, AppError::BranchFreshnessConflict) {
                    self.handle_branch_freshness_conflict(&handler, task_id, &auto_state)
                        .await;
                } else if matches!(&e, AppError::ReviewWorktreeMissing) {
                    use crate::domain::state_machine::machine::State;

                    tracing::warn!(
                        task_id = task_id.as_str(),
                        from = auto_status.as_str(),
                        "ReviewWorktreeMissing during auto-transition on_enter — routing to Escalated"
                    );

                    handler.on_exit(&auto_state, &State::Escalated).await;

                    if let Some(result) = self
                        .apply_corrective_transition(
                            task_id,
                            InternalStatus::Escalated,
                            None,
                            "system",
                        )
                        .await
                    {
                        // Emit corrective event for UI
                        if let Some(ref handle) = self._app_handle {
                            let _ = handle.emit(
                                "task:event",
                                serde_json::json!({
                                    "type": "status_changed",
                                    "taskId": task_id.as_str(),
                                    "from": result.from_status.as_str(),
                                    "to": "escalated",
                                    "changedBy": "system",
                                    "reason": "ReviewWorktreeMissing during auto-transition",
                                }),
                            );
                        }
                        // Dual-channel emit: persist to external_events table and fire webhook.
                        // The corrective path bypasses on_enter(Escalated), so we emit explicitly here.
                        let escalated_payload = serde_json::json!({
                            "task_id": task_id.as_str(),
                            "project_id": result.task.project_id.as_str(),
                            "timestamp": chrono::Utc::now().to_rfc3339(),
                        });
                        if let Some(ref repo) = self.external_events_repo {
                            let _ = repo
                                .insert_event(
                                    &EventType::ReviewEscalated.to_string(),
                                    result.task.project_id.as_str(),
                                    &escalated_payload.to_string(),
                                )
                                .await;
                        }
                        if let Some(ref publisher) = self.webhook_publisher {
                            publisher
                                .publish(
                                    EventType::ReviewEscalated,
                                    result.task.project_id.as_str(),
                                    escalated_payload,
                                )
                                .await;
                        }
                    }
                }
                break; // Error handled — exit auto-transition loop
            }
            tracing::debug!(?auto_state, "Auto-transition on_enter complete");
            current_state = auto_state;
        }
    }

    /// Shared handler for `BranchFreshnessConflict` errors.
    ///
    /// Called from both the initial `on_enter` error handler (startup recovery /
    /// direct re-entry via `execute_entry_actions`) and the auto-transition
    /// `on_enter` error handler.
    ///
    /// `current_state` is the state the task was in when the error occurred:
    /// - Initial path: `&state` (the state passed to `execute_entry_actions`)
    /// - Auto-transition path: `&auto_state` (the auto-transition target that failed)
    ///
    /// Operation order (load-bearing — do not reorder):
    /// metadata read → conditional count increment → cap enforcement →
    /// on_exit → worktree restoration → apply_corrective_transition →
    /// event emission → conditional merger spawn.
    ///
    /// Note: during startup recovery the task has no running agent, so
    /// `running_count` was never incremented. `on_exit` calls `saturating_sub` —
    /// no underflow is possible.
    async fn handle_branch_freshness_conflict(
        &self,
        handler: &crate::domain::state_machine::transition_handler::TransitionHandler<'_>,
        task_id: &TaskId,
        current_state: &crate::domain::state_machine::machine::State,
    ) {
        use crate::domain::state_machine::machine::State;
        use crate::domain::state_machine::transition_handler::freshness::FreshnessMetadata;

        // Step 1: Read freshness metadata written by on_enter before the error.
        let fresh_task = self.task_repo.get_by_id(task_id).await.ok().flatten();
        let task_meta_val: serde_json::Value = fresh_task
            .as_ref()
            .and_then(|t| t.metadata.as_deref())
            .and_then(|m| serde_json::from_str(m).ok())
            .unwrap_or_else(|| serde_json::json!({}));

        let freshness_origin = task_meta_val["freshness_origin_state"]
            .as_str()
            .map(|s| s.to_owned());
        let reviewing_origin = freshness_origin.as_deref() == Some("reviewing");
        let has_merge_conflict_evidence = task_meta_val["conflict_markers_detected"]
            .as_bool()
            .unwrap_or(false)
            || task_meta_val["source_update_conflict"]
                .as_bool()
                .unwrap_or(false)
            || task_meta_val["plan_update_conflict"]
                .as_bool()
                .unwrap_or(false);

        // Step 2: Conditional increment — only for the conflict marker scan path
        // (which doesn't call ensure_branches_fresh and never sets
        // freshness_count_incremented_by).
        let already_incremented = task_meta_val["freshness_count_incremented_by"]
            .as_str()
            .is_some();
        let conflict_count = if already_incremented {
            task_meta_val["freshness_conflict_count"]
                .as_u64()
                .unwrap_or(0) as u32
        } else {
            let mut freshness = FreshnessMetadata::from_task_metadata(&task_meta_val);
            freshness.freshness_conflict_count += 1;
            let mut updated_meta = task_meta_val.clone();
            freshness.merge_into(&mut updated_meta);
            let task_id_clone = task_id.clone();
            let _ = self
                .task_repo
                .update_metadata(&task_id_clone, Some(updated_meta.to_string()))
                .await;
            freshness.freshness_conflict_count
        };

        // Step 3: Cap enforcement — >= 5 during review → escalate to Failed.
        const FRESHNESS_RETRY_LIMIT: u32 = 5;
        if reviewing_origin
            && !has_merge_conflict_evidence
            && conflict_count >= FRESHNESS_RETRY_LIMIT
        {
            tracing::warn!(
                task_id = task_id.as_str(),
                conflict_count = conflict_count,
                "Freshness retry limit exceeded during review — escalating to Failed"
            );
            handler
                .on_exit(current_state, &State::Failed(Default::default()))
                .await;
            if let Some(result) = self
                .apply_corrective_transition(
                    task_id,
                    InternalStatus::Failed,
                    Some("Exceeded freshness retry limit during review".to_string()),
                    "system",
                )
                .await
            {
                if let Some(ref handle) = self._app_handle {
                    let _ = handle.emit(
                        "task:event",
                        serde_json::json!({
                            "type": "status_changed",
                            "taskId": task_id.as_str(),
                            "from": result.from_status.as_str(),
                            "to": "failed",
                            "changedBy": "system",
                            "reason": "Exceeded freshness retry limit during review",
                        }),
                    );
                }
            }
            return;
        }

        let (target_state, corrective_status, to_str) =
            if reviewing_origin && !has_merge_conflict_evidence {
                (
                    State::PendingReview,
                    InternalStatus::PendingReview,
                    "pending_review",
                )
            } else {
                (State::Merging, InternalStatus::Merging, "merging")
            };

        tracing::warn!(
            task_id = task_id.as_str(),
            origin = ?freshness_origin,
            routing_to = to_str,
            "BranchFreshnessConflict during on_enter — routing to {:?}",
            target_state
        );

        // Step 4: on_exit for the current state.
        handler.on_exit(current_state, &target_state).await;

        // Step 5: Worktree restoration — for reviewing_origin path returning to
        // PendingReview, restore worktree_path from merge-prefixed path back to
        // the task execution worktree. Must persist BEFORE apply_corrective_transition
        // (which re-fetches and preserves the current worktree_path).
        if reviewing_origin {
            if let Ok(Some(mut task)) = self.task_repo.get_by_id(task_id).await {
                let needs_restore = task
                    .worktree_path
                    .as_deref()
                    .map(crate::domain::state_machine::transition_handler::is_merge_worktree_path)
                    .unwrap_or(false);
                if needs_restore {
                    match self.project_repo.get_by_id(&task.project_id).await {
                        Ok(Some(project)) => {
                            let repo_path = std::path::Path::new(&project.working_directory);
                            match crate::domain::state_machine::transition_handler::restore_task_worktree(
                                &mut task, &project, repo_path,
                            )
                            .await
                            {
                                Ok(restored) => {
                                    tracing::info!(
                                        task_id = task_id.as_str(),
                                        restored_path = %restored.display(),
                                        "L1: restored worktree_path on BranchFreshnessConflict → PendingReview"
                                    );
                                    if let Err(e) = self.task_repo.update(&task).await {
                                        tracing::warn!(
                                            task_id = task_id.as_str(),
                                            error = %e,
                                            "L1: failed to persist restored worktree_path"
                                        );
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        task_id = task_id.as_str(),
                                        error = %e,
                                        "L1: failed to restore task worktree on BranchFreshnessConflict → PendingReview"
                                    );
                                }
                            }
                        }
                        Ok(None) => {
                            tracing::warn!(
                                task_id = task_id.as_str(),
                                "L1: project not found for worktree restoration"
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                task_id = task_id.as_str(),
                                error = %e,
                                "L1: failed to fetch project for worktree restoration"
                            );
                        }
                    }
                }
            }
        }

        // Step 6: apply_corrective_transition.
        if let Some(result) = self
            .apply_corrective_transition(task_id, corrective_status, None, "system")
            .await
        {
            let reason = if reviewing_origin && !has_merge_conflict_evidence {
                "BranchFreshnessConflict during review — re-queuing for review"
            } else if reviewing_origin {
                "BranchFreshnessConflict during review — routing to merge resolution"
            } else {
                "BranchFreshnessConflict during on_enter"
            };

            // Step 7: UI event emission.
            if let Some(ref handle) = self._app_handle {
                let _ = handle.emit(
                    "task:event",
                    serde_json::json!({
                        "type": "status_changed",
                        "taskId": task_id.as_str(),
                        "from": result.from_status.as_str(),
                        "to": to_str,
                        "changedBy": "system",
                        "reason": reason,
                    }),
                );
            }

            // Step 8: Conditional merger spawn — generic review-origin freshness conflicts
            // park in PendingReview, but actual merge-conflict evidence must route through
            // Merging so the merger agent can resolve the conflict.
            if reviewing_origin && !has_merge_conflict_evidence {
                tracing::info!(
                    task_id = task_id.as_str(),
                    "Parked task in PendingReview after review-origin freshness conflict; skipping immediate re-entry to Reviewing"
                );
            } else {
                let merging_state = State::Merging;
                if let Err(merge_err) = handler.on_enter(&merging_state).await {
                    tracing::error!(
                        error = %merge_err,
                        "on_enter(Merging) failed after BranchFreshnessConflict correction"
                    );
                }
            }
        }
    }

    /// Execute exit actions for a status transition.
    ///
    /// This method delegates to TransitionHandler::on_exit() to ensure we use
    /// the canonical exit action logic defined in the state machine module.
    /// This is critical for decrementing running count when tasks exit agent-active states.
    async fn execute_exit_actions(
        &self,
        task_id: &TaskId,
        task: &Task,
        from_status: InternalStatus,
        to_status: InternalStatus,
    ) {
        use crate::domain::state_machine::{
            context::TaskContext, machine::TaskStateMachine, transition_handler::TransitionHandler,
        };

        let from_state = internal_status_to_state(from_status);
        let to_state = internal_status_to_state(to_status);

        // Build common TaskServices, then add exit-specific fields.
        let services = self
            .build_task_services_common()
            .with_activity_event_repo(Arc::clone(&self.activity_event_repo));

        // Create TaskContext
        let context = TaskContext::new(task_id.as_str(), task.project_id.as_str(), services);

        // Create state machine and handler
        let mut machine = TaskStateMachine::new(context);
        let handler = TransitionHandler::new(&mut machine);

        // Execute exit action via TransitionHandler
        tracing::debug!(?from_state, ?to_state, "Calling TransitionHandler::on_exit");
        handler.on_exit(&from_state, &to_state).await;
        tracing::debug!("TransitionHandler::on_exit complete");
    }
}

/// Auto-compute metadata for specific status transitions.
///
/// Returns metadata updates that should be automatically applied when transitioning
/// to certain statuses (e.g., QaRefining/QaTesting get trigger_origin=qa).
///
/// # Arguments
/// * `status` - The target status being transitioned to
///
/// # Returns
/// * `Some(MetadataUpdate)` if auto-metadata applies to this status
/// * `None` if no auto-metadata for this status
fn auto_metadata_for_status(status: InternalStatus) -> Option<MetadataUpdate> {
    match status {
        InternalStatus::QaRefining | InternalStatus::QaTesting => {
            Some(build_trigger_origin_metadata("qa"))
        }
        _ => None,
    }
}

/// TaskStopper implementation — delegates to transition_task for graceful stop.
#[async_trait]
impl<R: Runtime> crate::application::TaskStopper for TaskTransitionService<R> {
    async fn transition_to_stopped(&self, task_id: &TaskId) -> AppResult<()> {
        self.transition_task(task_id, InternalStatus::Stopped)
            .await
            .map(|_| ())
    }

    async fn transition_to_stopped_with_context(
        &self,
        task_id: &TaskId,
        from_status: InternalStatus,
        reason: Option<String>,
    ) -> AppResult<()> {
        self.transition_to_stopped_with_context(task_id, from_status, reason)
            .await
            .map(|_| ())
    }
}

/// Build `ExecutionRecoveryMetadata` JSON for a git isolation failure.
///
/// Returns `Some(json)` if `blocked_reason` starts with `GIT_ISOLATION_ERROR_PREFIX`,
/// meaning this is a transient git isolation failure eligible for auto-recovery.
/// Returns `None` for all other `ExecutionBlocked` reasons (no recovery metadata needed).
///
/// The returned JSON merges the new `execution_recovery` key into `existing_metadata`,
/// preserving any other metadata keys already present on the task.
pub(crate) fn create_git_isolation_recovery_metadata_json(
    blocked_reason: &str,
    existing_metadata: Option<&str>,
) -> Option<String> {
    if !blocked_reason.starts_with(GIT_ISOLATION_ERROR_PREFIX) {
        return None;
    }
    let mut recovery = ExecutionRecoveryMetadata::new();
    // last_state defaults to Retrying (required by recover_timeout_failures() eligibility check)
    // stop_retrying defaults to false
    let event = ExecutionRecoveryEvent::new(
        ExecutionRecoveryEventKind::Failed,
        ExecutionRecoverySource::Auto,
        ExecutionRecoveryReasonCode::GitIsolationFailed,
        blocked_reason,
    )
    .with_failure_source(ExecutionFailureSource::GitIsolation);
    recovery.append_event(event);
    match recovery.update_task_metadata(existing_metadata) {
        Ok(json) => Some(json),
        Err(e) => {
            tracing::error!(
                error = %e,
                "Failed to serialize ExecutionRecoveryMetadata for git isolation"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests;
