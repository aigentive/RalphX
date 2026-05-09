// Merge orchestration helpers — extracted from attempt_programmatic_merge() for maintainability.
//
// Sub-functions for logical phases of the programmatic merge workflow:
// - fetch_merge_context: load task + project from repos
// - check_already_merged: detect if source is already merged into target
// - recover_deleted_source_branch: find task commits on target when source branch is gone
// - run_concurrent_merge_guard: TOCTOU-safe deferral under merge_lock
// - build_squash_commit_message: construct commit message for squash merges
// - dispatch_merge_strategy: timeout-wrapped strategy dispatch + outcome handling

use std::path::Path;
use std::sync::Arc;

use super::commit_messages::{build_plan_merge_commit_msg, build_squash_commit_msg};
use super::merge_completion::complete_merge_internal_with_pr_sync;
use super::merge_helpers::{
    clear_merge_deferred_metadata, compute_merge_worktree_path, has_merge_deferred_metadata,
    has_prior_rebase_conflict, has_prior_validation_failure, has_source_conflict_resolved,
    parse_metadata, task_targets_branch,
};
use super::merge_outcome_handler::{MergeContext, MergeHandlerOptions};
use super::merge_strategies::MergeOutcome;
use crate::application::GitService;
use crate::domain::entities::{
    task_metadata::{
        MergeRecoveryEvent, MergeRecoveryEventKind, MergeRecoveryMetadata, MergeRecoveryReasonCode,
        MergeRecoverySource, MergeRecoveryState,
    },
    InternalStatus, MergeStrategy, Project, Task, TaskCategory, TaskId,
};
use crate::domain::repositories::{PlanBranchRepository, TaskRepository};
use crate::domain::services::payload_enrichment::{
    emit_external_webhook_event, PresentationKind, WebhookPresentationContext,
};

/// Result of `fetch_merge_context`: the loaded task and project.
pub(super) struct MergeInputs {
    pub task: Task,
    pub project: Project,
}

/// Result of the concurrent merge guard check.
pub(super) enum ConcurrentGuardResult {
    /// No blocker — proceed with merge.
    Proceed,
    /// This task was deferred — caller should return early.
    Deferred,
}

impl<'a> super::TransitionHandler<'a> {
    /// Load task and project from repositories for the merge workflow.
    ///
    /// Returns `None` if repos are unavailable or records not found (caller should return early).
    pub(super) async fn fetch_merge_context(
        &self,
        task_id_str: &str,
        project_id_str: &str,
    ) -> Option<MergeInputs> {
        let (Some(ref task_repo), Some(ref project_repo)) = (
            &self.machine.context.services.task_repo,
            &self.machine.context.services.project_repo,
        ) else {
            tracing::error!(
                task_id = task_id_str,
                project_id = project_id_str,
                task_repo_available = self.machine.context.services.task_repo.is_some(),
                project_repo_available = self.machine.context.services.project_repo.is_some(),
                "Programmatic merge BLOCKED: repos not available — \
                 task will remain stuck in PendingMerge"
            );
            self.on_exit(
                &super::super::machine::State::PendingMerge,
                &super::super::machine::State::MergeIncomplete,
            )
            .await;
            return None;
        };

        let task_id = TaskId::from_string(task_id_str.to_string());
        let project_id =
            crate::domain::entities::ProjectId::from_string(project_id_str.to_string());

        let task_result = task_repo.get_by_id(&task_id).await;
        let project_result = project_repo.get_by_id(&project_id).await;

        let (Ok(Some(task)), Ok(Some(project))) = (task_result, project_result) else {
            tracing::error!(
                task_id = task_id_str,
                project_id = project_id_str,
                "Programmatic merge BLOCKED: failed to fetch task or project from database — \
                 task will remain stuck in PendingMerge"
            );
            return None;
        };

        Some(MergeInputs { task, project })
    }

