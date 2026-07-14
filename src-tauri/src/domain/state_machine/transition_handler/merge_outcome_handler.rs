// Shared merge outcome handler: uniform post-merge logic for all strategy arms
//
// Processes MergeOutcome variants (from merge_strategies.rs) and performs the
// appropriate side effects: success → validate → complete, conflict → spawn agent,
// branch-not-found → MergeIncomplete, error → defer or MergeIncomplete.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::application::git_service::git_cmd;
use crate::application::GitService;
use crate::domain::entities::{
    merge_progress_event::{MergePhase, MergePhaseStatus},
    task_metadata::{
        MergeFailureSource, MergeRecoveryEvent, MergeRecoveryEventKind, MergeRecoveryMetadata,
        MergeRecoveryReasonCode, MergeRecoverySource, MergeRecoveryState,
    },
    InternalStatus, MergeValidationMode, Project, Task, TaskId,
};
use crate::domain::repositories::{PlanBranchRepository, TaskRepository};
use crate::domain::state_machine::services::{NotificationContext, Notifier, TaskNotification};

use super::merge_completion::complete_merge_internal_with_pr_sync_and_notifier;
use super::merge_helpers::{
    compute_merge_worktree_path, is_pr_branch_publication_conflict_routed_error, parse_metadata,
    task_has_pr_branch_publication_conflict,
};
use super::merge_strategies::MergeOutcome;
use super::merge_validation::{
    emit_merge_progress, extract_cached_validation, format_validation_warn_metadata,
    run_validation_commands, take_skip_validation_flag, ValidationFailure,
};
use crate::infrastructure::agents::claude::reconciliation_config;

/// Bundles the parameters needed by handle_merge_outcome and its sub-handlers.
///
/// Groups task identity, project/repo context, branch info, repositories,
/// and strategy options into a single struct to replace the 11-parameter signature.
pub(super) struct MergeContext<'m> {
    pub task: &'m mut Task,
    pub task_id: &'m TaskId,
    pub task_id_str: &'m str,
    pub project: &'m Project,
    pub repo_path: &'m Path,
    pub source_branch: &'m str,
    pub target_branch: &'m str,
    pub task_repo: &'m Arc<dyn TaskRepository>,
    pub plan_branch_repo: &'m Option<Arc<dyn PlanBranchRepository>>,
    pub opts: &'m MergeHandlerOptions,
}

/// Per-arm options that vary between merge strategies.
pub(super) struct MergeHandlerOptions {
    pub strategy_label: &'static str,
    pub conflict_reason: &'static str,
    pub conflict_type: Option<&'static str>,
    pub agent_prompt_suffix: &'static str,
}

impl MergeHandlerOptions {
    pub fn merge() -> Self {
        Self {
            strategy_label: "merge",
            conflict_reason: "merge_conflict",
            conflict_type: None,
            agent_prompt_suffix: "",
        }
    }
    pub fn rebase() -> Self {
        Self {
            strategy_label: "rebase",
            conflict_reason: "rebase_conflict",
            conflict_type: Some("rebase"),
            agent_prompt_suffix:
                ". After resolving each file, run `git add <file>` then `git rebase --continue`",
        }
    }
    pub fn squash() -> Self {
        Self {
            strategy_label: "squash",
            conflict_reason: "merge_conflict",
            conflict_type: None,
            agent_prompt_suffix: "",
        }
    }
    pub fn rebase_squash() -> Self {
        Self {
            strategy_label: "rebase+squash",
            conflict_reason: "rebase_conflict",
            conflict_type: Some("rebase"),
            agent_prompt_suffix:
                ". After resolving each file, run `git add <file>` then `git rebase --continue`",
        }
    }
}

// ===== Shared helpers for repeated patterns =====

/// Load or create MergeRecoveryMetadata from task metadata.
fn get_or_create_recovery(task: &Task) -> MergeRecoveryMetadata {
    MergeRecoveryMetadata::from_task_metadata(task.metadata.as_deref())
        .unwrap_or(None)
        .unwrap_or_else(MergeRecoveryMetadata::new)
}

/// Count existing AutoRetryTriggered events in recovery metadata.
fn retry_attempt_count(recovery: &MergeRecoveryMetadata) -> u32 {
    recovery
        .events
        .iter()
        .filter(|e| matches!(e.kind, MergeRecoveryEventKind::AutoRetryTriggered))
        .count() as u32
        + 1
}

