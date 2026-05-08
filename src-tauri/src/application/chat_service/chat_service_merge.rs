// Merge auto-completion logic extracted from chat_service_send_background.rs
//
// Contains functions that handle automatic merge completion when the merger agent exits.
// Checks git state to determine if merge succeeded, had conflicts, or is incomplete.

use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::time::Duration;

use crate::application::chat_service::freshness_routing;
use crate::application::git_service::checkout_free::update_branch_ref;
use crate::application::git_service::{GitService, StaleRebaseResult};
use crate::application::harness_runtime_registry::{
    default_reconciliation_merge_watcher_grace_secs,
    default_reconciliation_merge_watcher_poll_secs, default_scheduler_merge_settle_ms,
};
use crate::application::interactive_process_registry::InteractiveProcessKey;
use crate::application::runtime_factory::{
    build_task_scheduler_with_fallback, build_transition_service_with_fallback, RuntimeFactoryDeps,
};
use crate::application::task_scheduler_service::TaskSchedulerService;
use crate::application::task_transition_service::TaskTransitionService;
use crate::application::AppState;
use crate::application::InteractiveProcessRegistry;
use crate::commands::ExecutionState;
use crate::domain::entities::{InternalStatus, Project, Task, TaskId};
use crate::domain::repositories::{
    ActivityEventRepository, AgentRunRepository, ArtifactRepository, ChatAttachmentRepository,
    ChatConversationRepository, ChatMessageRepository, ExecutionSettingsRepository,
    IdeationSessionRepository, MemoryEventRepository, PlanBranchRepository, ProjectRepository,
    TaskDependencyRepository, TaskRepository,
};
use crate::domain::services::{MessageQueue, RunningAgentRegistry};
use crate::domain::state_machine::resolve_merge_branches;
use crate::domain::state_machine::services::TaskScheduler;
use crate::domain::state_machine::transition_handler::{
    complete_merge_internal_with_pr_sync, PlanBranchPrSyncServices,
};
use crate::domain::state_machine::transition_handler::{
    format_validation_error_metadata, merge_metadata_into, parse_metadata, run_validation_commands,
    set_source_conflict_resolved,
};

/// RAII guard that removes a task from ExecutionState::auto_completes_in_flight on drop.
/// Ensures cleanup on all return paths, including early returns and panics.
#[doc(hidden)]
pub struct AutoCompleteGuard {
    execution_state: Arc<ExecutionState>,
    task_id: String,
}

impl Drop for AutoCompleteGuard {
    fn drop(&mut self) {
        self.execution_state.finish_auto_complete(&self.task_id);
    }
}

impl AutoCompleteGuard {
    #[doc(hidden)]
    pub fn for_test(execution_state: Arc<ExecutionState>, task_id: impl Into<String>) -> Self {
        Self {
            execution_state,
            task_id: task_id.into(),
        }
    }
}

/// Shared context for merge auto-complete operations.
/// Bundles the repository/service references passed through the merge pipeline.
pub(crate) struct MergeAutoCompleteContext<'a, R: Runtime> {
    pub task_id_str: &'a str,
    pub task_id: TaskId,
    pub task_repo: &'a Arc<dyn TaskRepository>,
    pub task_dependency_repo: &'a Arc<dyn TaskDependencyRepository>,
    pub project_repo: &'a Arc<dyn ProjectRepository>,
    pub artifact_repo: &'a Arc<dyn ArtifactRepository>,
    pub chat_message_repo: &'a Arc<dyn ChatMessageRepository>,
    pub chat_attachment_repo: &'a Arc<dyn ChatAttachmentRepository>,
    pub conversation_repo: &'a Arc<dyn ChatConversationRepository>,
    pub agent_run_repo: &'a Arc<dyn AgentRunRepository>,
    pub ideation_session_repo: &'a Arc<dyn IdeationSessionRepository>,
    pub activity_event_repo: &'a Arc<dyn ActivityEventRepository>,
    pub message_queue: &'a Arc<MessageQueue>,
    pub running_agent_registry: &'a Arc<dyn RunningAgentRegistry>,
    pub memory_event_repo: &'a Arc<dyn MemoryEventRepository>,
    pub execution_state: &'a Arc<ExecutionState>,
    pub execution_settings_repo: Option<&'a Arc<dyn ExecutionSettingsRepository>>,
    pub plan_branch_repo: &'a Option<Arc<dyn PlanBranchRepository>>,
    pub app_handle: Option<&'a AppHandle<R>>,
    pub interactive_process_registry: &'a Option<Arc<InteractiveProcessRegistry>>,
}

