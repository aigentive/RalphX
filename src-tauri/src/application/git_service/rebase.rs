//! Rebase and fetch operations for the two-phase merge workflow
//!
//! Extracted from `merge.rs` — contains fetch, rebase, abort, continue,
//! and stale rebase completion operations.

use super::git_cmd;
use super::*;
use crate::infrastructure::git_auth::{git_auth_error_from_failure, GitNetworkOperation};
use std::sync::LazyLock;
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};

/// Global mutex serializing all `git fetch origin` calls.
///
/// Multiple concurrent merges each call `fetch_origin()`, which can contend on
/// `.git/FETCH_HEAD` and remote-tracking ref locks. This mutex ensures only one
/// fetch runs at a time across the entire process.
///
/// Timeout: 60 s to acquire the lock. If the lock-holder hangs beyond that,
/// the waiter skips the fetch rather than blocking indefinitely.
static FETCH_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// Maximum time to wait for `FETCH_LOCK` before giving up and skipping the fetch.
const FETCH_LOCK_TIMEOUT_SECS: u64 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FetchOriginOutcome {
    Fetched,
    RemoteRefMissing,
    FailedNonFatal,
    NoOriginRemote,
    SkippedBusy,
}

enum FetchLockMode {
    Wait,
    TryMaintenance,
}

impl GitService {
    // =========================================================================
    // Rebase Operations (Phase 1 - fast path)
    // =========================================================================

    /// Fetch from origin (if remote exists), serialized via a process-wide mutex.
    ///
    /// Only one fetch runs at a time to avoid contention on `.git/FETCH_HEAD`
    /// and remote-tracking ref locks when multiple merge operations are in flight.
    /// If the lock cannot be acquired within [`FETCH_LOCK_TIMEOUT_SECS`] seconds
    /// (e.g., a prior fetch is hanging), the fetch is skipped non-fatally.
    ///
    /// # Arguments
    /// * `repo` - Path to the git repository
    pub async fn fetch_origin(repo: &Path) -> AppResult<()> {
        Self::fetch_origin_with_lock(repo, &["fetch", "--prune", "origin"], FetchLockMode::Wait)
            .await?;
        Ok(())
    }

    /// Fetch a base branch from origin and prove the resulting remote-tracking ref exists.
    ///
    /// Restart flows need a stronger contract than [`Self::fetch_origin`]: a skipped or
    /// best-effort failed fetch must not be treated as a fresh base revision.
    pub async fn fetch_origin_branch_strict(repo: &Path, branch: &str) -> AppResult<String> {
        let branch = branch
            .trim()
            .strip_prefix("refs/remotes/origin/")
            .or_else(|| branch.trim().strip_prefix("origin/"))
            .unwrap_or(branch.trim());
        if !Self::check_ref_format(repo, branch).await? {
            return Err(AppError::Validation(format!(
                "Restart base branch '{}' is not a valid local branch name",
                branch
            )));
        }

        let remote_ref = format!("origin/{branch}");
        let outcome = Self::fetch_origin_with_lock(
            repo,
            &["fetch", "--prune", "origin", branch],
            FetchLockMode::Wait,
        )
        .await?;
        if outcome != FetchOriginOutcome::Fetched || !Self::ref_exists(repo, &remote_ref).await? {
            return Err(AppError::GitOperation(format!(
                "Restart requires the latest origin base branch '{}', but it could not be fetched and resolved",
                branch
            )));
        }

        Ok(remote_ref)
    }

    pub(crate) async fn try_fetch_origin_ref_for_maintenance(
        repo: &Path,
        ref_name: &str,
    ) -> AppResult<FetchOriginOutcome> {
        Self::fetch_origin_with_lock(
            repo,
            &["fetch", "origin", ref_name],
            FetchLockMode::TryMaintenance,
        )
        .await
    }

    pub(crate) fn pull_request_head_review_ref(pr_number: i64) -> Option<String> {
        (pr_number > 0).then(|| format!("refs/ralphx/pr-heads/{pr_number}"))
    }

    pub(crate) async fn fetch_pull_request_head_for_review(
        repo: &Path,
        pr_number: i64,
    ) -> AppResult<Option<String>> {
        let Some(dest_ref) = Self::pull_request_head_review_ref(pr_number) else {
            return Ok(None);
        };

        let had_existing_ref = Self::ref_exists(repo, &dest_ref).await?;
        let source_ref = format!("refs/pull/{pr_number}/head");
        let refspec = format!("+{source_ref}:{dest_ref}");
        let outcome =
            Self::fetch_origin_with_lock(repo, &["fetch", "origin", &refspec], FetchLockMode::Wait)
                .await?;

        if (matches!(outcome, FetchOriginOutcome::Fetched) || had_existing_ref)
            && Self::ref_exists(repo, &dest_ref).await?
        {
            return Ok(Some(dest_ref));
        }

        Ok(None)
    }

