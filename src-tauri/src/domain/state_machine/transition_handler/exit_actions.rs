// Exit-action helpers extracted from mod.rs to reduce file size.
//
// Contains: auto-commit on execution done, revision-cap enforcement,
// deferred-merge retry trigger, and working-directory resolution.

use std::sync::Arc;

use crate::application::GitService;
use crate::domain::entities::{ProjectId, TaskId};
use crate::domain::review::config::ReviewSettings;
use crate::domain::state_machine::machine::State;
use crate::domain::state_machine::types::FailedData;

use super::merge_helpers;
use super::merge_helpers::clear_trigger_origin;

/// Context bag passed from TransitionHandler to avoid leaking `machine` internals.
pub(crate) struct ExitContext {
    pub task_id: String,
    pub project_id: String,
    pub task_repo: Option<Arc<dyn crate::domain::repositories::TaskRepository>>,
    pub project_repo: Option<Arc<dyn crate::domain::repositories::ProjectRepository>>,
    pub task_scheduler: Option<Arc<dyn crate::domain::state_machine::services::TaskScheduler>>,
}

/// Clear trigger_origin metadata when exiting agent-active states.
pub(crate) async fn clear_trigger_origin_on_exit(ctx: &ExitContext) {
    let Some(ref task_repo) = ctx.task_repo else {
        return;
    };
    let task_id = TaskId::from_string(ctx.task_id.clone());
    if let Ok(Some(mut task)) = task_repo.get_by_id(&task_id).await {
        clear_trigger_origin(&mut task);
        if let Err(e) = task_repo
            .update_metadata(&task_id, task.metadata.clone())
            .await
        {
            tracing::error!(
                task_id = %ctx.task_id,
                error = %e,
                "Failed to clear trigger_origin in metadata on state exit"
            );
        }
    }
}

/// Retry deferred merges when a task exits the merge workflow (PendingMerge | Merging).
pub(crate) fn spawn_deferred_merge_retry(ctx: &ExitContext, from: &State, to: &State) {
    let Some(ref scheduler) = ctx.task_scheduler else {
        return;
    };
    let scheduler = Arc::clone(scheduler);
    let project_id = ctx.project_id.clone();
    let from_state = format!("{:?}", from);
    let to_state = format!("{:?}", to);

    tracing::info!(
        task_id = %ctx.task_id,
        from = %from_state,
        to = %to_state,
        "Task exiting merge workflow, triggering deferred merge retry"
    );

    tokio::spawn(async move {
        scheduler.try_retry_deferred_merges(&project_id).await;
        // Always attempt main merge retry — do NOT guard with running_count == 0 here.
        //
        // The previous guard was a TOCTOU: by the time this spawned future evaluates
        // running_count, an auto-transition (e.g. PendingReview→Reviewing, 72ms window
        // observed in logs) may have already incremented running_count to 1, causing
        // try_retry_main_merges to be skipped entirely and the main merge task to remain
        // stuck with main_merge_deferred metadata until the reconciler retries (~minutes).
        //
        // The authoritative deferral gate is check_main_merge_deferral() inside
        // attempt_programmatic_merge(), which reads running_count fresh at merge-start time
        // and correctly re-defers if agents are still running when the merge would begin.
        scheduler.try_retry_main_merges().await;
    });
}

/// Check if the revision cap has been exceeded for a RevisionNeeded -> ReExecuting auto-transition.
///
/// If the task has exceeded `max_revision_cycles`, returns `State::Failed`.
/// Otherwise increments the revision count and returns `default_state`.
pub(crate) async fn check_revision_cap_or_fail(ctx: &ExitContext, default_state: State) -> State {
    let Some(ref task_repo) = ctx.task_repo else {
        tracing::debug!(
            task_id = %ctx.task_id,
            "Skipping revision cap check: task_repo not available"
        );
        return default_state;
    };

    let task_id = TaskId::from_string(ctx.task_id.clone());
    let Ok(Some(mut task)) = task_repo.get_by_id(&task_id).await else {
        tracing::warn!(
            task_id = %ctx.task_id,
            "Skipping revision cap check: failed to fetch task"
        );
        return default_state;
    };

    let settings = ReviewSettings::default();
    let revision_count = merge_helpers::get_revision_count(&task);

    if settings.exceeded_max_revisions(revision_count) {
        let max = settings.max_revision_cycles;
        tracing::warn!(
            task_id = %ctx.task_id,
            revision_count = revision_count,
            max_revision_cycles = max,
            "Revision cap exceeded, transitioning to Failed instead of ReExecuting"
        );
        return State::Failed(FailedData::new(format!(
            "Exceeded maximum revision cycles ({}/{}). Task has been through too many review-revise loops.",
            revision_count, max
        )));
    }

    // Increment revision count for this cycle
    let new_count = merge_helpers::increment_revision_count(&mut task);
    if let Err(e) = task_repo
        .update_metadata(&task_id, task.metadata.clone())
        .await
    {
        tracing::error!(
            task_id = %ctx.task_id,
            error = %e,
            "Failed to update revision_count in metadata"
        );
    } else {
        tracing::info!(
            task_id = %ctx.task_id,
            revision_count = new_count,
            max_revision_cycles = settings.max_revision_cycles,
            "Incremented revision count for re-execution cycle"
        );
    }

    default_state
}