impl<'a, R: Runtime + 'static> MergeAutoCompleteContext<'a, R> {
    fn build_runtime_factory_deps(&self) -> RuntimeFactoryDeps {
        RuntimeFactoryDeps::from_core(
            Arc::clone(self.task_repo),
            Arc::clone(self.task_dependency_repo),
            Arc::clone(self.project_repo),
            Arc::clone(self.artifact_repo),
            Arc::clone(self.chat_message_repo),
            Arc::clone(self.chat_attachment_repo),
            Arc::clone(self.conversation_repo),
            Arc::clone(self.agent_run_repo),
            Arc::clone(self.ideation_session_repo),
            Arc::clone(self.activity_event_repo),
            Arc::clone(self.message_queue),
            Arc::clone(self.running_agent_registry),
            Arc::clone(self.memory_event_repo),
        )
        .with_runtime_support(
            self.execution_settings_repo.map(Arc::clone),
            self.app_handle.and_then(|handle| {
                handle
                    .try_state::<AppState>()
                    .map(|app_state| Arc::clone(&app_state.agent_lane_settings_repo))
            }),
            self.plan_branch_repo.clone(),
            self.interactive_process_registry.clone(),
        )
    }

    fn build_transition_service(&self) -> Arc<TaskTransitionService<R>> {
        let deps = self.build_runtime_factory_deps();
        build_transition_service_with_fallback(
            &self.app_handle.cloned(),
            Arc::clone(self.execution_state),
            &deps,
        )
        .into_arc()
    }

    fn build_scheduler_service(&self) -> TaskSchedulerService<R> {
        let deps = self.build_runtime_factory_deps();
        build_task_scheduler_with_fallback(
            &self.app_handle.cloned(),
            Arc::clone(self.execution_state),
            &deps,
        )
    }

    async fn transition_incomplete(&self, reason: &str) {
        tracing::info!(
            task_id = self.task_id_str,
            reason = reason,
            "transition_to_merge_incomplete: transitioning task"
        );
        if let Err(e) = self
            .build_transition_service()
            .transition_task(&self.task_id, InternalStatus::MergeIncomplete)
            .await
        {
            tracing::error!(
                task_id = self.task_id_str,
                error = %e,
                "transition_to_merge_incomplete: failed to transition"
            );
        }
    }

    async fn transition_conflict(&self, reason: &str) {
        tracing::info!(
            task_id = self.task_id_str,
            reason = reason,
            "transition_to_merge_conflict: transitioning task"
        );
        if let Err(e) = self
            .build_transition_service()
            .transition_task(&self.task_id, InternalStatus::MergeConflict)
            .await
        {
            tracing::error!(
                task_id = self.task_id_str,
                error = %e,
                "transition_to_merge_conflict: failed to transition"
            );
        }
    }

    async fn retry_pending_merge(&self, reason: &str) {
        let transition_service = self.build_transition_service();
        match transition_service
            .transition_task_corrective_with_exit(
                &self.task_id,
                InternalStatus::PendingMerge,
                None,
                "merge_auto_complete",
            )
            .await
        {
            Ok(updated) => {
                transition_service
                    .execute_entry_actions(&self.task_id, &updated, InternalStatus::PendingMerge)
                    .await;
            }
            Err(e) => {
                tracing::error!(
                    task_id = self.task_id_str,
                    reason = reason,
                    error = %e,
                    "attempt_merge_auto_complete: failed to retry task merge via PendingMerge"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Sub-functions: each handles one phase of the merge auto-complete pipeline.
// ---------------------------------------------------------------------------

/// Get task and verify it is in Merging state.
/// Returns `None` if the task is missing, already transitioned, or on error.
async fn get_task_in_merging_state<R: Runtime + 'static>(
    ctx: &MergeAutoCompleteContext<'_, R>,
) -> Option<Task> {
    let task = match ctx.task_repo.get_by_id(&ctx.task_id).await {
        Ok(Some(task)) => task,
        Ok(None) => {
            tracing::warn!(
                task_id = ctx.task_id_str,
                "attempt_merge_auto_complete: task not found"
            );
            return None;
        }
        Err(e) => {
            tracing::error!(
                task_id = ctx.task_id_str,
                error = %e,
                "attempt_merge_auto_complete: failed to get task"
            );
            return None;
        }
    };

    // If task is not in Merging state, agent already transitioned (called complete_merge or report_conflict)
    if task.internal_status != InternalStatus::Merging {
        tracing::info!(
            task_id = ctx.task_id_str,
            status = ?task.internal_status,
            "attempt_merge_auto_complete: task already transitioned, skipping"
        );
        // Defence-in-depth: if task reached Merged via a path that bypassed TransitionHandler
        // (e.g. programmatic merge in side_effects.rs), unblock_dependents may not have fired.
        // Calling it here is idempotent — blocked→ready only applies if the dependent is still Blocked.
        if task.internal_status == InternalStatus::Merged {
            use crate::application::task_transition_service::RepoBackedDependencyManager;
            use crate::domain::state_machine::services::DependencyManager;

            let dependency_manager = RepoBackedDependencyManager::new(
                Arc::clone(ctx.task_dependency_repo),
                Arc::clone(ctx.task_repo),
                ctx.app_handle.cloned(),
            );
            dependency_manager.unblock_dependents(ctx.task_id_str).await;
        }
        return None;
    }

    Some(task)
}

/// Resolve the project and worktree path for a merging task.
/// Returns `None` if the project is missing or the worktree path is invalid.
async fn resolve_environment<R: Runtime + 'static>(
    ctx: &MergeAutoCompleteContext<'_, R>,
    task: &Task,
) -> Option<(Project, PathBuf)> {
    let project = match ctx.project_repo.get_by_id(&task.project_id).await {
        Ok(Some(project)) => project,
        Ok(None) => {
            tracing::error!(
                task_id = ctx.task_id_str,
                project_id = task.project_id.as_str(),
                "attempt_merge_auto_complete: project not found"
            );
            return None;
        }
        Err(e) => {
            tracing::error!(
                task_id = ctx.task_id_str,
                error = %e,
                "attempt_merge_auto_complete: failed to get project"
            );
            return None;
        }
    };

    // Resolve working path: MUST be a valid worktree or the main repo if that's where
    // the merge happened (checkout-free validation recovery sets merge_path = repo_path).
    // Never silently fall back to the main repo — that would run git operations in the
    // user's checkout and potentially switch their branch.
    let worktree_path = match &task.worktree_path {
        Some(wt) => {
            let path = PathBuf::from(wt);
            if crate::utils::path_safety::checked_exists(&path, "stored merge worktree")
                .unwrap_or(false)
            {
                path
            } else {
                tracing::error!(
                    task_id = ctx.task_id_str,
                    worktree_path = wt.as_str(),
                    "attempt_merge_auto_complete: worktree path was set but does not exist. \
                     Aborting to avoid running git operations in the user's main checkout."
                );
                return None;
            }
        }
        None => {
            tracing::error!(
                task_id = ctx.task_id_str,
                "attempt_merge_auto_complete: task has no worktree_path. \
                 Cannot safely determine merge working directory."
            );
            return None;
        }
    };

    Some((project, worktree_path))
}

/// Check git state for in-progress rebase/merge and conflict markers.
/// Returns `Break` if the task was transitioned and the caller should return early.
async fn check_git_state<R: Runtime + 'static>(
    ctx: &MergeAutoCompleteContext<'_, R>,
    worktree: &Path,
) -> ControlFlow<()> {
    match GitService::try_complete_stale_rebase(worktree).await {
        StaleRebaseResult::Completed => {
            tracing::info!(
                task_id = ctx.task_id_str,
                "attempt_merge_auto_complete: stale rebase completed successfully, continuing verification"
            );
            // Continue to remaining merge verification steps below
        }
        StaleRebaseResult::HasConflicts { files } => {
            tracing::info!(
                task_id = ctx.task_id_str,
                conflict_count = files.len(),
                "attempt_merge_auto_complete: stale rebase has real conflicts, transitioning to MergeConflict"
            );
            ctx.transition_conflict("Stale rebase has unresolved conflicts")
                .await;
            return ControlFlow::Break(());
        }
        StaleRebaseResult::Failed { reason } => {
            tracing::info!(
                task_id = ctx.task_id_str,
                reason = &reason,
                "attempt_merge_auto_complete: stale rebase recovery failed, transitioning to MergeConflict"
            );
            ctx.transition_conflict(&format!("Stale rebase recovery failed: {}", reason))
                .await;
            return ControlFlow::Break(());
        }
        StaleRebaseResult::NoRebase => {
            // No rebase in progress, continue to next checks
        }
    }

    // Safety net: check if rebase is somehow still in progress after recovery attempt
    if GitService::is_rebase_in_progress(worktree) {
        tracing::info!(
            task_id = ctx.task_id_str,
            "attempt_merge_auto_complete: rebase still in progress after recovery attempt, transitioning to MergeConflict"
        );
        ctx.transition_conflict("Rebase still in progress after recovery attempt")
            .await;
        return ControlFlow::Break(());
    }

    if GitService::is_merge_in_progress(worktree) {
        tracing::info!(
            task_id = ctx.task_id_str,
            "attempt_merge_auto_complete: merge in progress (MERGE_HEAD exists), transitioning to MergeConflict"
        );
        ctx.transition_conflict("Agent exited with incomplete merge (MERGE_HEAD exists)")
            .await;
        return ControlFlow::Break(());
    }

    match GitService::has_conflict_markers(worktree).await {
        Ok(true) => {
            tracing::info!(
                task_id = ctx.task_id_str,
                "attempt_merge_auto_complete: conflict markers found, transitioning to MergeConflict"
            );
            ctx.transition_conflict("Agent exited with unresolved conflict markers")
                .await;
            return ControlFlow::Break(());
        }
        Ok(false) => {
            // No conflicts - merge succeeded!
        }
        Err(e) => {
            tracing::error!(
                task_id = ctx.task_id_str,
                error = %e,
                "attempt_merge_auto_complete: failed to check conflict markers, transitioning to MergeIncomplete"
            );
            ctx.transition_incomplete(&format!("Auto-complete failed: {}", e))
                .await;
            return ControlFlow::Break(());
        }
    }

    ControlFlow::Continue(())
}

/// Resolve merge branches, parse task metadata, and apply TOCTOU guard for target branch.
/// Returns `None` if the source branch cannot be determined.
async fn resolve_branches_and_metadata<R: Runtime + 'static>(
    ctx: &MergeAutoCompleteContext<'_, R>,
    task: &Task,
    project: &Project,
) -> Option<(String, String, Option<serde_json::Value>)> {
    let (source_branch, mut target_branch) =
        resolve_merge_branches(task, project, ctx.plan_branch_repo).await;

    // Guard: source_branch should never be empty after resolve_merge_branches
    if source_branch.is_empty() {
        tracing::error!(
            task_id = ctx.task_id_str,
            "attempt_merge_auto_complete: source_branch is empty after resolve_merge_branches"
        );
        ctx.transition_incomplete("Auto-complete failed: could not determine source branch name")
            .await;
        return None;
    }

    let meta: Option<serde_json::Value> = task
        .metadata
        .as_ref()
        .and_then(|m| serde_json::from_str(m).ok());

    // TOCTOU guard: use the target branch that was resolved when the merge was initiated,
    // not the re-resolved value which may differ if plan state changed since then.
    if let Some(stored) = meta.as_ref().and_then(|v| {
        v.get("merge_target_branch")
            .or_else(|| v.get("target_branch"))
            .and_then(|s| s.as_str().map(String::from))
    }) {
        if stored != target_branch {
            tracing::info!(
                task_id = ctx.task_id_str,
                resolved = %target_branch,
                from_metadata = %stored,
                "Using target_branch from task metadata (TOCTOU guard)"
            );
            target_branch = stored;
        }
    }

    Some((source_branch, target_branch, meta))
}

