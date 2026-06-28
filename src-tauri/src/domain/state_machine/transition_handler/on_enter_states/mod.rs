// State entry dispatch — all `on_enter` match arms and helpers.
//
// Extracted from side_effects.rs for maintainability. The `on_enter` method
// signature stays in side_effects.rs and delegates here.

use std::path::Path;
use std::sync::Arc;

use chrono::Utc;

use super::super::machine::State;
use super::super::types::FailedData;
use super::freshness::{self, FreshnessAction};
use super::merge_helpers::{
    compute_merge_worktree_path, compute_task_worktree_path, is_merge_worktree_path,
    resolve_task_base_branch, resolve_task_plan_branch_record, restore_task_worktree, slugify,
};
use super::metadata_builder::{build_failed_metadata, MetadataUpdate};
use crate::application::git_service::git_cmd::ENOENT_MARKER;
use crate::application::GitService;
use crate::domain::entities::plan_branch::PlanBranchId;
use crate::domain::entities::task_metadata::StopRetryingReason;
use crate::domain::entities::task_metadata::GIT_ISOLATION_ERROR_PREFIX;
use crate::domain::entities::{
    ExecutionFailureSource, ExecutionRecoveryEvent, ExecutionRecoveryEventKind,
    ExecutionRecoveryMetadata, ExecutionRecoveryReasonCode, ExecutionRecoverySource,
    ExecutionRecoveryState, MergeFailureSource, MergeRecoveryEvent, MergeRecoveryEventKind,
    MergeRecoveryMetadata, MergeRecoveryReasonCode, MergeRecoverySource, MergeRecoveryState,
    Project, ProjectId, Task, TaskCategory, TaskId, TaskStepStatus,
};
use crate::domain::repositories::{PlanBranchRepository, TaskRepository};
use crate::domain::services::github_service::GithubServiceTrait;
use crate::domain::services::PlanPrDescriptionDrafter;
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::{reconciliation_config, scheduler_config};

mod execution;
mod merge;
mod outcomes;
mod qa;
mod review;

/// Get the plan branch name for a task by looking up via plan_branch_repo.
/// Returns None if task has no execution_plan_id or plan_branch can't be found,
/// or if the resolved branch equals the project base branch.
async fn get_task_plan_branch(
    task: &crate::domain::entities::Task,
    project: &crate::domain::entities::Project,
    plan_branch_repo: &Option<Arc<dyn crate::domain::repositories::PlanBranchRepository>>,
    _task_repo: &Option<Arc<dyn TaskRepository>>,
) -> Option<crate::domain::entities::PlanBranch> {
    let plan_branch_repo = plan_branch_repo.as_ref()?;
    let resolved = resolve_task_plan_branch_record(task, plan_branch_repo).await?;
    let project_base = project.base_branch.as_deref().unwrap_or("main");
    if resolved.branch_name != project_base {
        Some(resolved)
    } else {
        None
    }
}

/// Handle the result of ensure_branches_fresh() for an entry point.
/// Returns Ok(()) if fresh or skipped, Err if needs routing or blocking.
async fn apply_freshness_result(
    result: Result<freshness::FreshnessMetadata, FreshnessAction>,
    task: &crate::domain::entities::Task,
    task_id_str: &str,
    task_repo: &Arc<dyn TaskRepository>,
) -> AppResult<()> {
    let task_id = TaskId::from_string(task_id_str.to_string());
    match result {
        Ok(updated_meta) => {
            // Persist updated freshness metadata (last_freshness_check_at, etc.)
            let mut task_metadata: serde_json::Value = task
                .metadata
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_else(|| serde_json::json!({}));
            updated_meta.merge_into(&mut task_metadata);
            if let Err(e) = task_repo
                .update_metadata(&task_id, Some(task_metadata.to_string()))
                .await
            {
                tracing::warn!(task_id = task_id_str, error = %e, "Failed to persist freshness metadata");
            }
            Ok(())
        }
        Err(FreshnessAction::RouteToMerging {
            mut freshness_metadata,
            ..
        }) => {
            // INVARIANT: freshness_count_incremented_by signals to the corrective handler
            // (task_transition_service.rs) that freshness_conflict_count was already
            // incremented by ensure_branches_fresh(). The conflict marker scan path does
            // NOT set this field, so the corrective handler will increment there instead.
            freshness_metadata.freshness_count_incremented_by =
                Some("ensure_branches_fresh".to_string());
            // Store freshness metadata on task for merger agent context
            let mut task_metadata: serde_json::Value = task
                .metadata
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_else(|| serde_json::json!({}));
            freshness_metadata.merge_into(&mut task_metadata);
            if let Err(e) = task_repo
                .update_metadata(&task_id, Some(task_metadata.to_string()))
                .await
            {
                tracing::warn!(task_id = task_id_str, error = %e, "Failed to persist freshness conflict metadata");
            }
            Err(AppError::BranchFreshnessConflict)
        }
        Err(FreshnessAction::ExecutionBlocked { reason }) => {
            Err(AppError::ExecutionBlocked(reason))
        }
    }
}

