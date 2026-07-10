use std::path::{Path, PathBuf};

use crate::application::GitService;
use crate::domain::entities::merge_progress_event::{MergePhase, MergePhaseStatus};

/// Result of attempting to update a source (task/feature) branch from its target branch.
#[derive(Debug)]
pub(crate) enum SourceUpdateResult {
    /// Source branch already contains all of target's changes.
    AlreadyUpToDate,
    /// Source branch was updated (merge commit created).
    Updated,
    /// Merge target into source produced conflicts — needs agent resolution.
    Conflicts { conflict_files: Vec<PathBuf> },
    /// Source or target branch was missing before the update could run.
    BranchMissing { branch: String },
    /// Git error during the update attempt.
    Error(String),
}

/// Update the source (task/feature) branch from the target branch before merging.
///
/// When a task branch merges into a target (plan or main), the task branch may be behind
/// the target if other tasks were merged into it since this task branched off. This causes
/// false validation failures because the merged code doesn't include those changes.
///
/// This function checks if the target branch's HEAD is an ancestor of the source branch.
/// If not, it merges the target into the source using an isolated worktree to bring it
/// up-to-date. On conflict, returns `SourceUpdateResult::Conflicts` so the caller can
/// route to the merger agent.
pub(crate) async fn update_source_from_target(
    repo_path: &Path,
    source_branch: &str,
    target_branch: &str,
    project: &crate::domain::entities::Project,
    task_id_str: &str,
    event_sink: Option<&dyn ralphx_events::EventSink>,
) -> SourceUpdateResult {
    if !repo_path.exists() {
        return SourceUpdateResult::Error(format!(
            "repository path does not exist: {}",
            repo_path.display()
        ));
    }

    if !GitService::ref_exists(repo_path, target_branch)
        .await
        .unwrap_or(false)
    {
        tracing::warn!(
            task_id = task_id_str,
            target_branch = %target_branch,
            "Target branch missing before source update"
        );
        return SourceUpdateResult::BranchMissing {
            branch: target_branch.to_string(),
        };
    }

    // Check if target's HEAD is already an ancestor of source
    // (i.e., source already has all of target's changes)
    let target_sha = match GitService::get_branch_sha(repo_path, target_branch).await {
        Ok(sha) => sha,
        Err(e) => {
            tracing::warn!(
                task_id = task_id_str,
                error = %e,
                target_branch = %target_branch,
                "Failed to get SHA for target branch — skipping source update"
            );
            return SourceUpdateResult::Error(format!(
                "Failed to get SHA for {}: {}",
                target_branch, e
            ));
        }
    };

    match GitService::is_commit_on_branch(repo_path, &target_sha, source_branch).await {
        Ok(true) => {
            tracing::debug!(
                task_id = task_id_str,
                source_branch = %source_branch,
                target_branch = %target_branch,
                "Source branch is up-to-date with target — no update needed"
            );
            return SourceUpdateResult::AlreadyUpToDate;
        }
        Ok(false) => {
            tracing::info!(
                task_id = task_id_str,
                source_branch = %source_branch,
                target_branch = %target_branch,
                target_sha = %target_sha,
                "Source branch is behind target — updating before merge"
            );
        }
        Err(e) => {
            if !GitService::ref_exists(repo_path, source_branch)
                .await
                .unwrap_or(false)
            {
                tracing::warn!(
                    task_id = task_id_str,
                    source_branch = %source_branch,
                    "Source branch missing before source update"
                );
                return SourceUpdateResult::BranchMissing {
                    branch: source_branch.to_string(),
                };
            }
            tracing::warn!(
                task_id = task_id_str,
                error = %e,
                "Failed to check if source branch is up-to-date — skipping update"
            );
            return SourceUpdateResult::Error(format!("is_commit_on_branch check failed: {}", e));
        }
    }

    super::emit_merge_progress(
        event_sink,
        task_id_str,
        MergePhase::programmatic_merge(),
        MergePhaseStatus::Started,
        format!(
            "Updating {} from {} before merge",
            source_branch, target_branch
        ),
    );

    // Fallback: if the source branch is already checked out in an existing worktree
    // (e.g., from a prior task execution), merge target directly there instead of
    // trying to create a new worktree (which would fail with "already used by worktree").
    if let Ok(worktrees) = GitService::list_worktrees(repo_path).await {
        if let Some(wt) = worktrees
            .iter()
            .find(|w| w.branch.as_deref() == Some(source_branch))
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
                    source_branch = %source_branch,
                    worktree_path = %wt_path.display(),
                    "Source branch already checked out in existing worktree — merging target there"
                );
                match GitService::merge_branch(&wt_path, target_branch, source_branch).await {
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
                                "Conflicts detected updating source branch from target (existing worktree)"
                            );
                                return SourceUpdateResult::Conflicts {
                                    conflict_files: files.iter().map(PathBuf::from).collect(),
                                };
                            }
                        };
                        tracing::info!(
                            task_id = task_id_str,
                            commit_sha = %sha,
                            "Source branch updated from target (existing worktree)"
                        );
                        return SourceUpdateResult::Updated;
                    }
                    Err(e) => {
                        // Check if the error is because the worktree already has a merge in
                        // progress from a prior attempt.
                        if GitService::is_merge_in_progress(&wt_path) {
                            let conflict_files = GitService::get_conflict_files(&wt_path)
                                .await
                                .unwrap_or_default();
                            tracing::warn!(
                                task_id = task_id_str,
                                conflict_count = conflict_files.len(),
                                worktree_path = %wt_path.display(),
                                "Source branch already in merge conflict state in existing worktree \
                                 (prior attempt) — routing to merger agent without aborting"
                            );
                            return SourceUpdateResult::Conflicts { conflict_files };
                        }
                        // Not a conflict — abort any partial state and return as error.
                        let _ = GitService::abort_merge(&wt_path).await;
                        return SourceUpdateResult::Error(format!(
                            "merge in existing worktree failed: {}",
                            e
                        ));
                    }
                }
            } // end else (wt_path.exists())
        }
    }

    // Use isolated worktree to merge target into source
    let wt_path_str =
        super::merge_helpers::compute_source_update_worktree_path(project, task_id_str);
    let wt_path = PathBuf::from(&wt_path_str);

    // Clean up any stale worktree from a prior attempt
    super::merge_helpers::pre_delete_worktree(repo_path, &wt_path, task_id_str).await;

    // try_merge_in_worktree merges target_branch into source_branch
    let result = match GitService::try_merge_in_worktree(
        repo_path,
        target_branch, // source of changes = target branch
        source_branch, // branch to update = source/task branch
        &wt_path,
    )
    .await
    {
        Ok(crate::application::MergeAttemptResult::Success { commit_sha }) => {
            tracing::info!(
                task_id = task_id_str,
                source_branch = %source_branch,
                target_branch = %target_branch,
                commit_sha = %commit_sha,
                "Source branch updated from target via worktree"
            );
            SourceUpdateResult::Updated
        }
        Ok(crate::application::MergeAttemptResult::NeedsAgent { conflict_files }) => {
            tracing::warn!(
                task_id = task_id_str,
                conflict_count = conflict_files.len(),
                "Conflicts detected updating source branch from target via worktree"
            );
            SourceUpdateResult::Conflicts { conflict_files }
        }
        Ok(crate::application::MergeAttemptResult::BranchNotFound { branch }) => {
            SourceUpdateResult::BranchMissing { branch }
        }
        Err(e) => SourceUpdateResult::Error(format!("Source update merge failed: {}", e)),
    };

    // Clean up worktree (always, regardless of outcome)
    super::merge_helpers::pre_delete_worktree(repo_path, &wt_path, task_id_str).await;

    result
}