    async fn fetch_origin_with_lock(
        repo: &Path,
        args: &[&str],
        lock_mode: FetchLockMode,
    ) -> AppResult<FetchOriginOutcome> {
        debug!(args = ?args, "Fetching from origin in {:?}", repo);

        if !Self::has_origin_remote(repo).await {
            debug!("No origin remote configured, skipping fetch");
            return Ok(FetchOriginOutcome::NoOriginRemote);
        }

        let _guard = match lock_mode {
            FetchLockMode::Wait => {
                match timeout(
                    Duration::from_secs(FETCH_LOCK_TIMEOUT_SECS),
                    FETCH_LOCK.lock(),
                )
                .await
                {
                    Ok(guard) => guard,
                    Err(_elapsed) => {
                        warn!(
                            "fetch_origin: could not acquire FETCH_LOCK within {}s, skipping fetch for {:?}",
                            FETCH_LOCK_TIMEOUT_SECS, repo
                        );
                        return Ok(FetchOriginOutcome::SkippedBusy);
                    }
                }
            }
            FetchLockMode::TryMaintenance => match FETCH_LOCK.try_lock() {
                Ok(guard) => guard,
                Err(_busy) => {
                    debug!(
                        args = ?args,
                        "fetch_origin: FETCH_LOCK busy, skipping low-priority maintenance fetch for {:?}",
                        repo
                    );
                    return Ok(FetchOriginOutcome::SkippedBusy);
                }
            },
        };

        debug!(args = ?args, "fetch_origin: acquired FETCH_LOCK, fetching {:?}", repo);

        let output = git_cmd::run(args, repo).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if let Some(error) =
                git_auth_error_from_failure(GitNetworkOperation::Fetch, repo, &stderr).await
            {
                return Err(error);
            }
            warn!("Git fetch failed (non-fatal): {}", stderr);
            if stderr.contains("couldn't find remote ref")
                || stderr.contains("could not find remote ref")
                || stderr.contains("remote ref does not exist")
            {
                return Ok(FetchOriginOutcome::RemoteRefMissing);
            }
            return Ok(FetchOriginOutcome::FailedNonFatal);
        }

