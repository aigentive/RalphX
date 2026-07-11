// Merge strategy methods extracted from attempt_programmatic_merge
//
// Each strategy method handles the unique merge logic for a specific
// MergeStrategy using worktree isolation. They return a MergeOutcome which
// is then handled uniformly by the shared post-merge handler.
//
// Wired in side_effects.rs (Pass 2).

use std::path::{Path, PathBuf};

use crate::application::{GitService, MergeAttemptResult};
use crate::domain::entities::Project;
use crate::error::AppError;

use super::checkout_free_strategy::{
    checkout_free_fast_forward, checkout_free_merge, checkout_free_squash_merge,
    pre_delete_worktree, validate_branches,
};
use super::merge_helpers::{compute_merge_worktree_path, compute_rebase_worktree_path};

/// Outcome of a merge strategy execution.
///
/// This enum represents the result of attempting a merge operation,
/// allowing the caller to handle success, conflicts, missing branches,
/// and errors uniformly.
#[derive(Debug)]
pub(super) enum MergeOutcome {
    /// Merge succeeded, ready for post-merge validation/completion
    Success {
        commit_sha: String,
        /// Path where merge occurred (main repo or worktree)
        merge_path: PathBuf,
    },

    /// Merge conflicts detected, needs agent intervention
    NeedsAgent {
        conflict_files: Vec<PathBuf>,
        /// Worktree path for conflict resolution (None for checkout-free merges)
        merge_worktree: Option<PathBuf>,
    },

    /// Source or target branch not found
    BranchNotFound { branch: String },

    /// Merge deferred (e.g., branch lock held by another task, sibling tasks still running)
    #[allow(dead_code)]
    Deferred { reason: String },

    /// Git operation or other error
    GitError(AppError),

    /// Status already handled (early return case)
    #[allow(dead_code)]
    AlreadyHandled,
}

impl<'a> super::TransitionHandler<'a> {
    /// Strategy: (Merge, Worktree)
    ///
    /// Merge task branch into target using worktree isolation, or use checkout-free
    /// merge if target is already checked out in the main repo.
    pub(super) async fn merge_worktree_strategy(
        &self,
        repo_path: &Path,
        source_branch: &str,
        target_branch: &str,
        project: &Project,
        task_id_str: &str,
    ) -> MergeOutcome {
        let current_branch = GitService::get_current_branch(repo_path)
            .await
            .unwrap_or_default();
        if current_branch == target_branch {
            return checkout_free_merge(repo_path, source_branch, target_branch, task_id_str).await;
        }

        // Target not checked out — use isolated worktree

        if let Some(outcome) =
            validate_branches(repo_path, source_branch, target_branch, task_id_str).await
        {
            return outcome;
        }

        let merge_wt_path = compute_merge_worktree_path(project, task_id_str);
        let merge_wt = PathBuf::from(&merge_wt_path);

        tracing::info!(
            task_id = task_id_str,
            worktree = %merge_wt_path,
            "Attempting worktree merge"
        );

        pre_delete_worktree(repo_path, &merge_wt, task_id_str).await;

        match GitService::try_merge_in_worktree(repo_path, source_branch, target_branch, &merge_wt)
            .await
        {
            Ok(MergeAttemptResult::Success { commit_sha }) => {
                // If the inner function early-returned (e.g. branches_have_same_content guard),
                // no worktree was created. Fall back to repo_path for validation — branches are
                // identical so no code changed and the repo root is equivalent to a worktree.
                let actual_path = if merge_wt.exists() {
                    merge_wt
                } else {
                    repo_path.to_path_buf()
                };
                tracing::info!(
                    task_id = task_id_str,
                    commit_sha = %commit_sha,
                    merge_path = %actual_path.display(),
                    "Worktree merge succeeded"
                );
                MergeOutcome::Success {
                    commit_sha,
                    merge_path: actual_path,
                }
            }
            Ok(MergeAttemptResult::NeedsAgent { conflict_files }) => {
                tracing::warn!(
                    task_id = task_id_str,
                    conflict_count = conflict_files.len(),
                    "Worktree merge detected conflicts — worktree left for agent"
                );
                MergeOutcome::NeedsAgent {
                    conflict_files,
                    merge_worktree: Some(merge_wt),
                }
            }
            Ok(MergeAttemptResult::BranchNotFound { branch }) => {
                tracing::error!(task_id = task_id_str, branch = %branch, "Branch not found during merge");
                MergeOutcome::BranchNotFound { branch }
            }
            Err(e) => {
                tracing::error!(
                    task_id = task_id_str,
                    error = %e,
                    "Worktree merge failed"
                );
                if merge_wt.exists() {
                    // Best-effort cleanup: worktree deletion failure is non-fatal during error recovery
                    let _ = GitService::delete_worktree(repo_path, &merge_wt).await;
                }
                MergeOutcome::GitError(e)
            }
        }
    }