/// Transition task to MergeIncomplete, persist status change, and emit event.
async fn transition_to_merge_incomplete(
    task: &mut Task,
    task_id: &TaskId,
    task_id_str: &str,
    task_repo: &Arc<dyn TaskRepository>,
    event_emitter: &Arc<dyn super::super::services::EventEmitter>,
    notifier: &Arc<dyn Notifier>,
) {
    // `complete_merge_internal_with_pr_sync_and_notifier` can already have committed
    // this terminal state after a PR-publication failure. Preserve its history-id-scoped
    // notification instead of writing a duplicate MergeIncomplete entry/effect.
    if task.internal_status == InternalStatus::MergeIncomplete {
        event_emitter
            .emit_status_change(task_id_str, "pending_merge", "merge_incomplete")
            .await;
        return;
    }

    task.internal_status = InternalStatus::MergeIncomplete;
    task.touch();

    if let Err(e) = task_repo.update(task).await {
        tracing::error!(error = %e, "Failed to update task to MergeIncomplete status");
        return;
    }
    let history_entry_id = match task_repo
        .persist_status_change(
            task_id,
            InternalStatus::PendingMerge,
            InternalStatus::MergeIncomplete,
            "merge_incomplete",
        )
        .await
    {
        Ok(history_entry_id) => history_entry_id,
        Err(error) => {
            tracing::warn!(error = %error, "Failed to record merge incomplete transition; skipping notification effect");
            return;
        }
    };
    notifier
        .notify(
            NotificationContext {
                task: task.clone(),
                history_entry_id,
                project_id: task.project_id.clone(),
            },
            TaskNotification::StateEntered(InternalStatus::MergeIncomplete),
        )
        .await;
    event_emitter
        .emit_status_change(task_id_str, "pending_merge", "merge_incomplete")
        .await;
}

/// Classify whether a merge error is transient (worth retrying immediately)
/// vs permanent (should go to MergeIncomplete for reconciliation).
///
/// Transient: lock contention, index.lock, concurrent ref updates
/// Permanent: not a git repo, merge conflicts, unrelated histories
pub(super) fn is_transient_merge_error(error: &crate::error::AppError) -> bool {
    matches!(
        git_cmd::classify_git_failure_source(error),
        MergeFailureSource::TransientGit | MergeFailureSource::LockContention
    )
}

pub(in crate::domain::state_machine::transition_handler) fn classify_merge_failure_source(
    error: &crate::error::AppError,
) -> MergeFailureSource {
    git_cmd::classify_git_failure_source(error)
}

impl<'a> super::TransitionHandler<'a> {
    /// Handle a MergeOutcome uniformly for all merge strategy arms.
    pub(super) async fn handle_merge_outcome(
        &self,
        outcome: MergeOutcome,
        ctx: &mut MergeContext<'_>,
    ) {
        match outcome {
            MergeOutcome::Success {
                commit_sha,
                merge_path,
            } => {
                self.handle_outcome_success(
                    super::TaskCore {
                        task: &mut *ctx.task,
                        task_id: ctx.task_id,
                        task_id_str: ctx.task_id_str,
                        task_repo: ctx.task_repo,
                    },
                    super::BranchPair {
                        source_branch: ctx.source_branch,
                        target_branch: ctx.target_branch,
                    },
                    super::ProjectCtx {
                        project: ctx.project,
                        repo_path: ctx.repo_path,
                    },
                    ctx.plan_branch_repo,
                    &commit_sha,
                    &merge_path,
                    ctx.opts,
                )
                .await;
            }
            MergeOutcome::NeedsAgent {
                conflict_files,
                merge_worktree,
            } => {
                self.handle_outcome_needs_agent(
                    super::TaskCore {
                        task: &mut *ctx.task,
                        task_id: ctx.task_id,
                        task_id_str: ctx.task_id_str,
                        task_repo: ctx.task_repo,
                    },
                    super::BranchPair {
                        source_branch: ctx.source_branch,
                        target_branch: ctx.target_branch,
                    },
                    super::ProjectCtx {
                        project: ctx.project,
                        repo_path: ctx.repo_path,
                    },
                    &conflict_files,
                    merge_worktree.as_deref(),
                    ctx.opts,
                )
                .await;
            }
            MergeOutcome::BranchNotFound { branch } => {
                self.handle_outcome_branch_not_found(
                    super::TaskCore {
                        task: &mut *ctx.task,
                        task_id: ctx.task_id,
                        task_id_str: ctx.task_id_str,
                        task_repo: ctx.task_repo,
                    },
                    super::BranchPair {
                        source_branch: ctx.source_branch,
                        target_branch: ctx.target_branch,
                    },
                    &branch,
                    ctx.repo_path,
                )
                .await;
            }
            MergeOutcome::Deferred { reason } => {
                self.handle_outcome_deferred(
                    super::TaskCore {
                        task: &mut *ctx.task,
                        task_id: ctx.task_id,
                        task_id_str: ctx.task_id_str,
                        task_repo: ctx.task_repo,
                    },
                    super::BranchPair {
                        source_branch: ctx.source_branch,
                        target_branch: ctx.target_branch,
                    },
                    &reason,
                )
                .await;
            }
            MergeOutcome::GitError(e) => {
                self.handle_outcome_git_error(
                    super::TaskCore {
                        task: &mut *ctx.task,
                        task_id: ctx.task_id,
                        task_id_str: ctx.task_id_str,
                        task_repo: ctx.task_repo,
                    },
                    super::BranchPair {
                        source_branch: ctx.source_branch,
                        target_branch: ctx.target_branch,
                    },
                    e,
                    ctx.opts,
                )
                .await;
            }
            MergeOutcome::AlreadyHandled => {}
        }
    }

