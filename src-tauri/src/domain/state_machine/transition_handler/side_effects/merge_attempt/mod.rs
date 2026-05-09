use super::*;
use crate::domain::state_machine::transition_handler::{
    cleanup_helpers, merge_coordination, merge_helpers, BranchPair, ProjectCtx, TaskCore,
};
use crate::domain::state_machine::{State, TransitionHandler};

mod branch_discovery;
mod in_flight_guard;
mod pr_mode;
mod scope_backstop;

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
        let app_handle = self.machine.context.services.app_handle.as_ref();

        // Emit early phase list so the frontend can show pre-merge phases immediately
        // (validation start emits the full list including dynamic validation phases later)
        if let Some(handle) = app_handle {
            let _ = handle.emit(
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
            app_handle,
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
            app_handle,
            task_id_str,
            MergePhase::new(MergePhase::MERGE_PREPARATION),
            MergePhaseStatus::Passed,
            "Merge context loaded".to_string(),
        );

        // Pre-merge validation for plan_merge tasks
        emit_merge_progress(
            app_handle,
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
            self.machine.context.services.app_handle.as_ref(),
        )
        .await
        {
            return;
        }

        // Preconditions validated, branches resolved
        emit_merge_progress(
            app_handle,
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
            app_handle,
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
                    app_handle,
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
                    app_handle,
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
            app_handle,
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

        // Update plan branch from its source branch if behind (prevents false validation failures)
        emit_merge_progress(
            app_handle,
            task_id_str,
            MergePhase::new(MergePhase::BRANCH_FRESHNESS),
            MergePhaseStatus::Started,
            format!("Updating plan branch from {}...", base_branch),
        );
        let freshness_timeout =
            std::time::Duration::from_secs(reconciliation_config().branch_freshness_timeout_secs);
        let plan_update_start = std::time::Instant::now();
        let plan_update_result = tokio::time::timeout(
            freshness_timeout,
            merge_coordination::update_plan_from_main(
                repo_path,
                &target_branch,
                base_branch.as_str(),
                project,
                task_id_str,
                self.machine.context.services.app_handle.as_ref(),
            ),
        )
        .await;

        let plan_update_elapsed = plan_update_start.elapsed();
        match plan_update_result {
            Err(_elapsed) => {
                tracing::error!(
                    task_id = task_id_str,
                    timeout_secs = reconciliation_config().branch_freshness_timeout_secs,
                    elapsed_ms = plan_update_elapsed.as_millis() as u64,
                    "update_plan_from_main timed out — aborting merge"
                );
                let metadata = serde_json::json!({
                    "error": format!(
                        "update_plan_from_main timed out after {}s (limit: {}s)",
                        plan_update_elapsed.as_secs(),
                        reconciliation_config().branch_freshness_timeout_secs,
                    ),
                    "source_branch": source_branch,
                    "target_branch": target_branch,
                    "merge_failure_source": serde_json::to_value(MergeFailureSource::TransientGit).unwrap_or_default(),
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
            Ok(merge_coordination::PlanUpdateResult::AlreadyUpToDate)
            | Ok(merge_coordination::PlanUpdateResult::Updated)
            | Ok(merge_coordination::PlanUpdateResult::NotPlanBranch) => {
                tracing::info!(
                    task_id = task_id_str,
                    elapsed_ms = plan_update_elapsed.as_millis() as u64,
                    "update_plan_from_main completed"
                );
                // Continue with merge
            }
            Ok(merge_coordination::PlanUpdateResult::Conflicts { conflict_files }) => {
                tracing::warn!(
                    task_id = task_id_str,
                    conflict_count = conflict_files.len(),
                    "Plan branch update from source branch produced conflicts — routing to merger agent"
                );
                let metadata = serde_json::json!({
                    "error": "Conflicts detected while updating plan branch from its source branch. Merger agent needed.",
                    "conflict_files": conflict_files.iter().map(|f| f.display().to_string()).collect::<Vec<_>>(),
                    "source_branch": source_branch,
                    "target_branch": target_branch,
                    "base_branch": base_branch,
                    "plan_update_conflict": true,
                });
                merge_helpers::merge_metadata_into(task, &metadata);
                task.internal_status = InternalStatus::Merging;
                // Create a merge worktree with the plan branch (target) checked out.
                // The merger agent will run `git merge <base>` to reproduce and resolve conflicts.
                // First, clean up any stale plan-update worktree that holds the plan branch —
                // git won't allow the same branch in two worktrees simultaneously.
                let plan_update_wt =
                    merge_helpers::compute_plan_update_worktree_path(project, task_id_str);
                let plan_update_wt_path = std::path::PathBuf::from(&plan_update_wt);
                merge_helpers::pre_delete_worktree(repo_path, &plan_update_wt_path, task_id_str)
                    .await;
                let merge_wt = merge_helpers::compute_merge_worktree_path(project, task_id_str);
                let merge_wt_path = std::path::PathBuf::from(&merge_wt);
                if let Err(e) = GitService::checkout_existing_branch_worktree(
                    repo_path,
                    &merge_wt_path,
                    &target_branch,
                )
                .await
                {
                    tracing::error!(
                        task_id = task_id_str,
                        error = %e,
                        target_branch = %target_branch,
                        "Failed to create merge worktree for plan_update_conflict — falling back to MergeIncomplete"
                    );
                    self.transition_to_merge_incomplete(
                        TaskCore { task: &mut *task, task_id: &task_id, task_id_str, task_repo },
                        serde_json::json!({
                            "error": format!("Failed to create merge worktree for plan update conflict: {}", e),
                            "source_branch": source_branch,
                            "target_branch": target_branch,
                        }),
                        true,
                    ).await;
                    return;
                }
                task.worktree_path = Some(merge_wt);
                self.persist_merge_transition(
                    TaskCore {
                        task: &mut *task,
                        task_id: &task_id,
                        task_id_str,
                        task_repo,
                    },
                    InternalStatus::PendingMerge,
                    InternalStatus::Merging,
                    "plan_update_conflict",
                )
                .await;
                // Spawn the merger agent — mirrors handle_validation_failure's AutoFix path.
                // Without this call, the task sits in Merging with no agent and
                // attempt_merge_auto_complete transitions it straight to MergeIncomplete.
                if let Err(e) = Box::pin(self.on_enter_dispatch(&State::Merging)).await {
                    tracing::error!(
                        task_id = task_id_str,
                        error = %e,
                        "on_enter(Merging) failed during plan_update_conflict routing"
                    );
                }
                return;
            }
            Ok(merge_coordination::PlanUpdateResult::Error(err)) => {
                tracing::error!(
                    task_id = task_id_str,
                    error = %err,
                    elapsed_ms = plan_update_elapsed.as_millis() as u64,
                    "Plan branch update from source branch failed — aborting merge to prevent stale branch"
                );
                // Fatal: abort merge. Proceeding with a stale plan branch causes validation
                // failures that the fixer agent cannot resolve (missing code from the source branch).
                let metadata = serde_json::json!({
                    "error": format!("Plan branch update failed: {}", err),
                    "source_branch": source_branch,
                    "target_branch": target_branch,
                    "merge_failure_source": "PlanUpdateFailed",
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
        }

        // Update source branch from target if behind (prevents validation failures from stale code)
        emit_merge_progress(
            app_handle,
            task_id_str,
            MergePhase::new(MergePhase::BRANCH_FRESHNESS),
            MergePhaseStatus::Started,
            "Updating source branch from target...".to_string(),
        );
        let source_update_start = std::time::Instant::now();
        let source_update_result = tokio::time::timeout(
            freshness_timeout,
            merge_coordination::update_source_from_target(
                repo_path,
                &source_branch,
                &target_branch,
                project,
                task_id_str,
                self.machine.context.services.app_handle.as_ref(),
            ),
        )
        .await;

        let source_update_elapsed = source_update_start.elapsed();
        match source_update_result {
            Err(_elapsed) => {
                tracing::error!(
                    task_id = task_id_str,
                    timeout_secs = reconciliation_config().branch_freshness_timeout_secs,
                    elapsed_ms = source_update_elapsed.as_millis() as u64,
                    "update_source_from_target timed out — aborting merge"
                );
                let metadata = serde_json::json!({
                    "error": format!(
                        "update_source_from_target timed out after {}s (limit: {}s)",
                        source_update_elapsed.as_secs(),
                        reconciliation_config().branch_freshness_timeout_secs,
                    ),
                    "source_branch": source_branch,
                    "target_branch": target_branch,
                    "merge_failure_source": serde_json::to_value(MergeFailureSource::TransientGit).unwrap_or_default(),
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
            Ok(merge_coordination::SourceUpdateResult::AlreadyUpToDate)
            | Ok(merge_coordination::SourceUpdateResult::Updated) => {
                tracing::info!(
                    task_id = task_id_str,
                    elapsed_ms = source_update_elapsed.as_millis() as u64,
                    "update_source_from_target completed"
                );
                // Continue with merge
            }
            Ok(merge_coordination::SourceUpdateResult::Conflicts { conflict_files }) => {
                tracing::warn!(
                    task_id = task_id_str,
                    conflict_count = conflict_files.len(),
                    "Source branch update from target produced conflicts — routing to merger agent"
                );
                let metadata = serde_json::json!({
                    "error": "Conflicts detected while updating source branch from target. Merger agent needed.",
                    "conflict_files": conflict_files.iter().map(|f| f.display().to_string()).collect::<Vec<_>>(),
                    "source_branch": source_branch,
                    "target_branch": target_branch,
                    "source_update_conflict": true,
                });
                merge_helpers::merge_metadata_into(task, &metadata);
                task.internal_status = InternalStatus::Merging;
                // Create a merge worktree with source branch checked out so the merger
                // agent can resolve the conflict in an isolated directory. Mirrors the
                // plan_update_conflict path which sets worktree_path before persist.
                let merge_wt = merge_helpers::compute_merge_worktree_path(project, task_id_str);
                let merge_wt_path = std::path::PathBuf::from(&merge_wt);
                // RC#13: Clean up any stale merge worktree from a prior phase before
                // creating a fresh one. Without this, checkout_existing_branch_worktree
                // fails with "fatal: '/path/merge-{id}' already exists".
                merge_helpers::pre_delete_worktree(repo_path, &merge_wt_path, task_id_str).await;
                if let Err(e) = GitService::checkout_existing_branch_worktree(
                    repo_path,
                    &merge_wt_path,
                    &source_branch,
                )
                .await
                {
                    tracing::error!(
                        task_id = task_id_str,
                        error = %e,
                        source_branch = %source_branch,
                        "Failed to create merge worktree for source_update_conflict — falling back to MergeIncomplete"
                    );
                    self.transition_to_merge_incomplete(
                        TaskCore { task: &mut *task, task_id: &task_id, task_id_str, task_repo },
                        serde_json::json!({
                            "error": format!("Failed to create merge worktree for source update conflict: {}", e),
                            "source_branch": source_branch,
                            "target_branch": target_branch,
                        }),
                        true,
                    ).await;
                    return;
                }
                task.worktree_path = Some(merge_wt);
                self.persist_merge_transition(
                    TaskCore {
                        task: &mut *task,
                        task_id: &task_id,
                        task_id_str,
                        task_repo,
                    },
                    InternalStatus::PendingMerge,
                    InternalStatus::Merging,
                    "source_update_conflict",
                )
                .await;
                // Spawn the merger agent — mirrors handle_validation_failure's AutoFix path.
                // Without this call, the task sits in Merging with no agent and
                // attempt_merge_auto_complete transitions it straight to MergeIncomplete.
                if let Err(e) = Box::pin(self.on_enter_dispatch(&State::Merging)).await {
                    tracing::error!(
                        task_id = task_id_str,
                        error = %e,
                        "on_enter(Merging) failed during source_update_conflict routing"
                    );
                }
                return;
            }
            Ok(merge_coordination::SourceUpdateResult::Error(err)) => {
                tracing::warn!(
                    task_id = task_id_str,
                    error = %err,
                    elapsed_ms = source_update_elapsed.as_millis() as u64,
                    "Source branch update from target failed (non-fatal) — proceeding with merge"
                );
                // Non-fatal: continue with merge anyway. The source branch may still merge cleanly.
            }
        }

        // Branch freshness checks complete
        emit_merge_progress(
            app_handle,
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
            app_handle,
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
            remaining,
            deadline_secs,
        )
        .await;
    }
}