/// Handle plan_update_conflict resolution: verify plan branch is up-to-date, clear flag, retry.
/// Returns `Break` if the conflict was present (handled or failed) — caller should return.
/// Returns `Continue` if no plan_update_conflict flag was set.
async fn handle_plan_update_resolution<R: Runtime + 'static>(
    ctx: &MergeAutoCompleteContext<'_, R>,
    task: &mut Task,
    meta: &Option<serde_json::Value>,
    main_repo_path: &Path,
    target_branch: &str,
    project: &Project,
) -> ControlFlow<()> {
    let is_plan_update_conflict = meta
        .as_ref()
        .and_then(|v| v.get("plan_update_conflict")?.as_bool())
        .unwrap_or(false);

    if !is_plan_update_conflict {
        return ControlFlow::Continue(());
    }

    let base_branch = meta
        .as_ref()
        .and_then(|v| v.get("base_branch")?.as_str().map(String::from))
        .unwrap_or_else(|| "main".to_string());
    // Use target_branch from metadata: it was stored at conflict-detection time and is
    // more reliable than re-resolving via resolve_merge_branches, which can return the
    // base branch (e.g. "main") if the plan branch state changed between when the
    // conflict was detected and when this auto-complete runs.
    let plan_branch = meta
        .as_ref()
        .and_then(|v| v.get("target_branch")?.as_str().map(String::from))
        .unwrap_or_else(|| target_branch.to_string());

    // Check if the plan branch is now up-to-date with base_branch
    let plan_up_to_date = match GitService::get_branch_sha(main_repo_path, &base_branch).await {
        Ok(main_sha) => GitService::is_commit_on_branch(main_repo_path, &main_sha, &plan_branch)
            .await
            .unwrap_or(false),
        Err(e) => {
            tracing::warn!(
                task_id = ctx.task_id_str,
                error = %e,
                "attempt_merge_auto_complete: failed to get base branch SHA for plan_update_conflict check"
            );
            false
        }
    };

    if plan_up_to_date {
        // Retry limit: prevent infinite merge loops (max 2 retries)
        let retry_count = meta
            .as_ref()
            .and_then(|v| v.get("plan_update_retry_count")?.as_u64())
            .unwrap_or(0);
        if retry_count >= 2 {
            tracing::warn!(
                task_id = ctx.task_id_str,
                retry_count = retry_count,
                "attempt_merge_auto_complete: plan_update_conflict max retries exceeded"
            );
            ctx.transition_incomplete("Plan update conflict: max retries exceeded")
                .await;
            return ControlFlow::Break(());
        }

        tracing::info!(
            task_id = ctx.task_id_str,
            plan_branch = %plan_branch,
            base_branch = %base_branch,
            retry_count = retry_count,
            "attempt_merge_auto_complete: plan branch now up-to-date with base — retrying task merge (attempt {})",
            retry_count + 1
        );
        // RC#12: Clean up the merge-{id} worktree left over from Phase 1 (plan_update)
        // before retrying the task merge. Without this, Phase 2 fails with
        // "fatal: '/path/merge-{id}' already exists" when source_update_conflict
        // tries to create the same worktree path.
        {
            use crate::domain::state_machine::transition_handler::compute_merge_worktree_path;
            let merge_wt_path =
                PathBuf::from(compute_merge_worktree_path(project, ctx.task_id_str));
            if crate::utils::path_safety::checked_exists(&merge_wt_path, "merge worktree")
                .unwrap_or(false)
            {
                if let Err(e) = GitService::delete_worktree(main_repo_path, &merge_wt_path).await {
                    tracing::warn!(
                        task_id = ctx.task_id_str,
                        error = %e,
                        "attempt_merge_auto_complete: failed to clean merge worktree after plan update (non-fatal)"
                    );
                }
            }
        }
        // Clear plan_update_conflict flag, increment retry counter
        {
            let mut m = meta.clone().unwrap_or_else(|| serde_json::json!({}));
            if let Some(obj) = m.as_object_mut() {
                obj.remove("plan_update_conflict");
                obj.remove("conflict_files");
                obj.remove("error");
                obj.insert(
                    "plan_update_retry_count".to_string(),
                    serde_json::json!(retry_count + 1),
                );
            }
            task.metadata = Some(m.to_string());
            task.touch();
            if let Err(e) = ctx.task_repo.update(task).await {
                tracing::warn!(
                    task_id = ctx.task_id_str,
                    error = %e,
                    "attempt_merge_auto_complete: failed to clear plan_update_conflict metadata"
                );
            }
        }
        ctx.retry_pending_merge("plan_update_conflict resolved")
            .await;
    } else {
        tracing::warn!(
            task_id = ctx.task_id_str,
            plan_branch = %plan_branch,
            base_branch = %base_branch,
            "attempt_merge_auto_complete: plan branch still not up-to-date — merger agent did not resolve plan←main conflict"
        );
        ctx.transition_incomplete(&format!(
            "Merger agent exited but plan branch {} was not updated from {}",
            plan_branch, base_branch
        ))
        .await;
    }
    ControlFlow::Break(())
}

