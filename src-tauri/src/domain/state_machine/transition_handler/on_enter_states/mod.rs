// State entry dispatch — all `on_enter` match arms and helpers.
//
// Extracted from side_effects.rs for maintainability. The `on_enter` method
// signature stays in side_effects.rs and delegates here.

use std::path::Path;
use std::sync::Arc;

use crate::domain::entities::InternalStatus;
use crate::domain::state_machine::services::{
    BranchUpdateWorkflow, BranchUpdateWorkflowOutcome, TaskNotification,
};
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
mod branch_update;
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FreshnessApplyOutcome {
    Ready,
    Updated(crate::domain::entities::BranchUpdateDirection),
}

pub(super) async fn apply_freshness_result(
    result: Result<freshness::FreshnessMetadata, FreshnessAction>,
    task: &crate::domain::entities::Task,
    task_id_str: &str,
    task_repo: &Arc<dyn TaskRepository>,
    branch_update_repo: Option<&Arc<dyn crate::domain::repositories::BranchUpdateRepository>>,
    branch_update_workflow: Option<&Arc<dyn BranchUpdateWorkflow>>,
    project: &crate::domain::entities::Project,
    repo_path: &Path,
    origin_state: &str,
) -> AppResult<FreshnessApplyOutcome> {
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
            Ok(FreshnessApplyOutcome::Ready)
        }
        Err(FreshnessAction::RouteToBranchUpdate {
            mut freshness_metadata,
            conflict_type,
            conflict_files,
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
            let branch_update_repo = branch_update_repo.ok_or_else(|| {
                AppError::ExecutionBlocked(
                    "Branch update authority repository is unavailable".to_string(),
                )
            })?;
            let direction = if conflict_type == "plan_update" {
                crate::domain::entities::BranchUpdateDirection::PlanBranch
            } else {
                crate::domain::entities::BranchUpdateDirection::TaskBranch
            };
            let continuation = match origin_state {
                "re_executing" => crate::domain::entities::BranchUpdateContinuation::ResumeReExecution,
                "reviewing" => crate::domain::entities::BranchUpdateContinuation::ResumeReview,
                "waiting_on_pr" => crate::domain::entities::BranchUpdateContinuation::ResumeWaitingOnPr,
                "pr_branch_publication" => crate::domain::entities::BranchUpdateContinuation::FinalizePostMergePrPublication,
                "pending_merge" => crate::domain::entities::BranchUpdateContinuation::RetryPendingMerge,
                _ => crate::domain::entities::BranchUpdateContinuation::ResumeExecution,
            };
            let (source_branch, target_branch) = if direction
                == crate::domain::entities::BranchUpdateDirection::PlanBranch
            {
                (
                    freshness_metadata.source_branch.clone(),
                    freshness_metadata.target_branch.clone(),
                )
            } else {
                (
                    freshness_metadata.target_branch.clone(),
                    freshness_metadata.source_branch.clone(),
                )
            };
            let source_branch = source_branch.ok_or_else(|| {
                AppError::ExecutionBlocked("Branch update source is missing".to_string())
            })?;
            let target_branch = target_branch.ok_or_else(|| {
                AppError::ExecutionBlocked("Branch update target is missing".to_string())
            })?;
            let identity = GitService::canonical_target_identity(repo_path, &target_branch).await?;
            let workspace_path = if direction
                == crate::domain::entities::BranchUpdateDirection::PlanBranch
            {
                super::merge_helpers::compute_plan_update_worktree_path(project, task_id_str)
            } else {
                super::merge_helpers::compute_source_update_worktree_path(project, task_id_str)
            };
            let mut operation = crate::domain::entities::BranchUpdateOperation::new(
                task.id.clone(),
                direction,
                continuation,
                uuid::Uuid::new_v4().to_string(),
                source_branch,
                target_branch,
                crate::domain::entities::BranchUpdateWorkspaceOwnership::OperationWorktree,
                crate::domain::entities::BranchUpdateCapacityOwnership::Inherited,
                identity,
                chrono::Utc::now(),
            );
            operation.workspace_path = Some(std::path::PathBuf::from(workspace_path));
            operation.conflict_files = conflict_files.into_iter().map(Into::into).collect();
            operation.observed_source_sha =
                Some(GitService::resolve_ref_sha(repo_path, &operation.source_branch).await?);
            operation.observed_target_sha =
                Some(GitService::resolve_ref_sha(repo_path, &operation.target_branch).await?);
            let update_status = if direction
                == crate::domain::entities::BranchUpdateDirection::PlanBranch
            {
                InternalStatus::UpdatingPlanBranch
            } else {
                InternalStatus::UpdatingTaskBranch
            };
            let operation_for_execution = operation.clone();
            let fencing_epoch = match branch_update_repo
                .activate(crate::domain::repositories::BranchUpdateActivation {
                    operation,
                    expected_status: task.internal_status,
                    update_status,
                    trigger: "branch_freshness_conflict".to_string(),
                })
                .await?
            {
                crate::domain::repositories::BranchUpdateActivationOutcome::Applied {
                    fencing_epoch,
                    ..
                } => fencing_epoch,
                outcome => {
                    return Err(AppError::Conflict(format!(
                        "Branch update activation lost authority: {outcome:?}"
                    )))
                }
            };
            let workflow = branch_update_workflow.ok_or_else(|| {
                AppError::ExecutionBlocked(
                    "Branch update workflow adapter is unavailable".to_string(),
                )
            })?;
            match workflow
                .execute_programmatic(
                    Arc::clone(branch_update_repo),
                    Arc::clone(task_repo),
                    repo_path,
                    &operation_for_execution,
                    update_status,
                    fencing_epoch,
                )
                .await?
            {
                BranchUpdateWorkflowOutcome::Completed { .. } => {
                    Ok(FreshnessApplyOutcome::Updated(direction))
                }
                BranchUpdateWorkflowOutcome::ContinuationPending => {
                    Err(AppError::ExecutionBlocked(
                        "Branch update completed and is awaiting its durable publication continuation".to_string(),
                    ))
                }
                BranchUpdateWorkflowOutcome::NeedsAgent => {
                    Err(AppError::BranchFreshnessConflict)
                }
                BranchUpdateWorkflowOutcome::Blocked => {
                    Err(AppError::Conflict("Branch update is blocked and requires attention".to_string()))
                }
            }
        }
        Err(FreshnessAction::ExecutionBlocked { reason, .. }) => {
            Err(AppError::ExecutionBlocked(reason))
        }
    }
}

