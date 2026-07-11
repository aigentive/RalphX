// Checkout-free merge strategy helpers.
//
// When the target branch is already checked out in the primary repo, Git
// forbids creating a worktree for it. These helpers perform the merge using
// git plumbing (checkout-free) instead.
//
// Extracted from merge_strategies.rs to reduce duplication across 4 strategy
// methods that each had an identical ~70 LOC checkout-free block.

use std::path::Path;

use crate::application::git_service::checkout_free::{self, CheckoutFreeMergeResult};
use crate::application::GitService;
use crate::error::AppError;

use super::merge_strategies::MergeOutcome;

/// Validate that both source and target branches exist.
///
/// Returns `Some(MergeOutcome::BranchNotFound)` if either is missing,
/// `None` if both exist.
pub(super) async fn validate_branches(
    repo_path: &Path,
    source_branch: &str,
    target_branch: &str,
    task_id: &str,
) -> Option<MergeOutcome> {
    if !GitService::branch_exists(repo_path, source_branch)
        .await
        .unwrap_or(false)
    {
        tracing::error!(
            task_id = task_id,
            "Source branch '{}' does not exist",
            source_branch
        );
        return Some(MergeOutcome::BranchNotFound {
            branch: source_branch.to_string(),
        });
    }
    if !GitService::branch_exists(repo_path, target_branch)
        .await
        .unwrap_or(false)
    {
        tracing::error!(
            task_id = task_id,
            "Target branch '{}' does not exist",
            target_branch
        );
        return Some(MergeOutcome::BranchNotFound {
            branch: target_branch.to_string(),
        });
    }
    None
}

/// Map a `CheckoutFreeMergeResult` (or error) to a `MergeOutcome`,
/// resetting the working tree on success.
async fn handle_checkout_free_result(
    repo_path: &Path,
    result: Result<CheckoutFreeMergeResult, AppError>,
    task_id: &str,
    label: &str,
) -> MergeOutcome {
    match result {
        Ok(CheckoutFreeMergeResult::Success { commit_sha }) => {
            tracing::info!(
                task_id = task_id,
                commit_sha = %commit_sha,
                "Checkout-free {} succeeded",
                label,
            );

            if let Err(e) = GitService::hard_reset_to_head(repo_path).await {
                tracing::warn!(
                    task_id = task_id,
                    error = %e,
                    "Failed to reset working tree after checkout-free {} (non-fatal)",
                    label,
                );
            }

            MergeOutcome::Success {
                commit_sha,
                merge_path: repo_path.to_path_buf(),
            }
        }
        Ok(CheckoutFreeMergeResult::Conflict { files }) => {
            tracing::warn!(
                task_id = task_id,
                conflict_count = files.len(),
                "Checkout-free {} detected conflicts",
                label,
            );
            MergeOutcome::NeedsAgent {
                conflict_files: files,
                merge_worktree: None,
            }
        }
        Err(e) => {
            tracing::error!(
                task_id = task_id,
                error = %e,
                "Checkout-free {} failed",
                label,
            );
            MergeOutcome::GitError(e)
        }
    }
}

/// Checkout-free merge (used by merge_worktree_strategy).
pub(super) async fn checkout_free_merge(
    repo_path: &Path,
    source_branch: &str,
    target_branch: &str,
    task_id: &str,
) -> MergeOutcome {
    tracing::info!(
        task_id = task_id,
        target_branch = %target_branch,
        "Target branch is checked out, using checkout-free merge"
    );

    if let Some(outcome) = validate_branches(repo_path, source_branch, target_branch, task_id).await
    {
        return outcome;
    }

    let result =
        checkout_free::try_merge_checkout_free(repo_path, source_branch, target_branch).await;
    handle_checkout_free_result(repo_path, result, task_id, "merge").await
}

/// Checkout-free fast-forward (used by rebase_worktree_strategy).
pub(super) async fn checkout_free_fast_forward(
    repo_path: &Path,
    source_branch: &str,
    target_branch: &str,
    task_id: &str,
) -> MergeOutcome {
    tracing::info!(
        task_id = task_id,
        target_branch = %target_branch,
        "Target branch is checked out, using checkout-free rebase"
    );

    if let Some(outcome) = validate_branches(repo_path, source_branch, target_branch, task_id).await
    {
        return outcome;
    }

    let result =
        checkout_free::try_fast_forward_checkout_free(repo_path, source_branch, target_branch)
            .await;
    handle_checkout_free_result(repo_path, result, task_id, "fast-forward").await
}

/// Checkout-free squash merge (used by squash and rebase-squash strategies).
pub(super) async fn checkout_free_squash_merge(
    repo_path: &Path,
    source_branch: &str,
    target_branch: &str,
    squash_commit_msg: &str,
    task_id: &str,
) -> MergeOutcome {
    tracing::info!(
        task_id = task_id,
        target_branch = %target_branch,
        "Target checked out, using checkout-free squash"
    );

    if let Some(outcome) = validate_branches(repo_path, source_branch, target_branch, task_id).await
    {
        return outcome;
    }

    let result = checkout_free::try_squash_merge_checkout_free(
        repo_path,
        source_branch,
        target_branch,
        squash_commit_msg,
    )
    .await;
    handle_checkout_free_result(repo_path, result, task_id, "squash").await
}

// Re-export from merge_helpers (moved there for shared use across multiple modules)
pub(super) use super::merge_helpers::pre_delete_worktree;