    async fn handle_outcome_success(
        &self,
        tc: super::TaskCore<'_>,
        bp: super::BranchPair<'_>,
        pc: super::ProjectCtx<'_>,
        plan_branch_repo: &Option<Arc<dyn PlanBranchRepository>>,
        commit_sha: &str,
        merge_path: &Path,
        opts: &MergeHandlerOptions,
    ) {
        let (task, task_id, task_id_str, task_repo) =
            (tc.task, tc.task_id, tc.task_id_str, tc.task_repo);
        let (source_branch, target_branch) = (bp.source_branch, bp.target_branch);
        let (project, repo_path) = (pc.project, pc.repo_path);
        tracing::info!(task_id = task_id_str, commit_sha = %commit_sha, strategy = opts.strategy_label, "Merge succeeded");

        emit_merge_progress(
            self.machine.context.services.event_sink.as_deref(),
            task_id_str,
            MergePhase::programmatic_merge(),
            MergePhaseStatus::Passed,
            format!(
                "{} completed: {}",
                capitalize(opts.strategy_label),
                commit_sha
            ),
        );

        // Post-merge validation gate (runs under its own deadline, separate from git timeout)
        let skip_validation = take_skip_validation_flag(task);
        let validation_mode = &project.merge_validation_mode;
        if !skip_validation && *validation_mode != MergeValidationMode::Off {
            let validation_deadline_secs = reconciliation_config().validation_deadline_secs;
            let validation_timeout = std::time::Duration::from_secs(validation_deadline_secs);
            let source_sha = GitService::get_branch_sha(repo_path, source_branch)
                .await
                .ok();
            let cached_log = source_sha
                .as_deref()
                .and_then(|sha| extract_cached_validation(task, sha));

            // Set validation_in_progress timestamp so the reconciler doesn't
            // treat this task as stale while validation commands are running.
            set_validation_in_progress(task, task_id_str, task_repo).await;

            // Register a cancellable token in the shared DashMap so pre_merge_cleanup
            // can cancel this validation if a new merge attempt starts for the same task.
            let validation_cancel = tokio_util::sync::CancellationToken::new();
            self.machine
                .context
                .services
                .validation_tokens
                .insert(task_id_str.to_string(), validation_cancel.clone());
            let validation_result = tokio::time::timeout(
                validation_timeout,
                run_validation_commands(
                    project,
                    task,
                    merge_path,
                    task_id_str,
                    self.machine.context.services.event_sink.as_deref(),
                    cached_log.as_deref(),
                    validation_mode,
                    &validation_cancel,
                ),
            )
            .await;
            // Always clean up the token after validation completes (success, failure, or timeout).
            self.machine
                .context
                .services
                .validation_tokens
                .remove(task_id_str);

            match validation_result {
                Err(_) => {
                    // Validation timed out — treat as validation failure
                    tracing::error!(
                        task_id = task_id_str,
                        deadline_secs = validation_deadline_secs,
                        "Post-merge validation timed out after {}s",
                        validation_deadline_secs,
                    );
                    let timeout_failure = ValidationFailure {
                        command: "validation pipeline".to_string(),
                        path: ".".to_string(),
                        exit_code: None,
                        stderr: format!("Validation timed out after {}s", validation_deadline_secs),
                    };
                    self.handle_validation_failure(
                        super::TaskCore {
                            task: &mut *task,
                            task_id,
                            task_id_str,
                            task_repo,
                        },
                        super::BranchPair {
                            source_branch,
                            target_branch,
                        },
                        super::ProjectCtx { project, repo_path },
                        &[timeout_failure],
                        &[],
                        merge_path,
                        opts.strategy_label,
                        validation_mode,
                    )
                    .await;
                    // Clean up merge worktree after Block mode failure
                    // (AutoFix keeps worktree for the fixer agent)
                    if *validation_mode != MergeValidationMode::AutoFix && merge_path != repo_path {
                        if let Err(e) = GitService::delete_worktree(repo_path, merge_path).await {
                            tracing::warn!(task_id = task_id_str, error = %e, "Failed to delete merge worktree after validation timeout (non-fatal)");
                        }
                    }
                    clear_validation_in_progress(task, task_id_str, task_repo).await;
                    return;
                }
                Ok(Some(validation)) => {
                    if !validation.all_passed {
                        if *validation_mode == MergeValidationMode::Warn {
                            tracing::warn!(
                                task_id = task_id_str,
                                "Validation failed in Warn mode, proceeding"
                            );
                            task.metadata = Some(format_validation_warn_metadata(
                                &validation.log,
                                source_branch,
                                target_branch,
                            ));
                        } else {
                            self.handle_validation_failure(
                                super::TaskCore {
                                    task: &mut *task,
                                    task_id,
                                    task_id_str,
                                    task_repo,
                                },
                                super::BranchPair {
                                    source_branch,
                                    target_branch,
                                },
                                super::ProjectCtx { project, repo_path },
                                &validation.failures,
                                &validation.log,
                                merge_path,
                                opts.strategy_label,
                                validation_mode,
                            )
                            .await;
                            // Clean up merge worktree after Block mode failure
                            // (AutoFix keeps worktree for the fixer agent)
                            if *validation_mode != MergeValidationMode::AutoFix
                                && merge_path != repo_path
                            {
                                if let Err(e) =
                                    GitService::delete_worktree(repo_path, merge_path).await
                                {
                                    tracing::warn!(task_id = task_id_str, error = %e, "Failed to delete merge worktree after validation failure (non-fatal)");
                                }
                            }
                            clear_validation_in_progress(task, task_id_str, task_repo).await;
                            return;
                        }
                    } else {
                        task.metadata = Some(serde_json::json!({
                            "validation_log": validation.log, "validation_source_sha": source_sha,
                            "source_branch": source_branch, "target_branch": target_branch,
                        }).to_string());
                    }
                }
                Ok(None) => {
                    // No validation commands configured — proceed
                }
            }

            // Clear validation_in_progress flag after validation completes (success/warn path)
            clear_validation_in_progress(task, task_id_str, task_repo).await;
        }

        // Complete merge
        let event_sink = self.machine.context.services.event_sink.as_deref();
        let external_events_repo = self.machine.context.services.external_events_repo.as_ref();
        let webhook_publisher = self.machine.context.services.webhook_publisher.as_ref();
        if let Err(e) = complete_merge_internal_with_pr_sync_and_notifier(
            task,
            project,
            commit_sha,
            source_branch,
            target_branch,
            task_repo,
            external_events_repo,
            webhook_publisher,
            event_sink,
            None,
            Some(
                super::merge_helpers::PlanBranchPrSyncServices::from_task_services(
                    &self.machine.context.services,
                ),
            ),
            Some(&self.machine.context.services.notifier),
        )
        .await
        {
            if is_pr_branch_publication_conflict_routed_error(&e)
                || task_has_pr_branch_publication_conflict(task)
            {
                tracing::info!(
                    task_id = task_id_str,
                    strategy = opts.strategy_label,
                    "PR branch publication conflict routed to branch updater; skipping MergeIncomplete fallback"
                );
                if let Some(transition_service) = &self.machine.context.services.transition_service
                {
                    transition_service
                        .execute_entry_actions(task_id, task, task.internal_status)
                        .await;
                } else {
                    tracing::warn!(
                        task_id = task_id_str,
                        "PR branch publication conflict routed but transition_service is unavailable to start branch updater"
                    );
                }
                return;
            }
            tracing::error!(error = %e, task_id = task_id_str, strategy = opts.strategy_label, "Failed to complete merge");
            // Merge INTO existing metadata to preserve recovery history
            super::merge_helpers::merge_metadata_into(
                task,
                &serde_json::json!({
                    "error": format!("complete_merge_internal failed: {}", e),
                    "source_branch": source_branch,
                    "target_branch": target_branch,
                }),
            );
            transition_to_merge_incomplete(
                task,
                task_id,
                task_id_str,
                task_repo,
                &self.machine.context.services.event_emitter,
                &self.machine.context.services.notifier,
            )
            .await;
        } else {
            // Phase 2 complete: task is Merged. Now run post-merge activities
            // synchronously (fast: plan branch, deps, scheduling) then spawn
            // Phase 3 deferred cleanup in the background (slow: worktree, branch).
            self.post_merge_cleanup(task_id_str, task_id, repo_path, plan_branch_repo)
                .await;

            // merge:completed and task:status_changed external events are now emitted
            // inside complete_merge_internal — no duplicate emission needed here.

            // Phase 3: spawn fire-and-forget cleanup for slow operations
            let task_repo_clone = Arc::clone(task_repo);
            let task_id_clone = task_id.clone();
            let project_dir = project.working_directory.clone();
            let task_branch_clone = task.task_branch.clone();
            let worktree_path_clone = task.worktree_path.clone();
            let cleanup_plan_branch = Some(target_branch.to_string());
            tokio::spawn(async move {
                super::merge_completion::deferred_merge_cleanup(
                    task_id_clone,
                    task_repo_clone,
                    project_dir,
                    task_branch_clone,
                    worktree_path_clone,
                    cleanup_plan_branch,
                )
                .await;
            });
        }

        // Clean up merge worktree after merge completion (success or failure).
        // This is the temporary worktree used for the merge operation itself,
        // separate from the task's worktree (handled by Phase 3).
        if merge_path != repo_path {
            if let Err(e) =
                super::cleanup_helpers::remove_worktree_fast(merge_path, repo_path).await
            {
                tracing::warn!(task_id = task_id_str, error = %e, "Failed to delete merge worktree after merge completion (non-fatal)");
            }
        }
    }