fn stale_execution_worktree_cleanup_blocked_error(
    task_id_str: &str,
    worktree_path: &Path,
    error: impl std::fmt::Display,
    context: &str,
) -> AppError {
    AppError::ExecutionBlocked(format!(
        "{}: structural: failed to clean stale execution worktree '{}' during {} for task {}: {}",
        GIT_ISOLATION_ERROR_PREFIX,
        worktree_path.display(),
        context,
        task_id_str,
        error
    ))
}

fn registered_task_worktree_matches_branch(
    registered_path: &str,
    registered_branch: Option<&str>,
    expected_path: &Path,
    expected_branch: &str,
) -> bool {
    Path::new(registered_path) == expected_path && registered_branch == Some(expected_branch)
}

async fn existing_task_worktree_is_reusable(
    repo_path: &Path,
    worktree_path: &Path,
    branch: &str,
    task_id_str: &str,
) -> bool {
    match GitService::list_worktrees(repo_path).await {
        Ok(worktrees) => worktrees.iter().any(|worktree| {
            registered_task_worktree_matches_branch(
                &worktree.path,
                worktree.branch.as_deref(),
                worktree_path,
                branch,
            )
        }),
        Err(e) => {
            tracing::warn!(
                task_id = task_id_str,
                error = %e,
                worktree_path = %worktree_path.display(),
                "Could not list worktrees before stale task worktree cleanup"
            );
            false
        }
    }
}

async fn delete_existing_execution_worktree_or_block(
    repo_path: &Path,
    worktree_path: &Path,
    task_id_str: &str,
    context: &str,
) -> AppResult<()> {
    if !worktree_path.exists() {
        return Ok(());
    }

    GitService::delete_worktree(repo_path, worktree_path)
        .await
        .map_err(|e| {
            stale_execution_worktree_cleanup_blocked_error(task_id_str, worktree_path, e, context)
        })
}

#[derive(Debug, Clone)]
struct TaskExecutionWorktree {
    branch: String,
    worktree_path: std::path::PathBuf,
    base_ref: String,
    base_sha: String,
}