/// Create a fresh task branch and worktree for a task entering Executing state.
///
/// Resolves the base branch, computes the standard worktree path via
/// `compute_task_worktree_path`, cleans any stale worktree at that path, then
/// creates (or checks out an existing) branch into the worktree.
///
/// Does NOT update the database — the caller is responsible for persisting the
/// returned `(branch_name, worktree_path)` onto the task.
///
/// # Errors
/// Returns `AppError::ExecutionBlocked` if the git worktree cannot be created.
#[allow(clippy::too_many_arguments)]
async fn create_fresh_branch_and_worktree(
    task: &Task,
    project: &Project,
    task_id_str: &str,
    repo_path: &Path,
    plan_branch_repo: &Option<Arc<dyn PlanBranchRepository>>,
    task_repo_ref: &Option<Arc<dyn TaskRepository>>,
    pr_creation_guard: &Option<Arc<dashmap::DashMap<PlanBranchId, ()>>>,
    github_service: &Option<Arc<dyn GithubServiceTrait>>,
    plan_pr_description_drafter: &Option<Arc<dyn PlanPrDescriptionDrafter>>,
) -> AppResult<(String, std::path::PathBuf)> {
    let branch = format!("ralphx/{}/task-{}", slugify(&project.name), task_id_str);
    let resolved_base = resolve_task_base_branch(
        task,
        project,
        plan_branch_repo,
        task_repo_ref,
        pr_creation_guard,
        github_service,
        plan_pr_description_drafter,
    )
    .await;
    let base_branch = resolved_base.as_str();

    // Pre-validate that the base branch exists before attempting worktree creation.
    // This is DISTINCT from the task-branch check below (line ~178) which validates
    // the task branch (ralphx/{project}/task-{id}), not the base branch.
    // Conservative: only block if we can CONFIRM the branch is missing (Ok(false)).
    // Git errors (timeout, IO) fall through as Ok(true) — do not block on uncertainty.
    if !GitService::branch_exists(repo_path, base_branch)
        .await
        .unwrap_or(true)
    {
        return Err(AppError::ExecutionBlocked(format!(
            "{}: structural: base branch '{}' does not exist",
            GIT_ISOLATION_ERROR_PREFIX, base_branch
        )));
    }

    // Use compute_task_worktree_path for consistent path computation
    let worktree_path_str = compute_task_worktree_path(project, task_id_str);
    let worktree_path_buf = std::path::PathBuf::from(&worktree_path_str);

    // Clean up stale task worktree from a prior execution attempt
    if worktree_path_buf.exists() {
        if let Err(e) = GitService::delete_worktree(repo_path, &worktree_path_buf).await {
            tracing::warn!(
                task_id = task_id_str,
                error = %e,
                "Failed to clean stale task worktree (non-fatal)"
            );
        }
    }

    // Check if branch already exists from a previous execution attempt
    let branch_exists = GitService::branch_exists(repo_path, &branch)
        .await
        .unwrap_or(false);

    // Create worktree — use existing branch if it exists, create new one otherwise
    let result = if branch_exists {
        tracing::info!(
            task_id = task_id_str,
            branch = %branch,
            "Branch already exists, checking out existing branch into worktree"
        );
        GitService::checkout_existing_branch_worktree(repo_path, &worktree_path_buf, &branch).await
    } else {
        GitService::create_worktree(repo_path, &worktree_path_buf, &branch, base_branch).await
    };

    match result {
        Ok(_) => Ok((branch, worktree_path_buf)),
        Err(e) => Err(AppError::ExecutionBlocked(format!(
            "{}: could not create worktree at '{}': {}",
            GIT_ISOLATION_ERROR_PREFIX, worktree_path_str, e
        ))),
    }
}