    async fn handle_outcome_needs_agent(
        &self,
        tc: super::TaskCore<'_>,
        bp: super::BranchPair<'_>,
        pc: super::ProjectCtx<'_>,
        conflict_files: &[PathBuf],
        merge_worktree: Option<&Path>,
        opts: &MergeHandlerOptions,
    ) {
        let (task, task_id, task_id_str, task_repo) =
            (tc.task, tc.task_id, tc.task_id_str, tc.task_repo);
        let (source_branch, target_branch) = (bp.source_branch, bp.target_branch);
        let (project, repo_path) = (pc.project, pc.repo_path);
        tracing::info!(
            task_id = task_id_str,
            conflict_count = conflict_files.len(),
            strategy = opts.strategy_label,
            "Conflicts detected"
        );

        emit_merge_progress(
            self.machine.context.services.event_sink.as_deref(),
            task_id_str,
            MergePhase::programmatic_merge(),
            MergePhaseStatus::Failed,
            format!(
                "{} conflicts in {} files",
                capitalize(opts.strategy_label),
                conflict_files.len()
            ),
        );

        for file in conflict_files {
            tracing::debug!(task_id = task_id_str, file = %file.display(), "Conflict file");
        }

        // Persist conflict metadata
        let conflict_file_strings: Vec<String> = conflict_files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        super::merge_helpers::set_conflict_metadata(task, &conflict_file_strings, "programmatic");

        // Determine worktree path for agent
        let agent_wt = if let Some(wt) = merge_worktree {
            wt.to_path_buf()
        } else {
            // Checkout-free: create temp worktree for conflict resolution
            let wt_path = PathBuf::from(compute_merge_worktree_path(project, task_id_str));
            let target_sha = GitService::get_branch_sha(repo_path, target_branch)
                .await
                .unwrap_or_default();
            let resolve_branch = format!("merge-resolve/{}", task_id_str);
            if let Err(e) =
                GitService::create_branch_at(repo_path, &resolve_branch, &target_sha).await
            {
                tracing::error!(error = %e, task_id = task_id_str, "Failed to create resolve branch");
            }
            if let Err(e) =
                GitService::checkout_existing_branch_worktree(repo_path, &wt_path, &resolve_branch)
                    .await
            {
                tracing::error!(error = %e, task_id = task_id_str, "Failed to create merge worktree");
            }
            // Intentional: merge may fail with conflicts — agent will resolve them in the worktree
            let _ = git_cmd::run(&["merge", source_branch, "--no-edit"], &wt_path).await;
            wt_path
        };

        task.worktree_path = Some(agent_wt.to_string_lossy().to_string());

        // Set conflict_type metadata for rebase strategies
        if let Some(ct) = opts.conflict_type {
            let mut meta = parse_metadata(task).unwrap_or_else(|| serde_json::json!({}));
            if let Some(obj) = meta.as_object_mut() {
                obj.insert("conflict_type".to_string(), serde_json::json!(ct));
            }
            task.metadata = Some(meta.to_string());
        }

        task.internal_status = InternalStatus::Merging;
        task.touch();
        if let Err(e) = task_repo.update(task).await {
            tracing::error!(error = %e, "Failed to update task to Merging");
            return;
        }
        if let Err(e) = task_repo
            .persist_status_change(
                task_id,
                InternalStatus::PendingMerge,
                InternalStatus::Merging,
                opts.conflict_reason,
            )
            .await
        {
            tracing::warn!(error = %e, "Failed to record {} transition (non-fatal)", opts.conflict_reason);
        }
        self.machine
            .context
            .services
            .event_emitter
            .emit_status_change(task_id_str, "pending_merge", "merging")
            .await;

        // Emit merge:conflict via both channels: external_events (SSE/poll) + webhook publisher.
        {
            let project_id_str = project.id.to_string();
            let conflict_file_strings: Vec<String> = conflict_files
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect();
            let conflict_payload = serde_json::json!({
                "task_id": task_id_str,
                "project_id": project_id_str,
                "source_branch": source_branch,
                "target_branch": target_branch,
                "conflict_files": conflict_file_strings,
                "strategy": opts.strategy_label,
                "timestamp": chrono::Utc::now().to_rfc3339(),
            });
            if let Some(ref repo) = self.machine.context.services.external_events_repo {
                if let Err(e) = repo
                    .insert_event(
                        "merge:conflict",
                        &project_id_str,
                        &conflict_payload.to_string(),
                    )
                    .await
                {
                    tracing::warn!(
                        error = %e,
                        task_id = task_id_str,
                        "merge_outcome: failed to insert merge:conflict external event (non-fatal)"
                    );
                }
            }
            if let Some(ref publisher) = self.machine.context.services.webhook_publisher {
                publisher
                    .publish(
                        ralphx_domain::entities::EventType::MergeConflict,
                        &project_id_str,
                        conflict_payload,
                    )
                    .await;
            }
        }

        // Spawn merger agent
        let prompt = format!(
            "Resolve {} conflicts for task: {}{}",
            opts.strategy_label, task_id_str, opts.agent_prompt_suffix
        );
        tracing::info!(
            task_id = task_id_str,
            strategy = opts.strategy_label,
            "Spawning merger agent"
        );
        let result = self
            .machine
            .context
            .services
            .chat_service
            .send_message(
                crate::domain::entities::ChatContextType::Merge,
                task_id_str,
                &prompt,
                Default::default(),
            )
            .await;
        match &result {
            Ok(_) => tracing::info!(task_id = task_id_str, "Merger agent spawned"),
            Err(e) => {
                tracing::error!(task_id = task_id_str, error = %e, "Failed to spawn merger agent")
            }
        }
    }