fn persisted_task_branch_base_or_block(
    task: &Task,
    branch: &str,
    task_id_str: &str,
) -> AppResult<(String, String)> {
    let base_ref = task
        .task_branch_base_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let base_sha = task
        .task_branch_base_sha
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    match (base_ref, base_sha) {
        (Some(base_ref), Some(base_sha)) => Ok((base_ref.to_string(), base_sha.to_string())),
        _ => Err(AppError::ExecutionBlocked(format!(
            "{}: structural: existing task branch '{}' for task {} is missing immutable base metadata; reset/recreate the task branch before execution can continue",
            GIT_ISOLATION_ERROR_PREFIX, branch, task_id_str
        ))),
    }
}

/// Create a fresh task branch and worktree for a task entering Executing state.
///
/// Resolves the base branch, computes the standard worktree path via
/// `compute_task_worktree_path`, cleans any stale worktree at that path, then
/// creates (or checks out an existing) branch into the worktree.
///
/// Does NOT update the database — the caller is responsible for persisting the
/// returned branch, worktree path, base ref, and base SHA onto the task.
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
) -> AppResult<TaskExecutionWorktree> {
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
        if existing_task_worktree_is_reusable(repo_path, &worktree_path_buf, &branch, task_id_str)
            .await
        {
            let (base_ref, base_sha) =
                persisted_task_branch_base_or_block(task, &branch, task_id_str)?;
            tracing::info!(
                task_id = task_id_str,
                branch = %branch,
                worktree_path = %worktree_path_buf.display(),
                "Reusing existing registered task worktree"
            );
            return Ok(TaskExecutionWorktree {
                branch,
                worktree_path: worktree_path_buf,
                base_ref,
                base_sha,
            });
        }
        delete_existing_execution_worktree_or_block(
            repo_path,
            &worktree_path_buf,
            task_id_str,
            "fresh branch creation",
        )
        .await?;
    }

    // Check if branch already exists from a previous execution attempt
    let branch_exists = GitService::branch_exists(repo_path, &branch)
        .await
        .unwrap_or(false);

    // Create worktree — use existing branch if it exists, create new one otherwise
    let result: AppResult<TaskExecutionWorktree> = if branch_exists {
        let (base_ref, base_sha) =
            persisted_task_branch_base_or_block(task, &branch, task_id_str)?;
        tracing::info!(
            task_id = task_id_str,
            branch = %branch,
            "Branch already exists, checking out existing branch into worktree"
        );
        GitService::checkout_existing_branch_worktree(repo_path, &worktree_path_buf, &branch)
            .await
            .map(|_| TaskExecutionWorktree {
                branch,
                worktree_path: worktree_path_buf,
                base_ref,
                base_sha,
            })
    } else {
        let base_sha = GitService::get_branch_sha(repo_path, base_branch)
            .await
            .map_err(|e| {
                AppError::ExecutionBlocked(format!(
                    "{}: structural: could not resolve base branch '{}' to a commit SHA before creating task branch: {}",
                    GIT_ISOLATION_ERROR_PREFIX, base_branch, e
                ))
            })?;
        GitService::create_worktree(repo_path, &worktree_path_buf, &branch, base_branch)
            .await
            .map(|_| TaskExecutionWorktree {
                branch,
                worktree_path: worktree_path_buf,
                base_ref: base_branch.to_string(),
                base_sha,
            })
    };

    match result {
        Ok(worktree) => Ok(worktree),
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
    is_pr_branch_publication_conflict: bool,
    freshness_conflict_count: u32,
    base_branch: Option<String>,
    source_branch: Option<String>,
    target_branch: Option<String>,
}

impl<'a> super::TransitionHandler<'a> {
    pub(super) async fn on_enter_dispatch(&self, state: &State) -> AppResult<()> {
        if let Some(context) = self.machine.context.services.notification_context.clone() {
            let status = match state {
                State::QaFailed(_) => Some(InternalStatus::QaFailed),
                State::ReviewPassed => Some(InternalStatus::ReviewPassed),
                State::Escalated => Some(InternalStatus::Escalated),
                State::Failed(_) => Some(InternalStatus::Failed),
                State::MergeConflict => Some(InternalStatus::MergeConflict),
                State::MergeIncomplete => Some(InternalStatus::MergeIncomplete),
                State::Blocked => Some(InternalStatus::Blocked),
                _ => None,
            };
            if let Some(status) = status {
                self.machine
                    .context
                    .services
                    .notifier
                    .notify(context, TaskNotification::StateEntered(status))
                    .await;
            }
        }

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
            State::UpdatingPlanBranch | State::UpdatingTaskBranch => {
                self.enter_branch_update_state().await?;
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
