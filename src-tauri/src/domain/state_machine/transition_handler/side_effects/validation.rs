use super::*;
use crate::domain::state_machine::transition_handler::{
    merge_helpers, BranchPair, ProjectCtx, TaskCore,
};
use crate::domain::state_machine::TransitionHandler;

impl<'a> TransitionHandler<'a> {
    /// Handle post-merge validation failure: revert the merge commit, then transition
    /// to MergeIncomplete with error metadata.
    ///
    /// `repo_path` and `project` are needed in AutoFix mode to create a dedicated
    /// merge worktree when the merge was checkout-free (merge_path == repo_path).
    #[allow(clippy::too_many_arguments)]
    pub(in crate::domain::state_machine::transition_handler) async fn handle_validation_failure(
        &self,
        tc: TaskCore<'_>,
        bp: BranchPair<'_>,
        pc: ProjectCtx<'_>,
        failures: &[ValidationFailure],
        log: &[ValidationLogEntry],
        merge_path: &Path,
        mode_label: &str,
        validation_mode: &MergeValidationMode,
    ) {
        let (task, task_id, task_id_str, task_repo) =
            (tc.task, tc.task_id, tc.task_id_str, tc.task_repo);
        let (source_branch, target_branch) = (bp.source_branch, bp.target_branch);
        let (project, repo_path) = (pc.project, pc.repo_path);
        if *validation_mode == MergeValidationMode::AutoFix {
            // AutoFix: DON'T revert — keep the merged (failing) code for the agent to fix
            tracing::info!(
                task_id = task_id_str,
                failure_count = failures.len(),
                "Validation failed (AutoFix mode, {}), spawning merger agent to attempt fix",
                mode_label,
            );

            let failure_details: Vec<serde_json::Value> = failures
                .iter()
                .map(|f| {
                    serde_json::json!({
                        "command": f.command,
                        "path": f.path,
                        "exit_code": f.exit_code,
                        "stderr": truncate_str(&f.stderr, 2000),
                    })
                })
                .collect();

            // If merge was checkout-free (merge_path == repo_path), create a dedicated
            // worktree so the fixer agent doesn't run in the user's main checkout.
            let fixer_worktree_path: PathBuf = if merge_path == repo_path {
                let wt_path = PathBuf::from(merge_helpers::compute_merge_worktree_path(
                    project,
                    task_id_str,
                ));

                // Pre-delete stale worktree if it exists from a previous attempt
                merge_helpers::pre_delete_worktree(repo_path, &wt_path, task_id_str).await;

                // Create worktree on the target branch (which has the merged+failing code)
                match GitService::checkout_existing_branch_worktree(
                    repo_path,
                    &wt_path,
                    target_branch,
                )
                .await
                {
                    Ok(()) => {
                        tracing::info!(
                            task_id = task_id_str,
                            worktree = %wt_path.display(),
                            "Created dedicated fixer worktree for validation recovery"
                        );
                        wt_path
                    }
                    Err(e) => {
                        tracing::error!(
                            task_id = task_id_str,
                            error = %e,
                            "Failed to create fixer worktree — reverting merge and transitioning to MergeIncomplete"
                        );
                        // Revert the merge since we can't safely spawn a fixer
                        if let Err(revert_err) = GitService::reset_hard(repo_path, "HEAD~1").await {
                            tracing::error!(
                                task_id = task_id_str,
                                error = %revert_err,
                                "Failed to revert merge after worktree creation failure"
                            );
                        }
                        let metadata = serde_json::json!({
                            "error": format!("Failed to create fixer worktree: {}", e),
                            "source_branch": source_branch,
                            "target_branch": target_branch,
                        });
                        self.transition_to_merge_incomplete(
                            TaskCore {
                                task: &mut *task,
                                task_id,
                                task_id_str,
                                task_repo,
                            },
                            metadata,
                            true,
                        )
                        .await;
                        return;
                    }
                }
            } else {
                // Worktree-based merge — use the existing merge worktree
                merge_path.to_path_buf()
            };

            // Merge new validation recovery metadata into existing metadata
            // to preserve merge_recovery history and validation_revert_count
            merge_helpers::merge_metadata_into(
                task,
                &serde_json::json!({
                    "validation_recovery": true,
                    "validation_failures": failure_details,
                    "validation_log": log,
                    "source_branch": source_branch,
                    "target_branch": target_branch,
                }),
            );
            task.worktree_path = Some(fixer_worktree_path.to_string_lossy().to_string());
            task.internal_status = InternalStatus::Merging;

            self.persist_merge_transition(
                TaskCore {
                    task: &mut *task,
                    task_id,
                    task_id_str,
                    task_repo,
                },
                InternalStatus::PendingMerge,
                InternalStatus::Merging,
                "validation_auto_fix",
            )
            .await;

            // Delegate to on_enter(Merging) which handles symlink cleanup, stale
            // rebase/merge abort, and spawns the merger agent with the correct prompt
            // (reads validation_recovery from metadata). This avoids a dual spawn path
            // where handle_validation_failure spawns one agent and reconciler spawns another.
            tracing::info!(
                task_id = task_id_str,
                "Delegating to on_enter(Merging) for validation recovery agent spawn"
            );
            if let Err(e) = Box::pin(self.on_enter_dispatch(&State::Merging)).await {
                tracing::error!(
                    task_id = task_id_str,
                    error = %e,
                    "on_enter(Merging) failed during validation recovery"
                );
            }
        } else {
            // Block mode: revert merge and transition to MergeIncomplete
            tracing::warn!(
                task_id = task_id_str,
                failure_count = failures.len(),
                "Post-merge validation failed ({}), reverting merge and transitioning to MergeIncomplete",
                mode_label,
            );

            // Capture the merge commit SHA before attempting revert (needed for
            // unrevertable flag if reset_hard fails).
            let merge_head_sha = GitService::get_branch_sha(merge_path, "HEAD").await.ok();

            let revert_failed = match GitService::reset_hard(merge_path, "HEAD~1").await {
                Ok(_) => {
                    tracing::info!(
                        task_id = task_id_str,
                        "Successfully reverted merge commit after validation failure"
                    );
                    false
                }
                Err(e) => {
                    tracing::error!(
                        task_id = task_id_str,
                        error = %e,
                        merge_sha = ?merge_head_sha,
                        "Failed to revert merge commit after validation failure — target branch has failing code"
                    );
                    true
                }
            };

            // Merge error metadata INTO existing metadata to preserve all existing keys
            // (merge_retry_in_progress, merge_recovery, etc.) instead of replacing.
            let prev_revert_count: u32 = merge_helpers::parse_metadata(task)
                .and_then(|v| v.get("validation_revert_count")?.as_u64())
                .unwrap_or(0) as u32;
            let revert_count = prev_revert_count + 1;

            let error_metadata_str =
                format_validation_error_metadata(failures, log, source_branch, target_branch);
            if let Ok(error_obj) = serde_json::from_str::<serde_json::Value>(&error_metadata_str) {
                merge_helpers::merge_metadata_into(task, &error_obj);
            }
            let mut extra = serde_json::json!({
                "merge_failure_source": serde_json::to_value(MergeFailureSource::ValidationFailed)
                    .unwrap_or(serde_json::json!("validation_failed")),
                "validation_revert_count": revert_count,
            });
            // Flag unrevertable merge commits so check_already_merged() doesn't
            // fast-path to completion with a failing merge commit on the target.
            if revert_failed {
                extra["merge_commit_unrevertable"] = serde_json::json!(true);
                if let Some(ref sha) = merge_head_sha {
                    extra["unrevertable_commit_sha"] = serde_json::json!(sha);
                }
            }
            merge_helpers::merge_metadata_into(task, &extra);
            task.internal_status = InternalStatus::MergeIncomplete;

            self.persist_merge_transition(
                TaskCore {
                    task: &mut *task,
                    task_id,
                    task_id_str,
                    task_repo,
                },
                InternalStatus::PendingMerge,
                InternalStatus::MergeIncomplete,
                "validation_failed",
            )
            .await;
        }
    }
}
