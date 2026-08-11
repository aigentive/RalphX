use super::*;
#[cfg(test)]
use crate::domain::entities::task_metadata::{
    MergeRecoveryEvent, MergeRecoveryEventKind, MergeRecoveryMetadata, MergeRecoveryReasonCode,
    MergeRecoverySource, MergeRecoveryState,
};
use crate::domain::repositories::{
    AcquireGitTargetLease, AcquireGitTargetLeaseOutcome, BeginGitMutation, CompleteGitMutation,
    GitAuthorityCasOutcome,
};
use crate::domain::state_machine::transition_handler::{
    cleanup_helpers, freshness, merge_coordination, merge_helpers, BranchPair, ProjectCtx, TaskCore,
};
use crate::domain::state_machine::{State, TransitionHandler};
use crate::error::AppError;

mod branch_discovery;
mod in_flight_guard;
mod pr_mode;
mod scope_backstop;

#[cfg(test)]
pub(super) fn append_source_update_failure_recovery_event(
    task: &mut Task,
    err: &str,
    source_branch: &str,
    target_branch: &str,
) {
    let mut recovery = MergeRecoveryMetadata::from_task_metadata(task.metadata.as_deref())
        .unwrap_or(None)
        .unwrap_or_else(MergeRecoveryMetadata::new);
    let attempt = recovery
        .events
        .iter()
        .filter(|event| {
            matches!(
                event.kind,
                MergeRecoveryEventKind::AutoRetryTriggered | MergeRecoveryEventKind::AttemptFailed
            )
        })
        .count() as u32
        + 1;
    let event = MergeRecoveryEvent::new(
        MergeRecoveryEventKind::AttemptFailed,
        MergeRecoverySource::System,
        MergeRecoveryReasonCode::GitError,
        format!("Source branch update failed: {}", err),
    )
    .with_source_branch(source_branch)
    .with_target_branch(target_branch)
    .with_attempt(attempt)
    .with_failure_source(MergeFailureSource::TransientGit);
    recovery.append_event_with_state(event, MergeRecoveryState::Failed);

    match recovery.update_task_metadata(task.metadata.as_deref()) {
        Ok(updated_json) => task.metadata = Some(updated_json),
        Err(e) => tracing::error!(
            error = %e,
            "Failed to serialize source update recovery metadata"
        ),
    }
}