#[derive(Default)]
struct MergePromptContext {
    is_validation_recovery: bool,
    is_plan_update_conflict: bool,
    is_source_update_conflict: bool,
    is_pr_branch_update_conflict: bool,
    freshness_conflict_count: u32,
    base_branch: Option<String>,
    source_branch: Option<String>,
    target_branch: Option<String>,
}

impl<'a> super::TransitionHandler<'a> {
    pub(super) async fn on_enter_dispatch(&self, state: &State) -> AppResult<()> {
        match state {
            State::Ready => {
                // When entering Ready, spawn QA prep agent if enabled
                if self.machine.context.qa_enabled {
                    self.machine
                        .context
                        .services
                        .agent_spawner
                        .spawn_background("qa-prep", &self.machine.context.task_id)
                        .await;
                }

                // Delay auto-scheduling so UI sees task "settle" in Ready column
                // before it potentially moves to Executing (user-visible → ready_settle_ms)
                if let Some(ref scheduler) = self.machine.context.services.task_scheduler {
                    let scheduler = Arc::clone(scheduler);
                    let ready_settle_ms = scheduler_config().ready_settle_ms;
                    tokio::spawn(async move {
                        tokio::time::sleep(tokio::time::Duration::from_millis(ready_settle_ms))
                            .await;
                        scheduler.try_schedule_ready_tasks().await;
                    });
                }
            }
            State::Executing => {
                self.enter_executing_state().await?;
            }
            State::QaRefining => {
                self.enter_qa_refining_state().await;
            }
            State::QaTesting => {
                self.enter_qa_testing_state().await;
            }
            State::QaPassed => {
                self.enter_qa_passed_state().await;
            }
            State::QaFailed(data) => {
                self.enter_qa_failed_state(data).await;
            }
            State::PendingReview => {
                self.enter_pending_review_state().await;
            }
            State::Reviewing => {
                self.enter_reviewing_state().await?;
            }
            State::ReviewPassed => {
                self.enter_review_passed_state().await;
            }
            State::Escalated => {
                self.enter_escalated_state().await;
            }
            State::ReExecuting => {
                self.enter_reexecuting_state().await?;
            }
            State::RevisionNeeded => {
                self.enter_revision_needed_state().await;
            }
            State::Approved => {
                self.enter_approved_state().await;
            }
            State::Failed(data) => {
                self.enter_failed_state(data).await;
            }
            State::PendingMerge => {
                // Phase 1 of merge workflow: Attempt programmatic rebase and merge
                // This is the "fast path" - if successful, skip agent entirely
                // heap-allocate to prevent stack overflow from large inlined future
                Box::pin(self.attempt_programmatic_merge()).await;
            }
            State::Merging => {
                Box::pin(self.enter_merging_state()).await?;
            }
            State::WaitingOnPr => {
                self.maybe_start_pr_mode_merge_poller(&self.machine.context.task_id)
                    .await;
            }
            State::Merged => {
                self.enter_merged_state().await;
            }
            _ => {}
        }
        Ok(())
    }
}

/// Extract `restart_note` from task metadata JSON.
/// Returns `Some(note)` if the key exists and is a non-empty string, `None` otherwise.
fn extract_restart_note(metadata: Option<&str>) -> Option<String> {
    let metadata_str = metadata?;
    let obj = serde_json::from_str::<serde_json::Value>(metadata_str).ok()?;
    let note = obj.get("restart_note")?.as_str()?;
    if note.is_empty() {
        None
    } else {
        Some(note.to_string())
    }
}

/// Extract `preserve_steps` boolean from task metadata JSON.
/// Returns `true` if the key exists and is `true`, `false` otherwise.
fn extract_preserve_steps(metadata: Option<&str>) -> bool {
    metadata
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .and_then(|v| v.get("preserve_steps")?.as_bool())
        .unwrap_or(false)
}