/// Handle source_update_conflict resolution: verify source is up-to-date with target, retry.
/// Returns `Break` if the conflict was present — caller should return.
/// Returns `Continue` if no source_update_conflict flag was set.
async fn handle_source_update_resolution<R: Runtime + 'static>(
    ctx: &MergeAutoCompleteContext<'_, R>,
    task: &mut Task,
    meta: &Option<serde_json::Value>,
    source_branch: &str,
    target_branch: &str,
    main_repo_path: &Path,
) -> ControlFlow<()> {
    let is_source_update_conflict = meta
        .as_ref()
        .and_then(|v| v.get("source_update_conflict")?.as_bool())
        .unwrap_or(false);

    if !is_source_update_conflict {
        return ControlFlow::Continue(());
    }

    // Use target_branch from metadata: same reasoning as plan_update_conflict above —
    // resolve_merge_branches may return the base branch if plan branch state changed.
    let target_branch = meta
        .as_ref()
        .and_then(|v| v.get("target_branch")?.as_str().map(String::from))
        .unwrap_or_else(|| target_branch.to_string());
    let source_up_to_date = match GitService::get_branch_sha(main_repo_path, &target_branch).await {
        Ok(target_sha) => {
            GitService::is_commit_on_branch(main_repo_path, &target_sha, source_branch)
                .await
                .unwrap_or(false)
        }
        Err(e) => {
            tracing::warn!(
                task_id = ctx.task_id_str,
                error = %e,
                "attempt_merge_auto_complete: failed to get target branch SHA for source_update_conflict check"
            );
            false
        }
    };

    if source_up_to_date {
        // Retry limit: prevent infinite merge loops (max 2 retries)
        let retry_count = meta
            .as_ref()
            .and_then(|v| v.get("source_update_retry_count")?.as_u64())
            .unwrap_or(0);
        if retry_count >= 2 {
            tracing::warn!(
                task_id = ctx.task_id_str,
                retry_count = retry_count,
                "attempt_merge_auto_complete: source_update_conflict max retries exceeded"
            );
            ctx.transition_incomplete("Source update conflict: max retries exceeded")
                .await;
            return ControlFlow::Break(());
        }

        tracing::info!(
            task_id = ctx.task_id_str,
            source_branch = %source_branch,
            target_branch = %target_branch,
            retry_count = retry_count,
            "attempt_merge_auto_complete: source branch now up-to-date with target — retrying task merge (attempt {})",
            retry_count + 1
        );
        // Clear source_update_conflict flag, increment retry counter, and set
        // source_conflict_resolved so the PendingMerge retry uses squash-only instead of
        // rebase. Rebasing would drop the agent's merge commit (target merged INTO source)
        // and replay individual commits, re-encountering the same conflicts.
        {
            let mut m = meta.clone().unwrap_or_else(|| serde_json::json!({}));
            if let Some(obj) = m.as_object_mut() {
                obj.remove("source_update_conflict");
                obj.remove("conflict_files");
                obj.remove("error");
                obj.insert(
                    "source_update_retry_count".to_string(),
                    serde_json::json!(retry_count + 1),
                );
            }
            task.metadata = Some(m.to_string());
            set_source_conflict_resolved(task);
            task.touch();
            if let Err(e) = ctx.task_repo.update(task).await {
                tracing::warn!(
                    task_id = ctx.task_id_str,
                    error = %e,
                    "attempt_merge_auto_complete: failed to clear source_update_conflict metadata"
                );
            }
        }
        ctx.retry_pending_merge("source_update_conflict resolved")
            .await;
    } else {
        tracing::warn!(
            task_id = ctx.task_id_str,
            source_branch = %source_branch,
            target_branch = %target_branch,
            "attempt_merge_auto_complete: source branch still not up-to-date — merger agent did not resolve source←target conflict"
        );
        ctx.transition_incomplete(&format!(
            "Merger agent exited but source branch {} is not yet up-to-date with {}",
            source_branch, target_branch
        ))
        .await;
    }
    ControlFlow::Break(())
}

/// Handle validation recovery mode: re-run validation commands after agent fix attempt.
/// Returns `Break` if validation failed and the caller should return early.
/// Returns `Continue` if not in recovery mode or validation passed.
async fn handle_validation_recovery<R: Runtime + 'static>(
    ctx: &MergeAutoCompleteContext<'_, R>,
    task: &mut Task,
    worktree_path: &Path,
    worktree: &Path,
    main_repo_path: &Path,
    project: &Project,
    is_validation_recovery: bool,
) -> ControlFlow<()> {
    if !is_validation_recovery {
        return ControlFlow::Continue(());
    }

    tracing::info!(
        task_id = ctx.task_id_str,
        "attempt_merge_auto_complete: validation recovery mode — re-running validation"
    );

    // Clear stale validation data and set revalidating flag so the UI shows re-validation state
    {
        let mut val = task
            .metadata
            .as_deref()
            .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        if let Some(obj) = val.as_object_mut() {
            obj.remove("validation_log");
            obj.remove("validation_failures");
            obj.insert("revalidating".to_string(), serde_json::json!(true));
        }
        task.metadata = Some(val.to_string());
        task.touch();
        let _ = ctx.task_repo.update(task).await;
    }

    // Emit validation_start event so the frontend clears stale live steps
    if let Some(handle) = ctx.app_handle {
        let _ = handle.emit(
            "merge:validation_start",
            serde_json::json!({
                "task_id": ctx.task_id_str,
            }),
        );
    }

    // Downcast generic app_handle to Wry for run_validation_commands
    let wry_handle: Option<tauri::AppHandle<tauri::Wry>> = ctx.app_handle.and_then(|h| {
        let any: Box<dyn std::any::Any> = Box::new(h.clone());
        any.downcast::<tauri::AppHandle<tauri::Wry>>()
            .ok()
            .map(|b| *b)
    });

    // Re-run validation commands on the merge path
    let validation_cancel = tokio_util::sync::CancellationToken::new();
    match run_validation_commands(
        project,
        task,
        worktree,
        ctx.task_id_str,
        wry_handle.as_ref(),
        None,
        &project.merge_validation_mode,
        &validation_cancel,
    )
    .await
    {
        Some(result) if !result.all_passed => {
            // Agent didn't fix it — revert and fall back to MergeIncomplete
            tracing::warn!(
                task_id = ctx.task_id_str,
                failure_count = result.failures.len(),
                "attempt_merge_auto_complete: re-validation failed, reverting merge"
            );
            // Defence-in-depth: never reset_hard on the main repo
            if worktree_path == main_repo_path {
                tracing::error!(
                    task_id = ctx.task_id_str,
                    "attempt_merge_auto_complete: BUG — worktree_path equals main_repo_path, \
                     refusing to reset_hard on user's checkout"
                );
            } else if let Err(e) = GitService::reset_hard(worktree, "HEAD~1").await {
                tracing::error!(
                    task_id = ctx.task_id_str,
                    error = %e,
                    "attempt_merge_auto_complete: failed to revert merge after validation failure"
                );
            }

            // Update task metadata with validation failure details
            let (source_branch, target_branch) =
                resolve_merge_branches(task, project, ctx.plan_branch_repo).await;
            let error_metadata_str = format_validation_error_metadata(
                &result.failures,
                &result.log,
                &source_branch,
                &target_branch,
            );
            if let Ok(error_obj) = serde_json::from_str::<serde_json::Value>(&error_metadata_str) {
                merge_metadata_into(task, &error_obj);
            }
            let prev_revert_count: u32 = parse_metadata(task)
                .and_then(|v| v.get("validation_revert_count")?.as_u64())
                .unwrap_or(0) as u32;
            merge_metadata_into(
                task,
                &serde_json::json!({
                    "validation_revert_count": prev_revert_count + 1,
                    "merge_failure_source": "ValidationFailed",
                }),
            );
            // Remove revalidating flag (merge_metadata_into only inserts, so remove manually)
            {
                let mut meta = parse_metadata(task).unwrap_or_else(|| serde_json::json!({}));
                if let Some(obj) = meta.as_object_mut() {
                    obj.remove("revalidating");
                }
                task.metadata = Some(meta.to_string());
            }
            task.touch();
            let _ = ctx.task_repo.update(task).await;

            ctx.transition_incomplete("Validation re-check failed after agent fix attempt")
                .await;
            return ControlFlow::Break(());
        }
        Some(result) => {
            tracing::info!(
                task_id = ctx.task_id_str,
                "attempt_merge_auto_complete: re-validation passed — proceeding to complete merge"
            );
            // Update metadata with fresh validation_log so the UI shows the new results
            let mut merged_meta = task
                .metadata
                .as_deref()
                .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
                .unwrap_or_else(|| serde_json::json!({}));
            if let Some(obj) = merged_meta.as_object_mut() {
                obj.insert("validation_log".to_string(), serde_json::json!(result.log));
                obj.remove("revalidating");
            }
            task.metadata = Some(merged_meta.to_string());
            task.touch();
            let _ = ctx.task_repo.update(task).await;
        }
        None => {
            // No validation commands configured — proceed normally
            tracing::info!(
                task_id = ctx.task_id_str,
                "attempt_merge_auto_complete: no validation commands found, proceeding"
            );
            // Clear revalidating flag
            let mut val = task
                .metadata
                .as_deref()
                .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
                .unwrap_or_else(|| serde_json::json!({}));
            if let Some(obj) = val.as_object_mut() {
                obj.remove("revalidating");
            }
            task.metadata = Some(val.to_string());
            task.touch();
            let _ = ctx.task_repo.update(task).await;
        }
    }

    ControlFlow::Continue(())
}