/// Auto-commit uncommitted changes when exiting Executing/ReExecuting.
///
/// Skips the commit when the exit is due to a transient failure (Timeout, ParseStall,
/// AgentCrash) to keep the branch clean before the reconciler retries (GAPs B4, H5).
///
/// On non-failure exits, updates `execution_recovery.last_state` to `Succeeded` if
/// recovery metadata exists, preventing a stale "retrying" badge in the UI (GAP H11).
pub(crate) async fn auto_commit_on_execution_done(ctx: &ExitContext) {
    let (Some(ref task_repo), Some(ref project_repo)) = (&ctx.task_repo, &ctx.project_repo) else {
        tracing::debug!(
            task_id = %ctx.task_id,
            "Skipping auto-commit: repos not available"
        );
        return;
    };

    let task_id = TaskId::from_string(ctx.task_id.clone());
    let project_id = ProjectId::from_string(ctx.project_id.clone());

    let task_result = task_repo.get_by_id(&task_id).await;
    let project_result = project_repo.get_by_id(&project_id).await;

    let (Ok(Some(task)), Ok(Some(project))) = (task_result, project_result) else {
        tracing::warn!(
            task_id = %ctx.task_id,
            "Skipping auto-commit: failed to fetch task or project"
        );
        return;
    };

    // GAPs B4, H5: Skip auto-commit for transient failures — the reconciler will retry
    // with a fresh worktree/branch, so partial commits would only pollute the branch.
    if let Ok(Some(recovery)) =
        crate::domain::entities::ExecutionRecoveryMetadata::from_task_metadata(
            task.metadata.as_deref(),
        )
    {
        if recovery.last_failure_is_transient() {
            tracing::info!(
                task_id = %ctx.task_id,
                "Skipping auto-commit for transient failure (will retry)"
            );
            return;
        }
    }

    let working_path = resolve_automatic_commit_target(&task, &project);

    match working_path {
        Some(working_path) => match GitService::has_uncommitted_changes(&working_path).await {
            Ok(true) => {
                let prefix = "feat: ";
                let message = format!("{}{}", prefix, task.title);

                if super::automatic_commit_policy::protects_primary_checkout(
                    &project,
                    &working_path,
                ) {
                    tracing::warn!(
                        task_id = %ctx.task_id,
                        reason = "pr_mode_primary_checkout_protected",
                        "Skipping auto-commit because the target no longer resolves to an isolated worktree"
                    );
                } else {
                    match GitService::commit_all(&working_path, &message).await {
                        Ok(Some(sha)) => {
                            tracing::info!(
                                task_id = %ctx.task_id,
                                commit_sha = %sha,
                                message = %message,
                                "Auto-committed changes on execution completion"
                            );
                        }
                        Ok(None) => {
                            tracing::debug!(
                                task_id = %ctx.task_id,
                                "Auto-commit: no staged changes to commit"
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                task_id = %ctx.task_id,
                                error = %e,
                                "Auto-commit failed (non-fatal)"
                            );
                        }
                    }
                }
            }
            Ok(false) => {
                tracing::debug!(
                    task_id = %ctx.task_id,
                    "No uncommitted changes to auto-commit"
                );
            }
            Err(e) => {
                tracing::warn!(
                    task_id = %ctx.task_id,
                    error = %e,
                    "Failed to check uncommitted changes (non-fatal)"
                );
            }
        },
        None => tracing::info!(
            task_id = %ctx.task_id,
            reason = "pr_mode_primary_checkout_protected",
            "Skipping auto-commit because PR mode has no verified isolated task worktree"
        ),
    }

    // GAP H11: Update execution_recovery.last_state to Succeeded when the task exits
    // Executing/ReExecuting successfully. This clears the stale "retrying" badge in the
    // UI after a successful retry. Only runs when the transient-failure guard above did
    // not return early (i.e., this is not a transient failure exit).
    // Guard: skip if the last event in the log is a Failed event — this function is
    // called for both success and error paths in some edge cases, and we must not
    // overwrite a Failed state with Succeeded.
    if let Ok(Some(mut recovery)) =
        crate::domain::entities::ExecutionRecoveryMetadata::from_task_metadata(
            task.metadata.as_deref(),
        )
    {
        let last_event_is_failure = recovery
            .events
            .last()
            .map(|e| {
                matches!(
                    e.kind,
                    crate::domain::entities::ExecutionRecoveryEventKind::Failed
                )
            })
            .unwrap_or(false);
        // Also skip if stop_retrying=true — the E7/wall-clock pre-write sets last_state=Failed
        // intentionally; overwriting it with Succeeded would make the task look retryable.
        if !last_event_is_failure
            && !recovery.stop_retrying
            && recovery.last_state != crate::domain::entities::ExecutionRecoveryState::Succeeded
        {
            recovery.last_state = crate::domain::entities::ExecutionRecoveryState::Succeeded;
            match recovery.update_task_metadata(task.metadata.as_deref()) {
                Ok(new_metadata) => {
                    if let Err(e) = task_repo
                        .update_metadata(&task_id, Some(new_metadata))
                        .await
                    {
                        tracing::warn!(
                            task_id = %ctx.task_id,
                            error = %e,
                            "Failed to update execution_recovery.last_state to Succeeded"
                        );
                    } else {
                        tracing::info!(
                            task_id = %ctx.task_id,
                            "Updated execution_recovery.last_state to Succeeded"
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        task_id = %ctx.task_id,
                        error = %e,
                        "Failed to serialize updated execution_recovery metadata"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "exit_actions_tests.rs"]
mod tests;

/// Resolve an automatic commit target, rejecting a PR-mode primary checkout.
fn resolve_automatic_commit_target(
    task: &crate::domain::entities::Task,
    project: &crate::domain::entities::Project,
) -> Option<std::path::PathBuf> {
    let candidate = task
        .worktree_path
        .as_ref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(&project.working_directory));

    if super::automatic_commit_policy::protects_primary_checkout(project, &candidate) {
        None
    } else {
        Some(candidate)
    }
}