    async fn handle_outcome_branch_not_found(
        &self,
        tc: super::TaskCore<'_>,
        bp: super::BranchPair<'_>,
        missing_branch: &str,
        repo_path: &Path,
    ) {
        let (task, task_id, task_id_str, task_repo) =
            (tc.task, tc.task_id, tc.task_id_str, tc.task_repo);
        let (source_branch, target_branch) = (bp.source_branch, bp.target_branch);
        tracing::warn!(task_id = task_id_str, missing_branch = %missing_branch, "Branch not found, re-checking");

        // Re-check if branch exists now (race condition: concurrent operation may have created it)
        let branch_exists = git_cmd::run_status(
            &[
                "rev-parse",
                "--verify",
                &format!("refs/heads/{}", missing_branch),
            ],
            repo_path,
        )
        .await
        .unwrap_or(false);

        if branch_exists {
            tracing::info!(
                task_id = task_id_str,
                branch = %missing_branch,
                "Branch found on re-check — deferring for fast retry"
            );
            self.handle_outcome_deferred(
                super::TaskCore {
                    task: &mut *task,
                    task_id,
                    task_id_str,
                    task_repo,
                },
                super::BranchPair {
                    source_branch,
                    target_branch,
                },
                &format!(
                    "branch '{}' appeared on re-check (race condition)",
                    missing_branch
                ),
            )
            .await;
            return;
        }

        tracing::error!(task_id = task_id_str, missing_branch = %missing_branch, "Branch confirmed missing after re-check");

        let mut recovery = get_or_create_recovery(task);
        let attempt = retry_attempt_count(&recovery);
        let event = MergeRecoveryEvent::new(
            MergeRecoveryEventKind::AutoRetryTriggered,
            MergeRecoverySource::Auto,
            MergeRecoveryReasonCode::BranchNotFound,
            format!(
                "Target branch {} does not exist. Create the branch or update the plan.",
                missing_branch
            ),
        )
        .with_target_branch(target_branch)
        .with_source_branch(source_branch)
        .with_attempt(attempt);
        recovery.append_event(event);

        match recovery.update_task_metadata(task.metadata.as_deref()) {
            Ok(updated_json) => {
                if let Ok(mut obj) = serde_json::from_str::<serde_json::Value>(&updated_json) {
                    if let Some(m) = obj.as_object_mut() {
                        m.insert("branch_missing".to_string(), serde_json::json!(true));
                    }
                    task.metadata = Some(obj.to_string());
                } else {
                    task.metadata = Some(updated_json);
                }
            }
            Err(e) => {
                tracing::error!(task_id = task_id_str, error = %e, "Failed to serialize recovery metadata");
                task.metadata = Some(
                    serde_json::json!({
                        "error": format!("Target branch {} does not exist. Create the branch or update the plan.", missing_branch),
                        "missing_branch": missing_branch, "source_branch": source_branch,
                        "target_branch": target_branch, "branch_missing": true
                    })
                    .to_string(),
                );
            }
        }

        transition_to_merge_incomplete(
            task,
            task_id,
            task_id_str,
            task_repo,
            &self.machine.context.services.event_emitter,
            &self.machine.context.services.notifier,
        )
        .await;
    }

