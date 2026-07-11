// Shared freshness return-routing logic used by BOTH:
//  - `complete_merge` HTTP handler (http_server/handlers/git.rs)
//  - `attempt_merge_auto_complete` in `chat_service_merge.rs`
//
// When the merger agent resolves a plan←main freshness conflict (not the actual
// task merge), the task should be routed back to its origin state instead of
// completing the merge and potentially losing the task's work.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::application::git_service::GitService;
use crate::application::interactive_process_registry::{
    InteractiveProcessKey, InteractiveProcessRegistry,
};
use crate::application::task_transition_service::TaskTransitionService;
use crate::domain::entities::{InternalStatus, Project, Task};
use crate::domain::repositories::TaskRepository;
use crate::domain::state_machine::transition_handler::{
    is_merge_worktree_path, merge_metadata_into, publish_plan_branch_pr_after_freshness_update,
    restore_task_worktree, sync_plan_branch_pr_after_regular_task_merge, PlanBranchPrSyncOutcome,
    PlanBranchPrSyncServices,
};
use crate::error::{AppError, AppResult};

/// Outcome returned by [`freshness_return_route`].
pub(crate) enum FreshnessRouteResult {
    /// Freshness intercept triggered — the task was routed back to its origin
    /// state. The contained string is the origin state name (e.g. `"reviewing"`).
    /// **Callers must return early** and not proceed with the normal merge path.
    FreshnessRouted(String),

    /// No freshness intercept needed — `plan_update_conflict` was absent or
    /// `false`. Callers should proceed with the normal merge pipeline.
    NormalMerge,
}

