// Transition handler - orchestrates side effects for state transitions
// This module wraps the state machine and handles entry/exit actions,
// especially for QA-related transitions.

use std::path::Path;
use std::sync::Arc;

use crate::domain::entities::{Project, Task, TaskId};
use crate::domain::repositories::TaskRepository;

use super::events::TaskEvent;
use super::machine::{Response, State, TaskStateMachine};

/// Bundles task identity and repository access for merge pipeline functions.
pub(super) struct TaskCore<'a> {
    pub task: &'a mut Task,
    pub task_id: &'a TaskId,
    pub task_id_str: &'a str,
    pub task_repo: &'a Arc<dyn TaskRepository>,
}

/// Bundles source and target branch names for merge pipeline functions.
pub(super) struct BranchPair<'a> {
    pub source_branch: &'a str,
    pub target_branch: &'a str,
}

/// Bundles project and repo path for merge pipeline functions.
pub(super) struct ProjectCtx<'a> {
    pub project: &'a Project,
    pub repo_path: &'a Path,
}

mod automatic_commit_policy;
mod checkout_free_strategy;
pub(crate) mod cleanup_helpers;
mod commit_messages;
mod exit_actions;
pub mod freshness;
mod merge_completion;
mod merge_coordination;
mod merge_helpers;
mod merge_orchestrator;
mod merge_outcome_handler;
mod merge_strategies;
mod merge_validation;
pub mod metadata_builder;
pub(crate) mod on_enter_states;
mod side_effects;
#[cfg(test)]
mod tests;

// -- Public re-exports --
pub use merge_completion::complete_merge_internal;
pub(crate) use merge_completion::complete_merge_internal_with_pr_sync_and_notifier;
pub use merge_completion::{
    clear_pending_cleanup_metadata, deferred_merge_cleanup, has_no_code_changes_metadata,
    has_pending_cleanup_metadata, set_no_code_changes_metadata, set_pending_cleanup_metadata,
};
pub(crate) use merge_coordination::{
    update_plan_from_main_isolated, update_source_from_target, PlanUpdateResult, SourceUpdateResult,
};
pub use merge_helpers::resolve_merge_branches;
pub use metadata_builder::{build_failed_metadata, build_trigger_origin_metadata, MetadataUpdate};

// -- Crate-visible re-exports (merge_helpers) --
#[doc(hidden)]
pub use merge_helpers::DEFERRED_MERGE_TIMEOUT_SECONDS;
pub(crate) use merge_helpers::{
    build_commit_hook_review_note_body, build_commit_hook_revision_feedback,
    classify_commit_hook_failure_text, clear_github_auto_merge_correction_marker_for_terminal_pr,
    clear_main_merge_deferred_metadata, clear_merge_deferred_metadata,
    commit_hook_failure_fingerprint, commit_hook_repeat_count, compute_merge_worktree_path,
    compute_plan_update_worktree_path, compute_rebase_worktree_path,
    compute_source_update_worktree_path,
    create_draft_pr_if_needed, draft_plan_pr_description_for_write, extract_commit_hook_merge_error,
    get_trigger_origin, has_branch_missing_metadata, has_main_merge_deferred_metadata,
    has_merge_deferred_metadata, is_commit_hook_merge_error_text, is_main_merge_deferred_timed_out,
    is_merge_deferred_timed_out, is_merge_worktree_path,
    is_pr_branch_publication_conflict_routed_error, is_repeated_commit_hook_failure,
    merge_metadata_into, plan_branch_has_reviewable_diff, plan_regular_tasks_complete,
    publish_plan_branch_pr_after_freshness_update, resolve_plan_branch_pr_base,
    resolve_task_plan_branch_record,
    restore_task_worktree, set_conflict_metadata, set_source_conflict_resolved,
    sync_plan_branch_pr_after_regular_task_merge, sync_plan_branch_pr_if_needed,
    task_has_commit_hook_merge_failure, task_has_pr_branch_publication_conflict,
    task_has_pr_branch_publication_failure, CommitHookFailureKind, PlanBranchPrSyncOutcome,
    PlanBranchPrSyncServices,
};
#[doc(hidden)]
pub use merge_helpers::{parse_metadata, set_trigger_origin};

// -- Crate-visible re-exports (merge_validation) --
pub(crate) use merge_validation::{format_validation_error_metadata, run_validation_commands};