    async fn handle_outcome_deferred(
        &self,
        tc: super::TaskCore<'_>,
        bp: super::BranchPair<'_>,
        reason: &str,
    ) {
        let task = tc.task;
        let task_id_str = tc.task_id_str;
        let task_repo = tc.task_repo;
        let (source_branch, target_branch) = (bp.source_branch, bp.target_branch);
        tracing::warn!(task_id = task_id_str, reason = %reason, "Merge deferred, staying in PendingMerge");

        let mut recovery = get_or_create_recovery(task);
        let event = MergeRecoveryEvent::new(
            MergeRecoveryEventKind::Deferred,
            MergeRecoverySource::System,
            MergeRecoveryReasonCode::GitError,
            format!("Merge deferred: {}", reason),
        )
        .with_target_branch(target_branch)
        .with_source_branch(source_branch);
        recovery.append_event_with_state(event, MergeRecoveryState::Deferred);

        match recovery.update_task_metadata(task.metadata.as_deref()) {
            Ok(updated_json) => {
                task.metadata = Some(updated_json);
            }
            Err(e) => {
                tracing::error!(task_id = task_id_str, error = %e, "Failed to serialize recovery metadata");
                task.metadata = Some(
                    serde_json::json!({
                        "merge_deferred": true, "error": reason,
                        "source_branch": source_branch, "target_branch": target_branch,
                    })
                    .to_string(),
                );
            }
        }

        task.touch();
        if let Err(e) = task_repo.update(task).await {
            tracing::error!(error = %e, "Failed to update task with merge_deferred metadata");
        }
    }

