use super::super::super::merge_helpers::restore_github_auto_merge_after_pr_correction;
use super::*;
use crate::application::publish_resilience::push_publish_branch;
use crate::domain::entities::plan_branch::PrPushStatus;
use crate::domain::entities::task_metadata::{
    MergeRecoveryEvent, MergeRecoveryEventKind, MergeRecoveryMetadata, MergeRecoveryReasonCode,
    MergeRecoverySource, MergeRecoveryState,
};
use crate::domain::services::{PlanPrPublisher, PrReviewState};
use crate::domain::state_machine::transition_handler::{
    draft_plan_pr_description_for_write, merge_outcome_handler::classify_merge_failure_source,
    resolve_plan_branch_pr_base, TaskCore,
};
use crate::domain::state_machine::{State, TransitionHandler};

impl<'a> TransitionHandler<'a> {
    /// PR-mode PendingMerge path (AD17).
    ///
    /// Called when pr_eligible=true AND github_service is available.
    /// Pushes the plan branch and marks the PR ready, then transitions to WaitingOnPr.
    /// The WaitingOnPr entry point starts the PR poller.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::domain::state_machine::transition_handler::side_effects) async fn run_pr_mode_pending_merge(
        &self,
        task: &mut Task,
        project: &Project,
        plan_branch: PlanBranch,
        task_id: TaskId,
        task_id_str: &str,
        task_repo: &Arc<dyn TaskRepository>,
        github_service: &Arc<dyn GithubServiceTrait>,
        plan_branch_repo: &Arc<dyn PlanBranchRepository>,
    ) {
        tracing::info!(task_id = task_id_str, pr_number = ?plan_branch.pr_number, "PR-mode PendingMerge: starting PR path");

        // 1. Clear stale local-mode metadata (defense-in-depth for prior failed local attempts)
        {
            let mut changed = false;
            if let Some(ref meta_str) = task.metadata.clone() {
                if let Ok(mut meta_json) = serde_json::from_str::<serde_json::Value>(meta_str) {
                    if let Some(obj) = meta_json.as_object_mut() {
                        changed = obj.remove("merge_pipeline_active").is_some()
                            | obj.remove("validation_in_progress").is_some()
                            | obj.remove("merge_retry_in_progress").is_some();
                    }
                    if changed {
                        task.metadata = Some(meta_json.to_string());
                        task.touch();
                        let _ = task_repo.update(task).await;
                    }
                }
            }
        }

        // 2. Run concurrent merge guard with PR exclusion (AD14)
        // PR-polling tasks are excluded from the blocking check (they wait for GitHub, not local pipeline)
        let pb_repo_opt: Option<Arc<dyn PlanBranchRepository>> = Some(Arc::clone(plan_branch_repo));
        let target_branch = resolve_plan_branch_pr_base(project, &plan_branch);
        if matches!(
            self.run_concurrent_merge_guard(
                task,
                task_id_str,
                &target_branch,
                project,
                task_repo,
                &pb_repo_opt
            )
            .await,
            ConcurrentGuardResult::Deferred
        ) {
            return;
        }

        // 3. Re-entry guard: if already polling and poller is alive, return (no-op)
        if plan_branch.pr_polling_active {
            if let Some(ref registry) = self.machine.context.services.pr_poller_registry {
                if registry.is_polling(&task_id) {
                    tracing::info!(
                        task_id = task_id_str,
                        "PR-mode PendingMerge: re-entry guard — already polling, skipping"
                    );
                    return;
                }
            }
        }

        // 4. Set pr_polling_active = true in DB
        // update_last_polled_at also sets pr_polling_active = 1 in the same SQL statement
        if let Err(e) = plan_branch_repo
            .update_last_polled_at(&plan_branch.id, chrono::Utc::now())
            .await
        {
            tracing::warn!(task_id = task_id_str, error = %e, "PR-mode: failed to set pr_polling_active (non-fatal)");
        }

        // 5. Get working directory
        let working_dir = std::path::PathBuf::from(&project.working_directory);
        let branch_name = plan_branch.branch_name.clone();

        // 5.5. Draft PR description via describer agent before any PR body write.
        let pr_description_result = draft_plan_pr_description_for_write(
            project,
            &plan_branch,
            self.machine
                .context
                .services
                .plan_pr_description_drafter
                .as_ref(),
            PrReviewState::Ready,
        )
        .await;

        // 6. Perform PR operation: push branch and mark PR ready (or create PR if missing)
        let pr_op_result: Result<i64, crate::error::AppError> =
            if let Err(e) = pr_description_result.as_ref() {
                Err(crate::error::AppError::Infrastructure(format!(
                    "failed to draft plan PR description: {e}"
                )))
            } else if let Some(existing_pr_number) = plan_branch.pr_number {
                let pr_description = pr_description_result
                    .as_ref()
                    .expect("description result was checked above");
                // Has PR: push latest commits then mark ready
                tracing::info!(
                    task_id = task_id_str,
                    pr_number = existing_pr_number,
                    "PR-mode: pushing branch and marking PR ready"
                );
                match push_publish_branch(github_service, &working_dir, &branch_name).await {
                    Err(e) => Err(e),
                    Ok(_) => {
                        let _ = plan_branch_repo
                            .update_pr_push_status(&plan_branch.id, PrPushStatus::Pushed)
                            .await;
                        let publisher = PlanPrPublisher::new(
                            github_service,
                            self.machine.context.services.ideation_session_repo.as_ref(),
                            self.machine.context.services.artifact_repo.as_ref(),
                        );
                        match publisher
                            .sync_existing_pr(
                                task,
                                project,
                                &plan_branch,
                                PrReviewState::Ready,
                                pr_description,
                            )
                            .await
                        {
                            Ok(()) => github_service
                                .mark_pr_ready(&working_dir, existing_pr_number)
                                .await
                                .map(|_| existing_pr_number),
                            Err(e) => Err(e),
                        }
                    }
                }
            } else {
                // No PR yet: wait for CAS guard to clear (AD15), re-read, then create if still missing
                tracing::info!(
                    task_id = task_id_str,
                    "PR-mode: no pr_number, waiting for CAS guard (AD15)"
                );
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                while std::time::Instant::now() < deadline {
                    if let Some(ref registry) = self.machine.context.services.pr_poller_registry {
                        if !registry.pr_creation_guard.contains_key(&plan_branch.id) {
                            break; // guard cleared — draft PR may have been created concurrently
                        }
                    } else {
                        break;
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }

                // Re-read pr_number from DB
                let refreshed_pr_number = plan_branch_repo
                    .get_by_merge_task_id(&task_id)
                    .await
                    .ok()
                    .flatten()
                    .and_then(|pb| pb.pr_number);

                if let Some(new_pr_number) = refreshed_pr_number {
                    let pr_description = pr_description_result
                        .as_ref()
                        .expect("description result was checked above");
                    // Concurrent creation succeeded: push + mark ready
                    tracing::info!(
                        task_id = task_id_str,
                        pr_number = new_pr_number,
                        "PR-mode: found PR from concurrent creation"
                    );
                    match push_publish_branch(github_service, &working_dir, &branch_name).await {
                        Err(e) => Err(e),
                        Ok(_) => {
                            let _ = plan_branch_repo
                                .update_pr_push_status(&plan_branch.id, PrPushStatus::Pushed)
                                .await;
                            let mut refreshed_plan_branch = plan_branch.clone();
                            refreshed_plan_branch.pr_number = Some(new_pr_number);
                            let publisher = PlanPrPublisher::new(
                                github_service,
                                self.machine.context.services.ideation_session_repo.as_ref(),
                                self.machine.context.services.artifact_repo.as_ref(),
                            );
                            match publisher
                                .sync_existing_pr(
                                    task,
                                    project,
                                    &refreshed_plan_branch,
                                    PrReviewState::Ready,
                                    pr_description,
                                )
                                .await
                            {
                                Ok(()) => github_service
                                    .mark_pr_ready(&working_dir, new_pr_number)
                                    .await
                                    .map(|_| new_pr_number),
                                Err(e) => Err(e),
                            }
                        }
                    }
                } else {
                    let pr_description = pr_description_result
                        .as_ref()
                        .expect("description result was checked above");
                    // Still no PR: push + create non-draft PR directly
                    tracing::info!(
                        task_id = task_id_str,
                        "PR-mode: creating non-draft PR directly"
                    );
                    match push_publish_branch(github_service, &working_dir, &branch_name).await {
                        Err(e) => Err(e),
                        Ok(_) => {
                            let publisher = PlanPrPublisher::new(
                                github_service,
                                self.machine.context.services.ideation_session_repo.as_ref(),
                                self.machine.context.services.artifact_repo.as_ref(),
                            );
                            match publisher
                                .create_draft_pr(task, project, &plan_branch, pr_description)
                                .await
                            {
                                Err(e) => Err(e),
                                Ok((new_pr_number, pr_url)) => {
                                    let _ = plan_branch_repo
                                        .update_pr_info(
                                            &plan_branch.id,
                                            new_pr_number,
                                            pr_url,
                                            crate::domain::entities::plan_branch::PrStatus::Open,
                                            true,
                                        )
                                        .await;

                                    let mut ready_plan_branch = plan_branch.clone();
                                    ready_plan_branch.pr_number = Some(new_pr_number);
                                    match publisher
                                        .sync_existing_pr(
                                            task,
                                            project,
                                            &ready_plan_branch,
                                            PrReviewState::Ready,
                                            pr_description,
                                        )
                                        .await
                                    {
                                        Ok(()) => github_service
                                            .mark_pr_ready(&working_dir, new_pr_number)
                                            .await
                                            .map(|_| new_pr_number),
                                        Err(e) => Err(e),
                                    }
                                }
                            }
                        }
                    }
                }
            };

        let pr_op_result = match pr_op_result {
            Ok(pr_number) => restore_github_auto_merge_after_pr_correction(
                task,
                task_id_str,
                &working_dir,
                pr_number,
                task_repo,
                github_service,
            )
            .await
            .map(|()| pr_number),
            Err(error) => Err(error),
        };

        // 7. Handle result
        match pr_op_result {
            Ok(_pr_number) => {
                // Success: transition PendingMerge → WaitingOnPr.
                tracing::info!(
                    task_id = task_id_str,
                    "PR-mode: success, transitioning to WaitingOnPr"
                );
                task.internal_status = InternalStatus::WaitingOnPr;
                if self
                    .persist_merge_transition(
                        TaskCore {
                            task: &mut *task,
                            task_id: &task_id,
                            task_id_str,
                            task_repo,
                        },
                        InternalStatus::PendingMerge,
                        InternalStatus::WaitingOnPr,
                        "pr_mode_mark_ready",
                    )
                    .await
                {
                    // Trigger on_enter(WaitingOnPr) to start the PR poller.
                    if let Err(e) = Box::pin(self.on_enter_dispatch(&State::WaitingOnPr)).await {
                        tracing::error!(task_id = task_id_str, error = %e, "on_enter(WaitingOnPr) failed in PR mode");
                    }
                }
            }
            Err(e) => {
                // Failure: clear pr_polling_active, transition to MergeIncomplete
                tracing::warn!(task_id = task_id_str, error = %e, "PR-mode: operation failed, transitioning to MergeIncomplete");
                let _ = plan_branch_repo
                    .clear_polling_active_by_task(&task_id)
                    .await;
                let _ = plan_branch_repo
                    .update_pr_push_status(&plan_branch.id, PrPushStatus::Failed)
                    .await;
                let failure_source = classify_merge_failure_source(&e);
                let mut recovery =
                    MergeRecoveryMetadata::from_task_metadata(task.metadata.as_deref())
                        .unwrap_or(None)
                        .unwrap_or_else(MergeRecoveryMetadata::new);
                let attempt = recovery.active_attempt();
                let event = MergeRecoveryEvent::new(
                    MergeRecoveryEventKind::AttemptFailed,
                    MergeRecoverySource::System,
                    MergeRecoveryReasonCode::GitError,
                    format!("PR operation failed: {}", e),
                )
                .with_source_branch(&branch_name)
                .with_target_branch(&target_branch)
                .with_attempt(attempt)
                .with_failure_source(failure_source);
                recovery.append_event_with_state(event, MergeRecoveryState::Failed);
                match recovery.update_task_metadata(task.metadata.as_deref()) {
                    Ok(updated_json) => task.metadata = Some(updated_json),
                    Err(metadata_error) => tracing::error!(
                        task_id = task_id_str,
                        error = %metadata_error,
                        "PR-mode: failed to serialize merge recovery metadata"
                    ),
                }
                let metadata = serde_json::json!({
                    "error": format!("PR operation failed: {}", e),
                    "error_code": "pr_operation_failed",
                    "merge_failure_source": serde_json::to_value(failure_source)
                        .unwrap_or_default(),
                });
                self.transition_to_merge_incomplete(
                    TaskCore {
                        task: &mut *task,
                        task_id: &task_id,
                        task_id_str,
                        task_repo,
                    },
                    metadata,
                    true,
                )
                .await;
            }
        }
    }
}