// -- Public re-exports (merge_validation for testing) --
pub use merge_validation::{run_pre_execution_setup, PreExecSetupResult};

/// Result of handling a transition
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionResult {
    /// Transition completed successfully
    Success(State),
    /// Event was not handled in current state
    NotHandled,
    /// Auto-transition triggered (e.g., ExecutionDone -> QaRefining)
    AutoTransition(State),
}

impl TransitionResult {
    pub fn state(&self) -> Option<&State> {
        match self {
            TransitionResult::Success(s) | TransitionResult::AutoTransition(s) => Some(s),
            TransitionResult::NotHandled => None,
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(
            self,
            TransitionResult::Success(_) | TransitionResult::AutoTransition(_)
        )
    }
}

/// Handler for state transitions with side effects
pub struct TransitionHandler<'a> {
    machine: &'a mut TaskStateMachine,
}

impl<'a> TransitionHandler<'a> {
    pub fn new(machine: &'a mut TaskStateMachine) -> Self {
        Self { machine }
    }

    /// Replace the notification authority carried into subsequent entry actions.
    pub fn replace_notification_context(
        &mut self,
        context: Option<crate::domain::state_machine::services::NotificationContext>,
    ) {
        self.machine.context.services.notification_context = context;
    }

    /// Build an ExitContext snapshot from the current machine context.
    fn exit_context(&self) -> exit_actions::ExitContext {
        let ctx = &self.machine.context;
        exit_actions::ExitContext {
            task_id: ctx.task_id.clone(),
            project_id: ctx.project_id.clone(),
            task_repo: ctx.services.task_repo.clone(),
            project_repo: ctx.services.project_repo.clone(),
            task_scheduler: ctx.services.task_scheduler.clone(),
        }
    }