    async fn handle_outcome_git_error(
        &self,
        tc: super::TaskCore<'_>,
        bp: super::BranchPair<'_>,
        error: crate::error::AppError,
        opts: &MergeHandlerOptions,
    ) {
        let (task, task_id, task_id_str, task_repo) =
            (tc.task, tc.task_id, tc.task_id_str, tc.task_repo);
        let (source_branch, target_branch) = (bp.source_branch, bp.target_branch);
        if GitService::is_branch_lock_error(&error) {
            tracing::warn!(task_id = task_id_str, error = %error, strategy = opts.strategy_label, "Branch lock, deferring");
            self.handle_outcome_deferred(
                super::TaskCore {
                    task: &mut *task,
                    task_id,
                    task_id_str,
                    task_repo,
                },
                super::BranchPair {
                    source_branch,
                    target_branch,
                },
                &format!("branch lock: {}", error),
            )
            .await;
            return;
        }

        // Transient errors (lock contention, index.lock, etc.) → defer instead of
        // MergeIncomplete. This avoids the 60s+ backoff from reconciliation and lets
        // the PendingMerge reconciler retry in seconds.
        if is_transient_merge_error(&error) {
            tracing::warn!(
                task_id = task_id_str,
                error = %error,
                strategy = opts.strategy_label,
                "Transient git error, deferring for fast retry"
            );
            self.handle_outcome_deferred(
                super::TaskCore {
                    task: &mut *task,
                    task_id,
                    task_id_str,
                    task_repo,
                },
                super::BranchPair {
                    source_branch,
                    target_branch,
                },
                &format!("transient git error: {}", error),
            )
            .await;
            return;
        }

        let full_error = error.to_string();
        if super::is_commit_hook_merge_error_text(&full_error) {
            let kind = super::classify_commit_hook_failure_text(&full_error);
            let fingerprint = super::commit_hook_failure_fingerprint(&full_error);
            let repeated = super::is_repeated_commit_hook_failure(task, &fingerprint);

            if matches!(kind, super::CommitHookFailureKind::EnvironmentFailure) || repeated {
                self.block_commit_hook_failure_as_merge_incomplete(
                    super::TaskCore {
                        task: &mut *task,
                        task_id,
                        task_id_str,
                        task_repo,
                    },
                    super::BranchPair {
                        source_branch,
                        target_branch,
                    },
                    &full_error,
                    kind,
                    &fingerprint,
                    repeated,
                    opts,
                )
                .await;
                return;
            }

            if self
                .route_commit_hook_failure_to_revision(task_id, &full_error)
                .await
            {
                tracing::warn!(
                    task_id = task_id_str,
                    strategy = opts.strategy_label,
                    "Commit-hook merge failure rerouted to revision flow"
                );
                return;
            }

            tracing::warn!(
                task_id = task_id_str,
                strategy = opts.strategy_label,
                "Commit-hook merge reroute unavailable; falling back to MergeIncomplete"
            );
        }

        tracing::error!(task_id = task_id_str, error = %error, strategy = opts.strategy_label, "Merge failed → MergeIncomplete");

        let mut recovery = get_or_create_recovery(task);
        let attempt = retry_attempt_count(&recovery);
        let failure_source = git_cmd::classify_git_failure_source(&error);
        let failed_event = MergeRecoveryEvent::new(
            MergeRecoveryEventKind::AttemptFailed,
            MergeRecoverySource::System,
            MergeRecoveryReasonCode::GitError,
            format!("Merge failed ({}): {}", opts.strategy_label, error),
        )
        .with_target_branch(target_branch)
        .with_source_branch(source_branch)
        .with_attempt(attempt)
        .with_failure_source(failure_source);
        recovery.append_event_with_state(failed_event, MergeRecoveryState::Failed);

        match recovery.update_task_metadata(task.metadata.as_deref()) {
            Ok(updated_json) => {
                if let Ok(mut meta) = serde_json::from_str::<serde_json::Value>(&updated_json) {
                    if let Some(obj) = meta.as_object_mut() {
                        obj.insert("error".to_string(), serde_json::json!(error.to_string()));
                        obj.insert(
                            "source_branch".to_string(),
                            serde_json::json!(source_branch),
                        );
                        obj.insert(
                            "target_branch".to_string(),
                            serde_json::json!(target_branch),
                        );
                    }
                    task.metadata = Some(meta.to_string());
                } else {
                    task.metadata = Some(updated_json);
                }
            }
            Err(e) => {
                tracing::error!(task_id = task_id_str, error = %e, "Failed to serialize recovery metadata");
                task.metadata = Some(serde_json::json!({
                    "error": error.to_string(), "source_branch": source_branch, "target_branch": target_branch,
                }).to_string());
            }
        }

        transition_to_merge_incomplete(
            task,
            task_id,
            task_id_str,
            task_repo,
            &self.machine.context.services.event_emitter,
            &self.machine.context.services.notifier,
        )
        .await;
    }