    /// Check if the source branch is already an ancestor of the target branch.
    ///
    /// If so, the merge was completed by a prior agent run that died before calling
    /// `complete_merge`. Cleans up orphaned worktrees and completes the merge.
    ///
    /// Returns `true` if the merge was already done (caller should return early).
    pub(super) async fn check_already_merged(
        &self,
        tc: super::TaskCore<'_>,
        bp: super::BranchPair<'_>,
        pc: super::ProjectCtx<'_>,
        plan_branch_repo: &Option<Arc<dyn PlanBranchRepository>>,
    ) -> bool {
        let (task, task_id, task_id_str, task_repo) =
            (tc.task, tc.task_id, tc.task_id_str, tc.task_repo);
        let (source_branch, target_branch) = (bp.source_branch, bp.target_branch);
        let (project, repo_path) = (pc.project, pc.repo_path);

        // Pre-fetch session_title for payload enrichment
        let session_title = if let (Some(ref sid), Some(session_repo)) = (
            &task.ideation_session_id,
            self.machine
                .context
                .services
                .ideation_session_repo
                .as_deref(),
        ) {
            session_repo
                .get_by_id(sid)
                .await
                .ok()
                .flatten()
                .and_then(|s| s.title)
        } else {
            None
        };

        // Defense-in-depth: If task belongs to a plan, check the plan branch independently.
        // This catches cases where target_branch was incorrectly resolved but the merge
        // already landed on the plan branch from a prior cycle.
        if let Some(ref session_id) = task.ideation_session_id {
            if let Some(ref pb_repo) = plan_branch_repo {
                if let Ok(Some(pb)) = pb_repo.get_by_session_id(session_id).await {
                    if pb.branch_name != target_branch && task.category != TaskCategory::PlanMerge {
                        // Plan branch differs from resolved target — check plan branch too
                        if let Ok(source_sha) =
                            GitService::get_branch_sha(repo_path, source_branch).await
                        {
                            if let Ok(true) = GitService::is_commit_on_branch(
                                repo_path,
                                &source_sha,
                                &pb.branch_name,
                            )
                            .await
                            {
                                if has_prior_validation_failure(task) {
                                    tracing::warn!(
                                        %task_id_str,
                                        plan_branch = %pb.branch_name,
                                        resolved_target = %target_branch,
                                        "check_already_merged: source on plan branch but prior \
                                         validation failures — skipping fast-path"
                                    );
                                } else {
                                    tracing::warn!(
                                        %task_id_str,
                                        plan_branch = %pb.branch_name,
                                        resolved_target = %target_branch,
                                        "check_already_merged: task already merged to plan branch \
                                         (target mismatch detected)"
                                    );

                                    // Clean up orphaned merge worktree
                                    let merge_wt =
                                        compute_merge_worktree_path(project, task_id_str);
                                    let merge_wt_path = Path::new(&merge_wt);
                                    if crate::utils::path_safety::checked_exists(
                                        merge_wt_path,
                                        "orphaned merge worktree",
                                    )
                                    .unwrap_or(false)
                                    {
                                        if let Err(e) =
                                            GitService::delete_worktree(repo_path, merge_wt_path)
                                                .await
                                        {
                                            tracing::warn!(
                                                error = %e,
                                                "Failed to clean up orphaned merge worktree (non-fatal)"
                                            );
                                        }
                                    }

                                    // Complete merge using the CORRECT target (plan branch)
                                    let plan_sha =
                                        GitService::get_branch_sha(repo_path, &pb.branch_name)
                                            .await
                                            .unwrap_or_else(|_| source_sha.clone());

                                    if let Err(e) = complete_merge_internal_with_pr_sync(
                                        task,
                                        project,
                                        &plan_sha,
                                        source_branch,
                                        &pb.branch_name,
                                        task_repo,
                                        self.machine.context.services.external_events_repo.as_ref(),
                                        self.machine.context.services.webhook_publisher.as_ref(),
                                        self.machine.context.services.app_handle.as_ref(),
                                        session_title.clone(),
                                        Some(super::merge_helpers::PlanBranchPrSyncServices::from_task_services(
                                            &self.machine.context.services,
                                        )),
                                    )
                                    .await
                                    {
                                        tracing::error!(
                                            error = %e,
                                            "Failed to complete already-merged task (plan branch)"
                                        );
                                    } else {
                                        self.post_merge_cleanup(
                                            task_id_str,
                                            task_id,
                                            repo_path,
                                            plan_branch_repo,
                                        )
                                        .await;

                                        // Phase 3: fire-and-forget cleanup
                                        let cleanup_task_id = task_id.clone();
                                        let cleanup_repo = Arc::clone(task_repo);
                                        let cleanup_dir = project.working_directory.clone();
                                        let cleanup_branch = task.task_branch.clone();
                                        let cleanup_wt = task.worktree_path.clone();
                                        let plan_branch_name = pb.branch_name.clone();
                                        tokio::spawn(async move {
                                            super::merge_completion::deferred_merge_cleanup(
                                                cleanup_task_id, cleanup_repo, cleanup_dir,
                                                cleanup_branch, cleanup_wt,
                                                Some(plan_branch_name),
                                            ).await;
                                        });
                                    }
                                    return true;
                                }
                            }
                        }
                    } else if task.category == TaskCategory::PlanMerge {
                        tracing::debug!(
                            %task_id_str,
                            plan_branch = %pb.branch_name,
                            "check_already_merged: skipping plan branch defense-in-depth \
                             for PlanMerge task (source IS plan branch — check is tautological)"
                        );
                    }
                }
            }
        }

        let Ok(source_sha) = GitService::get_branch_sha(repo_path, source_branch).await else {
            return false;
        };
        let Ok(true) = GitService::is_commit_on_branch(repo_path, &source_sha, target_branch).await
        else {
            return false;
        };

        // Ghost-merge guard: if source has 0 unique commits not on target, the source
        // branch never genuinely diverged — source_sha is an ancestor of target for
        // trivial reasons (e.g. plan branch was created from main and never advanced).
        // Firing the "already merged" fast-path here is a false positive: DB records
        // Merged but no plan work actually landed on the target. Return false so the
        // pipeline runs properly (no-op merge or proper error).
        match GitService::count_commits_not_on_branch(repo_path, source_branch, target_branch).await
        {
            Ok(0) => {
                tracing::warn!(
                    task_id = task_id_str,
                    source_branch = %source_branch,
                    target_branch = %target_branch,
                    source_sha = %source_sha,
                    "check_already_merged: source SHA is ancestor of target but source has \
                     0 unique commits — branch never genuinely diverged, skipping fast-path \
                     to prevent ghost merge"
                );
                return false;
            }
            Ok(count) => {
                tracing::debug!(
                    task_id = task_id_str,
                    source_branch = %source_branch,
                    target_branch = %target_branch,
                    unique_commits = count,
                    "check_already_merged: source has {} unique commits — \
                     proceeding with already-merged detection",
                    count
                );
            }
            Err(e) => {
                // Non-fatal: if we can't count commits, fall through to the standard
                // checks. Failing open is safer than blocking a legitimate
                // already-merged detection.
                tracing::warn!(
                    task_id = task_id_str,
                    error = %e,
                    source_branch = %source_branch,
                    target_branch = %target_branch,
                    "check_already_merged: could not count unique commits (non-fatal), \
                     proceeding with already-merged detection"
                );
            }
        }

        // Don't fast-path to completion if prior validation failures exist.
        // The target branch may contain broken code — proceeding would mark
        // the task as Merged with failing code on the target.
        if has_prior_validation_failure(task) {
            tracing::warn!(
                task_id = task_id_str,
                source_branch = %source_branch,
                target_branch = %target_branch,
                "Source appears merged but prior validation failures detected — \
                 skipping fast-path (target may have failing code)"
            );
            return false;
        }

        tracing::info!(
            task_id = task_id_str,
            source_branch = %source_branch,
            target_branch = %target_branch,
            source_sha = %source_sha,
            "Source branch already merged into target — skipping merge"
        );

        // Clean up orphaned merge worktree (if any from prior agent run)
        let merge_wt = compute_merge_worktree_path(project, task_id_str);
        let merge_wt_path = Path::new(&merge_wt);
        if crate::utils::path_safety::checked_exists(merge_wt_path, "orphaned merge worktree")
            .unwrap_or(false)
        {
            if let Err(e) = GitService::delete_worktree(repo_path, merge_wt_path).await {
                tracing::warn!(
                    error = %e,
                    "Failed to clean up orphaned merge worktree (non-fatal)"
                );
            }
        }

        // Use target branch HEAD as the merge commit SHA
        let target_sha = GitService::get_branch_sha(repo_path, target_branch)
            .await
            .unwrap_or_else(|_| source_sha.clone());

        if let Err(e) = complete_merge_internal_with_pr_sync(
            task,
            project,
            &target_sha,
            source_branch,
            target_branch,
            task_repo,
            self.machine.context.services.external_events_repo.as_ref(),
            self.machine.context.services.webhook_publisher.as_ref(),
            self.machine.context.services.app_handle.as_ref(),
            session_title.clone(),
            Some(
                super::merge_helpers::PlanBranchPrSyncServices::from_task_services(
                    &self.machine.context.services,
                ),
            ),
        )
        .await
        {
            tracing::error!(error = %e, "Failed to complete already-merged task");
        } else {
            self.post_merge_cleanup(task_id_str, task_id, repo_path, plan_branch_repo)
                .await;

            // Phase 3: fire-and-forget cleanup
            let cleanup_task_id = task_id.clone();
            let cleanup_repo = Arc::clone(task_repo);
            let cleanup_dir = project.working_directory.clone();
            let cleanup_branch = task.task_branch.clone();
            let cleanup_wt = task.worktree_path.clone();
            let cleanup_plan_branch = Some(target_branch.to_string());
            tokio::spawn(async move {
                super::merge_completion::deferred_merge_cleanup(
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
        true
    }

    /// Recover from a deleted source branch by finding task commits on the target.
    ///
    /// If the source branch ref is gone but the task's commits are already on
    /// the target branch (e.g. detached HEAD, premature cleanup), completes the merge.
    ///
    /// Returns `true` if recovery succeeded (caller should return early),
    /// `false` if no recovery was possible (caller should continue).
    pub(super) async fn recover_deleted_source_branch(
        &self,
        tc: super::TaskCore<'_>,
        bp: super::BranchPair<'_>,
        pc: super::ProjectCtx<'_>,
        plan_branch_repo: &Option<Arc<dyn PlanBranchRepository>>,
    ) -> bool {
        let (task, task_id, task_id_str, task_repo) =
            (tc.task, tc.task_id, tc.task_id_str, tc.task_repo);
        let (source_branch, target_branch) = (bp.source_branch, bp.target_branch);
        let (project, repo_path) = (pc.project, pc.repo_path);

        // Pre-fetch session_title for payload enrichment
        let session_title = if let (Some(ref sid), Some(session_repo)) = (
            &task.ideation_session_id,
            self.machine
                .context
                .services
                .ideation_session_repo
                .as_deref(),
        ) {
            session_repo
                .get_by_id(sid)
                .await
                .ok()
                .flatten()
                .and_then(|s| s.title)
        } else {
            None
        };

        if GitService::branch_exists(repo_path, source_branch)
            .await
            .unwrap_or(false)
        {
            return false;
        }

        // Defense-in-depth: If task belongs to a plan, check the plan branch independently.
        // Same rationale as check_already_merged — target_branch may have been incorrectly
        // resolved but the task's commits already landed on the plan branch.
        if let Some(ref session_id) = task.ideation_session_id {
            if let Some(ref pb_repo) = plan_branch_repo {
                if let Ok(Some(pb)) = pb_repo.get_by_session_id(session_id).await {
                    if pb.branch_name != target_branch && task.category != TaskCategory::PlanMerge {
                        if let Ok(Some(found_sha)) = GitService::find_commit_by_message_grep(
                            repo_path,
                            source_branch,
                            &pb.branch_name,
                        )
                        .await
                        {
                            if has_prior_validation_failure(task) {
                                tracing::warn!(
                                    task_id = task_id_str,
                                    plan_branch = %pb.branch_name,
                                    resolved_target = %target_branch,
                                    found_sha = %found_sha,
                                    "recover_deleted_source_branch: commits on plan branch but \
                                     prior validation failures — skipping recovery"
                                );
                            } else {
                                tracing::warn!(
                                    task_id = task_id_str,
                                    plan_branch = %pb.branch_name,
                                    resolved_target = %target_branch,
                                    found_sha = %found_sha,
                                    "recover_deleted_source_branch: task commits found on plan \
                                     branch (target mismatch detected) — recovering"
                                );

                                let merge_wt = compute_merge_worktree_path(project, task_id_str);
                                let merge_wt_path = Path::new(&merge_wt);
                                if crate::utils::path_safety::checked_exists(
                                    merge_wt_path,
                                    "orphaned merge worktree",
                                )
                                .unwrap_or(false)
                                {
                                    if let Err(e) =
                                        GitService::delete_worktree(repo_path, merge_wt_path).await
                                    {
                                        tracing::warn!(
                                            error = %e,
                                            "Failed to clean up orphaned merge worktree (non-fatal)"
                                        );
                                    }
                                }

                                let plan_sha =
                                    GitService::get_branch_sha(repo_path, &pb.branch_name)
                                        .await
                                        .unwrap_or_else(|_| found_sha.clone());

                                if let Err(e) = complete_merge_internal_with_pr_sync(
                                    task,
                                    project,
                                    &plan_sha,
                                    source_branch,
                                    &pb.branch_name,
                                    task_repo,
                                    self.machine.context.services.external_events_repo.as_ref(),
                                    self.machine.context.services.webhook_publisher.as_ref(),
                                    self.machine.context.services.app_handle.as_ref(),
                                    session_title.clone(),
                                    Some(super::merge_helpers::PlanBranchPrSyncServices::from_task_services(
                                        &self.machine.context.services,
                                    )),
                                )
                                .await
                                {
                                    tracing::error!(
                                        error = %e,
                                        "Failed to complete recovered task (plan branch)"
                                    );
                                } else {
                                    self.post_merge_cleanup(
                                        task_id_str,
                                        task_id,
                                        repo_path,
                                        plan_branch_repo,
                                    )
                                    .await;

                                    // Phase 3: fire-and-forget cleanup
                                    let cleanup_task_id = task_id.clone();
                                    let cleanup_repo = Arc::clone(task_repo);
                                    let cleanup_dir = project.working_directory.clone();
                                    let cleanup_branch = task.task_branch.clone();
                                    let cleanup_wt = task.worktree_path.clone();
                                    let plan_branch_name = pb.branch_name.clone();
                                    tokio::spawn(async move {
                                        super::merge_completion::deferred_merge_cleanup(
                                            cleanup_task_id, cleanup_repo, cleanup_dir,
                                            cleanup_branch, cleanup_wt,
                                            Some(plan_branch_name),
                                        ).await;
                                    });
                                }
                                return true;
                            }
                        }
                    } else if task.category == TaskCategory::PlanMerge {
                        tracing::debug!(
                            task_id = task_id_str,
                            plan_branch = %pb.branch_name,
                            "recover_deleted_source_branch: skipping plan branch defense-in-depth \
                             for PlanMerge task (source IS plan branch — check is tautological)"
                        );
                    }
                }
            }
        }

        match GitService::find_commit_by_message_grep(repo_path, source_branch, target_branch).await
        {
            Ok(Some(found_sha)) => {
                // Safety gate: don't fast-path to completion if prior validation
                // failures exist. The commits on target may be from a merge that
                // was reverted (or failed to revert) due to validation errors.
                // Returning false lets the normal merge flow handle it.
                if has_prior_validation_failure(task) {
                    tracing::warn!(
                        task_id = task_id_str,
                        source_branch = %source_branch,
                        target_branch = %target_branch,
                        found_sha = %found_sha,
                        "Source branch missing, commits on target, but prior validation \
                         failures detected — skipping recovery (will retry with fresh merge)"
                    );
                    return false;
                }

                tracing::info!(
                    task_id = task_id_str,
                    source_branch = %source_branch,
                    target_branch = %target_branch,
                    found_sha = %found_sha,
                    "Source branch missing but task commits found on target — recovering"
                );

                // Clean up orphaned merge worktree (same as "already merged" path)
                let merge_wt = compute_merge_worktree_path(project, task_id_str);
                let merge_wt_path = Path::new(&merge_wt);
                if crate::utils::path_safety::checked_exists(
                    merge_wt_path,
                    "orphaned merge worktree",
                )
                .unwrap_or(false)
                {
                    if let Err(e) = GitService::delete_worktree(repo_path, merge_wt_path).await {
                        tracing::warn!(
                            error = %e,
                            "Failed to clean up orphaned merge worktree (non-fatal)"
                        );
                    }
                }

                let target_sha = GitService::get_branch_sha(repo_path, target_branch)
                    .await
                    .unwrap_or_else(|_| found_sha.clone());

                if let Err(e) = complete_merge_internal_with_pr_sync(
                    task,
                    project,
                    &target_sha,
                    source_branch,
                    target_branch,
                    task_repo,
                    self.machine.context.services.external_events_repo.as_ref(),
                    self.machine.context.services.webhook_publisher.as_ref(),
                    self.machine.context.services.app_handle.as_ref(),
                    session_title,
                    Some(
                        super::merge_helpers::PlanBranchPrSyncServices::from_task_services(
                            &self.machine.context.services,
                        ),
                    ),
                )
                .await
                {
                    tracing::error!(error = %e, "Failed to complete merge for recovered task");
                } else {
                    self.post_merge_cleanup(task_id_str, task_id, repo_path, plan_branch_repo)
                        .await;

                    // Phase 3: fire-and-forget cleanup
                    let cleanup_task_id = task_id.clone();
                    let cleanup_repo = Arc::clone(task_repo);
                    let cleanup_dir = project.working_directory.clone();
                    let cleanup_branch = task.task_branch.clone();
                    let cleanup_wt = task.worktree_path.clone();
                    let cleanup_plan_branch = Some(target_branch.to_string());
                    tokio::spawn(async move {
                        super::merge_completion::deferred_merge_cleanup(
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
                true
            }
            Ok(None) => {
                tracing::debug!(
                    task_id = task_id_str,
                    source_branch = %source_branch,
                    "Source branch missing, no task commits on target — falling through"
                );
                false
            }
            Err(e) => {
                tracing::warn!(
                    task_id = task_id_str,
                    error = %e,
                    "Failed to search for task commits on target branch"
                );
                false
            }
        }
    }

    /// Run the concurrent merge guard under the merge_lock.
    ///
    /// This is the TOCTOU-safe check-and-set: acquires `merge_lock`, checks if another task
    /// is already merging to the same target branch, and either defers this task or clears
    /// a prior deferral flag.
    ///
    /// IMPORTANT: The deferral metadata write MUST happen inside the lock scope to prevent
    /// two tasks from both reading "no blocker" and both proceeding.
    ///
    /// Returns `ConcurrentGuardResult::Deferred` if the task was deferred (caller should return),
    /// or `ConcurrentGuardResult::Proceed` if the task should continue with the merge.
    pub(super) async fn run_concurrent_merge_guard(
        &self,
        task: &mut Task,
        task_id_str: &str,
        target_branch: &str,
        project: &Project,
        task_repo: &Arc<dyn TaskRepository>,
        plan_branch_repo: &Option<Arc<dyn PlanBranchRepository>>,
    ) -> ConcurrentGuardResult {
        let _merge_guard = self.machine.context.services.merge_lock.lock().await;

        let all_tasks = task_repo
            .get_by_project(&project.id)
            .await
            .unwrap_or_default();
        let merge_states = [InternalStatus::PendingMerge, InternalStatus::Merging];

        let this_pending_merge_at = task_repo
            .get_status_entered_at(&task.id, InternalStatus::PendingMerge)
            .await
            .unwrap_or(None);

        // Snapshot in-flight merge task IDs so we can detect tasks that are
        // past this lock and actively merging (cleanup/strategy phase).
        let in_flight_ids: std::collections::HashSet<String> = self
            .machine
            .context
            .services
            .merges_in_flight
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();

        // Pre-load PR-polling task IDs to exclude from blocking check (AD14).
        // PR-mode tasks wait for GitHub merge and must not block the local pipeline.
        let pr_polling_ids: std::collections::HashSet<String> =
            if let Some(ref pbr) = plan_branch_repo {
                pbr.find_pr_polling_task_ids()
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .map(|id| id.as_str().to_string())
                    .collect()
            } else {
                std::collections::HashSet::new()
            };

        let blocking_task_info = Self::find_blocking_merge_task(
            task,
            task_id_str,
            target_branch,
            project,
            &all_tasks,
            &merge_states,
            this_pending_merge_at,
            &in_flight_ids,
            task_repo,
            plan_branch_repo,
            &pr_polling_ids,
        )
        .await;

        if blocking_task_info.is_some() {
            self.defer_merge_for_blocker(
                task,
                task_id_str,
                target_branch,
                blocking_task_info,
                task_repo,
            )
            .await;
            return ConcurrentGuardResult::Deferred;
        }

        // If this task was previously deferred, log attempt_started and clear the flag
        if has_merge_deferred_metadata(task) {
            self.record_deferred_retry(task, task_id_str, target_branch, task_repo)
                .await;
        }

        ConcurrentGuardResult::Proceed
    }

    /// Find a blocking task that is already merging to the same target branch.
    ///
    /// Returns `Some(TaskId)` of the blocker if this task should defer, `None` to proceed.
    async fn find_blocking_merge_task(
        task: &Task,
        task_id_str: &str,
        target_branch: &str,
        project: &Project,
        all_tasks: &[Task],
        merge_states: &[InternalStatus],
        this_pending_merge_at: Option<chrono::DateTime<chrono::Utc>>,
        in_flight_ids: &std::collections::HashSet<String>,
        task_repo: &Arc<dyn TaskRepository>,
        plan_branch_repo: &Option<Arc<dyn PlanBranchRepository>>,
        pr_polling_task_ids: &std::collections::HashSet<String>,
    ) -> Option<TaskId> {
        for other in all_tasks {
            // Skip self
            if other.id == task.id {
                continue;
            }
            // Skip PR-polling tasks (AD14): they are waiting for GitHub merge, not
            // blocking the local merge pipeline. PR-mode tasks have their own guard.
            if pr_polling_task_ids.contains(other.id.as_str()) {
                continue;
            }
            // Only consider tasks in merge states
            if !merge_states.contains(&other.internal_status) {
                continue;
            }
            // Skip tasks that are themselves deferred — UNLESS they are
            // currently in-flight (past the lock, actively merging).
            let other_in_flight = in_flight_ids.contains(other.id.as_str());
            if has_merge_deferred_metadata(other) && !other_in_flight {
                continue;
            }
            // Skip archived tasks
            if other.archived_at.is_some() {
                continue;
            }
            // Check if targeting the same branch
            if !task_targets_branch(other, project, plan_branch_repo, target_branch).await {
                continue;
            }

            // If the other task is already in-flight, always defer to it
            if other_in_flight {
                tracing::info!(
                    event = "merge_arbitration_decision",
                    winner_task_id = other.id.as_str(),
                    loser_task_id = task_id_str,
                    target_branch = %target_branch,
                    reason = "other_task_already_in_flight",
                    "Merge arbitration: deferring — other task is actively merging"
                );
                return Some(other.id.clone());
            }

            // Get other task's pending_merge entry timestamp
            let other_pending_merge_at = task_repo
                .get_status_entered_at(&other.id, InternalStatus::PendingMerge)
                .await
                .unwrap_or(None);

            // Determine priority: earliest pending_merge entry wins
            let should_defer = Self::compare_merge_priority(
                other_pending_merge_at,
                this_pending_merge_at,
                other.id.as_str(),
                task.id.as_str(),
            );

            if should_defer {
                let reason = match (other_pending_merge_at, this_pending_merge_at) {
                    (Some(_), Some(_)) => "earlier_pending_merge_timestamp",
                    (Some(_), None) => "other_has_timestamp_this_missing",
                    (None, None) => "lexical_task_id_tiebreaker",
                    _ => "unknown",
                };

                tracing::info!(
                    event = "merge_arbitration_decision",
                    winner_task_id = other.id.as_str(),
                    loser_task_id = task_id_str,
                    winner_pending_merge_at = ?other_pending_merge_at,
                    loser_pending_merge_at = ?this_pending_merge_at,
                    target_branch = %target_branch,
                    reason = reason,
                    "Merge arbitration: deferring loser task"
                );
                return Some(other.id.clone());
            }
        }
        None
    }

    /// Compare merge priority between two tasks.
    ///
    /// Returns `true` if `other` should win (i.e., `this` task should defer).
    fn compare_merge_priority(
        other_pending_merge_at: Option<chrono::DateTime<chrono::Utc>>,
        this_pending_merge_at: Option<chrono::DateTime<chrono::Utc>>,
        other_id: &str,
        this_id: &str,
    ) -> bool {
        match (other_pending_merge_at, this_pending_merge_at) {
            (Some(other_time), Some(this_time)) => {
                use std::cmp::Ordering;
                match other_time.cmp(&this_time) {
                    Ordering::Less => true,
                    Ordering::Equal => other_id < this_id,
                    Ordering::Greater => false,
                }
            }
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => other_id < this_id,
        }
    }

    /// Set merge_deferred metadata and emit events when this task must defer.
    ///
    /// MUST be called inside the merge_lock scope to prevent TOCTOU races.
    async fn defer_merge_for_blocker(
        &self,
        task: &mut Task,
        task_id_str: &str,
        target_branch: &str,
        blocking_task_info: Option<TaskId>,
        task_repo: &Arc<dyn TaskRepository>,
    ) {
        let now = chrono::Utc::now().to_rfc3339();

        let blocking_task_id_str = blocking_task_info
            .as_ref()
            .map(|id| id.as_str().to_string());

        let mut recovery = MergeRecoveryMetadata::from_task_metadata(task.metadata.as_deref())
            .unwrap_or(None)
            .unwrap_or_else(MergeRecoveryMetadata::new);

        let mut event = MergeRecoveryEvent::new(
            MergeRecoveryEventKind::Deferred,
            MergeRecoverySource::System,
            MergeRecoveryReasonCode::TargetBranchBusy,
            format!(
                "Merge deferred: another task is merging to {}",
                target_branch
            ),
        )
        .with_target_branch(target_branch)
        .with_source_branch(task.task_branch.as_deref().unwrap_or("unknown"));

        if let Some(blocker_id) = blocking_task_info {
            event = event.with_blocking_task(blocker_id);
        }

        recovery.append_event_with_state(event, MergeRecoveryState::Deferred);

        match recovery.update_task_metadata(task.metadata.as_deref()) {
            Ok(updated_json) => {
                task.metadata = Some(updated_json);
            }
            Err(e) => {
                tracing::error!(
                    task_id = task_id_str,
                    error = %e,
                    "Failed to serialize merge recovery metadata, falling back to legacy"
                );
                let mut meta = parse_metadata(task).unwrap_or_else(|| serde_json::json!({}));
                if let Some(obj) = meta.as_object_mut() {
                    obj.insert("merge_deferred".to_string(), serde_json::json!(true));
                    obj.insert(
                        "merge_deferred_at".to_string(),
                        serde_json::json!(now.clone()),
                    );
                }
                task.metadata = Some(meta.to_string());
            }
        }

        task.touch();

        if let Err(e) = task_repo.update(task).await {
            tracing::error!(
                task_id = task_id_str,
                error = %e,
                "Failed to update task with merge_deferred metadata"
            );
        }

        self.machine
            .context
            .services
            .event_emitter
            .emit_status_change(task_id_str, "pending_merge", "pending_merge")
            .await;

        tracing::info!(
            event = "merge_deferred",
            deferred_task_id = task_id_str,
            blocking_task_id = blocking_task_id_str.as_deref().unwrap_or("unknown"),
            target_branch = %target_branch,
            reason_code = "target_branch_busy",
            deferred_at = %now,
            "Task merge deferred due to competing merge on same target branch"
        );
    }

    /// Record an `attempt_started` event for a previously-deferred task and clear the deferred flag.
    async fn record_deferred_retry(
        &self,
        task: &mut Task,
        task_id_str: &str,
        target_branch: &str,
        task_repo: &Arc<dyn TaskRepository>,
    ) {
        let mut recovery = MergeRecoveryMetadata::from_task_metadata(task.metadata.as_deref())
            .unwrap_or(None)
            .unwrap_or_else(MergeRecoveryMetadata::new);

        let attempt_count = recovery
            .events
            .iter()
            .filter(|e| matches!(e.kind, MergeRecoveryEventKind::AttemptStarted))
            .count() as u32
            + 1;

        let event = MergeRecoveryEvent::new(
            MergeRecoveryEventKind::AttemptStarted,
            MergeRecoverySource::Auto,
            MergeRecoveryReasonCode::TargetBranchBusy,
            format!("Starting merge attempt {} after deferral", attempt_count),
        )
        .with_target_branch(target_branch)
        .with_source_branch(task.task_branch.as_deref().unwrap_or("unknown"))
        .with_attempt(attempt_count);

        recovery.append_event_with_state(event, MergeRecoveryState::Retrying);

        match recovery.update_task_metadata(task.metadata.as_deref()) {
            Ok(updated_json) => {
                task.metadata = Some(updated_json);
            }
            Err(e) => {
                tracing::error!(
                    task_id = task_id_str,
                    error = %e,
                    "Failed to serialize merge recovery metadata for attempt_started"
                );
            }
        }

        clear_merge_deferred_metadata(task);
        task.touch();
        if let Err(e) = task_repo.update(task).await {
            tracing::warn!(
                task_id = task_id_str,
                error = %e,
                "Failed to persist merge attempt_started metadata"
            );
        }

        tracing::info!(
            event = "merge_arbitration_winner_retry",
            task_id = task_id_str,
            target_branch = %target_branch,
            attempt = attempt_count,
            "Recorded attempt_started event for retrying merge"
        );
    }

    /// Build the squash commit message based on task category.
    pub(super) async fn build_squash_commit_message(
        &self,
        task: &Task,
        task_id_str: &str,
        source_branch: &str,
        target_branch: &str,
    ) -> String {
        if task.category == TaskCategory::PlanMerge {
            if let (Some(session_id), Some(task_repo), Some(session_repo)) = (
                task.ideation_session_id.as_ref(),
                self.machine.context.services.task_repo.as_deref(),
                self.machine
                    .context
                    .services
                    .ideation_session_repo
                    .as_deref(),
            ) {
                build_plan_merge_commit_msg(
                    session_id,
                    source_branch,
                    target_branch,
                    task_repo,
                    session_repo,
                )
                .await
            } else {
                tracing::warn!(
                    task_id = task_id_str,
                    has_session_id = task.ideation_session_id.is_some(),
                    has_task_repo = self.machine.context.services.task_repo.is_some(),
                    has_session_repo = self
                        .machine
                        .context
                        .services
                        .ideation_session_repo
                        .is_some(),
                    "build_plan_merge_commit_msg: repos unavailable, using generic message"
                );
                format!("feat: {}\n\nPlan branch: {}", task.title, source_branch)
            }
        } else {
            build_squash_commit_msg(&task.category, &task.title, source_branch)
        }
    }

    /// Dispatch the merge strategy with a timeout and handle the outcome.
    ///
    /// The git strategy itself runs under the merge deadline timeout (fast, seconds).
    /// Outcome handling (including post-merge validation) runs outside that timeout
    /// so long-running validation commands (e.g. `cargo test`) don't compete with
    /// the git operation deadline.
    pub(super) async fn dispatch_merge_strategy(
        &self,
        tc: super::TaskCore<'_>,
        bp: super::BranchPair<'_>,
        pc: super::ProjectCtx<'_>,
        squash_commit_msg: &str,
        plan_branch_repo: &Option<Arc<dyn PlanBranchRepository>>,
        remaining: std::time::Duration,
        deadline_secs: u64,
    ) {
        let (task, task_id, task_id_str, task_repo) =
            (tc.task, tc.task_id, tc.task_id_str, tc.task_repo);
        let (source_branch, target_branch) = (bp.source_branch, bp.target_branch);
        let (project, repo_path) = (pc.project, pc.repo_path);

        // Pre-fetch session_title for merge:ready enrichment
        let session_title_for_ready = if let (Some(ref sid), Some(session_repo)) = (
            &task.ideation_session_id,
            self.machine
                .context
                .services
                .ideation_session_repo
                .as_deref(),
        ) {
            session_repo
                .get_by_id(sid)
                .await
                .ok()
                .flatten()
                .and_then(|s| s.title)
        } else {
            None
        };

        tracing::info!(
            task_id = task_id_str,
            strategy = ?project.merge_strategy,
            source_branch = %source_branch,
            target_branch = %target_branch,
            remaining_ms = remaining.as_millis() as u64,
            deadline_secs,
            "Dispatching merge strategy"
        );

        // Emit merge:ready via both channels: external_events (SSE/poll) + webhook publisher.
        {
            let project_id_str = project.id.to_string();
            let mut ready_payload = serde_json::json!({
                "task_id": task_id_str,
                "project_id": project_id_str,
                "session_id": task.ideation_session_id,
                "category": task.category.to_string(),
                "source_branch": source_branch,
                "target_branch": target_branch,
                "timestamp": chrono::Utc::now().to_rfc3339(),
            });
            let ctx = WebhookPresentationContext {
                project_name: Some(project.name.clone()),
                session_title: session_title_for_ready,
                task_title: Some(task.title.clone()),
                presentation_kind: Some(PresentationKind::MergeReady),
            };
            ctx.inject_into(&mut ready_payload);
            if let (Some(ref repo), Some(ref publisher)) = (
                &self.machine.context.services.external_events_repo,
                &self.machine.context.services.webhook_publisher,
            ) {
                if let Err(e) = emit_external_webhook_event(
                    "merge:ready",
                    &project_id_str,
                    ready_payload,
                    repo,
                    publisher,
                )
                .await
                {
                    tracing::warn!(
                        error = %e,
                        task_id = task_id_str,
                        "merge_orchestrator: merge:ready emit failed (non-fatal)"
                    );
                }
            }
        }

        // Phase 1: Run git strategy under merge deadline (fast, seconds only)
        let git_result = tokio::time::timeout(remaining, async {
            // Early return: if branches are already identical, skip merge entirely (prevents empty
            // commits on main). Covers all strategies including plan merge path.
            if GitService::branches_have_same_content(repo_path, source_branch, target_branch)
                .await
                .unwrap_or(false)
            {
                tracing::debug!(
                    task_id = task_id_str,
                    source_branch = %source_branch,
                    target_branch = %target_branch,
                    "branches already identical — skipping merge strategy dispatch to prevent empty commit"
                );
                let commit_sha = match GitService::get_branch_sha(repo_path, target_branch).await {
                    Ok(sha) => sha,
                    Err(e) => return (MergeOutcome::GitError(e), MergeHandlerOptions::merge()),
                };
                let opts = match project.merge_strategy {
                    MergeStrategy::Merge => MergeHandlerOptions::merge(),
                    MergeStrategy::Rebase => MergeHandlerOptions::rebase(),
                    MergeStrategy::Squash | MergeStrategy::RebaseSquash => {
                        MergeHandlerOptions::squash()
                    }
                };
                // Branches are identical — no merge performed, no worktree needed.
                // Validation (if any) runs in the project root; this is safe because no code
                // changed and the repo state is identical to what a worktree would contain.
                return (MergeOutcome::Success { commit_sha, merge_path: repo_path.to_path_buf() }, opts);
            }

            match project.merge_strategy {
                MergeStrategy::Merge => {
                    let outcome = self
                        .merge_worktree_strategy(
                            repo_path,
                            source_branch,
                            target_branch,
                            project,
                            task_id_str,
                        )
                        .await;
                    (outcome, MergeHandlerOptions::merge())
                }
                MergeStrategy::Rebase => {
                    let outcome = self
                        .rebase_worktree_strategy(
                            repo_path,
                            source_branch,
                            target_branch,
                            project,
                            task_id_str,
                        )
                        .await;
                    (outcome, MergeHandlerOptions::rebase())
                }
                MergeStrategy::Squash => {
                    let outcome = self
                        .squash_worktree_strategy(
                            repo_path,
                            source_branch,
                            target_branch,
                            squash_commit_msg,
                            project,
                            task_id_str,
                        )
                        .await;
                    (outcome, MergeHandlerOptions::squash())
                }
                MergeStrategy::RebaseSquash => {
                    // If a previous attempt hit rebase conflicts or a source←target conflict was
                    // resolved by the merger agent, skip the rebase step and use squash-only.
                    // In both cases the source branch contains merge commits that rebase would
                    // drop, replaying individual commits and re-encountering the same conflicts.
                    let skip_rebase = has_prior_rebase_conflict(task)
                        || has_source_conflict_resolved(task);
                    if skip_rebase {
                        tracing::info!(
                            task_id = task_id_str,
                            prior_rebase_conflict = has_prior_rebase_conflict(task),
                            source_conflict_resolved = has_source_conflict_resolved(task),
                            "Skipping rebase — using squash-only to avoid re-encountering conflicts"
                        );
                        let outcome = self
                            .squash_worktree_strategy(
                                repo_path,
                                source_branch,
                                target_branch,
                                squash_commit_msg,
                                project,
                                task_id_str,
                            )
                            .await;
                        (outcome, MergeHandlerOptions::squash())
                    } else {
                        let outcome = self
                            .rebase_squash_worktree_strategy(
                                repo_path,
                                source_branch,
                                target_branch,
                                squash_commit_msg,
                                project,
                                task_id_str,
                            )
                            .await;
                        (outcome, MergeHandlerOptions::rebase_squash())
                    }
                }
            }
        })
        .await;

        // Phase 2: Handle outcome (incl. validation) — outside the git deadline
        match git_result {
            Ok((outcome, opts)) => {
                let mut ctx = MergeContext {
                    task,
                    task_id,
                    task_id_str,
                    project,
                    repo_path,
                    source_branch,
                    target_branch,
                    task_repo,
                    plan_branch_repo,
                    opts: &opts,
                };
                self.handle_merge_outcome(outcome, &mut ctx).await;
            }
            Err(_) => {
                tracing::error!(
                    task_id = task_id_str,
                    deadline_secs = deadline_secs,
                    "Programmatic merge exceeded deadline during strategy dispatch — transitioning to MergeIncomplete"
                );
                let metadata = serde_json::json!({
                    "error": format!("Merge attempt timed out after {}s (strategy dispatch exceeded deadline)", deadline_secs),
                    "source_branch": source_branch,
                    "target_branch": target_branch,
                    "strategy": format!("{:?}", project.merge_strategy),
                });
                self.transition_to_merge_incomplete(
                    super::TaskCore {
                        task: &mut *task,
                        task_id,
                        task_id_str,
                        task_repo,
                    },
                    metadata,
                    true,
                )
                .await;
            }
        }
    }
}