    /// Handle a state transition with side effects
    ///
    /// 1. Dispatches the event to the state machine
    /// 2. Executes entry actions for the new state (if applicable)
    /// 3. Handles auto-transitions (e.g., ExecutionDone -> QaRefining when QA enabled)
    pub async fn handle_transition(
        &mut self,
        current_state: &State,
        event: &TaskEvent,
    ) -> TransitionResult {
        let response = self.machine.dispatch(current_state, event);

        match response {
            Response::Transition(new_state) => {
                self.on_exit(current_state, &new_state).await;

                if let Err(e) = self.on_enter(&new_state).await {
                    self.emit_on_enter_error(&new_state, &e).await;
                    tracing::error!(error = %e, "on_enter failed for state {:?}", new_state);

                    if matches!(e, crate::error::AppError::ExecutionBlocked(_)) {
                        let error_msg = e.to_string();
                        tracing::warn!("ExecutionBlocked detected, dispatching ExecutionFailed to transition to Failed");
                        let failed_event = TaskEvent::ExecutionFailed { error: error_msg };
                        let failed_response = self.machine.dispatch(&new_state, &failed_event);
                        if let Response::Transition(failed_state) = failed_response {
                            self.on_exit(&new_state, &failed_state).await;
                            if let Err(e) = self.on_enter(&failed_state).await {
                                tracing::error!(error = %e, "on_enter failed for recovery state {:?}", failed_state);
                            }
                            return TransitionResult::Success(failed_state);
                        }
                    } else if matches!(e, crate::error::AppError::BranchFreshnessConflict) {
                        let update_state = if let Some(task_repo) =
                            self.machine.context.services.task_repo.as_ref()
                        {
                            match task_repo
                                .get_by_id(&TaskId::from_string(
                                    self.machine.context.task_id.clone(),
                                ))
                                .await
                            {
                                Ok(Some(task))
                                    if task.internal_status
                                        == crate::domain::entities::InternalStatus::UpdatingPlanBranch =>
                                {
                                    Some(State::UpdatingPlanBranch)
                                }
                                Ok(Some(task))
                                    if task.internal_status
                                        == crate::domain::entities::InternalStatus::UpdatingTaskBranch =>
                                {
                                    Some(State::UpdatingTaskBranch)
                                }
                                _ => None,
                            }
                        } else {
                            None
                        };
                        if let Some(update_state) = update_state {
                            tracing::info!(
                                task_id = %self.machine.context.task_id,
                                state = ?update_state,
                                "Freshness conflict activated a dedicated branch update"
                            );
                            if let Err(error) = self.on_enter(&update_state).await {
                                tracing::error!(
                                    error = %error,
                                    "Branch updater failed to start after durable activation"
                                );
                            }
                            return TransitionResult::Success(update_state);
                        }
                    }
                }

                if let Some(mut auto_state) = self.check_auto_transition(&new_state) {
                    if matches!(new_state, State::RevisionNeeded)
                        && matches!(auto_state, State::ReExecuting)
                    {
                        let ctx = self.exit_context();
                        auto_state =
                            exit_actions::check_revision_cap_or_fail(&ctx, auto_state).await;
                    }

                    // Skip merge pipeline for branchless tasks or no-code-changes tasks.
                    // - Branchless: task has no task_branch (e.g., external repo work).
                    // - No-code-changes: reviewer used approved_no_changes and confirmed
                    //   no code diff exists (metadata set by complete_review handler).
                    // Both cases bypass PendingMerge and go directly to Merged.
                    if matches!(new_state, State::Approved)
                        && matches!(auto_state, State::PendingMerge)
                    {
                        if let Some(ref task_repo) = self.machine.context.services.task_repo {
                            let task_id = TaskId::from_string(self.machine.context.task_id.clone());
                            if let Ok(Some(task)) = task_repo.get_by_id(&task_id).await {
                                let is_branchless = task.task_branch.is_none();
                                let has_no_changes = has_no_code_changes_metadata(&task);

                                if is_branchless || has_no_changes {
                                    let reason = if has_no_changes {
                                        "no_code_changes metadata"
                                    } else {
                                        "no task branch"
                                    };
                                    tracing::info!(
                                        task_id = %self.machine.context.task_id,
                                        reason = %reason,
                                        "Skipping merge pipeline, auto-transitioning to Merged"
                                    );
                                    auto_state = State::Merged;

                                    // For no-code-changes path: clear merge progress and
                                    // spawn deferred cleanup (branch/worktree deletion).
                                    // Branchless tasks have no branch/worktree to clean up.
                                    if has_no_changes && !is_branchless {
                                        let task_id_str = task_id.as_str().to_string();
                                        crate::domain::entities::merge_progress_event::clear_merge_progress(&task_id_str);

                                        let task_branch = task.task_branch.clone();
                                        let worktree_path = task.worktree_path.clone();

                                        // Fetch project for working_directory
                                        if let Some(ref project_repo) =
                                            self.machine.context.services.project_repo
                                        {
                                            match project_repo.get_by_id(&task.project_id).await {
                                                Ok(Some(project)) => {
                                                    tokio::spawn(deferred_merge_cleanup(
                                                        task_id.clone(),
                                                        Arc::clone(task_repo),
                                                        project.working_directory.clone(),
                                                        task_branch,
                                                        worktree_path,
                                                        None,
                                                    ));
                                                }
                                                Ok(None) => {
                                                    tracing::warn!(
                                                        task_id = %task_id_str,
                                                        "Project not found for no-code-changes cleanup (non-fatal)"
                                                    );
                                                }
                                                Err(e) => {
                                                    tracing::warn!(
                                                        task_id = %task_id_str,
                                                        error = %e,
                                                        "Failed to fetch project for no-code-changes cleanup (non-fatal)"
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    self.on_exit(&new_state, &auto_state).await;
                    if let Err(e) = self.on_enter(&auto_state).await {
                        self.emit_on_enter_error(&auto_state, &e).await;
                        tracing::error!(error = %e, "on_enter failed for auto-transition state {:?}", auto_state);
                    }
                    return TransitionResult::AutoTransition(auto_state);
                }

                TransitionResult::Success(new_state)
            }
            Response::Handled => TransitionResult::Success(current_state.clone()),
            Response::NotHandled => TransitionResult::NotHandled,
        }
    }

    /// Emit a task:on_enter_error event for UI visibility.
    pub(crate) async fn emit_on_enter_error(&self, state: &State, error: &crate::error::AppError) {
        self.machine
            .context
            .services
            .event_emitter
            .emit_with_payload(
                "task:on_enter_error",
                &self.machine.context.task_id,
                &format!(r#"{{"state":"{:?}","error":"{}"}}"#, state, error),
            )
            .await;

        if let Some(context) = self.machine.context.services.notification_context.clone() {
            self.machine
                .context
                .services
                .notifier
                .notify(
                    context,
                    crate::domain::state_machine::services::TaskNotification::TaskStuck {
                        message: format!("Could not enter {state:?}: {error}"),
                    },
                )
                .await;
        }
    }

    /// Execute on-exit action for a state.
    ///
    /// Public so `TaskTransitionService` can trigger exit actions for direct status
    /// changes (e.g., stop command) without the full event-based transition flow.
    pub async fn on_exit(&self, from: &State, to: &State) {
        // Evict stale project stats cache whenever a task changes state.
        crate::commands::metrics_commands::invalidate_project_stats_cache(
            &self.machine.context.project_id,
        );

        // Decrement running count for agent-active states
        match from {
            State::Executing
            | State::QaRefining
            | State::QaTesting
            | State::Reviewing
            | State::ReExecuting
            | State::Merging => {
                if let Some(ref exec) = self.machine.context.services.execution_state {
                    exec.decrement_running();
                    let new_count = exec.running_count();
                    tracing::debug!(
                        task_id = %self.machine.context.task_id,
                        from_state = ?from,
                        new_count = new_count,
                        "Decremented running count on state exit"
                    );

                    if let Some(handle) = self.machine.context.services.event_sink.as_deref() {
                        exec.emit_status_changed_to_sink(handle, "task_completed");
                    }

                    if new_count == 0 {
                        tracing::info!(
                            task_id = %self.machine.context.task_id,
                            "All agents idle, triggering main merge retry"
                        );
                        self.try_retry_main_merges().await;
                    }

                    self.try_schedule_ready_tasks().await;
                }

                let ctx = self.exit_context();
                exit_actions::clear_trigger_origin_on_exit(&ctx).await;
            }
            _ => {}
        }

        // Defense-in-depth: stop poller when task transitions to Stopped or Cancelled
        // via normal state machine paths (non-cascade). This handles user-initiated stops,
        // bulk cancels, and any other path that bypasses cascade_stop_sibling_tasks. (AD11)
        if matches!(to, State::Stopped | State::Cancelled) {
            if let Some(ref registry) = self.machine.context.services.pr_poller_registry {
                let task_id = TaskId::from_string(self.machine.context.task_id.clone());
                registry.stop_polling(&task_id);
                tracing::debug!(
                    task_id = %self.machine.context.task_id,
                    to_state = ?to,
                    "on_exit: stopped poller (defense-in-depth for Stopped/Cancelled)"
                );
            }
        }

        // State-specific exit actions
        match from {
            State::Executing | State::ReExecuting => {
                let ctx = self.exit_context();
                exit_actions::auto_commit_on_execution_done(&ctx).await;
            }
            State::QaTesting => {}
            State::Reviewing => {
                self.machine
                    .context
                    .services
                    .event_emitter
                    .emit("review:state_exited", &self.machine.context.task_id)
                    .await;
            }
            State::PendingMerge | State::Merging => {
                let ctx = self.exit_context();
                exit_actions::spawn_deferred_merge_retry(&ctx, from, to);
            }
            _ => {}
        }
    }

    /// Check for auto-transitions from the given state
    pub fn check_auto_transition(&self, state: &State) -> Option<State> {
        match state {
            State::QaPassed => Some(State::PendingReview),
            State::RevisionNeeded => Some(State::ReExecuting),
            State::PendingReview => Some(State::Reviewing),
            State::Approved => Some(State::PendingMerge),
            _ => None,
        }
    }

    /// Try to schedule Ready tasks if execution slots are available.
    pub async fn try_schedule_ready_tasks(&self) {
        if let Some(ref scheduler) = self.machine.context.services.task_scheduler {
            tracing::debug!(
                task_id = %self.machine.context.task_id,
                "Triggering ready task scheduling"
            );
            scheduler.try_schedule_ready_tasks().await;
        }
    }

    /// Retry main-branch merges that were deferred because agents were running.
    pub async fn try_retry_main_merges(&self) {
        if let Some(ref scheduler) = self.machine.context.services.task_scheduler {
            tracing::debug!(
                task_id = %self.machine.context.task_id,
                "Triggering main merge retry (all agents idle)"
            );
            scheduler.try_retry_main_merges().await;
        }
    }
}
