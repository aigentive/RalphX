use super::*;
use crate::domain::state_machine::TransitionHandler;
use crate::domain::state_machine::transition_handler::{merge_helpers, TaskCore};

impl<'a> TransitionHandler<'a> {
    /// Persist a merge status transition: touch -> update -> persist_status_change -> emit.
    ///
    /// Callers must set `task.metadata` and `task.internal_status` before calling.
    /// Returns `false` if the update failed (caller should return early).
    pub(super) async fn persist_merge_transition(
        &self,
        tc: TaskCore<'_>,
        from_status: InternalStatus,
        to_status: InternalStatus,
        persist_label: &str,
    ) -> bool {
        let (task, task_id, task_id_str, task_repo) = (tc.task, tc.task_id, tc.task_id_str, tc.task_repo);
        task.touch();

        if let Err(e) = task_repo.update(task).await {
            tracing::error!(error = %e, "Failed to update task to {:?} status", to_status);
            return false;
        }

        let from_str = serde_json::to_value(from_status)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| format!("{:?}", from_status));
        let to_str = serde_json::to_value(to_status)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| format!("{:?}", to_status));

        if let Err(e) = task_repo
            .persist_status_change(task_id, from_status, to_status, persist_label)
            .await
        {
            tracing::warn!(error = %e, "Failed to record {} transition (non-fatal)", persist_label);
        }

        self.machine
            .context
            .services
            .event_emitter
            .emit_status_change(task_id_str, &from_str, &to_str)
            .await;

        true
    }

    /// Transition a task to MergeIncomplete with the given metadata JSON.
    ///
    /// Merges `metadata` INTO the task's existing metadata so that recovery history
    /// (e.g., `merge_recovery` events, `validation_revert_count`, attempt counters)
    /// is preserved across MergeIncomplete->PendingMerge retry cycles.
    ///
    /// Handles the full transition: merge metadata -> persist status change -> emit event.
    /// Optionally triggers on_exit (needed when the caller wants deferred-merge retry).
    pub(in crate::domain::state_machine::transition_handler) async fn transition_to_merge_incomplete(
        &self,
        tc: TaskCore<'_>,
        metadata: serde_json::Value,
        trigger_on_exit: bool,
    ) {
        let (task, task_id, task_id_str, task_repo) = (tc.task, tc.task_id, tc.task_id_str, tc.task_repo);
        // Merge new metadata INTO existing metadata to preserve recovery history
        merge_helpers::merge_metadata_into(task, &metadata);
        task.internal_status = InternalStatus::MergeIncomplete;

        if !self.persist_merge_transition(
            TaskCore { task: &mut *task, task_id, task_id_str, task_repo },
            InternalStatus::PendingMerge, InternalStatus::MergeIncomplete,
            "merge_incomplete",
        ).await {
            return;
        }

        if trigger_on_exit {
            self.on_exit(&State::PendingMerge, &State::MergeIncomplete)
                .await;
        }
    }

    /// Post-merge cleanup: update plan branch status, delete feature branch, unblock dependents.
    ///
    /// Shared between all merge strategy success paths in `attempt_programmatic_merge()`.
    pub(in crate::domain::state_machine::transition_handler) async fn post_merge_cleanup(
        &self,
        task_id_str: &str,
        task_id: &TaskId,
        repo_path: &Path,
        plan_branch_repo: &Option<Arc<dyn PlanBranchRepository>>,
    ) {
        let app_handle = self.machine.context.services.app_handle.as_ref();

        if let Some(ref plan_branch_repo) = plan_branch_repo {
            if let Ok(Some(pb)) = plan_branch_repo.get_by_merge_task_id(task_id).await {
                // Idempotency guard: if already merged, return early (AD20)
                if pb.status == crate::domain::entities::PlanBranchStatus::Merged {
                    tracing::debug!(
                        task_id = task_id_str,
                        "post_merge_cleanup: plan branch already merged, skipping (idempotency guard)"
                    );
                    return;
                }
                if let Err(e) = plan_branch_repo.set_merged(&pb.id).await {
                    tracing::warn!(
                        error = %e,
                        task_id = task_id_str,
                        plan_branch_id = pb.id.as_str(),
                        "Failed to mark plan branch as merged (non-fatal)"
                    );
                }

                // PR mode: GitHub may have auto-deleted the plan branch; use delete_remote_branch
                // (idempotent — already-deleted is treated as no-op). Push-to-main: delete local.
                let pr_mode = pb.pr_eligible
                    && pb.pr_number.is_some()
                    && self.machine.context.services.github_service.is_some();

                if pr_mode {
                    if let Some(ref github) = self.machine.context.services.github_service {
                        if let Err(e) = github.delete_remote_branch(repo_path, &pb.branch_name).await {
                            tracing::warn!(
                                error = %e,
                                task_id = task_id_str,
                                branch = %pb.branch_name,
                                "PR mode: failed to delete remote plan branch (non-fatal)"
                            );
                        } else {
                            tracing::info!(
                                task_id = task_id_str,
                                branch = %pb.branch_name,
                                "PR mode: deleted remote plan branch"
                            );
                        }
                    }
                    if let Some(ref project_repo) = self.machine.context.services.project_repo {
                        match project_repo.get_by_id(&pb.project_id).await {
                            Ok(Some(project)) => {
                                let mut cleanup_pb = pb.clone();
                                cleanup_pb.status =
                                    crate::domain::entities::PlanBranchStatus::Merged;
                                match crate::application::git_artifact_cleanup::cleanup_merged_plan_branch_local_artifacts(
                                    &project,
                                    &cleanup_pb,
                                )
                                .await
                                {
                                    Ok(report) if report.branch_deleted => {
                                        tracing::info!(task_id = task_id_str, branch = %pb.branch_name, "PR mode: deleted local plan branch after merge");
                                    }
                                    Ok(report) => {
                                        tracing::debug!(task_id = task_id_str, branch = %pb.branch_name, skipped_reason = report.skipped_reason.as_deref(), "PR mode: skipped local plan branch cleanup");
                                    }
                                    Err(e) => {
                                        tracing::warn!(task_id = task_id_str, branch = %pb.branch_name, error = %e, "PR mode: failed local plan branch cleanup (non-fatal)");
                                    }
                                }
                            }
                            Ok(None) => {
                                tracing::debug!(task_id = task_id_str, project_id = pb.project_id.as_str(), "PR mode: skipped local plan branch cleanup because project was not found");
                            }
                            Err(e) => {
                                tracing::warn!(task_id = task_id_str, project_id = pb.project_id.as_str(), error = %e, "PR mode: failed to load project for local plan branch cleanup");
                            }
                        }
                    }
                } else if let Err(e) = GitService::delete_feature_branch(repo_path, &pb.branch_name).await {
                    tracing::warn!(
                        error = %e,
                        task_id = task_id_str,
                        branch = %pb.branch_name,
                        "Failed to delete feature branch after merge (non-fatal)"
                    );
                } else {
                    tracing::info!(
                        task_id = task_id_str,
                        branch = %pb.branch_name,
                        "Deleted feature branch after plan merge"
                    );
                }

                if let Some(handle) = app_handle {
                    let _ = handle.emit(
                        "plan:merge_complete",
                        serde_json::json!({
                            "plan_artifact_id": pb.plan_artifact_id.as_str(),
                            "plan_branch_id": pb.id.as_str(),
                            "merge_task_id": task_id_str,
                            "branch_name": pb.branch_name,
                        }),
                    );
                }

                // Cascade stop: cancel/stop sibling tasks in the same plan
                self.cascade_stop_sibling_tasks(task_id, task_id_str, &pb)
                    .await;

                // PR mode: delete remote task branches for all plan siblings
                if pr_mode {
                    if let (Some(ref github), Some(ref task_repo)) = (
                        &self.machine.context.services.github_service,
                        &self.machine.context.services.task_repo,
                    ) {
                        self.delete_remote_task_branches(
                            task_id,
                            task_id_str,
                            &pb,
                            task_repo,
                            github.as_ref(),
                            repo_path,
                        )
                        .await;
                    }
                }
            }
        }

        self.machine
            .context
            .services
            .dependency_manager
            .unblock_dependents(task_id_str)
            .await;

        if let Some(ref scheduler) = self.machine.context.services.task_scheduler {
            let scheduler = Arc::clone(scheduler);
            let merge_settle_ms = scheduler_config().merge_settle_ms;
            tokio::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_millis(merge_settle_ms)).await;
                scheduler.try_schedule_ready_tasks().await;
            });
        }
    }

    /// Cascade stop sibling tasks after a plan merge completes.
    ///
    /// Uses `execution_plan_id` from the PlanBranch to find siblings (precise).
    /// Falls back to `ideation_session_id` if `execution_plan_id` is not set.
    /// Transitions non-terminal siblings to Stopped or Cancelled.
    /// The merge task itself is excluded from cascade.
    ///
    /// For states that don't have a valid transition to Stopped or Cancelled
    /// (e.g., QaPassed, PendingReview, ReviewPassed, Escalated, Approved),
    /// we force the transition via `persist_status_change` — this is an
    /// emergency cascade, not normal state machine flow.
    pub(in crate::domain::state_machine::transition_handler) async fn cascade_stop_sibling_tasks(
        &self,
        merge_task_id: &TaskId,
        merge_task_id_str: &str,
        plan_branch: &PlanBranch,
    ) {
        let Some(ref task_repo) = self.machine.context.services.task_repo else {
            return;
        };

        // Query siblings by execution_plan_id (precise) or fall back to session_id
        let siblings = if let Some(ref ep_id) = plan_branch.execution_plan_id {
            match task_repo
                .list_paginated(
                    &plan_branch.project_id,
                    None,  // all statuses
                    0,
                    10_000, // large limit to get all
                    false,  // exclude archived
                    None,   // no session filter
                    Some(ep_id.as_str()),
                    None,   // no category filter
                )
                .await
            {
                Ok(tasks) => tasks,
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        merge_task_id = merge_task_id_str,
                        execution_plan_id = ep_id.as_str(),
                        "Failed to query sibling tasks by execution_plan_id for cascade stop"
                    );
                    return;
                }
            }
        } else {
            match task_repo
                .get_by_ideation_session(&plan_branch.session_id)
                .await
            {
                Ok(tasks) => tasks,
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        merge_task_id = merge_task_id_str,
                        session_id = plan_branch.session_id.as_str(),
                        "Failed to query sibling tasks by session_id for cascade stop"
                    );
                    return;
                }
            }
        };

        let mut stopped_count = 0u32;
        for sibling in &siblings {
            // Skip the merge task itself
            if sibling.id == *merge_task_id {
                continue;
            }

            // Skip already-terminal tasks
            if sibling.internal_status.is_terminal() {
                continue;
            }

            let from = sibling.internal_status;

            // Choose target state:
            // - States with valid → Stopped: use Stopped
            // - States with valid → Cancelled but not Stopped: use Cancelled
            // - States with neither (QaPassed, PendingReview, ReviewPassed, etc.):
            //   force Stopped via persist_status_change (emergency cascade)
            let to = if from.can_transition_to(InternalStatus::Stopped) {
                InternalStatus::Stopped
            } else if from.can_transition_to(InternalStatus::Cancelled) {
                InternalStatus::Cancelled
            } else {
                // Force stop — these states (QaPassed, PendingReview, ReviewPassed,
                // Escalated, Approved, QaFailed) don't have valid exit transitions
                // to Stopped/Cancelled, but we must stop them to prevent execution
                // on a merged branch.
                tracing::warn!(
                    task_id = sibling.id.as_str(),
                    from = %from,
                    merge_task_id = merge_task_id_str,
                    "Force-stopping sibling in state with no valid Stopped/Cancelled transition"
                );
                InternalStatus::Stopped
            };

            // Stop poller BEFORE status change (AD11: TOCTOU prevention).
            // stop_polling is synchronous fire-and-forget — abort never panics.
            if let Some(ref registry) = self.machine.context.services.pr_poller_registry {
                registry.stop_polling(&sibling.id);
                tracing::debug!(
                    task_id = sibling.id.as_str(),
                    "cascade_stop: stopped poller for sibling before persist_status_change"
                );
            }

            // PR-mode running count fix: cascade_stop uses persist_status_change (bypasses
            // on_exit hooks). For PR-mode Merging siblings, increment_running() was called in
            // on_enter(Merging) — decrement explicitly here to prevent u32::MAX underflow (AD11).
            if from == InternalStatus::Merging && plan_branch.pr_number.is_some() {
                if let Some(ref exec) = self.machine.context.services.execution_state {
                    exec.decrement_running();
                    tracing::debug!(
                        task_id = sibling.id.as_str(),
                        "cascade_stop: decremented running count for PR-mode Merging sibling"
                    );
                }
            }

            if let Err(e) = task_repo
                .persist_status_change(
                    &sibling.id,
                    from,
                    to,
                    "post_merge_cascade_stop",
                )
                .await
            {
                tracing::warn!(
                    error = %e,
                    task_id = sibling.id.as_str(),
                    from = %from,
                    to = %to,
                    "Failed to cascade-stop sibling task"
                );
            } else {
                stopped_count += 1;
                tracing::info!(
                    task_id = sibling.id.as_str(),
                    from = %from,
                    to = %to,
                    merge_task_id = merge_task_id_str,
                    "Cascade-stopped sibling task after plan merge"
                );
            }
        }

        if stopped_count > 0 {
            tracing::info!(
                merge_task_id = merge_task_id_str,
                stopped_count,
                "Post-merge cascade stop complete — agents may still be running for stopped tasks"
            );
        }
    }

    /// Delete remote task branches for all siblings in a plan after PR-mode merge.
    ///
    /// Called from `post_merge_cleanup` in PR mode only.
    /// Each task has a `task_branch` field (e.g. `ralphx/ralphx/task-xxx`).
    /// Failures are non-fatal: logged as warn, cleanup continues for remaining branches.
    async fn delete_remote_task_branches(
        &self,
        merge_task_id: &TaskId,
        merge_task_id_str: &str,
        plan_branch: &PlanBranch,
        task_repo: &Arc<dyn crate::domain::repositories::TaskRepository>,
        github: &dyn GithubServiceTrait,
        repo_path: &Path,
    ) {
        let siblings = if let Some(ref ep_id) = plan_branch.execution_plan_id {
            match task_repo
                .list_paginated(
                    &plan_branch.project_id,
                    None,
                    0,
                    10_000,
                    false,
                    None,
                    Some(ep_id.as_str()),
                    None,
                )
                .await
            {
                Ok(tasks) => tasks,
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        merge_task_id = merge_task_id_str,
                        "PR mode: failed to query siblings for task branch cleanup"
                    );
                    return;
                }
            }
        } else {
            match task_repo.get_by_ideation_session(&plan_branch.session_id).await {
                Ok(tasks) => tasks,
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        merge_task_id = merge_task_id_str,
                        "PR mode: failed to query siblings by session for task branch cleanup"
                    );
                    return;
                }
            }
        };

        for sibling in &siblings {
            if sibling.id == *merge_task_id {
                continue;
            }
            let Some(ref branch) = sibling.task_branch else {
                continue;
            };
            if let Err(e) = github.delete_remote_branch(repo_path, branch).await {
                tracing::warn!(
                    error = %e,
                    task_id = sibling.id.as_str(),
                    branch = %branch,
                    merge_task_id = merge_task_id_str,
                    "PR mode: failed to delete remote task branch (non-fatal)"
                );
            } else {
                tracing::info!(
                    task_id = sibling.id.as_str(),
                    branch = %branch,
                    merge_task_id = merge_task_id_str,
                    "PR mode: deleted remote task branch"
                );
            }
        }
    }
    /// Emit a system ActivityEvent for a merge pipeline phase.
    ///
    /// Non-fatal: if the repo is unavailable or the save fails, logs a warning and continues.
    pub(super) async fn emit_merge_activity_event(
        &self,
        task_id_str: &str,
        content: impl Into<String>,
        phase_id: &str,
        result: &str,
    ) {
        let Some(ref repo) = self.machine.context.services.activity_event_repo else {
            return;
        };
        let tid = TaskId::from_string(task_id_str.to_string());
        let metadata = serde_json::json!({ "phase_id": phase_id, "result": result }).to_string();
        let event = ActivityEvent::new_task_event(tid, ActivityEventType::System, content)
            .with_role(ActivityEventRole::System)
            .with_metadata(metadata);
        if let Err(e) = repo.save(event).await {
            tracing::warn!(
                task_id = task_id_str,
                phase_id = phase_id,
                error = %e,
                "Failed to save merge activity event (non-fatal)"
            );
        }
    }
}
