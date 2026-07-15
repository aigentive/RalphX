use std::path::Path;
use std::sync::Arc;

use crate::application::GitService;
use crate::domain::entities::{InternalStatus, TaskId};
use crate::domain::repositories::TaskRepository;
use crate::infrastructure::agents::claude::git_runtime_config;

mod agents;
mod guard;
mod worktrees;

use agents::cancel_validation_and_stop_agents;
use guard::maybe_skip_first_attempt_cleanup;
use worktrees::cleanup_stale_worktrees;

impl<'a> super::TransitionHandler<'a> {
    /// Phase 1 GUARD: fast pre-merge cleanup with first-attempt skip optimization.
    ///
    /// On first clean attempt (no prior failure metadata, no running agents),
    /// skips cleanup entirely — returns in microseconds.
    ///
    /// On retry attempts or when agents are running, executes targeted cleanup:
    ///   0a. Cancel in-flight validation tokens (instant)
    ///   0b. Stop running agents — uses SIGKILL immediate (no SIGTERM grace period)
    ///   1.  Remove stale `.git/index.lock`
    ///   2.  Delete the task worktree to unlock the task branch
    ///   3.  Prune stale worktree references
    ///   4.  Delete own merge/rebase/plan-update/source-update worktrees (PARALLEL)
    ///
    /// Step 5 (orphaned worktree scan) has been moved to Phase 3 deferred cleanup —
    /// it's not critical for merge success and is the slowest step.
    pub(crate) async fn pre_merge_cleanup(
        &self,
        task_id_str: &str,
        task: &crate::domain::entities::Task,
        project: &crate::domain::entities::Project,
        repo_path: &Path,
        target_branch: &str,
        task_repo: &Arc<dyn TaskRepository>,
    ) {
        let cleanup_start = std::time::Instant::now();
        let event_sink = self.machine.context.services.event_sink.as_deref();
        let is_first = super::is_first_clean_attempt(task);

        if maybe_skip_first_attempt_cleanup(self, task_id_str, task, cleanup_start).await {
            return;
        }

        cancel_validation_and_stop_agents(self, task_id_str, task, event_sink).await;

        // Best-effort: clean orphaned task worktrees before the main stale-worktree cleanup.
        // Placed between cancel_validation_and_stop_agents and cleanup_stale_worktrees so agents
        // are already stopped before we touch the filesystem. Uses best-effort semantics —
        // failure here must never abort the merge pipeline.
        let task_wt_path = task.worktree_path.as_deref().map(std::path::Path::new);
        if let Err(e) = GitService::cleanup_stale_worktree_artifacts(
            repo_path,
            task_wt_path,
            project,
            task_id_str,
        )
        .await
        {
            tracing::warn!(
                task_id = task_id_str,
                error = %e,
                "cleanup_stale_worktree_artifacts failed (non-fatal, continuing merge pipeline)"
            );
        }

        // Compute outer deadline for Step 2 retry budget:
        // original_timeout + original_timeout + 10s gives enough room for one retry.
        let worktree_timeout_secs = git_runtime_config().cleanup_worktree_timeout_secs;
        let outer_deadline = std::time::Instant::now()
            + std::time::Duration::from_secs(worktree_timeout_secs * 2 + 10);
        cleanup_stale_worktrees(
            task_id_str,
            task,
            project,
            repo_path,
            task_repo,
            event_sink,
            outer_deadline,
        )
        .await;

        tracing::info!(
            task_id = task_id_str,
            total_elapsed_ms = cleanup_start.elapsed().as_millis() as u64,
            is_first_attempt = is_first,
            target_branch = target_branch,
            "pre_merge_cleanup: complete"
        );
    }
}

/// Clear `worktree_path` from the DB after a Step 2 deletion timeout.
///
/// Race guard: only clears if the task's current status is NOT [`InternalStatus::Merging`].
/// When the task is actively merging, the worktree is still needed by the merge agent
/// and must not be cleared.
pub(crate) async fn clear_stale_worktree_path_on_timeout(
    task_id: &TaskId,
    task_id_str: &str,
    task_repo: &Arc<dyn TaskRepository>,
) {
    match task_repo.get_by_id(task_id).await {
        Ok(Some(mut fresh_task))
            if !matches!(fresh_task.internal_status, InternalStatus::Merging) =>
        {
            fresh_task.worktree_path = None;
            if let Err(e) = task_repo.update(&fresh_task).await {
                tracing::warn!(
                    task_id = task_id_str,
                    error = %e,
                    "Failed to clear stale worktree_path from DB after timeout (non-fatal)"
                );
            } else {
                tracing::info!(
                    task_id = task_id_str,
                    "Cleared stale worktree_path from DB after deletion timeout"
                );
            }
        }
        Ok(Some(_)) => {
            tracing::info!(
                task_id = task_id_str,
                "Skipping worktree_path clear — task is actively merging"
            );
        }
        // DB error or task not found: skip silently (non-fatal)
        _ => {}
    }
}