/// Resolve the merge commit SHA via fast-forward or verification.
/// For validation recovery, uses target branch HEAD directly (ancestry check would fail for squash merges).
/// For normal merges, fast-forwards target from merge-resolve branch if present, then verifies.
/// Returns `None` if the merge could not be verified (task was transitioned to MergeIncomplete).
async fn resolve_merge_commit<R: Runtime + 'static>(
    ctx: &MergeAutoCompleteContext<'_, R>,
    task: &Task,
    main_repo_path: &Path,
    source_branch: &str,
    target_branch: &str,
    is_validation_recovery: bool,
) -> Option<String> {
    if is_validation_recovery {
        return match GitService::get_branch_sha(main_repo_path, target_branch).await {
            Ok(sha) => {
                tracing::info!(
                    task_id = ctx.task_id_str,
                    target_sha = %sha,
                    "attempt_merge_auto_complete: validation recovery — skipping ancestry check, using target HEAD"
                );
                Some(sha)
            }
            Err(e) => {
                tracing::error!(
                    task_id = ctx.task_id_str,
                    error = %e,
                    target_branch = %target_branch,
                    "attempt_merge_auto_complete: validation recovery — failed to get target branch SHA"
                );
                ctx.transition_incomplete(&format!(
                    "Auto-complete failed: cannot resolve target branch {}: {}",
                    target_branch, e
                ))
                .await;
                None
            }
        };
    }

    // Fast-forward target branch from merge-resolve/{task_id} if it exists.
    // When checkout-free merge detects conflicts, merge_outcome_handler creates a
    // merge-resolve/{task_id} branch + worktree for agent conflict resolution.
    // After the agent resolves and commits, the merge commit lives on merge-resolve/,
    // NOT on the target branch. We must fast-forward the target branch before verification.
    let resolve_branch = format!("merge-resolve/{}", ctx.task_id_str);
    if let Ok(resolve_sha) = GitService::get_branch_sha(main_repo_path, &resolve_branch).await {
        tracing::info!(
            task_id = ctx.task_id_str,
            resolve_branch = %resolve_branch,
            resolve_sha = %resolve_sha,
            target_branch = %target_branch,
            "attempt_merge_auto_complete: merge-resolve branch found, fast-forwarding target branch"
        );
        if let Err(e) = update_branch_ref(main_repo_path, target_branch, &resolve_sha).await {
            tracing::error!(
                task_id = ctx.task_id_str,
                error = %e,
                target_branch = %target_branch,
                resolve_sha = %resolve_sha,
                "attempt_merge_auto_complete: failed to fast-forward target branch from merge-resolve"
            );
            ctx.transition_incomplete(&format!(
                "Auto-complete failed: could not fast-forward {} to merge-resolve commit {}: {}",
                target_branch, resolve_sha, e
            ))
            .await;
            return None;
        }
        tracing::info!(
            task_id = ctx.task_id_str,
            target_branch = %target_branch,
            resolve_sha = %resolve_sha,
            "attempt_merge_auto_complete: target branch fast-forwarded successfully"
        );

        // Clean up: delete the merge-resolve worktree and branch
        let merge_wt_path = task.worktree_path.as_deref().map(PathBuf::from);
        if let Some(ref wt_path) = merge_wt_path {
            if crate::utils::path_safety::checked_exists(wt_path, "merge resolve worktree")
                .unwrap_or(false)
            {
                if let Err(e) = GitService::delete_worktree(main_repo_path, wt_path).await {
                    tracing::warn!(
                        task_id = ctx.task_id_str,
                        error = %e,
                        worktree = %wt_path.display(),
                        "attempt_merge_auto_complete: failed to delete merge-resolve worktree (non-fatal)"
                    );
                }
            }
        }
        if let Err(e) = GitService::delete_branch(main_repo_path, &resolve_branch, true).await {
            tracing::warn!(
                task_id = ctx.task_id_str,
                error = %e,
                resolve_branch = %resolve_branch,
                "attempt_merge_auto_complete: failed to delete merge-resolve branch (non-fatal)"
            );
        }
    }

    match verify_merge_on_target(main_repo_path, source_branch, target_branch).await {
        MergeVerification::Merged(sha) => {
            // Task branch is merged - capture the merge commit SHA
            Some(sha)
        }
        MergeVerification::NotMerged => {
            tracing::warn!(
                task_id = ctx.task_id_str,
                source_branch = %source_branch,
                target_branch = %target_branch,
                "attempt_merge_auto_complete: task branch not merged to target, transitioning to MergeIncomplete"
            );
            ctx.transition_incomplete(&format!(
                "Agent exited but task branch {} not merged to {}",
                source_branch, target_branch
            ))
            .await;
            None
        }
        MergeVerification::SourceBranchMissing => {
            tracing::error!(
                task_id = ctx.task_id_str,
                source_branch = %source_branch,
                "attempt_merge_auto_complete: source branch does not exist or cannot be resolved"
            );
            ctx.transition_incomplete(&format!(
                "Auto-complete failed: source branch {} does not exist or cannot be resolved",
                source_branch
            ))
            .await;
            None
        }
        MergeVerification::TargetBranchMissing => {
            tracing::error!(
                task_id = ctx.task_id_str,
                target_branch = %target_branch,
                "attempt_merge_auto_complete: target branch does not exist or cannot be resolved"
            );
            ctx.transition_incomplete(&format!(
                "Auto-complete failed: target branch {} does not exist or cannot be resolved",
                target_branch
            ))
            .await;
            None
        }
    }
}