        Ok(FetchOriginOutcome::Fetched)
    }

    #[cfg(test)]
    pub(crate) async fn fetch_lock_guard_for_test() -> tokio::sync::MutexGuard<'static, ()> {
        FETCH_LOCK.lock().await
    }

    /// Check whether the repository has an `origin` remote configured.
    pub async fn has_origin_remote(repo: &Path) -> bool {
        git_cmd::run(&["remote", "get-url", "origin"], repo)
            .await
            .is_ok_and(|output| output.status.success())
    }

    /// Rebase current branch onto a base branch
    ///
    /// # Arguments
    /// * `path` - Path to the git repository or worktree
    /// * `base` - Name of the base branch to rebase onto
    pub async fn rebase_onto(path: &Path, base: &str) -> AppResult<RebaseResult> {
        debug!("Rebasing onto '{}' in {:?}", base, path);

        let output = git_cmd::run(&["rebase", base], path).await?;

        if output.status.success() {
            return Ok(RebaseResult::Success);
        }

        // Check if it's a conflict
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("CONFLICT") || stderr.contains("conflict") {
            let conflict_files = Self::get_conflict_files(path).await?;

            // If no unmerged files and no conflict markers, git auto-resolved the conflicts
            // Try to continue the rebase programmatically
            if conflict_files.is_empty() && !Self::has_conflict_markers(path).await.unwrap_or(true)
            {
                debug!("Git auto-resolved conflicts, attempting to continue rebase");
                return Self::try_continue_rebase(path).await;
            }

            return Ok(RebaseResult::Conflict {
                files: conflict_files,
            });
        }

        Err(AppError::GitOperation(format!("Rebase failed: {}", stderr)))
    }

    /// Abort an in-progress rebase
    ///
    /// # Arguments
    /// * `path` - Path to the git repository or worktree
    pub async fn abort_rebase(path: &Path) -> AppResult<()> {
        debug!("Aborting rebase in {:?}", path);

        let output = git_cmd::run(&["rebase", "--abort"], path).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Don't error if no rebase in progress
            if !stderr.contains("No rebase in progress") {
                return Err(AppError::GitOperation(format!(
                    "Failed to abort rebase: {}",
                    stderr
                )));
            }
        }

        Ok(())
    }

    /// Continue an in-progress rebase, looping until completion or real conflicts
    ///
    /// When git auto-resolves all conflicts (stderr contains "CONFLICT" but no unmerged files),
    /// this helper programmatically completes the rebase by running `git add --all` +
    /// `git rebase --continue` (with GIT_EDITOR=true). Handles multi-step rebases by looping
    /// up to 50 iterations.
    ///
    /// # Arguments
    /// * `path` - Path to the git repository or worktree
    pub async fn try_continue_rebase(path: &Path) -> AppResult<RebaseResult> {
        const MAX_ITERATIONS: u32 = 50;
        debug!(
            "Attempting to continue rebase in {:?} (max {} iterations)",
            path, MAX_ITERATIONS
        );

        for iteration in 0..MAX_ITERATIONS {
            debug!(
                "try_continue_rebase iteration {} of {}",
                iteration + 1,
                MAX_ITERATIONS
            );

            // Stage all changes
            let add_output = git_cmd::run(&["add", "--all"], path).await?;

            if !add_output.status.success() {
                let stderr = String::from_utf8_lossy(&add_output.stderr);
                return Err(AppError::GitOperation(format!(
                    "git add failed: {}",
                    stderr
                )));
            }

            // Continue the rebase with GIT_EDITOR=true to auto-accept all prompts
            let continue_output =
                git_cmd::run_with_env(&["rebase", "--continue"], path, &[("GIT_EDITOR", "true")])
                    .await?;

            // Check if rebase completed successfully
            if continue_output.status.success() {
                debug!(
                    "Rebase completed successfully at iteration {}",
                    iteration + 1
                );
                return Ok(RebaseResult::Success);
            }

            let stderr = String::from_utf8_lossy(&continue_output.stderr);
            let stdout = String::from_utf8_lossy(&continue_output.stdout);

            // Check if there are real conflicts (unmerged files or conflict markers)
            if stderr.contains("CONFLICT")
                || stderr.contains("conflict")
                || stdout.contains("CONFLICT")
            {
                let conflict_files = Self::get_conflict_files(path).await?;
                if !conflict_files.is_empty() {
                    debug!(
                        "Real conflict detected at iteration {} with {} files",
                        iteration + 1,
                        conflict_files.len()
                    );
                    return Ok(RebaseResult::Conflict {
                        files: conflict_files,
                    });
                }

                // Check for conflict markers
                if Self::has_conflict_markers(path).await.unwrap_or(false) {
                    debug!("Conflict markers found at iteration {}", iteration + 1);
                    let conflict_files = Self::get_conflict_files(path).await?;
                    return Ok(RebaseResult::Conflict {
                        files: conflict_files,
                    });
                }

                // No real conflicts detected, auto-resolved step - continue looping
                debug!(
                    "Auto-resolved step detected at iteration {}, continuing loop",
                    iteration + 1
                );
                continue;
            }

            // Check for "No rebase in progress" - rebase already done
            if stderr.contains("No rebase in progress") || stdout.contains("No rebase in progress")
            {
                debug!("No rebase in progress - rebase completed successfully");
                return Ok(RebaseResult::Success);
            }

            // Unexpected error
            let error_msg = format!(
                "Rebase --continue failed: stderr={} stdout={}",
                stderr, stdout
            );
            return Err(AppError::GitOperation(error_msg));
        }

        // Hit iteration limit without completing
        Err(AppError::GitOperation(format!(
            "Rebase did not complete within {} iterations",
            MAX_ITERATIONS
        )))
    }

    /// Attempt to complete a stale rebase by checking if conflicts are resolved
    ///
    /// Called during merge timeout recovery. If a rebase is in progress:
    /// - Checks if all conflicts are resolved (no unmerged files, no conflict markers)
    /// - If resolved: runs `git add --all` + `git rebase --continue` (with GIT_EDITOR=true)
    /// - Returns Completed if the rebase finishes, HasConflicts if real conflicts remain
    /// - Loops up to 50 iterations for multi-step rebases
    ///
    /// # Arguments
    /// * `worktree` - Path to the git worktree or repository
    pub async fn try_complete_stale_rebase(worktree: &Path) -> StaleRebaseResult {
        const MAX_ITERATIONS: u32 = 50;

        // Check if a rebase is actually in progress
        if !Self::is_rebase_in_progress(worktree) {
            return StaleRebaseResult::NoRebase;
        }

        debug!("Attempting to complete stale rebase in {:?}", worktree);

        for iteration in 0..MAX_ITERATIONS {
            // Check for unmerged files
            match Self::get_conflict_files(worktree).await {
                Ok(conflict_files) if !conflict_files.is_empty() => {
                    debug!(
                        "Real conflicts detected in stale rebase iteration {} with {} files",
                        iteration + 1,
                        conflict_files.len()
                    );
                    return StaleRebaseResult::HasConflicts {
                        files: conflict_files,
                    };
                }
                Err(e) => {
                    return StaleRebaseResult::Failed {
                        reason: format!("Failed to check conflict files: {}", e),
                    };
                }
                _ => {}
            }

            // Check for conflict markers
            match Self::has_conflict_markers(worktree).await {
                Ok(true) => {
                    debug!(
                        "Conflict markers found in stale rebase iteration {}",
                        iteration + 1
                    );
                    match Self::get_conflict_files(worktree).await {
                        Ok(files) => {
                            return StaleRebaseResult::HasConflicts { files };
                        }
                        Err(e) => {
                            return StaleRebaseResult::Failed {
                                reason: format!(
                                    "Failed to get conflict files after marker check: {}",
                                    e
                                ),
                            };
                        }
                    }
                }
                Ok(false) => {}
                Err(_) => {
                    // has_conflict_markers returned Err — treat conservatively
                }
            }

            // No real conflicts - attempt to continue
            debug!(
                "No real conflicts in iteration {}, attempting to continue rebase",
                iteration + 1
            );

            // Stage all changes
            let add_result = git_cmd::run(&["add", "--all"], worktree).await;

            match add_result {
                Ok(output) if output.status.success() => {}
                Ok(output) => {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    debug!("git add failed: {}", stderr);
                    return StaleRebaseResult::Failed {
                        reason: format!("git add failed: {}", stderr),
                    };
                }
                Err(e) => {
                    return StaleRebaseResult::Failed {
                        reason: format!("Failed to run git add: {}", e),
                    };
                }
            }

            // Continue the rebase
            let continue_result = git_cmd::run_with_env(
                &["rebase", "--continue"],
                worktree,
                &[("GIT_EDITOR", "true")],
            )
            .await;

            let (continue_status, stdout, stderr) = match continue_result {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    (output.status.success(), stdout, stderr)
                }
                Err(e) => {
                    return StaleRebaseResult::Failed {
                        reason: format!("Failed to run git rebase --continue: {}", e),
                    };
                }
            };

            // Check if rebase completed
            if continue_status {
                debug!(
                    "Stale rebase completed successfully at iteration {}",
                    iteration + 1
                );
                return StaleRebaseResult::Completed;
            }

            // Check if no rebase in progress (already done)
            if stderr.contains("No rebase in progress") || stdout.contains("No rebase in progress")
            {
                debug!("No rebase in progress - rebase already completed");
                return StaleRebaseResult::Completed;
            }

            // Check for conflict
            if stderr.contains("CONFLICT")
                || stderr.contains("conflict")
                || stdout.contains("CONFLICT")
            {
                // Re-check for real conflicts
                match Self::get_conflict_files(worktree).await {
                    Ok(conflict_files) if !conflict_files.is_empty() => {
                        debug!(
                            "Real conflicts found after rebase --continue in iteration {}",
                            iteration + 1
                        );
                        return StaleRebaseResult::HasConflicts {
                            files: conflict_files,
                        };
                    }
                    Ok(_) => {
                        // Auto-resolved step, continue looping
                        debug!(
                            "Auto-resolved step at iteration {}, continuing",
                            iteration + 1
                        );
                        continue;
                    }
                    Err(e) => {
                        return StaleRebaseResult::Failed {
                            reason: format!("Failed to check conflicts after --continue: {}", e),
                        };
                    }
                }
            } else {
                // Unexpected error from git rebase --continue
                return StaleRebaseResult::Failed {
                    reason: format!(
                        "Unexpected git rebase --continue failure: stderr={} stdout={}",
                        stderr, stdout
                    ),
                };
            }
        }

        // Hit iteration limit
        StaleRebaseResult::Failed {
            reason: format!(
                "Rebase did not complete within {} iterations",
                MAX_ITERATIONS
            ),
        }
    }
}
