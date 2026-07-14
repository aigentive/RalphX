use std::path::{Path, PathBuf};

use crate::application::GitService;
use crate::domain::entities::merge_progress_event::{MergePhase, MergePhaseStatus};

/// Result of attempting to update a plan branch from its source/base branch.
#[derive(Debug)]
pub(crate) enum PlanUpdateResult {
    /// Plan branch was already up-to-date with its source/base branch (no action needed).
    AlreadyUpToDate,
    /// Plan branch was updated (fast-forward or merge commit created).
    Updated,
    /// Plan branch target is already the source/base branch itself — skip update.
    NotPlanBranch,
    /// Merge source/base branch into the plan branch produced conflicts — needs agent resolution.
    Conflicts { conflict_files: Vec<PathBuf> },
    /// Git error during the update attempt.
    Error(String),
}

/// Update a plan branch from its source/base branch before a task→plan merge.
///
/// When a task merges into a plan branch (not its source/base branch), the plan branch may be behind
/// its source/base branch if fixes were committed there after the plan branch was created. This causes
/// false validation failures because the plan branch code doesn't have those fixes.
///
/// This function checks if the plan branch is behind the source/base branch and, if so, merges it
/// into the plan branch using an isolated worktree. On conflict, returns
/// `PlanUpdateResult::Conflicts` so the caller can route to the merger agent.
///
/// Only runs when `target_branch != base_branch` (i.e., target is a plan branch).
#[cfg(test)]
pub(crate) async fn update_plan_from_main(
    repo_path: &Path,
    target_branch: &str,
    base_branch: &str,
    project: &crate::domain::entities::Project,
    task_id_str: &str,
    event_sink: Option<&dyn ralphx_events::EventSink>,
) -> PlanUpdateResult {
    update_plan_from_main_with_options(
        repo_path,
        target_branch,
        base_branch,
        project,
        task_id_str,
        event_sink,
        true,
    )
    .await
}

/// Update a plan branch from its source/base branch without mutating the primary checkout.
///
/// This is used by PR-mode freshness reconciliation, where background repair must never
/// change the target project's currently-open working tree.
pub(crate) async fn update_plan_from_main_isolated(
    repo_path: &Path,
    target_branch: &str,
    base_branch: &str,
    project: &crate::domain::entities::Project,
    task_id_str: &str,
    event_sink: Option<&dyn ralphx_events::EventSink>,
) -> PlanUpdateResult {
    update_plan_from_main_with_options(
        repo_path,
        target_branch,
        base_branch,
        project,
        task_id_str,
        event_sink,
        false,
    )
    .await
}

fn is_project_primary_checkout(
    repo_path: &Path,
    project: &crate::domain::entities::Project,
) -> bool {
    let project_path = Path::new(project.working_directory.as_str());
    let repo_path = repo_path
        .canonicalize()
        .unwrap_or_else(|_| repo_path.to_path_buf());
    let project_path = project_path
        .canonicalize()
        .unwrap_or_else(|_| project_path.to_path_buf());
    repo_path == project_path
}

