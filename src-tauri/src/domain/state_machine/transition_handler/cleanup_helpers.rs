// Reusable helpers for pre-merge cleanup steps.
//
// - os_thread_timeout: OS-thread-based timeout immune to tokio timer driver starvation
// - run_cleanup_step: timeout-wrapped async operation with standardized logging
// - CleanupStepResult: typed outcome replacing bare bool

use std::path::Path;
use std::time::Duration;

use crate::infrastructure::tool_paths::{resolve_git_cli_path, resolve_rm_cli_path};

/// Returned when an [`os_thread_timeout`] expires.
#[derive(Debug)]
pub(crate) struct OsTimeoutElapsed;

/// Race `fut` against an OS-thread-based deadline.
///
/// Unlike `tokio::time::timeout`, this is immune to tokio timer driver
/// starvation. The deadline runs on a real OS thread via `std::thread::sleep`,
/// signalling back through a `oneshot` channel (which uses `Waker`, not the
/// timer driver).
///
/// When the future wins, the OS thread remains sleeping until its duration
/// expires (then exits cleanly). This is acceptable for our timeout durations.
pub(crate) async fn os_thread_timeout<F, T>(
    duration: Duration,
    fut: F,
) -> Result<T, OsTimeoutElapsed>
where
    F: std::future::Future<Output = T>,
{
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();

    std::thread::spawn(move || {
        std::thread::sleep(duration);
        let _ = tx.send(());
    });

    tokio::pin!(fut);

    tokio::select! {
        result = &mut fut => Ok(result),
        _ = rx => Err(OsTimeoutElapsed),
    }
}

/// Outcome of a single cleanup step — richer than a bare `bool`.
#[derive(Debug)]
pub(crate) enum CleanupStepResult {
    /// The operation completed successfully.
    Ok,
    /// The operation exceeded its deadline.
    TimedOut { elapsed: Duration },
    /// The operation returned an error.
    Error { message: String },
}

impl CleanupStepResult {
    /// Convenience predicate matching the old `bool` semantics.
    #[allow(dead_code)]
    pub fn is_ok(&self) -> bool {
        matches!(self, CleanupStepResult::Ok)
    }
}

/// Run a cleanup step with a timeout, logging success/error/timeout uniformly.
///
/// Uses [`os_thread_timeout`] internally — immune to tokio timer driver starvation.
///
/// Returns a [`CleanupStepResult`] describing the outcome.
/// All failures are non-fatal (logged as warn) to allow cleanup to continue.
pub(crate) async fn run_cleanup_step<F, E>(
    label: &str,
    timeout_secs: u64,
    task_id: &str,
    fut: F,
) -> CleanupStepResult
where
    F: std::future::Future<Output = Result<(), E>>,
    E: std::fmt::Display,
{
    let deadline = Duration::from_secs(timeout_secs);
    match os_thread_timeout(deadline, fut).await {
        Ok(Ok(())) => CleanupStepResult::Ok,
        Ok(Err(e)) => {
            let message = e.to_string();
            tracing::warn!(
                task_id = task_id,
                error = %message,
                step = label,
                "pre_merge_cleanup: {} failed (non-fatal, continuing)",
                label,
            );
            CleanupStepResult::Error { message }
        }
        Err(OsTimeoutElapsed) => {
            tracing::warn!(
                task_id = task_id,
                step = label,
                timeout_secs = timeout_secs,
                "pre_merge_cleanup: {} timed out after {}s (non-fatal, continuing)",
                label,
                timeout_secs,
            );
            CleanupStepResult::TimedOut { elapsed: deadline }
        }
    }
}

/// Fast worktree removal: `rm -rf` + `git worktree prune`.
///
/// Unlike `GitService::delete_worktree` which calls `git worktree remove --force`
/// (6-10s because it runs `git status` internally), this function removes the
/// directory directly and then prunes git's worktree tracking.
///
/// Safe for merge cleanup because worktree data is irrelevant after the merge
/// commit is on the target branch.
///
/// # Errors
///
/// Returns an error if directory removal fails AND the path still exists.
/// Non-existent paths are treated as success (idempotent).
/// `git worktree prune` failures are logged but not propagated (best-effort).
pub(crate) async fn remove_worktree_fast(
    worktree_path: &Path,
    repo_path: &Path,
) -> Result<(), String> {
    // Unlock first (ignore errors — worktree may not be locked, or path may be gone).
    // This allows `git worktree prune` to clean up stale locked metadata entries.
    let _ = tokio::process::Command::new(resolve_git_cli_path())
        .args(["worktree", "unlock", worktree_path.to_str().unwrap_or_default()])
        .current_dir(repo_path)
        .output()
        .await;

    // Try scoped git worktree remove with double-force (cleans git metadata atomically).
    // -f -f overrides locks (git 2.17+); single --force only bypasses dirty-tree checks.
    // Ignore errors — path may already be gone.
    let _ = tokio::process::Command::new(resolve_git_cli_path())
        .args(["worktree", "remove", "-f", "-f", worktree_path.to_str().unwrap_or_default()])
        .current_dir(repo_path)
        .output()
        .await;

    // Also remove the directory (in case git worktree remove didn't delete it)
    remove_worktree_dir(worktree_path).await?;

    // Final prune to clean any remaining stale entries
    git_worktree_prune(repo_path).await;
    Ok(())
}

/// Remove worktree directory only (rm -rf), WITHOUT running `git worktree prune`.
///
/// Use this in parallel loops where multiple worktrees are deleted concurrently.
/// After all deletions complete, call `git_worktree_prune` once to avoid
/// concurrent git lock contention.
pub(crate) async fn remove_worktree_dir(worktree_path: &Path) -> Result<(), String> {
    if !worktree_path.exists() {
        tracing::debug!(
            worktree = %worktree_path.display(),
            "remove_worktree_dir: path does not exist, skipping"
        );
        return Ok(());
    }

    // rm -rf via tokio async fs
    if let Err(e) = tokio::fs::remove_dir_all(worktree_path).await {
        tracing::warn!(
            worktree = %worktree_path.display(),
            error = %e,
            "remove_worktree_dir: tokio::fs::remove_dir_all failed, trying rm -rf fallback"
        );
        // Fallback: shell rm -rf for permission issues (e.g., read-only .git dirs)
        let output = tokio::process::Command::new(resolve_rm_cli_path())
            .args(["-rf", worktree_path.to_str().unwrap_or_default()])
            .output()
            .await
            .map_err(|e| format!("Failed to spawn rm -rf: {}", e))?;

        if !output.status.success() && worktree_path.exists() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "rm -rf failed for '{}': {}",
                worktree_path.display(),
                stderr
            ));
        }
    }

    Ok(())
}

/// Run `git worktree prune` to clean up git's worktree tracking.
///
/// Call this once after removing worktree directories (not per-worktree)
/// to avoid concurrent git index.lock contention.
pub(crate) async fn git_worktree_prune(repo_path: &Path) {
    match tokio::process::Command::new(resolve_git_cli_path())
        .args(["worktree", "prune"])
        .current_dir(repo_path)
        .output()
        .await
    {
        Ok(output) if !output.status.success() => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!(
                repo = %repo_path.display(),
                error = %stderr,
                "git_worktree_prune: failed (non-fatal)"
            );
        }
        Err(e) => {
            tracing::warn!(
                repo = %repo_path.display(),
                error = %e,
                "git_worktree_prune: failed to spawn (non-fatal)"
            );
        }
        _ => {}
    }
}