/// Complete the merge via complete_merge_internal, unblock dependents, and schedule ready tasks.
async fn complete_merge_and_schedule<R: Runtime + 'static>(
    ctx: &MergeAutoCompleteContext<'_, R>,
    task: &mut Task,
    project: &Project,
    commit_sha: &str,
    source_branch: &str,
    target_branch: &str,
    worktree_path: &Path,
    main_repo_path: &Path,
    worktree: &Path,
) {
    // Idempotency guard: prevent duplicate merge completion
    if task.merge_commit_sha.is_some() {
        tracing::info!(
            task_id = ctx.task_id_str,
            existing_sha = ?task.merge_commit_sha,
            "Merge already completed with existing commit SHA — skipping duplicate completion"
        );
        return;
    }

    tracing::info!(
        task_id = ctx.task_id_str,
        commit_sha = %commit_sha,
        "attempt_merge_auto_complete: merge verified on target branch, completing"
    );

    let app_state = ctx
        .app_handle
        .and_then(|handle| handle.try_state::<AppState>());
    let pr_sync_services = PlanBranchPrSyncServices {
        task_repo: Some(Arc::clone(ctx.task_repo)),
        plan_branch_repo: ctx.plan_branch_repo.clone(),
        pr_creation_guard: app_state
            .as_ref()
            .map(|state| Arc::clone(&state.pr_poller_registry.pr_creation_guard)),
        github_service: app_state
            .as_ref()
            .and_then(|state| state.github_service.clone()),
        ideation_session_repo: Some(Arc::clone(ctx.ideation_session_repo)),
        artifact_repo: Some(Arc::clone(ctx.artifact_repo)),
    };

    if let Err(e) = complete_merge_internal_with_pr_sync(
        task,
        project,
        commit_sha,
        source_branch,
        target_branch,
        ctx.task_repo,
        None,
        None,
        ctx.app_handle,
        None,
        Some(pr_sync_services),
    )
    .await
    {
        tracing::error!(
            task_id = ctx.task_id_str,
            error = %e,
            "attempt_merge_auto_complete: complete_merge_internal failed — transitioning to MergeIncomplete"
        );
        // Clean up fixer worktree on failure (if it's not the main repo)
        if worktree_path != main_repo_path {
            if let Err(cleanup_err) = GitService::delete_worktree(main_repo_path, worktree).await {
                tracing::warn!(task_id = ctx.task_id_str, error = %cleanup_err, "Failed to cleanup fixer worktree after merge failure (non-fatal)");
            }
        }
        // Transition via TaskTransitionService so on_exit(Merging) fires:
        // - decrements running_count (prevents concurrency limit leak)
        // - triggers try_retry_deferred_merges
        // - surfaces task in needs_attention panel
        ctx.transition_incomplete(&format!(
            "Auto-complete failed: complete_merge_internal error: {}",
            e
        ))
        .await;
    } else {
        // Phase 3: fire-and-forget cleanup (worktree, branch, metadata)
        {
            use crate::domain::state_machine::transition_handler::deferred_merge_cleanup;
            let cleanup_task_id = ctx.task_id.clone();
            let cleanup_repo = Arc::clone(ctx.task_repo);
            let cleanup_dir = project.working_directory.clone();
            let cleanup_branch = task.task_branch.clone();
            let cleanup_wt = task.worktree_path.clone();
            let cleanup_plan_branch = Some(target_branch.to_string());
            tokio::spawn(async move {
                deferred_merge_cleanup(
                    cleanup_task_id,
                    cleanup_repo,
                    cleanup_dir,
                    cleanup_branch,
                    cleanup_wt,
                    cleanup_plan_branch,
                )
                .await;
            });
        }

        // Auto-unblock tasks that were waiting on this task
        // (auto-complete merge path - on_enter(Merged) won't be triggered)
        use crate::application::task_transition_service::RepoBackedDependencyManager;
        use crate::domain::state_machine::services::DependencyManager;

        let dependency_manager = RepoBackedDependencyManager::new(
            Arc::clone(ctx.task_dependency_repo),
            Arc::clone(ctx.task_repo),
            ctx.app_handle.cloned(),
        );
        dependency_manager.unblock_dependents(ctx.task_id_str).await;

        // Clean up fixer worktree after successful merge (if it's not the main repo)
        if worktree_path != main_repo_path {
            if let Err(cleanup_err) = GitService::delete_worktree(main_repo_path, worktree).await {
                tracing::warn!(task_id = ctx.task_id_str, error = %cleanup_err, "Failed to cleanup fixer worktree after merge success (non-fatal)");
            }
        }

        // Schedule newly-unblocked tasks (e.g. plan_merge tasks that just became Ready)
        let scheduler = ctx.build_scheduler_service();
        let scheduler = Arc::new(scheduler);
        scheduler.set_self_ref(Arc::clone(&scheduler) as Arc<dyn TaskScheduler>);
        // Auto-complete path is internal — no UI settle needed → merge_settle_ms
        let merge_settle_ms = default_scheduler_merge_settle_ms();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(merge_settle_ms)).await;
            scheduler.try_schedule_ready_tasks().await;
        });
    }
}

// ---------------------------------------------------------------------------
// Main orchestrator
// ---------------------------------------------------------------------------