async fn update_plan_from_main_with_options(
    repo_path: &Path,
    target_branch: &str,
    base_branch: &str,
    project: &crate::domain::entities::Project,
    task_id_str: &str,
    event_sink: Option<&dyn ralphx_events::EventSink>,
    allow_primary_checkout_merge: bool,
) -> PlanUpdateResult {
    // Only update when merging to a plan branch (not the source/base branch itself)
    if target_branch == base_branch {
        return PlanUpdateResult::NotPlanBranch;
    }

    // Check if the source/base branch HEAD is already an ancestor of the plan branch
    // (i.e., the plan branch already has all of the source/base branch changes)
    let main_sha = match GitService::get_branch_sha(repo_path, base_branch).await {
        Ok(sha) => sha,
        Err(e) => {
            tracing::warn!(
                task_id = task_id_str,
                error = %e,
                base_branch = %base_branch,
                "Failed to get SHA for base branch — skipping plan branch update"
            );
            return PlanUpdateResult::Error(format!(
                "Failed to get SHA for {}: {}",
                base_branch, e
            ));
        }
    };

    match GitService::is_commit_on_branch(repo_path, &main_sha, target_branch).await {
        Ok(true) => {
            tracing::debug!(
                task_id = task_id_str,
                target_branch = %target_branch,
                base_branch = %base_branch,
                "Plan branch is up-to-date with its source branch — no update needed"
            );
            return PlanUpdateResult::AlreadyUpToDate;
        }
        Ok(false) => {
            tracing::info!(
                task_id = task_id_str,
                target_branch = %target_branch,
                base_branch = %base_branch,
                main_sha = %main_sha,
                "Plan branch is behind its source branch — updating before task merge"
            );
        }
        Err(e) => {
            tracing::warn!(
                task_id = task_id_str,
                error = %e,
                "Failed to check if plan branch is up-to-date — skipping update"
            );
            return PlanUpdateResult::Error(format!("is_commit_on_branch check failed: {}", e));
        }
    }

    super::emit_merge_progress(
        event_sink,
        task_id_str,
        MergePhase::programmatic_merge(),
        MergePhaseStatus::Started,
        format!(
            "Updating {} from {} before merge",
            target_branch, base_branch
        ),
    );

    // Use checkout-free merge if target is already checked out in this repo/worktree.
    let current_branch = GitService::get_current_branch(repo_path)
        .await
        .unwrap_or_default();
    if current_branch == target_branch {
        let is_primary_checkout = is_project_primary_checkout(repo_path, project);
        if !allow_primary_checkout_merge && is_primary_checkout {
            tracing::warn!(
                task_id = task_id_str,
                target_branch = %target_branch,
                repo_path = %repo_path.display(),
                "Plan branch update refused because target branch is checked out in the primary repo"
            );
            return PlanUpdateResult::Error(format!(
                "Refusing to update plan branch '{target_branch}' in the primary checkout"
            ));
        }
        // Target is checked out here — merge the source/base branch directly.
        match GitService::merge_branch(repo_path, base_branch, target_branch).await {
            Ok(result) => {
                let sha = match &result {
                    crate::application::MergeResult::Success { commit_sha }
                    | crate::application::MergeResult::FastForward { commit_sha } => {
                        commit_sha.clone()
                    }
                    crate::application::MergeResult::Conflict { files } => {
                        // Abort the in-progress merge so the working tree is clean
                        let _ = GitService::abort_merge(repo_path).await;
                        tracing::warn!(
                            task_id = task_id_str,
                            conflict_count = files.len(),
                            "Conflicts detected updating plan branch from source branch (checkout-free)"
                        );
                        return PlanUpdateResult::Conflicts {
                            conflict_files: files.iter().map(PathBuf::from).collect(),
                        };
                    }
                };
                tracing::info!(
                    task_id = task_id_str,
                    commit_sha = %sha,
                    "Plan branch updated from source branch (checkout-free)"
                );
                return PlanUpdateResult::Updated;
            }
            Err(e) => {
                return PlanUpdateResult::Error(format!("checkout-free merge failed: {}", e));
            }
        }
    }

    // Fallback: if the plan branch is already checked out in an existing worktree
    // (e.g., merge worktree from a prior attempt), merge the source/base branch directly there instead
    // of trying to create a new worktree (which would fail with "already used by worktree").
    if let Ok(worktrees) = GitService::list_worktrees(repo_path).await {
        if let Some(wt) = worktrees
            .iter()
            .find(|w| w.branch.as_deref() == Some(target_branch))
        {
            let wt_path = PathBuf::from(&wt.path);
            if !wt_path.exists() {
                tracing::warn!(
                    task_id = task_id_str,
                    path = %wt_path.display(),
                    "Stale worktree entry — path deleted, pruning before fresh creation"
                );
                // Prune the stale entry so fresh worktree creation won't hit "already checked out"
                let _ = GitService::delete_worktree(repo_path, &wt_path).await;
                super::cleanup_helpers::git_worktree_prune(repo_path).await;
                // Fall through to fresh worktree creation below
            } else {
                tracing::info!(
                    task_id = task_id_str,
                    target_branch = %target_branch,
                    worktree_path = %wt_path.display(),
                    "Plan branch already checked out in existing worktree — merging source branch there"
                );
                match GitService::merge_branch(&wt_path, base_branch, target_branch).await {
                    Ok(result) => {
                        let sha = match &result {
                            crate::application::MergeResult::Success { commit_sha }
                            | crate::application::MergeResult::FastForward { commit_sha } => {
                                commit_sha.clone()
                            }
                            crate::application::MergeResult::Conflict { files } => {
                                let _ = GitService::abort_merge(&wt_path).await;
                                tracing::warn!(
                                task_id = task_id_str,
                                conflict_count = files.len(),
                                "Conflicts detected updating plan branch from source branch (existing worktree)"
                            );
                                return PlanUpdateResult::Conflicts {
                                    conflict_files: files.iter().map(PathBuf::from).collect(),
                                };
                            }
                        };
                        tracing::info!(
                            task_id = task_id_str,
                            commit_sha = %sha,
                            "Plan branch updated from source branch (existing worktree)"
                        );
                        return PlanUpdateResult::Updated;
                    }
                    Err(e) => {
                        // Check if the error is because the worktree already has a merge in
                        // progress from a prior attempt. When git sees MERGE_HEAD it refuses
                        // to start a new merge and returns "You have not concluded your merge"
                        // — no "CONFLICT" in stderr, so merge_branch returns Err instead of
                        // Ok(MergeResult::Conflict). Route to Conflicts so the agent can
                        // resolve the existing conflict markers without discarding them.
                        if GitService::is_merge_in_progress(&wt_path) {
                            let conflict_files = GitService::get_conflict_files(&wt_path)
                                .await
                                .unwrap_or_default();
                            tracing::warn!(
                                task_id = task_id_str,
                                conflict_count = conflict_files.len(),
                                worktree_path = %wt_path.display(),
                                "Plan branch already in merge conflict state in existing worktree \
                                 (prior attempt) — routing to merger agent without aborting"
                            );
                            return PlanUpdateResult::Conflicts { conflict_files };
                        }
                        // Not a conflict — abort any partial state and return as error.
                        let _ = GitService::abort_merge(&wt_path).await;
                        return PlanUpdateResult::Error(format!(
                            "merge in existing worktree failed: {}",
                            e
                        ));
                    }
                }
            } // end else (wt_path.exists())
        }
    }

    // Target not checked out anywhere — use isolated worktree
    // try_merge_in_worktree merges base_branch (main) into target_branch (plan)
    let wt_path_str = super::merge_helpers::compute_plan_update_worktree_path(project, task_id_str);
    let wt_path = PathBuf::from(&wt_path_str);

    // Clean up any stale worktree from a prior attempt
    super::merge_helpers::pre_delete_worktree(repo_path, &wt_path, task_id_str).await;

    let result = match GitService::try_merge_in_worktree(
        repo_path,
        base_branch,   // source = main
        target_branch, // target = plan branch
        &wt_path,
    )
    .await
    {
        Ok(crate::application::MergeAttemptResult::Success { commit_sha }) => {
            tracing::info!(
                task_id = task_id_str,
                target_branch = %target_branch,
                base_branch = %base_branch,
                commit_sha = %commit_sha,
                "Plan branch updated from main via worktree"
            );
            PlanUpdateResult::Updated
        }
        Ok(crate::application::MergeAttemptResult::NeedsAgent { conflict_files }) => {
            tracing::warn!(
                task_id = task_id_str,
                conflict_count = conflict_files.len(),
                "Conflicts detected updating plan branch from main via worktree"
            );
            PlanUpdateResult::Conflicts { conflict_files }
        }
        Ok(crate::application::MergeAttemptResult::BranchNotFound { branch }) => {
            PlanUpdateResult::Error(format!("Branch not found during plan update: {}", branch))
        }
        Err(e) => PlanUpdateResult::Error(format!("Plan update merge failed: {}", e)),
    };

    // Clean up worktree (always, regardless of outcome)
    super::merge_helpers::pre_delete_worktree(repo_path, &wt_path, task_id_str).await;

    result
}
