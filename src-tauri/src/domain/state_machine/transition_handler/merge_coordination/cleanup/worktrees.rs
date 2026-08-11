use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::application::GitService;
use crate::domain::entities::{
    merge_progress_event::{MergePhase, MergePhaseStatus},
    InternalStatus,
};
use crate::domain::repositories::TaskRepository;
use crate::domain::state_machine::transition_handler::cleanup_helpers::{
    run_cleanup_step, CleanupStepResult,
};
use crate::infrastructure::agents::claude::git_runtime_config;

use super::super::{cleanup_helpers, emit_merge_progress, merge_helpers};
use super::clear_stale_worktree_path_on_timeout;

pub(super) async fn cleanup_stale_worktrees(
    task_id_str: &str,
    task: &crate::domain::entities::Task,
    project: &crate::domain::entities::Project,
    repo_path: &Path,
    task_repo: &Arc<dyn TaskRepository>,
    event_sink: Option<&dyn ralphx_events::EventSink>,
    outer_deadline: std::time::Instant,
) {
    // --- Step 1: Remove stale index.lock ---
    let index_lock_stale_secs = git_runtime_config().index_lock_stale_secs;
    match GitService::remove_stale_index_lock(repo_path, index_lock_stale_secs) {
        Ok(true) => {
            tracing::info!(
                task_id = task_id_str,
                "Removed stale index.lock before merge attempt"
            );
        }
        Ok(false) => {}
        Err(e) => {
            tracing::warn!(
                task_id = task_id_str,
                error = %e,
                "Failed to remove stale index.lock (non-fatal)"
            );
        }
    }

    // --- Step 2: Delete task worktree ---
    {
        emit_merge_progress(
            event_sink,
            task_id_str,
            MergePhase::new(MergePhase::MERGE_CLEANUP),
            MergePhaseStatus::Started,
            "Removing stale worktrees...".to_string(),
        );
        if let Some(ref worktree_path) = task.worktree_path {
            let worktree_path_buf = PathBuf::from(worktree_path);
            if worktree_path_buf == repo_path {
                tracing::warn!(
                    task_id = task_id_str,
                    worktree_path = %worktree_path,
                    "Skipping task worktree deletion — path is the main working tree"
                );
            } else if worktree_path_buf.exists() {
                // Step 2 TOCTOU guard: re-read status from DB before deleting.
                // A concurrent handle_outcome_needs_agent may have set Merging
                // and written this worktree_path as the merge agent's working dir.
                let task_repo_step2 = Arc::clone(task_repo);
                let task_id_for_step2 = task.id.clone();
                let should_skip_step2 = match task_repo_step2.get_by_id(&task_id_for_step2).await {
                    Ok(Some(ref fresh_task))
                        if matches!(fresh_task.internal_status, InternalStatus::Merging) =>
                    {
                        true
                    }
                    // Error or None: proceed with deletion (safe default)
                    _ => false,
                };

                if should_skip_step2 {
                    tracing::info!(
                        task_id = task_id_str,
                        worktree_path = %worktree_path,
                        "Skipping task worktree deletion — task is actively merging"
                    );
                } else {
                    merge_helpers::clean_stale_git_state(&worktree_path_buf, task_id_str).await;
                    let deletion_start = std::time::Instant::now();
                    match run_cleanup_step(
                        "step 2 task worktree deletion (fast)",
                        git_runtime_config().cleanup_worktree_timeout_secs,
                        task_id_str,
                        cleanup_helpers::remove_worktree_fast(&worktree_path_buf, repo_path),
                    )
                    .await
                    {
                        CleanupStepResult::Ok => {
                            tracing::info!(
                                task_id = task_id_str,
                                elapsed_ms = deletion_start.elapsed().as_millis() as u64,
                                "Task worktree deletion succeeded"
                            );
                        }
                        CleanupStepResult::TimedOut { elapsed } => {
                            tracing::warn!(
                                task_id = task_id_str,
                                elapsed_ms = elapsed.as_millis() as u64,
                                "Task worktree deletion timed out — attempting bounded retry"
                            );
                            // Bounded single retry: reuse the same timeout if sufficient outer
                            // deadline remains. Formula: min(original_timeout, remaining - 2s).
                            // Skip retry entirely if remaining < original_timeout + 2s.
                            let original_timeout_secs =
                                git_runtime_config().cleanup_worktree_timeout_secs;
                            let original_timeout =
                                std::time::Duration::from_secs(original_timeout_secs);
                            let remaining =
                                outer_deadline.saturating_duration_since(std::time::Instant::now());
                            let retry_succeeded = if remaining
                                > original_timeout + std::time::Duration::from_secs(2)
                            {
                                let bounded = remaining - std::time::Duration::from_secs(2);
                                let retry_timeout = original_timeout.min(bounded);
                                tracing::info!(
                                    task_id = task_id_str,
                                    retry_timeout_secs = retry_timeout.as_secs(),
                                    "Task worktree deletion retry with bounded timeout"
                                );
                                match run_cleanup_step(
                                    "step 2 task worktree deletion retry",
                                    retry_timeout.as_secs(),
                                    task_id_str,
                                    cleanup_helpers::remove_worktree_fast(
                                        &worktree_path_buf,
                                        repo_path,
                                    ),
                                )
                                .await
                                {
                                    CleanupStepResult::Ok => {
                                        tracing::info!(
                                            task_id = task_id_str,
                                            "Task worktree deletion retry succeeded"
                                        );
                                        true
                                    }
                                    CleanupStepResult::TimedOut {
                                        elapsed: retry_elapsed,
                                    } => {
                                        tracing::warn!(
                                            task_id = task_id_str,
                                            elapsed_ms = retry_elapsed.as_millis() as u64,
                                            "Task worktree deletion retry also timed out"
                                        );
                                        false
                                    }
                                    CleanupStepResult::Error { ref message } => {
                                        tracing::warn!(
                                            task_id = task_id_str,
                                            error = %message,
                                            "Task worktree deletion retry also failed"
                                        );
                                        false
                                    }
                                }
                            } else {
                                tracing::warn!(
                                        task_id = task_id_str,
                                        remaining_ms = remaining.as_millis() as u64,
                                        "Task worktree deletion timed out — skipping retry (insufficient time remaining)"
                                    );
                                false
                            };
                            if !retry_succeeded {
                                // Stale path cleanup: clear worktree_path from DB since deletion
                                // failed. Race guard inside prevents clearing when task is actively
                                // Merging.
                                clear_stale_worktree_path_on_timeout(
                                    &task_id_for_step2,
                                    task_id_str,
                                    task_repo,
                                )
                                .await;
                            }
                        }
                        CleanupStepResult::Error { ref message } => {
                            tracing::warn!(
                                task_id = task_id_str,
                                error = %message,
                                "Task worktree deletion failed — branch may still be locked"
                            );
                        }
                    }
                }
            }
        }

        // --- Step 3: Prune stale worktree refs ---
        run_cleanup_step(
            "prune_worktrees",
            git_runtime_config().cleanup_git_op_timeout_secs,
            task_id_str,
            GitService::prune_worktrees(repo_path),
        )
        .await;

        // --- Step 4: Delete own stale merge/rebase worktrees (PARALLEL) ---
        let step_start = std::time::Instant::now();
        tracing::info!(
            task_id = task_id_str,
            "pre_merge_cleanup: step 4 starting — parallel deletion of stale worktrees"
        );
        let worktree_specs: Vec<(&str, String)> = vec![
            (
                "task",
                merge_helpers::compute_task_worktree_path(project, task_id_str),
            ),
            (
                "merge",
                merge_helpers::compute_merge_worktree_path(project, task_id_str),
            ),
            (
                "rebase",
                merge_helpers::compute_rebase_worktree_path(project, task_id_str),
            ),
            (
                "plan-update",
                merge_helpers::compute_plan_update_worktree_path(project, task_id_str),
            ),
            (
                "source-update",
                merge_helpers::compute_source_update_worktree_path(project, task_id_str),
            ),
        ];

        // Filter to only existing worktrees, then delete in parallel
        let existing_worktrees: Vec<(&str, PathBuf)> = worktree_specs
            .iter()
            .filter_map(|(label, path_str)| {
                let path = PathBuf::from(path_str);
                if path.exists() {
                    tracing::info!(
                        task_id = task_id_str,
                        worktree_path = %path_str,
                        wt_type = *label,
                        "Cleaning up stale {} worktree from previous attempt",
                        label
                    );
                    Some((*label, path))
                } else {
                    None
                }
            })
            .collect();

        if !existing_worktrees.is_empty() {
            let cleanup_timeout = git_runtime_config().cleanup_git_op_timeout_secs;
            // Pre-allocate step labels so the borrow checker is happy
            let step_labels: Vec<String> = existing_worktrees
                .iter()
                .map(|(label, _)| format!("step 4 {} worktree deletion (fast)", label))
                .collect();
            // Use remove_worktree_fast (unlock + double-force + rm-rf + prune) in parallel.
            // remove_worktree_fast handles locked worktrees via unlock + -f -f before removal.
            // Step 4 TOCTOU guard: for "merge" worktrees, check DB status INSIDE the
            // async future body (not in filter_map) to close the race window where
            // handle_outcome_needs_agent sets Merging after filter_map but before join_all.
            let task_id_for_step4 = task.id.clone();
            let task_id_str_owned = task_id_str.to_string();
            let repo_path_owned = repo_path.to_path_buf();
            let futs: Vec<_> = existing_worktrees
            .iter()
            .zip(step_labels.iter())
            .map(|((label, wt_path), step_label)| {
                let label_owned = label.to_string();
                let wt_path_owned = wt_path.clone();
                let step_label_owned = step_label.clone();
                let task_id_guard = task_id_for_step4.clone();
                let task_repo_step4 = Arc::clone(task_repo);
                let task_id_log = task_id_str_owned.clone();
                let repo_path_step4 = repo_path_owned.clone();
                async move {
                    // Guard applies only to "merge" label — these are the worktrees
                    // used by merge agents. Other labels (task/rebase/plan-update/
                    // source-update) are never needed by an active merge agent.
                    if label_owned == "merge" {
                        match task_repo_step4.get_by_id(&task_id_guard).await {
                            Ok(Some(ref fresh_task))
                                if matches!(
                                    fresh_task.internal_status,
                                    InternalStatus::Merging
                                ) =>
                            {
                                tracing::info!(
                                    task_id = %task_id_log,
                                    worktree_path = %wt_path_owned.display(),
                                    "Skipping merge worktree deletion — task is actively merging"
                                );
                                return CleanupStepResult::Ok;
                            }
                            // Error or None: proceed with deletion (safe default)
                            _ => {}
                        }
                    }
                    run_cleanup_step(
                        &step_label_owned,
                        cleanup_timeout,
                        &task_id_log,
                        cleanup_helpers::remove_worktree_fast(
                            &wt_path_owned,
                            &repo_path_step4,
                        ),
                    )
                    .await
                }
            })
            .collect();

            let results = futures::future::join_all(futs).await;
            for (i, result) in results.iter().enumerate() {
                let (label, wt_path): &(&str, PathBuf) = &existing_worktrees[i];
                match result {
                    CleanupStepResult::Ok => {}
                    CleanupStepResult::TimedOut { elapsed } => {
                        tracing::warn!(
                            task_id = task_id_str,
                            worktree_path = %wt_path.display(),
                            wt_type = *label,
                            elapsed_ms = elapsed.as_millis() as u64,
                            "Stale {} worktree deletion timed out",
                            label
                        );
                    }
                    CleanupStepResult::Error { ref message } => {
                        tracing::warn!(
                            task_id = task_id_str,
                            worktree_path = %wt_path.display(),
                            wt_type = *label,
                            error = %message,
                            "Stale {} worktree deletion failed",
                            label
                        );
                    }
                }
            }
        }
        tracing::info!(
            task_id = task_id_str,
            elapsed_ms = step_start.elapsed().as_millis() as u64,
            deleted_count = existing_worktrees.len(),
            "pre_merge_cleanup: step 4 complete (parallel worktree deletion)"
        );

        // Step 5 DEFERRED: orphaned merge worktree scan moved to Phase 3 (fire-and-forget).
        // The scan is not critical for merge success — it's a hygiene operation that
        // lists all worktrees and checks each against the task repo, which is slow.
        // TODO(Phase 3): Move to deferred cleanup after merge completion.
    }
}