    async fn block_commit_hook_failure_as_merge_incomplete(
        &self,
        tc: super::TaskCore<'_>,
        bp: super::BranchPair<'_>,
        full_error: &str,
        kind: super::CommitHookFailureKind,
        fingerprint: &str,
        repeated: bool,
        opts: &MergeHandlerOptions,
    ) {
        let (task, task_id, task_id_str, task_repo) =
            (tc.task, tc.task_id, tc.task_id_str, tc.task_repo);
        let (source_branch, target_branch) = (bp.source_branch, bp.target_branch);
        let mut recovery = get_or_create_recovery(task);
        let attempt = retry_attempt_count(&recovery);
        let repeat_count = if repeated {
            super::commit_hook_repeat_count(task, fingerprint) + 1
        } else {
            super::commit_hook_repeat_count(task, fingerprint)
        };
        let failure_source = if repeated {
            MergeFailureSource::RepeatedHookFailure
        } else {
            MergeFailureSource::HookEnvironment
        };
        let reason = if repeated {
            "repeated_hook_failure"
        } else {
            "hook_environment_failure"
        };
        let message = if repeated {
            format!(
                "Merge blocked: repository hook failure repeated after re-execution ({})",
                opts.strategy_label
            )
        } else {
            format!(
                "Merge blocked: repository hook environment failed ({})",
                opts.strategy_label
            )
        };

        let failed_event = MergeRecoveryEvent::new(
            MergeRecoveryEventKind::AttemptFailed,
            MergeRecoverySource::System,
            MergeRecoveryReasonCode::GitError,
            format!("{message}: {full_error}"),
        )
        .with_target_branch(target_branch)
        .with_source_branch(source_branch)
        .with_attempt(attempt)
        .with_failure_source(failure_source);
        recovery.append_event_with_state(failed_event, MergeRecoveryState::Failed);

        match recovery.update_task_metadata(task.metadata.as_deref()) {
            Ok(updated_json) => {
                let mut meta = serde_json::from_str::<serde_json::Value>(&updated_json)
                    .unwrap_or_else(|_| serde_json::json!({}));
                if let Some(obj) = meta.as_object_mut() {
                    obj.insert("error".to_string(), serde_json::json!(full_error));
                    obj.insert(
                        "source_branch".to_string(),
                        serde_json::json!(source_branch),
                    );
                    obj.insert(
                        "target_branch".to_string(),
                        serde_json::json!(target_branch),
                    );
                    obj.insert(
                        "merge_hook_failure_kind".to_string(),
                        serde_json::json!(kind.as_str()),
                    );
                    obj.insert(
                        "merge_hook_failure_fingerprint".to_string(),
                        serde_json::json!(fingerprint),
                    );
                    obj.insert(
                        "merge_hook_failure_repeat_count".to_string(),
                        serde_json::json!(repeat_count),
                    );
                    obj.insert(
                        "merge_hook_blocked_reason".to_string(),
                        serde_json::json!(reason),
                    );
                    obj.insert(
                        "merge_hook_reexecution_requested".to_string(),
                        serde_json::json!(false),
                    );
                    obj.insert(
                        "merge_revision_error".to_string(),
                        serde_json::json!(full_error),
                    );
                    if repeated {
                        obj.insert(
                            "merge_hook_repeated_error".to_string(),
                            serde_json::json!(full_error),
                        );
                    } else {
                        obj.insert(
                            "merge_hook_environment_error".to_string(),
                            serde_json::json!(full_error),
                        );
                    }
                }
                task.metadata = Some(meta.to_string());
            }
            Err(e) => {
                tracing::error!(task_id = task_id_str, error = %e, "Failed to serialize hook failure metadata");
                task.metadata = Some(
                    serde_json::json!({
                        "error": full_error,
                        "source_branch": source_branch,
                        "target_branch": target_branch,
                        "merge_hook_failure_kind": kind.as_str(),
                        "merge_hook_failure_fingerprint": fingerprint,
                        "merge_hook_failure_repeat_count": repeat_count,
                        "merge_hook_blocked_reason": reason,
                        "merge_hook_reexecution_requested": false,
                    })
                    .to_string(),
                );
            }
        }

        tracing::warn!(
            task_id = task_id_str,
            kind = kind.as_str(),
            repeated,
            strategy = opts.strategy_label,
            "Commit-hook merge failure blocked without code re-execution"
        );

        transition_to_merge_incomplete(
            task,
            task_id,
            task_id_str,
            task_repo,
            &self.machine.context.services.event_emitter,
            &self.machine.context.services.notifier,
        )
        .await;
    }

    async fn route_commit_hook_failure_to_revision(
        &self,
        task_id: &TaskId,
        full_error: &str,
    ) -> bool {
        let Some(transition_service) = &self.machine.context.services.transition_service else {
            tracing::warn!(
                task_id = task_id.as_str(),
                "transition_service unavailable; cannot reroute commit-hook merge failure"
            );
            return false;
        };

        match transition_service
            .reroute_commit_hook_merge_failure(
                task_id,
                Some(full_error.to_string()),
                true,
                "system",
            )
            .await
        {
            Ok(_) => true,
            Err(e) => {
                tracing::warn!(
                    task_id = task_id.as_str(),
                    error = %e,
                    "Failed to corrective-transition commit-hook merge failure to RevisionNeeded"
                );
                false
            }
        }
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

/// Set validation_in_progress timestamp in task metadata so the reconciler
/// knows validation commands are actively running and skips stale-detection.
async fn set_validation_in_progress(
    task: &mut Task,
    task_id_str: &str,
    task_repo: &Arc<dyn TaskRepository>,
) {
    let now = chrono::Utc::now().to_rfc3339();
    let mut meta = task
        .metadata
        .as_deref()
        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if let Some(obj) = meta.as_object_mut() {
        obj.insert("validation_in_progress".to_string(), serde_json::json!(now));
    }
    task.metadata = Some(meta.to_string());
    task.touch();
    if let Err(e) = task_repo.update(task).await {
        tracing::warn!(
            task_id = task_id_str,
            error = %e,
            "Failed to persist validation_in_progress flag (non-fatal)"
        );
    }
}

/// Clear validation_in_progress flag from task metadata after validation completes.
async fn clear_validation_in_progress(
    task: &mut Task,
    task_id_str: &str,
    task_repo: &Arc<dyn TaskRepository>,
) {
    let mut meta = task
        .metadata
        .as_deref()
        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if let Some(obj) = meta.as_object_mut() {
        obj.remove("validation_in_progress");
    }
    task.metadata = Some(meta.to_string());
    task.touch();
    if let Err(e) = task_repo.update(task).await {
        tracing::warn!(
            task_id = task_id_str,
            error = %e,
            "Failed to clear validation_in_progress flag (non-fatal)"
        );
    }
}