/// Attempt to auto-complete a merge when the merger agent exits.
///
/// Called after process_stream_background returns for ChatContextType::Merge.
/// Checks if the task is still in Merging state (agent didn't explicitly transition)
/// and determines the appropriate transition based on git state:
/// - Rebase complete + no conflict markers → transition to Merged
/// - Rebase in progress or conflict markers → transition to MergeConflict
///
/// This enables "fire and forget" merge agents that don't need to call complete_merge.
pub(crate) async fn attempt_merge_auto_complete<R: Runtime + 'static>(
    ctx: &MergeAutoCompleteContext<'_, R>,
) {
    // Dedup guard: prevent concurrent auto-complete calls for the same task.
    if !ctx.execution_state.try_start_auto_complete(ctx.task_id_str) {
        tracing::info!(
            task_id = ctx.task_id_str,
            "attempt_merge_auto_complete: skipping — another auto-complete is already in flight for this task"
        );
        return;
    }
    // RAII guard: ensure we remove the task from the in-flight set on all exit paths.
    let _auto_complete_guard = AutoCompleteGuard {
        execution_state: Arc::clone(ctx.execution_state),
        task_id: ctx.task_id_str.to_string(),
    };

    // 1. Get task — verify it is still in Merging state
    let mut task = match get_task_in_merging_state(ctx).await {
        Some(t) => t,
        None => return,
    };

    // Early-exit if task already has a merge commit SHA (another path completed it)
    if task.merge_commit_sha.is_some() {
        tracing::info!(
            task_id = ctx.task_id_str,
            existing_sha = ?task.merge_commit_sha,
            "attempt_merge_auto_complete: task already has merge_commit_sha — skipping redundant auto-complete"
        );
        return;
    }

    // 2. Resolve project and worktree path
    let (project, worktree_path) = match resolve_environment(ctx, &task).await {
        Some(v) => v,
        None => return,
    };
    let worktree = Path::new(&worktree_path);

    // 3. Check git state (stale rebase, merge in progress, conflict markers)
    if check_git_state(ctx, worktree).await.is_break() {
        return;
    }

    // 4. Resolve merge branches, parse metadata, apply TOCTOU guard
    let main_repo_path = PathBuf::from(&project.working_directory);
    let (source_branch, target_branch, meta) =
        match resolve_branches_and_metadata(ctx, &task, &project).await {
            Some(v) => v,
            None => return,
        };

    // 5. Handle freshness conflict return routing.
    // Uses shared freshness_return_route() which checks plan_update_conflict (more robust
    // than branch_freshness_conflict which can be cleared prematurely).
    let ts = ctx.build_transition_service();
    match freshness_routing::freshness_return_route(
        &task,
        Arc::clone(ctx.task_repo),
        ts.as_ref(),
        &project,
        ctx.interactive_process_registry
            .as_ref()
            .map(|arc| arc.as_ref()),
    )
    .await
    {
        Ok(freshness_routing::FreshnessRouteResult::FreshnessRouted(_)) => return,
        Ok(freshness_routing::FreshnessRouteResult::NormalMerge) => {}
        Err(e) => {
            tracing::error!(
                task_id = ctx.task_id_str,
                error = %e,
                "attempt_merge_auto_complete: freshness_return_route failed — aborting merge for safety"
            );
            return;
        }
    }

    // 6. Handle plan_update_conflict resolution
    if handle_plan_update_resolution(
        ctx,
        &mut task,
        &meta,
        &main_repo_path,
        &target_branch,
        &project,
    )
    .await
    .is_break()
    {
        return;
    }

    // 7. Handle source_update_conflict resolution
    if handle_source_update_resolution(
        ctx,
        &mut task,
        &meta,
        &source_branch,
        &target_branch,
        &main_repo_path,
    )
    .await
    .is_break()
    {
        return;
    }

    // 8. Handle validation recovery
    let is_validation_recovery = meta
        .as_ref()
        .and_then(|v| v.get("validation_recovery")?.as_bool())
        .unwrap_or(false);
    if handle_validation_recovery(
        ctx,
        &mut task,
        &worktree_path,
        worktree,
        &main_repo_path,
        &project,
        is_validation_recovery,
    )
    .await
    .is_break()
    {
        return;
    }

    // 9. Resolve the merge commit SHA (fast-forward + verify)
    let commit_sha = match resolve_merge_commit(
        ctx,
        &task,
        &main_repo_path,
        &source_branch,
        &target_branch,
        is_validation_recovery,
    )
    .await
    {
        Some(sha) => sha,
        None => return,
    };

    // 10. Complete merge, unblock dependents, schedule ready tasks
    complete_merge_and_schedule(
        ctx,
        &mut task,
        &project,
        &commit_sha,
        &source_branch,
        &target_branch,
        &worktree_path,
        &main_repo_path,
        worktree,
    )
    .await;
}

/// Reconcile merge state when agent run finished but status is still Merging.
pub(crate) async fn reconcile_merge_auto_complete<R: Runtime + 'static>(
    ctx: &MergeAutoCompleteContext<'_, R>,
) {
    attempt_merge_auto_complete(ctx).await;
}

/// Result of merge verification on target branch
#[derive(Debug, PartialEq)]
#[doc(hidden)]
pub enum MergeVerification {
    /// Source branch was successfully merged to target (includes merge commit SHA)
    Merged(String),
    /// Source branch exists but is not merged to target
    NotMerged,
    /// Source branch does not exist or is empty
    SourceBranchMissing,
    /// Target branch does not exist
    TargetBranchMissing,
}

/// Verify if source branch has been merged to target branch.
///
/// Uses git operations from main repo to avoid race conditions with worktree deletion.
/// Returns:
/// - `Merged(sha)` if source branch tip is on target branch (includes target HEAD SHA)
/// - `NotMerged` if source exists but is not on target
/// - `SourceBranchMissing` if source branch doesn't exist or can't be resolved
/// - `TargetBranchMissing` if target branch doesn't exist or can't be resolved
#[doc(hidden)]
pub async fn verify_merge_on_target(
    main_repo: &Path,
    source_branch: &str,
    target_branch: &str,
) -> MergeVerification {
    // Get source branch SHA
    let source_sha = match GitService::get_branch_sha(main_repo, source_branch).await {
        Ok(sha) => sha,
        Err(_) => return MergeVerification::SourceBranchMissing,
    };

    // Get target branch SHA
    let target_sha = match GitService::get_branch_sha(main_repo, target_branch).await {
        Ok(sha) => sha,
        Err(_) => return MergeVerification::TargetBranchMissing,
    };

    // Check if source commit is on target branch
    match GitService::is_commit_on_branch(main_repo, &source_sha, target_branch).await {
        Ok(true) => MergeVerification::Merged(target_sha),
        Ok(false) => MergeVerification::NotMerged,
        Err(_) => MergeVerification::TargetBranchMissing,
    }
}

// ---------------------------------------------------------------------------
// Merge Completion Watcher
// ---------------------------------------------------------------------------
//
// Background tokio task that polls git state to detect when a merger agent has
// finished its work, then closes the agent's stdin via IPR so it exits
// gracefully. After exit, the existing `attempt_merge_auto_complete` pipeline
// handles verification and state transitions.

/// Spawn a background task that watches for merge completion and closes IPR
/// when the merger agent appears done.
///
/// The watcher resolves branches internally — callers only need to provide
/// repo handles and the worktree path.
pub(crate) fn spawn_merge_completion_watcher(
    task_id: String,
    worktree_path: PathBuf,
    ipr: Arc<InteractiveProcessRegistry>,
    task_repo: Arc<dyn TaskRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    plan_branch_repo: Option<Arc<dyn PlanBranchRepository>>,
) {
    let initial_grace = Duration::from_secs(default_reconciliation_merge_watcher_grace_secs());
    let poll_interval = Duration::from_secs(default_reconciliation_merge_watcher_poll_secs());
    tokio::spawn(async move {
        merge_completion_watcher_loop(
            task_id,
            worktree_path,
            ipr,
            task_repo,
            project_repo,
            plan_branch_repo,
            initial_grace,
            poll_interval,
        )
        .await;
    });
}