impl<'a> TransitionHandler<'a> {
    /// Inner body of `attempt_programmatic_merge`. Extracted so the outer wrapper
    /// can guarantee the `merge_pipeline_active` flag is always cleared on exit.
    pub(super) async fn run_merge_pipeline_body(
        &self,
        task: &mut Task,
        project: &Project,
        task_id_str: &str,
        task_repo: &Arc<dyn TaskRepository>,
        attempt_start: std::time::Instant,
    ) {
        let event_sink = self.machine.context.services.event_sink.as_deref();

        // Emit early phase list so the frontend can show pre-merge phases immediately
        // (validation start emits the full list including dynamic validation phases later)
        if let Some(sink) = event_sink {
            sink.emit(
                "task:merge_phases",
                serde_json::json!({
                    "task_id": task_id_str,
                    "phases": [
                        { "id": MergePhase::MERGE_PREPARATION, "label": "Preparation" },
                        { "id": MergePhase::PRECONDITION_CHECK, "label": "Preconditions" },
                        { "id": MergePhase::BRANCH_FRESHNESS, "label": "Branch Freshness" },
                        { "id": MergePhase::MERGE_CLEANUP, "label": "Cleanup" },
                        { "id": MergePhase::WORKTREE_SETUP, "label": "Worktree Setup" },
                        { "id": MergePhase::PROGRAMMATIC_MERGE, "label": "Merge" },
                        { "id": MergePhase::FINALIZE, "label": "Finalize" },
                    ],
                }),
            );
        }

        // Signal that merge preparation has started
        emit_merge_progress(
            event_sink,
            task_id_str,
            MergePhase::new(MergePhase::MERGE_PREPARATION),
            MergePhaseStatus::Started,
            "Preparing merge...".to_string(),
        );

        // Attempt to discover and re-attach orphaned task branch
        self.log_branch_discovery(task, project, task_repo, task_id_str)
            .await;

        // Preparation complete (branch discovery + context loaded)
        emit_merge_progress(
            event_sink,
            task_id_str,
            MergePhase::new(MergePhase::MERGE_PREPARATION),
            MergePhaseStatus::Passed,
            "Merge context loaded".to_string(),
        );

        // Pre-merge validation for plan_merge tasks
        emit_merge_progress(
            event_sink,
            task_id_str,
            MergePhase::new(MergePhase::PRECONDITION_CHECK),
            MergePhaseStatus::Started,
            "Validating merge preconditions...".to_string(),
        );
        let plan_branch_repo = &self.machine.context.services.plan_branch_repo;
        let task_id = TaskId::from_string(task_id_str.to_string());
        if let Err(validation_err) =
            validate_plan_merge_preconditions(task, project, plan_branch_repo).await
        {
            let error_msg = validation_err.message();
            let error_code = validation_err.error_code();
            tracing::warn!(
                task_id = task_id_str,
                error_code = error_code,
                error = %error_msg,
                "Pre-merge validation failed for plan_merge task — transitioning to MergeIncomplete"
            );
            let metadata = serde_json::json!({
                "error": error_msg,
                "error_code": error_code,
                "category": task.category,
            });
            self.transition_to_merge_incomplete(
                TaskCore {
                    task: &mut *task,
                    task_id: &task_id,
                    task_id_str,
                    task_repo,
                },
                metadata,
                false,
            )
            .await;
            return;
        }

        // Resolve source and target branches
        let (source_branch, target_branch) =
            resolve_merge_branches(task, project, plan_branch_repo).await;

        // Ensure we have a source branch to merge
        if source_branch.is_empty() {
            tracing::error!(
                task_id = task_id_str,
                category = %task.category,
                task_branch = ?task.task_branch,
                "Programmatic merge failed: empty source branch resolved — \
                 transitioning to MergeIncomplete"
            );
            let metadata = serde_json::json!({
                "error": "Empty source branch resolved. This typically means plan_branch_repo \
                          was unavailable when resolving merge branches for a plan_merge task.",
                "source_branch": source_branch,
                "target_branch": target_branch,
                "category": task.category,
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
            return;
        }

        if let Some(violation) = self
            .evaluate_merge_scope_backstop(task, project, &target_branch)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(
                    task_id = task_id_str,
                    error = %e,
                    "Merge scope backstop failed to evaluate; allowing merge attempt to continue"
                );
                None
            })
        {
            tracing::warn!(
                task_id = task_id_str,
                out_of_scope_files = ?violation.out_of_scope_files,
                reason = %violation.reason,
                "Merge scope backstop blocked PendingMerge and is routing back to revision"
            );
            crate::domain::entities::merge_progress_event::clear_merge_progress(task_id_str);
            let metadata = serde_json::json!({
                "error": violation.reason,
                "error_code": "merge_scope_drift_guard",
                "scope_guard_triggered": true,
                "scope_guard_out_of_scope_files": violation.out_of_scope_files,
                "source_branch": source_branch,
                "target_branch": target_branch,
            });
            if self
                .route_merge_scope_violation_to_revision(&task_id, task_id_str, metadata)
                .await
            {
                return;
            }

            self.transition_to_merge_incomplete(
                TaskCore {
                    task: &mut *task,
                    task_id: &task_id,
                    task_id_str,
                    task_repo,
                },
                serde_json::json!({
                    "error": "Merge scope backstop could not route task back to revision",
                    "error_code": "merge_scope_drift_guard_fallback",
                    "scope_guard_triggered": true,
                    "scope_guard_out_of_scope_files": violation.out_of_scope_files,
                    "source_branch": source_branch,
                    "target_branch": target_branch,
                }),
                true,
            )
            .await;
            return;
        }

        // Cache resolved branches in task metadata so auto-complete uses the same target
        // branch (TOCTOU guard: plan state can change between merge start and auto-complete)
        {
            let mut meta: serde_json::Value = task
                .metadata
                .as_ref()
                .and_then(|m| serde_json::from_str(m).ok())
                .unwrap_or_else(|| serde_json::json!({}));
            meta["merge_source_branch"] = serde_json::json!(source_branch);
            meta["merge_target_branch"] = serde_json::json!(target_branch);
            task.metadata = Some(meta.to_string());
            if let Err(e) = task_repo.update(task).await {
                tracing::warn!(
                    task_id = task_id_str,
                    error = %e,
                    "Failed to cache merge branches in task metadata"
                );
            }
        }

        // Main-merge deferral check
        let base_branch = merge_helpers::resolve_effective_base_branch(
            task,
            project,
            plan_branch_repo,
            Some(&target_branch),
        )
        .await;
        let running_count = self
            .machine
            .context
            .services
            .execution_state
            .as_ref()
            .map(|s| s.running_count());
        if merge_coordination::check_main_merge_deferral(
            TaskCore {
                task: &mut *task,
                task_id: &task_id,
                task_id_str,
                task_repo,
            },
            BranchPair {
                source_branch: &source_branch,
                target_branch: &target_branch,
            },
            base_branch.as_str(),
            running_count,
            self.machine.context.services.event_sink.as_deref(),
        )
        .await
        {
            return;
        }

        // Preconditions validated, branches resolved
        emit_merge_progress(
            event_sink,
            task_id_str,
            MergePhase::new(MergePhase::PRECONDITION_CHECK),
            MergePhaseStatus::Passed,
            "Preconditions met".to_string(),
        );
        self.emit_merge_activity_event(
            task_id_str,
            "Merge pipeline: preconditions validated",
            MergePhase::PRECONDITION_CHECK,
            "passed",
        )
        .await;

        let repo_path = Path::new(&project.working_directory);

        // Pre-merge cleanup: runs before freshness checks so all worktrees are
        // cleaned before freshness checks try to create new ones.
        emit_merge_progress(
            event_sink,
            task_id_str,
            MergePhase::new(MergePhase::MERGE_CLEANUP),
            MergePhaseStatus::Started,
            "Cleaning up previous merge artifacts...".to_string(),
        );
        let cleanup_timeout_secs = reconciliation_config().pre_merge_cleanup_timeout_secs;
        match cleanup_helpers::os_thread_timeout(
            std::time::Duration::from_secs(cleanup_timeout_secs),
            self.pre_merge_cleanup(
                task_id_str,
                task,
                project,
                repo_path,
                &target_branch,
                task_repo,
            ),
        )
        .await
        {
            Ok(()) => {
                emit_merge_progress(
                    event_sink,
                    task_id_str,
                    MergePhase::new(MergePhase::MERGE_CLEANUP),
                    MergePhaseStatus::Passed,
                    "Cleanup complete".to_string(),
                );
                self.emit_merge_activity_event(
                    task_id_str,
                    "Merge pipeline: cleanup complete",
                    MergePhase::MERGE_CLEANUP,
                    "passed",
                )
                .await;
            }
            Err(_os_elapsed) => {
                tracing::warn!(
                    task_id = %task_id_str,
                    cleanup_timeout_secs,
                    "pre_merge_cleanup timed out (OS-thread timeout) — proceeding to merge anyway (cleanup is best-effort)"
                );
                // Set debris metadata so GUARD knows this is a retry on next attempt
                // (prevents is_first_clean_attempt from skipping cleanup when stale worktree remains)
                merge_helpers::merge_metadata_into(
                    task,
                    &serde_json::json!({
                        "merge_failure_source": serde_json::to_value(MergeFailureSource::CleanupTimeout).unwrap_or_default(),
                        "cleanup_phase": serde_json::to_value(CleanupPhase::PreMergeWorktreeScan).unwrap_or_default(),
                    }),
                );
                if let Err(e) = task_repo.update(task).await {
                    tracing::warn!(
                        task_id = %task_id_str,
                        error = %e,
                        "Failed to persist cleanup_timeout debris metadata"
                    );
                }
                emit_merge_progress(
                    event_sink,
                    task_id_str,
                    MergePhase::new(MergePhase::MERGE_CLEANUP),
                    MergePhaseStatus::Passed,
                    format!("Cleanup timed out after {cleanup_timeout_secs}s — proceeding"),
                );
                self.emit_merge_activity_event(
                    task_id_str,
                    "Merge pipeline: cleanup timed out (best-effort, proceeding)",
                    MergePhase::MERGE_CLEANUP,
                    "warning",
                )
                .await;
            }
        }

        // Branch freshness: ensure plan branch, update from its source branch, update source from target
        emit_merge_progress(
            event_sink,
            task_id_str,
            MergePhase::new(MergePhase::BRANCH_FRESHNESS),
            MergePhaseStatus::Started,
            "Checking branch freshness...".to_string(),
        );

        // Ensure plan branch exists as git ref (lazy creation for merge target)
        merge_coordination::ensure_plan_branch_exists(
            task,
            repo_path,
            &target_branch,
            plan_branch_repo,
        )
        .await;

        let mut dedicated_source_updated = false;
        let mut dedicated_freshness_converged = false;
        for _ in 0..3 {
            let Some(refreshed_task) = task_repo.get_by_id(&task_id).await.ok().flatten() else {
                self.transition_to_merge_incomplete(
                    TaskCore { task: &mut *task, task_id: &task_id, task_id_str, task_repo },
                    serde_json::json!({"error": "Task disappeared during dedicated branch freshness checkpoint", "error_code": "branch_update_context_corrupt"}),
                    true,
                ).await;
                return;
            };
            *task = refreshed_task;
            let result = freshness::ensure_branches_fresh(
                repo_path,
                task,
                project,
                task_id_str,
                Some(&target_branch),
                Some(base_branch.as_str()),
                self.machine.context.services.event_sink.as_deref(),
                self.machine.context.services.activity_event_repo.as_ref(),
                "pending_merge",
                reconciliation_config(),
            )
            .await;
            let missing_branch = match &result {
                Err(freshness::FreshnessAction::ExecutionBlocked { branch_missing, .. }) => {
                    branch_missing.clone()
                }
                _ => None,
            };
            match crate::domain::state_machine::transition_handler::on_enter_states::apply_freshness_result(
                result, task, task_id_str, task_repo,
                self.machine.context.services.branch_update_repo.as_ref(),
                self.machine.context.services.branch_update_workflow.as_ref(), project, repo_path,
                "pending_merge",
            ).await {
                Ok(crate::domain::state_machine::transition_handler::on_enter_states::FreshnessApplyOutcome::Ready) => {
                    dedicated_freshness_converged = true;
                    break;
                }
                Ok(crate::domain::state_machine::transition_handler::on_enter_states::FreshnessApplyOutcome::Updated(direction)) => {
                    dedicated_source_updated |= direction == crate::domain::entities::BranchUpdateDirection::TaskBranch;
                }
                Err(AppError::BranchFreshnessConflict) => {
                    let update_state = task_repo.get_by_id(&task_id).await.ok().flatten().and_then(|current| match current.internal_status {
                        InternalStatus::UpdatingPlanBranch => Some(State::UpdatingPlanBranch),
                        InternalStatus::UpdatingTaskBranch => Some(State::UpdatingTaskBranch),
                        _ => None,
                    });
                    if let Some(update_state) = update_state {
                        if let Err(error) = Box::pin(self.on_enter_dispatch(&update_state)).await {
                            tracing::error!(task_id = task_id_str, error = %error, "Failed to spawn dedicated updater from pending merge");
                        }
                    }
                    return;
                }
                Err(error) => {
                    let mut metadata = serde_json::json!({
                        "error": error.to_string(),
                        "error_code": "branch_update_checkpoint_failed",
                    });
                    if let Some(branch) = missing_branch {
                        metadata["branch_missing"] = serde_json::json!(true);
                        metadata["missing_branch"] = serde_json::json!(branch);
                    }
                    self.transition_to_merge_incomplete(
                        TaskCore { task: &mut *task, task_id: &task_id, task_id_str, task_repo },
                        metadata,
                        true,
                    ).await;
                    return;
                }
            }
        }
        if !dedicated_freshness_converged {
            self.transition_to_merge_incomplete(
                TaskCore { task: &mut *task, task_id: &task_id, task_id_str, task_repo },
                serde_json::json!({"error": "Dedicated branch freshness checkpoints did not converge", "error_code": "branch_update_checkpoint_non_convergent"}),
                true,
            ).await;
            return;
        }

        let source_updated_from_target = dedicated_source_updated;

        // Branch freshness checks complete
        emit_merge_progress(
            event_sink,
            task_id_str,
            MergePhase::new(MergePhase::BRANCH_FRESHNESS),
            MergePhaseStatus::Passed,
            "Branches are up to date".to_string(),
        );
        self.emit_merge_activity_event(
            task_id_str,
            "Merge pipeline: branch freshness check passed",
            MergePhase::BRANCH_FRESHNESS,
            "passed",
        )
        .await;

        // "Already merged" early exit
        if self
            .check_already_merged(
                TaskCore {
                    task: &mut *task,
                    task_id: &task_id,
                    task_id_str,
                    task_repo,
                },
                BranchPair {
                    source_branch: &source_branch,
                    target_branch: &target_branch,
                },
                ProjectCtx { project, repo_path },
                plan_branch_repo,
            )
            .await
        {
            return;
        }

        // "Deleted source branch" recovery
        if self
            .recover_deleted_source_branch(
                TaskCore {
                    task: &mut *task,
                    task_id: &task_id,
                    task_id_str,
                    task_repo,
                },
                BranchPair {
                    source_branch: &source_branch,
                    target_branch: &target_branch,
                },
                ProjectCtx { project, repo_path },
                plan_branch_repo,
            )
            .await
        {
            return;
        }

        // Emit merge progress event
        emit_merge_progress(
            event_sink,
            task_id_str,
            MergePhase::programmatic_merge(),
            MergePhaseStatus::Started,
            format!("Merging {} into {}", source_branch, target_branch),
        );

        tracing::info!(
            task_id = task_id_str,
            source_branch = %source_branch,
            target_branch = %target_branch,
            "Attempting programmatic merge (Phase 1)"
        );

        // Concurrent merge guard (TOCTOU-safe deferral under merge_lock)
        if matches!(
            self.run_concurrent_merge_guard(
                task,
                task_id_str,
                &target_branch,
                project,
                task_repo,
                plan_branch_repo,
            )
            .await,
            ConcurrentGuardResult::Deferred
        ) {
            return;
        }

        // Overall merge deadline — computed from function start to bound the full pipeline
        // (cleanup + freshness + dispatch). Previously this was a NOP because the deadline
        // was created and checked at the same instant (always passed).
        let deadline_secs = reconciliation_config().attempt_merge_deadline_secs;
        let deadline_duration = std::time::Duration::from_secs(deadline_secs);

        // Check deadline after cleanup+freshness (using attempt_start from function top)
        if attempt_start.elapsed() >= deadline_duration {
            tracing::error!(
                task_id = task_id_str,
                deadline_secs = deadline_secs,
                elapsed_ms = attempt_start.elapsed().as_millis() as u64,
                "Programmatic merge exceeded deadline during cleanup — transitioning to MergeIncomplete"
            );
            // Guard will clear merge_pipeline_active flag on return
            let metadata = serde_json::json!({
                "error": format!("Merge attempt timed out after {}s (cleanup phase exceeded deadline)", deadline_secs),
                "source_branch": source_branch,
                "target_branch": target_branch,
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
            return;
        }

        // Build squash commit message
        let squash_commit_msg = self
            .build_squash_commit_message(task, task_id_str, &source_branch, &target_branch)
            .await;

        // Dispatch merge strategy with timeout — remaining time computed from function start
        let remaining = deadline_duration.saturating_sub(attempt_start.elapsed());
        tracing::info!(
            task_id = task_id_str,
            elapsed_ms = attempt_start.elapsed().as_millis() as u64,
            remaining_ms = remaining.as_millis() as u64,
            deadline_secs = deadline_secs,
            "Merge pipeline: cleanup + freshness complete, dispatching strategy"
        );
        self.emit_merge_activity_event(
            task_id_str,
            format!(
                "Merge pipeline: merging {} into {}",
                source_branch, target_branch
            ),
            MergePhase::PROGRAMMATIC_MERGE,
            "started",
        )
        .await;
        let Some(authority_repo) = self.machine.context.services.branch_update_repo.as_ref() else {
            self.transition_to_merge_incomplete(
                TaskCore {
                    task: &mut *task,
                    task_id: &task_id,
                    task_id_str,
                    task_repo,
                },
                serde_json::json!({
                    "error": "Canonical Git target authority is unavailable",
                    "error_code": "git_target_authority_unavailable",
                }),
                true,
            )
            .await;
            return;
        };
        let target_identity =
            match GitService::canonical_target_identity(repo_path, &target_branch).await {
                Ok(identity) => identity,
                Err(error) => {
                    self.transition_to_merge_incomplete(
                        TaskCore {
                            task: &mut *task,
                            task_id: &task_id,
                            task_id_str,
                            task_repo,
                        },
                        serde_json::json!({
                            "error": error.to_string(),
                            "error_code": "git_target_identity_failed",
                        }),
                        true,
                    )
                    .await;
                    return;
                }
            };
        let merge_owner = crate::domain::entities::GitTargetLeaseOwner::merge_attempt(
            task_id_str,
            format!("pending-merge:{task_id_str}:{target_branch}"),
        );
        let fencing_epoch = match authority_repo
            .acquire_target_lease(AcquireGitTargetLease {
                identity: target_identity.clone(),
                owner: merge_owner.clone(),
            })
            .await
        {
            Ok(AcquireGitTargetLeaseOutcome::Acquired { fencing_epoch })
            | Ok(AcquireGitTargetLeaseOutcome::AlreadyOwned { fencing_epoch }) => fencing_epoch,
            Ok(AcquireGitTargetLeaseOutcome::TargetBusy {
                owner,
                fencing_epoch,
            }) => {
                tracing::info!(
                    task_id = task_id_str,
                    owner = ?owner,
                    fencing_epoch,
                    "Programmatic merge deferred because canonical target authority is busy"
                );
                return;
            }
            Err(error) => {
                tracing::error!(
                    task_id = task_id_str,
                    error = %error,
                    "Programmatic merge failed to acquire canonical target authority"
                );
                return;
            }
        };
        let mutation_claim_id = uuid::Uuid::new_v4().to_string();
        match authority_repo
            .begin_git_mutation(BeginGitMutation {
                identity: target_identity.clone(),
                owner: merge_owner.clone(),
                fencing_epoch,
                claim_id: mutation_claim_id.clone(),
                kind: crate::domain::entities::GitMutationKind::Merge,
            })
            .await
        {
            Ok(GitAuthorityCasOutcome::Applied { .. }) => {}
            Ok(outcome) => {
                tracing::warn!(
                    task_id = task_id_str,
                    outcome = ?outcome,
                    "Programmatic merge deferred because its target mutation claim was rejected"
                );
                return;
            }
            Err(error) => {
                tracing::error!(
                    task_id = task_id_str,
                    error = %error,
                    "Programmatic merge failed to persist its target mutation claim"
                );
                return;
            }
        }

        self.dispatch_merge_strategy(
            TaskCore {
                task: &mut *task,
                task_id: &task_id,
                task_id_str,
                task_repo,
            },
            BranchPair {
                source_branch: &source_branch,
                target_branch: &target_branch,
            },
            ProjectCtx { project, repo_path },
            &squash_commit_msg,
            plan_branch_repo,
            source_updated_from_target,
            remaining,
            deadline_secs,
        )
        .await;

        let mutation_completion = authority_repo
            .complete_git_mutation(CompleteGitMutation {
                identity: target_identity.clone(),
                owner: merge_owner.clone(),
                fencing_epoch,
                claim_id: mutation_claim_id,
            })
            .await;
        if !matches!(
            mutation_completion,
            Ok(GitAuthorityCasOutcome::Applied { .. })
        ) {
            tracing::error!(
                task_id = task_id_str,
                outcome = ?mutation_completion,
                "Programmatic merge left canonical target authority fenced after completion"
            );
            return;
        }
        let release = authority_repo
            .release_target_lease(&target_identity, &merge_owner, fencing_epoch)
            .await;
        if !matches!(release, Ok(GitAuthorityCasOutcome::Applied { .. })) {
            tracing::error!(
                task_id = task_id_str,
                outcome = ?release,
                "Programmatic merge could not release canonical target authority"
            );
        }
    }
}