/// Shared freshness routing logic for merge completion.
///
/// Checks whether `plan_update_conflict=true` in the task metadata, indicating
/// the merger was resolving a plan←main freshness conflict rather than the
/// actual task→plan squash merge. If so, routes the task back to its origin
/// state (Reviewing → PendingReview, Executing → Ready) to prevent work loss.
///
/// Called from BOTH:
/// - `complete_merge` HTTP handler — primary bug fix (Bug 1)
/// - `attempt_merge_auto_complete` — secondary guard replacement
///
/// # Arguments
/// * `task` — Current task snapshot (used for initial `plan_update_conflict`
///   check and worktree path; the DB copy is re-read before mutation).
/// * `task_repo` — Repository for DB read-modify-write.
/// * `transition_service` — Service to transition the task to its origin state.
/// * `project` — Project providing the main repo path for worktree cleanup.
/// * `interactive_process_registry` — IPR for closing the merger agent.
///   `None` is allowed: logs a warning and skips IPR removal (agent times out).
///
/// # Errors
/// Returns `Err` if the DB update or task transition fails. On transition
/// failure the function re-inserts `plan_update_conflict` and
/// `branch_freshness_conflict` so the next attempt can retry.
pub(crate) async fn freshness_return_route(
    task: &Task,
    task_repo: Arc<dyn TaskRepository>,
    transition_service: &TaskTransitionService,
    project: &Project,
    interactive_process_registry: Option<&InteractiveProcessRegistry>,
    pr_sync_services: Option<&PlanBranchPrSyncServices>,
    commit_sha: Option<&str>,
) -> AppResult<FreshnessRouteResult> {
    // -----------------------------------------------------------------------
    // Step 1: Check plan_update_conflict in task metadata.
    // We use plan_update_conflict (NOT branch_freshness_conflict) because the
    // branch_freshness_conflict flag may have been cleared by set_source_conflict_resolved,
    // while plan_update_conflict is cleared only by this function.
    // -----------------------------------------------------------------------
    let initial_meta: serde_json::Value = task
        .metadata
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    let plan_update_conflict = initial_meta
        .get("plan_update_conflict")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // -----------------------------------------------------------------------
    // Step 2: Not a freshness-routed merge — proceed with normal merge path.
    // -----------------------------------------------------------------------
    if !plan_update_conflict {
        return Ok(FreshnessRouteResult::NormalMerge);
    }

    // -----------------------------------------------------------------------
    // Step 3: Determine target status from freshness_origin_state.
    // Defaults to PendingReview (review is safer — prevents work loss if the
    // original state was Reviewing; Ready would re-execute from scratch).
    // -----------------------------------------------------------------------
    let origin_state_opt = initial_meta
        .get("freshness_origin_state")
        .and_then(|v| v.as_str())
        .map(str::to_owned);

    let (target_status, origin_state_name) = match origin_state_opt.as_deref() {
        Some("executing") | Some("re_executing") => {
            let name = origin_state_opt
                .as_deref()
                .unwrap_or("executing")
                .to_owned();
            tracing::info!(
                task_id = %task.id,
                origin = %name,
                "freshness_return_route: routing to Ready (execution origin)"
            );
            (InternalStatus::Ready, name)
        }
        Some("reviewing") => {
            tracing::info!(
                task_id = %task.id,
                "freshness_return_route: routing to PendingReview (review origin)"
            );
            (InternalStatus::PendingReview, "reviewing".to_owned())
        }
        Some("waiting_on_pr") => {
            tracing::info!(
                task_id = %task.id,
                "freshness_return_route: routing back to WaitingOnPr (PR branch update origin)"
            );
            (InternalStatus::WaitingOnPr, "waiting_on_pr".to_owned())
        }
        Some("pr_branch_publication") => {
            tracing::info!(
                task_id = %task.id,
                "freshness_return_route: finalizing regular task after PR branch publication conflict"
            );
            (InternalStatus::Merged, "pr_branch_publication".to_owned())
        }
        Some(unknown) => {
            tracing::warn!(
                task_id = %task.id,
                origin = unknown,
                "freshness_return_route: unknown freshness_origin_state — defaulting to PendingReview"
            );
            (InternalStatus::PendingReview, unknown.to_owned())
        }
        None => {
            tracing::error!(
                task_id = %task.id,
                "freshness_return_route: freshness_origin_state absent — defaulting to PendingReview (review is safer: prevents work loss)"
            );
            (InternalStatus::PendingReview, "PendingReview".to_owned())
        }
    };

    // -----------------------------------------------------------------------
    // Step 4: Re-read task from DB for atomic read-modify-write.
    // Captures any metadata changes the merger agent wrote during its run.
    // -----------------------------------------------------------------------
    let mut fresh_task = match task_repo.get_by_id(&task.id).await? {
        Some(t) => t,
        None => {
            tracing::warn!(
                task_id = %task.id,
                "freshness_return_route: task not found in DB during metadata refresh"
            );
            return Err(AppError::NotFound(format!(
                "freshness_return_route: task {} not found",
                task.id
            )));
        }
    };

    let mut meta_val: serde_json::Value = fresh_task
        .metadata
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    if target_status == InternalStatus::WaitingOnPr {
        let publication_result = match pr_sync_services {
            Some(services) => {
                publish_plan_branch_pr_after_freshness_update(&fresh_task, project, services).await
            }
            None => Err(AppError::GitOperation(
                "Cannot publish PR branch during freshness return: PR sync services unavailable"
                    .to_string(),
            )),
        };

        if let Err(error) = publication_result {
            tracing::warn!(
                task_id = %task.id,
                error = %error,
                "freshness_return_route: PR branch publication failed before returning to WaitingOnPr"
            );
            merge_metadata_into(
                &mut fresh_task,
                &serde_json::json!({
                    "error": format!("PR branch publication failed: {}", error),
                    "error_code": "pr_branch_publication_failed",
                    "commit_sha": commit_sha,
                }),
            );
            fresh_task.internal_status = InternalStatus::MergeIncomplete;
            fresh_task.touch();
            task_repo.update(&fresh_task).await?;
            if let Ok(history_entry_id) = task_repo
                .persist_status_change(
                    &task.id,
                    InternalStatus::Merging,
                    InternalStatus::MergeIncomplete,
                    "pr_branch_publication_failed",
                )
                .await
            {
                transition_service
                    .notify_state_entered(
                        &fresh_task,
                        history_entry_id,
                        InternalStatus::MergeIncomplete,
                    )
                    .await;
            }
            return Err(error);
        }
    } else if origin_state_name == "pr_branch_publication" {
        if let (Some(commit_sha), Some(target_branch)) = (
            commit_sha,
            meta_val.get("target_branch").and_then(|v| v.as_str()),
        ) {
            let repo_path = PathBuf::from(&project.working_directory);
            let commit_on_target =
                GitService::is_commit_on_branch(&repo_path, commit_sha, target_branch).await?;
            if !commit_on_target {
                return Err(AppError::Validation(format!(
                    "Commit {} is not on PR publication target branch {}",
                    commit_sha, target_branch
                )));
            }
        }

        let publication_result = match pr_sync_services {
            Some(services) => {
                match sync_plan_branch_pr_after_regular_task_merge(&fresh_task, project, services)
                    .await
                {
                    Ok(PlanBranchPrSyncOutcome::Complete) => Ok(()),
                    Ok(PlanBranchPrSyncOutcome::Conflict(conflict)) => {
                        Err(conflict.conflict_error())
                    }
                    Err(error) => Err(error),
                }
            }
            None => Err(AppError::GitOperation(
                "Cannot publish PR branch during publication conflict return: PR sync services unavailable"
                    .to_string(),
            )),
        };

        if let Err(error) = publication_result {
            tracing::warn!(
                task_id = %task.id,
                error = %error,
                "freshness_return_route: PR branch publication failed before finalizing regular task"
            );
            merge_metadata_into(
                &mut fresh_task,
                &serde_json::json!({
                    "error": format!("PR branch publication failed: {}", error),
                    "error_code": "pr_branch_publication_failed",
                    "commit_sha": commit_sha,
                }),
            );
            fresh_task.internal_status = InternalStatus::MergeIncomplete;
            fresh_task.touch();
            task_repo.update(&fresh_task).await?;
            if let Ok(history_entry_id) = task_repo
                .persist_status_change(
                    &task.id,
                    InternalStatus::Merging,
                    InternalStatus::MergeIncomplete,
                    "pr_branch_publication_failed",
                )
                .await
            {
                transition_service
                    .notify_state_entered(
                        &fresh_task,
                        history_entry_id,
                        InternalStatus::MergeIncomplete,
                    )
                    .await;
            }
            return Err(error);
        }

        let final_commit_sha = match commit_sha {
            Some(sha) => Some(sha.to_string()),
            None => {
                let target_branch = meta_val.get("target_branch").and_then(|v| v.as_str());
                match target_branch {
                    Some(branch) => {
                        GitService::get_branch_sha(Path::new(&project.working_directory), branch)
                            .await
                            .ok()
                    }
                    None => meta_val
                        .get("commit_sha")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned),
                }
            }
        };
        if let Some(sha) = final_commit_sha {
            fresh_task.merge_commit_sha = Some(sha);
        }
    }

    // -----------------------------------------------------------------------
    // Step 4 (continued): Targeted metadata cleanup BEFORE transition.
    //
    // Remove ONLY the routing-trigger flags. We do NOT use
    // FreshnessCleanupScope::RoutingOnly here because that scope clears ALL
    // routing flags (including freshness_origin_state) atomically — if the
    // subsequent transition_task() call fails we would have no way to retry.
    //
    // Strategy: remove the minimum set of fields, then re-insert on failure.
    // - plan_update_conflict → remove (routing trigger)
    // - branch_freshness_conflict → remove (prevents stale flag from triggering
    //   redundant freshness cycle when on_enter of origin state calls
    //   ensure_branches_fresh())
    // - freshness_backoff_until → remove (stale after conflict resolution)
    // - freshness_origin_state and other fields → leave intact for audit/debug
    // -----------------------------------------------------------------------
    if let Some(obj) = meta_val.as_object_mut() {
        obj.remove("plan_update_conflict");
        obj.remove("pr_branch_update_conflict");
        obj.remove("pr_branch_publication_conflict");
        obj.remove("pr_branch_update_source");
        obj.remove("branch_freshness_conflict");
        obj.remove("freshness_backoff_until");
        if origin_state_name == "pr_branch_publication" {
            obj.remove("error");
            obj.remove("error_code");
            obj.remove("publication_remote_ref");
            obj.remove("conflict_files");
            obj.insert("pending_cleanup".to_owned(), serde_json::json!(true));
        }
    }

    if matches!(target_status, InternalStatus::Ready)
        && fresh_task
            .worktree_path
            .as_deref()
            .map(is_merge_worktree_path)
            .unwrap_or(false)
    {
        let stale_path = fresh_task.worktree_path.clone().unwrap_or_default();
        match restore_task_worktree(
            &mut fresh_task,
            project,
            Path::new(&project.working_directory),
        )
        .await
        {
            Ok(restored) => {
                tracing::info!(
                    task_id = %task.id,
                    restored_path = %restored.display(),
                    stale_path,
                    "freshness_return_route: restored stale merge worktree before execution return"
                );
            }
            Err(e) => {
                tracing::warn!(
                    task_id = %task.id,
                    error = %e,
                    stale_path,
                    "freshness_return_route: failed to restore stale merge worktree before execution return — clearing worktree_path for execution self-heal"
                );
                fresh_task.worktree_path = None;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Step 5: Persist metadata update to DB.
    // -----------------------------------------------------------------------
    fresh_task.metadata = Some(meta_val.to_string());
    fresh_task.touch();

    if let Err(e) = task_repo.update(&fresh_task).await {
        tracing::error!(
            task_id = %task.id,
            error = %e,
            "freshness_return_route: failed to persist metadata cleanup"
        );
        return Err(e);
    }

    // -----------------------------------------------------------------------
    // Step 6: Transition the task back to its origin state.
    // -----------------------------------------------------------------------
    let transition_result = transition_service
        .transition_task_corrective_with_exit(&task.id, target_status, None, "system")
        .await;

    let routed_task = match transition_result {
        Ok(task) => task,
        Err(e) => {
            // -----------------------------------------------------------------------
            // Step 8: Transition failed — re-insert routing flags so the next
            // invocation can retry. We intentionally do NOT propagate a re-insert
            // failure (best-effort: if this also fails we log and move on).
            // -----------------------------------------------------------------------
            tracing::error!(
                task_id = %task.id,
                error = %e,
                target = ?target_status,
                "freshness_return_route: transition failed — re-inserting routing flags for retry"
            );

            if let Ok(Some(mut rollback_task)) = task_repo.get_by_id(&task.id).await {
                let mut rollback_meta: serde_json::Value = rollback_task
                    .metadata
                    .as_deref()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or_else(|| serde_json::json!({}));

                if let Some(obj) = rollback_meta.as_object_mut() {
                    obj.insert(
                        "plan_update_conflict".to_owned(),
                        serde_json::Value::Bool(true),
                    );
                    obj.insert(
                        "branch_freshness_conflict".to_owned(),
                        serde_json::Value::Bool(true),
                    );
                    if target_status == InternalStatus::WaitingOnPr
                        || origin_state_name == "pr_branch_publication"
                    {
                        obj.insert(
                            "pr_branch_update_conflict".to_owned(),
                            serde_json::Value::Bool(true),
                        );
                    }
                    if origin_state_name == "pr_branch_publication" {
                        obj.insert(
                            "pr_branch_publication_conflict".to_owned(),
                            serde_json::Value::Bool(true),
                        );
                    }
                }

                rollback_task.metadata = Some(rollback_meta.to_string());
                rollback_task.touch();
                if let Err(re_err) = task_repo.update(&rollback_task).await {
                    tracing::warn!(
                        task_id = %task.id,
                        error = %re_err,
                        "freshness_return_route: failed to re-insert routing flags (best-effort)"
                    );
                }
            } else {
                tracing::warn!(
                    task_id = %task.id,
                    "freshness_return_route: could not re-read task for routing flag rollback"
                );
            }

            return Err(e);
        }
    };

    if target_status == InternalStatus::WaitingOnPr {
        transition_service
            .execute_entry_actions(&task.id, &routed_task, target_status)
            .await;
    }

    // -----------------------------------------------------------------------
    // Step 7: Transition succeeded — clean up merge worktree and close IPR.
    // -----------------------------------------------------------------------

    // Worktree cleanup (idempotent — safe if worktree does not exist).
    if let Some(ref worktree_path_str) = task.worktree_path {
        let repo_path = PathBuf::from(&project.working_directory);
        let worktree_path = PathBuf::from(worktree_path_str);
        if let Err(e) = GitService::delete_worktree(&repo_path, &worktree_path).await {
            // Non-fatal: log and continue. The worktree may already be gone or
            // may be cleaned up by the next git worktree prune.
            tracing::warn!(
                task_id = %task.id,
                error = %e,
                "freshness_return_route: failed to delete merge worktree (non-fatal)"
            );
        }
    }

    // Close IPR entry to stop the running merger agent by closing its stdin pipe.
    match interactive_process_registry {
        Some(ipr) => {
            let ipr_key = InteractiveProcessKey::new("merge", task.id.as_str());
            ipr.remove(&ipr_key).await;
        }
        None => {
            tracing::warn!(
                task_id = %task.id,
                "freshness_return_route: no InteractiveProcessRegistry provided — merger agent will time out naturally"
            );
        }
    }

    tracing::info!(
        task_id = %task.id,
        origin_state = %origin_state_name,
        "freshness_return_route: task successfully routed back to origin state"
    );

    let routed_state_name = if target_status == InternalStatus::Merged {
        "merged".to_owned()
    } else {
        origin_state_name
    };

    Ok(FreshnessRouteResult::FreshnessRouted(routed_state_name))
}