#[doc(hidden)]
pub async fn merge_completion_watcher_loop(
    task_id: String,
    _worktree_path: PathBuf,
    ipr: Arc<InteractiveProcessRegistry>,
    task_repo: Arc<dyn TaskRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    plan_branch_repo: Option<Arc<dyn PlanBranchRepository>>,
    initial_grace: Duration,
    poll_interval: Duration,
) {
    let key = InteractiveProcessKey::new("merge", &task_id);

    // Grace period — let agent start working
    tokio::time::sleep(initial_grace).await;

    // Resolve branches once (they don't change during the merge)
    let (source_branch, target_branch, main_repo_path) = match resolve_watcher_context(
        &task_id,
        &task_repo,
        &project_repo,
        &plan_branch_repo,
    )
    .await
    {
        Some(ctx) => ctx,
        None => {
            tracing::warn!(task_id = %task_id, "Merge watcher: could not resolve branches, stopping");
            return;
        }
    };

    loop {
        tokio::time::sleep(poll_interval).await;

        // Check if IPR entry still exists (agent might have already exited)
        if !ipr.has_process(&key).await {
            tracing::debug!(task_id = %task_id, "Merge watcher: IPR entry gone, stopping");
            return;
        }

        // Check if task is still in Merging state
        let task = match task_repo
            .get_by_id(&TaskId::from_string(task_id.clone()))
            .await
        {
            Ok(Some(t)) => t,
            _ => {
                tracing::warn!(task_id = %task_id, "Merge watcher: task not found, stopping");
                return;
            }
        };

        if task.internal_status != InternalStatus::Merging {
            tracing::debug!(
                task_id = %task_id,
                status = ?task.internal_status,
                "Merge watcher: task no longer Merging, stopping"
            );
            return;
        }

        // Check for validation_recovery mode — different detection logic
        let meta: Option<serde_json::Value> = task
            .metadata
            .as_ref()
            .and_then(|m| serde_json::from_str(m).ok());
        let is_validation_recovery = meta
            .as_ref()
            .and_then(|m| m.get("validation_recovery"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if !is_validation_recovery {
            // NORMAL MERGE: Check if source is merged into target
            if let MergeVerification::Merged(_sha) =
                verify_merge_on_target(&main_repo_path, &source_branch, &target_branch).await
            {
                tracing::info!(
                    task_id = %task_id,
                    "Merge watcher: merge verified on target branch, closing IPR"
                );
                if ipr.remove(&key).await.is_some() {
                    tracing::info!(task_id = %task_id, "Merge watcher: IPR removed (merge verified, first remover)");
                } else {
                    tracing::debug!(task_id = %task_id, "Merge watcher: IPR already removed by another path (dedup guard)");
                }
                return;
            }
        }

        // No consecutive_clean auto-close — agent exits via:
        // 1. Terminal MCP tool call → HTTP handler closes IPR (primary path)
        // 2. verify_merge_on_target above (safety net)
        // 3. Max agent timeout kills process (last resort)
    }
}

/// Resolve branches and main repo path for the watcher.
/// Returns None if task/project can't be found or branches can't be resolved.
#[doc(hidden)]
pub async fn resolve_watcher_context(
    task_id: &str,
    task_repo: &Arc<dyn TaskRepository>,
    project_repo: &Arc<dyn ProjectRepository>,
    plan_branch_repo: &Option<Arc<dyn PlanBranchRepository>>,
) -> Option<(String, String, PathBuf)> {
    let task = task_repo
        .get_by_id(&TaskId::from_string(task_id.to_string()))
        .await
        .ok()??;

    let project = project_repo.get_by_id(&task.project_id).await.ok()??;

    let (source, target) = resolve_merge_branches(&task, &project, plan_branch_repo).await;

    Some((source, target, PathBuf::from(&project.working_directory)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::{MergeStrategy, MergeValidationMode, ReviewScopeMetadata};
    use std::process::Command;
    use tokio::time::Duration;

    fn run_git(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("run git command");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn setup_source_update_scope_drift_repo() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().expect("create temp repo");
        let repo = dir.path();

        run_git(repo, &["init", "-b", "main"]);
        run_git(repo, &["config", "user.email", "test@test.com"]);
        run_git(repo, &["config", "user.name", "Test"]);

        std::fs::write(repo.join("README.md"), "# test repo\n").expect("write readme");
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", "initial commit"]);

        run_git(repo, &["checkout", "-b", "task/scope-drift"]);
        std::fs::create_dir_all(repo.join("backend/app/services")).expect("create services dir");
        std::fs::write(
            repo.join("backend/app/services/applicability_evaluator.rb"),
            "class ApplicabilityEvaluator\nend\n",
        )
        .expect("write drift file");
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", "feat: out of scope drift"]);
        run_git(repo, &["checkout", "main"]);

        dir
    }

    async fn wait_for_status(
        task_repo: &Arc<dyn TaskRepository>,
        task_id: &TaskId,
        expected: InternalStatus,
    ) -> Task {
        let mut last = None;
        for _ in 0..50 {
            let task = task_repo
                .get_by_id(task_id)
                .await
                .expect("get task")
                .expect("task exists");
            if task.internal_status == expected {
                return task;
            }
            last = Some(task.internal_status);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        panic!(
            "task did not reach expected status {:?}; last state was {:?}",
            expected, last
        );
    }

    #[tokio::test]
    async fn source_update_conflict_auto_complete_scope_drift_routes_to_reexecution() {
        let repo = setup_source_update_scope_drift_repo();
        let repo_path = repo.path();
        let app_state = AppState::new_test();
        let execution_state = Arc::new(ExecutionState::new());
        execution_state.set_max_concurrent(10);

        let mut project = Project::new(
            "Auto-complete scope drift project".to_string(),
            repo_path.to_string_lossy().to_string(),
        );
        project.base_branch = Some("main".to_string());
        project.merge_strategy = MergeStrategy::Merge;
        project.merge_validation_mode = MergeValidationMode::Off;
        app_state
            .project_repo
            .create(project.clone())
            .await
            .expect("create project");

        let mut task = Task::new(
            project.id.clone(),
            "Source update auto-complete should revise".to_string(),
        );
        task.internal_status = InternalStatus::Merging;
        task.task_branch = Some("task/scope-drift".to_string());
        task.worktree_path = Some(repo_path.to_string_lossy().to_string());
        let base_metadata = serde_json::json!({
            "source_update_conflict": true,
            "target_branch": "main",
            "source_branch": "task/scope-drift"
        })
        .to_string();
        task.metadata = Some(
            ReviewScopeMetadata::new(
                vec!["frontend/src".to_string()],
                Vec::new(),
                Some("unrelated_drift".to_string()),
                Some("backend service file was never classified during review".to_string()),
            )
            .update_task_metadata(Some(&base_metadata))
            .expect("scope metadata"),
        );
        let task_id = task.id.clone();
        app_state.task_repo.create(task).await.expect("create task");

        let plan_branch_repo = Some(Arc::clone(&app_state.plan_branch_repo));
        let interactive_process_registry = None;
        let merge_ctx = MergeAutoCompleteContext {
            task_id_str: task_id.as_str(),
            task_id: task_id.clone(),
            task_repo: &app_state.task_repo,
            task_dependency_repo: &app_state.task_dependency_repo,
            project_repo: &app_state.project_repo,
            artifact_repo: &app_state.artifact_repo,
            chat_message_repo: &app_state.chat_message_repo,
            chat_attachment_repo: &app_state.chat_attachment_repo,
            conversation_repo: &app_state.chat_conversation_repo,
            agent_run_repo: &app_state.agent_run_repo,
            ideation_session_repo: &app_state.ideation_session_repo,
            activity_event_repo: &app_state.activity_event_repo,
            message_queue: &app_state.message_queue,
            running_agent_registry: &app_state.running_agent_registry,
            memory_event_repo: &app_state.memory_event_repo,
            execution_state: &execution_state,
            execution_settings_repo: Some(&app_state.execution_settings_repo),
            plan_branch_repo: &plan_branch_repo,
            app_handle: None::<&AppHandle<tauri::Wry>>,
            interactive_process_registry: &interactive_process_registry,
        };

        attempt_merge_auto_complete(&merge_ctx).await;

        let updated = wait_for_status(&app_state.task_repo, &task_id, InternalStatus::ReExecuting)
            .await;
        let metadata: serde_json::Value =
            serde_json::from_str(updated.metadata.as_deref().unwrap_or("{}"))
                .expect("task metadata is JSON");
        assert_eq!(metadata["error_code"], "merge_scope_drift_guard");
        assert_ne!(
            metadata["error_code"], "merge_scope_drift_guard_fallback",
            "auto-complete retry path must have TaskServices.transition_service available"
        );
    }
}