    /// Strategy: (Rebase, Worktree)
    ///
    /// Rebase in worktree then fast-forward merge, or use checkout-free if target checked out.
    pub(super) async fn rebase_worktree_strategy(
        &self,
        repo_path: &Path,
        source_branch: &str,
        target_branch: &str,
        project: &Project,
        task_id_str: &str,
    ) -> MergeOutcome {
        let current_branch = GitService::get_current_branch(repo_path)
            .await
            .unwrap_or_default();
        if current_branch == target_branch {
            return checkout_free_fast_forward(
                repo_path,
                source_branch,
                target_branch,
                task_id_str,
            )
            .await;
        }

        // Target not checked out — use worktree

        if let Some(outcome) =
            validate_branches(repo_path, source_branch, target_branch, task_id_str).await
        {
            return outcome;
        }

        let rebase_wt_path = compute_rebase_worktree_path(project, task_id_str);
        let rebase_wt = PathBuf::from(&rebase_wt_path);
        let merge_wt_path = compute_merge_worktree_path(project, task_id_str);
        let merge_wt = PathBuf::from(&merge_wt_path);

        tracing::info!(
            task_id = task_id_str,
            rebase_worktree = %rebase_wt_path,
            merge_worktree = %merge_wt_path,
            "Attempting worktree rebase-and-merge"
        );

        pre_delete_worktree(repo_path, &rebase_wt, task_id_str).await;
        pre_delete_worktree(repo_path, &merge_wt, task_id_str).await;

        match GitService::try_rebase_and_merge_in_worktree(
            repo_path,
            source_branch,
            target_branch,
            &rebase_wt,
            &merge_wt,
        )
        .await
        {
            Ok(MergeAttemptResult::Success { commit_sha }) => {
                // Clean up rebase worktree (no longer needed)
                let _ = GitService::delete_worktree(repo_path, &rebase_wt).await;
                // If the inner function early-returned (e.g. branches_have_same_content guard),
                // no merge worktree was created. Fall back to repo_path — branches are identical
                // so no code changed and the repo root is equivalent to a worktree.
                let actual_path = if merge_wt.exists() {
                    merge_wt
                } else {
                    repo_path.to_path_buf()
                };
                tracing::info!(
                    task_id = task_id_str,
                    commit_sha = %commit_sha,
                    merge_path = %actual_path.display(),
                    "Worktree rebase and merge succeeded"
                );
                MergeOutcome::Success {
                    commit_sha,
                    merge_path: actual_path,
                }
            }
            Ok(MergeAttemptResult::NeedsAgent { conflict_files }) => {
                tracing::warn!(
                    task_id = task_id_str,
                    conflict_count = conflict_files.len(),
                    "Worktree rebase detected conflicts — rebase worktree left for agent"
                );
                MergeOutcome::NeedsAgent {
                    conflict_files,
                    merge_worktree: Some(rebase_wt),
                }
            }
            Ok(MergeAttemptResult::BranchNotFound { branch }) => {
                tracing::error!(task_id = task_id_str, branch = %branch, "Branch not found during rebase");
                MergeOutcome::BranchNotFound { branch }
            }
            Err(e) => {
                tracing::error!(error = %e, "Worktree rebase failed");
                // Best-effort cleanup: worktree deletion failure is non-fatal during error recovery
                if rebase_wt.exists() {
                    let _ = GitService::delete_worktree(repo_path, &rebase_wt).await;
                }
                if merge_wt.exists() {
                    let _ = GitService::delete_worktree(repo_path, &merge_wt).await;
                }
                MergeOutcome::GitError(e)
            }
        }
    }

    /// Strategy: (Squash, Worktree)
    ///
    /// Squash merge in worktree, or use checkout-free if target checked out.
    pub(super) async fn squash_worktree_strategy(
        &self,
        repo_path: &Path,
        source_branch: &str,
        target_branch: &str,
        squash_commit_msg: &str,
        project: &Project,
        task_id_str: &str,
    ) -> MergeOutcome {
        let current_branch = GitService::get_current_branch(repo_path)
            .await
            .unwrap_or_default();
        if current_branch == target_branch {
            return checkout_free_squash_merge(
                repo_path,
                source_branch,
                target_branch,
                squash_commit_msg,
                task_id_str,
            )
            .await;
        }

        let merge_wt_path = compute_merge_worktree_path(project, task_id_str);
        let merge_wt = PathBuf::from(&merge_wt_path);

        pre_delete_worktree(repo_path, &merge_wt, task_id_str).await;

        match GitService::try_squash_merge_in_worktree(
            repo_path,
            source_branch,
            target_branch,
            &merge_wt,
            squash_commit_msg,
        )
        .await
        {
            Ok(MergeAttemptResult::Success { commit_sha }) => {
                // If inner function early-returned (branches_have_same_content)
                // the worktree was never created. Create one for validation instead of
                // falling back to repo_path — running validation in project root is unsafe.
                let actual_path = if merge_wt.exists() {
                    merge_wt
                } else {
                    tracing::info!(
                        task_id = task_id_str,
                        "Merge worktree not created (trivial merge), creating one for validation"
                    );
                    match GitService::checkout_existing_branch_worktree(
                        repo_path,
                        &merge_wt,
                        target_branch,
                    )
                    .await
                    {
                        Ok(_) => merge_wt,
                        Err(e) => {
                            tracing::error!(
                                task_id = task_id_str,
                                error = %e,
                                "Failed to create validation worktree after trivial merge"
                            );
                            return MergeOutcome::GitError(e);
                        }
                    }
                };
                MergeOutcome::Success {
                    commit_sha,
                    merge_path: actual_path,
                }
            }
            Ok(MergeAttemptResult::NeedsAgent { conflict_files }) => MergeOutcome::NeedsAgent {
                conflict_files,
                merge_worktree: Some(merge_wt),
            },
            Ok(MergeAttemptResult::BranchNotFound { branch }) => {
                MergeOutcome::BranchNotFound { branch }
            }
            Err(e) => MergeOutcome::GitError(e),
        }
    }

    /// Strategy: (RebaseSquash, Worktree)
    ///
    /// Rebase in worktree, then squash merge. Uses checkout-free if target checked out.
    pub(super) async fn rebase_squash_worktree_strategy(
        &self,
        repo_path: &Path,
        source_branch: &str,
        target_branch: &str,
        squash_commit_msg: &str,
        project: &Project,
        task_id_str: &str,
    ) -> MergeOutcome {
        let current_branch = GitService::get_current_branch(repo_path)
            .await
            .unwrap_or_default();
        if current_branch == target_branch {
            return checkout_free_squash_merge(
                repo_path,
                source_branch,
                target_branch,
                squash_commit_msg,
                task_id_str,
            )
            .await;
        }

        // Dual-worktree strategy: rebase worktree first, then squash merge in separate worktree

        if let Some(outcome) =
            validate_branches(repo_path, source_branch, target_branch, task_id_str).await
        {
            return outcome;
        }

        let rebase_wt_path = compute_rebase_worktree_path(project, task_id_str);
        let rebase_wt = PathBuf::from(&rebase_wt_path);
        let merge_wt_path = compute_merge_worktree_path(project, task_id_str);
        let merge_wt = PathBuf::from(&merge_wt_path);

        tracing::info!(
            task_id = task_id_str,
            rebase_worktree = %rebase_wt_path,
            merge_worktree = %merge_wt_path,
            "Attempting worktree rebase-squash"
        );

        pre_delete_worktree(repo_path, &rebase_wt, task_id_str).await;
        pre_delete_worktree(repo_path, &merge_wt, task_id_str).await;

        match GitService::try_rebase_squash_merge_in_worktree(
            repo_path,
            source_branch,
            target_branch,
            &rebase_wt,
            &merge_wt,
            squash_commit_msg,
        )
        .await
        {
            Ok(MergeAttemptResult::Success { commit_sha }) => {
                // Clean up rebase worktree (no longer needed)
                let _ = GitService::delete_worktree(repo_path, &rebase_wt).await;
                // If inner function early-returned (branches_have_same_content or
                // base_commit_count <= 1 with identical branches) the merge worktree
                // was never created. Create one for validation instead of falling back
                // to repo_path — running validation in project root is unsafe (interferes
                // with user's dev server, cargo locks, etc.).
                let actual_path = if merge_wt.exists() {
                    merge_wt
                } else {
                    tracing::info!(
                        task_id = task_id_str,
                        "Merge worktree not created (trivial merge), creating one for validation"
                    );
                    match GitService::checkout_existing_branch_worktree(
                        repo_path,
                        &merge_wt,
                        target_branch,
                    )
                    .await
                    {
                        Ok(_) => merge_wt,
                        Err(e) => {
                            tracing::error!(
                                task_id = task_id_str,
                                error = %e,
                                "Failed to create validation worktree after trivial merge"
                            );
                            return MergeOutcome::GitError(e);
                        }
                    }
                };
                tracing::info!(
                    task_id = task_id_str,
                    commit_sha = %commit_sha,
                    merge_path = %actual_path.display(),
                    "Worktree rebase-squash succeeded"
                );
                MergeOutcome::Success {
                    commit_sha,
                    merge_path: actual_path,
                }
            }
            Ok(MergeAttemptResult::NeedsAgent { conflict_files }) => {
                tracing::warn!(
                    task_id = task_id_str,
                    conflict_count = conflict_files.len(),
                    "Worktree rebase-squash detected conflicts — rebase worktree left for agent"
                );
                MergeOutcome::NeedsAgent {
                    conflict_files,
                    merge_worktree: Some(rebase_wt),
                }
            }
            Ok(MergeAttemptResult::BranchNotFound { branch }) => {
                tracing::error!(task_id = task_id_str, branch = %branch, "Branch not found during rebase-squash");
                MergeOutcome::BranchNotFound { branch }
            }
            Err(e) => {
                tracing::error!(error = %e, "Worktree rebase-squash failed");
                // Best-effort cleanup: worktree deletion failure is non-fatal during error recovery
                if rebase_wt.exists() {
                    let _ = GitService::delete_worktree(repo_path, &rebase_wt).await;
                }
                if merge_wt.exists() {
                    let _ = GitService::delete_worktree(repo_path, &merge_wt).await;
                }
                MergeOutcome::GitError(e)
            }
        }
    }
}
