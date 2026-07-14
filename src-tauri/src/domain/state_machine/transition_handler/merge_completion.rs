// Merge completion: finalize merge and cleanup branch/worktree
//
// Extracted from side_effects.rs — handles post-merge finalization and resource cleanup.
//
// Phase 2 MERGE: complete_merge_internal marks Merged immediately (no blocking cleanup).
// Phase 3 CLEANUP: deferred_merge_cleanup runs in background via tokio::spawn.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::application::GitService;
use crate::domain::entities::{
    merge_progress_event::{MergePhase, MergePhaseStatus},
    task_metadata::{
        CleanupPhase, MergeFailureSource, MergeRecoveryEvent, MergeRecoveryEventKind,
        MergeRecoveryMetadata, MergeRecoveryReasonCode, MergeRecoverySource, MergeRecoveryState,
    },
    InternalStatus, Project, Task, TaskCategory, TaskId,
};
use crate::domain::repositories::TaskRepository;
use crate::domain::state_machine::services::{
    NotificationContext, Notifier, TaskNotification, WebhookPublisher,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::git_runtime_config;
use crate::utils::path_safety::validate_absolute_non_root_path;
use ralphx_domain::repositories::ExternalEventsRepository;
use ralphx_events::EventSink;

use crate::domain::services::payload_enrichment::{
    emit_external_webhook_event, PresentationKind, WebhookPresentationContext,
};

use super::merge_helpers::{
    merge_metadata_into, sync_plan_branch_pr_after_regular_task_merge, PlanBranchPrSyncOutcome,
    PlanBranchPrSyncServices, PrBranchPublicationConflict,
};
use super::merge_validation::emit_merge_progress;

/// Complete a merge operation by transitioning task to Merged (Phase 2 MERGE).
///
/// Marks the task Merged immediately without blocking on cleanup. Sets
/// `pending_cleanup` metadata so Phase 3 (deferred_merge_cleanup) can
/// handle worktree/branch deletion in the background.
///
/// This is shared logic used by:
/// - Programmatic merge success path (PendingMerge side effect)
/// - Merge auto-completion on agent exit (Phase 76)
/// - complete_merge HTTP handler (backwards compatibility)
///
/// # Arguments
/// * `task` - Mutable task to update (must be in appropriate state)
/// * `project` - Project for branch/worktree cleanup info
/// * `commit_sha` - The merge commit SHA (must be on target_branch)
/// * `target_branch` - The branch the merge was supposed to happen on
/// * `task_repo` - Repository to persist task changes
/// * `event_sink` - Optional event sink for emitting frontend/runtime events
///
/// # Side Effects
/// 1. Updates task.merge_commit_sha
/// 2. Updates task.internal_status to Merged
/// 3. Persists status change to history
/// 4. Sets `pending_cleanup` metadata (cleared by Phase 3)
/// 5. Emits task:merged and task:status_changed events
///
/// # Errors
/// Returns `AppError::Validation` if the commit is not on the target branch.
/// Returns `AppError::GitOperation` if git verification itself fails (protects against
/// ghost merges — setting Merged status without confirmation is a data integrity error).
#[allow(clippy::too_many_arguments)]
pub async fn complete_merge_internal(
    task: &mut Task,
    project: &Project,
    commit_sha: &str,
    source_branch: &str,
    target_branch: &str,
    task_repo: &Arc<dyn TaskRepository>,
    external_events_repo: Option<&Arc<dyn ExternalEventsRepository>>,
    webhook_publisher: Option<&Arc<dyn WebhookPublisher>>,
    event_sink: Option<&dyn EventSink>,
    session_title: Option<String>,
) -> AppResult<()> {
    complete_merge_internal_impl(
        task,
        project,
        commit_sha,
        source_branch,
        target_branch,
        task_repo,
        external_events_repo,
        webhook_publisher,
        event_sink,
        session_title,
        None,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn complete_merge_internal_with_pr_sync_and_notifier(
    task: &mut Task,
    project: &Project,
    commit_sha: &str,
    source_branch: &str,
    target_branch: &str,
    task_repo: &Arc<dyn TaskRepository>,
    external_events_repo: Option<&Arc<dyn ExternalEventsRepository>>,
    webhook_publisher: Option<&Arc<dyn WebhookPublisher>>,
    event_sink: Option<&dyn EventSink>,
    session_title: Option<String>,
    pr_sync_services: Option<PlanBranchPrSyncServices>,
    notifier: Option<&Arc<dyn Notifier>>,
) -> AppResult<()> {
    complete_merge_internal_impl(
        task,
        project,
        commit_sha,
        source_branch,
        target_branch,
        task_repo,
        external_events_repo,
        webhook_publisher,
        event_sink,
        session_title,
        pr_sync_services,
        notifier,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn complete_merge_internal_impl(
    task: &mut Task,
    project: &Project,
    commit_sha: &str,
    source_branch: &str,
    target_branch: &str,
    task_repo: &Arc<dyn TaskRepository>,
    external_events_repo: Option<&Arc<dyn ExternalEventsRepository>>,
    webhook_publisher: Option<&Arc<dyn WebhookPublisher>>,
    event_sink: Option<&dyn EventSink>,
    session_title: Option<String>,
    pr_sync_services: Option<PlanBranchPrSyncServices>,
    notifier: Option<&Arc<dyn Notifier>>,
) -> AppResult<()> {
    // Clone task_id early to avoid borrow conflicts with mutable task
    let task_id = task.id.clone();
    let task_id_str = task_id.as_str();
    let old_status = task.internal_status.clone();
    let repo_path = Path::new(&project.working_directory);

    // VERIFY: Commit must be on target branch to prevent false merges
    match GitService::is_commit_on_branch(repo_path, commit_sha, target_branch).await {
        Ok(true) => {
            tracing::debug!(
                task_id = task_id_str,
                commit_sha = %commit_sha,
                target_branch = %target_branch,
                "complete_merge_internal: commit verified on target branch"
            );
        }
        Ok(false) => {
            tracing::error!(
                task_id = task_id_str,
                commit_sha = %commit_sha,
                target_branch = %target_branch,
                "complete_merge_internal: commit NOT on target branch - rejecting false merge"
            );
            return Err(AppError::Validation(format!(
                "Commit {} is not on target branch {} - merge verification failed",
                commit_sha, target_branch
            )));
        }
        Err(e) => {
            // Fatal: git verification failed — we cannot confirm the merge succeeded.
            // Setting Merged status without verification risks data corruption (ghost merge).
            // The caller will handle the error; reconciliation will retry the merge.
            tracing::error!(
                task_id = task_id_str,
                error = %e,
                commit_sha = %commit_sha,
                target_branch = %target_branch,
                "complete_merge_internal: git verification failed — rejecting Merged \
                 status to protect data integrity"
            );
            return Err(AppError::GitOperation(format!(
                "Cannot confirm merge: git verification of commit {} on branch {} failed: {}",
                commit_sha, target_branch, e
            )));
        }
    }

    tracing::info!(
        task_id = task_id_str,
        commit_sha = %commit_sha,
        old_status = ?old_status,
        "complete_merge_internal: completing merge"
    );

    // Emit finalize merge progress event
    emit_merge_progress(
        event_sink,
        task_id_str,
        MergePhase::finalize(),
        MergePhaseStatus::Started,
        "Finalizing merge and cleaning up".to_string(),
    );

    // STATE FRESHNESS CHECK: Re-fetch task from DB to detect concurrent transitions
    // (e.g., reconciler may have moved task to MergeIncomplete while we were running).
    // This guards against "ghost merges" — writing Merged over a reconciler transition.
    if let Ok(Some(current_task)) = task_repo.get_by_id(&task_id).await {
        if !matches!(
            current_task.internal_status,
            InternalStatus::PendingMerge | InternalStatus::Merging
        ) {
            tracing::warn!(
                task_id = task_id_str,
                expected = "PendingMerge|Merging",
                actual = ?current_task.internal_status,
                "merge completion aborted — task was concurrently transitioned (likely by reconciler)"
            );
            return Ok(());
        }
    }

    if let Some(pr_sync_services) = pr_sync_services.as_ref() {
        match sync_plan_branch_pr_after_regular_task_merge(task, project, pr_sync_services).await {
            Ok(PlanBranchPrSyncOutcome::Complete) => {}
            Ok(PlanBranchPrSyncOutcome::Conflict(conflict)) => {
                tracing::warn!(
                    task_id = task_id_str,
                    remote_ref = %conflict.remote_ref,
                    "complete_merge_internal: PR branch publication requires conflict resolution before Merged status"
                );
                let finalized = route_pr_branch_publication_conflict(
                    task,
                    project,
                    &conflict,
                    commit_sha,
                    source_branch,
                    target_branch,
                    task_repo,
                    pr_sync_services,
                    old_status.clone(),
                )
                .await?;
                if finalized {
                    emit_merge_progress(
                        event_sink,
                        task_id_str,
                        MergePhase::finalize(),
                        MergePhaseStatus::Passed,
                        format!("Merge finalized successfully: {commit_sha}"),
                    );
                    return Ok(());
                }
                return Err(conflict.routed_error());
            }
            Err(error) => {
                tracing::warn!(
                    task_id = task_id_str,
                    error = %error,
                    "complete_merge_internal: PR branch publication failed before Merged status"
                );
                merge_metadata_into(
                    task,
                    &serde_json::json!({
                        "error": format!("PR branch publication failed: {}", error),
                        "error_code": "pr_branch_publication_failed",
                        "target_branch": target_branch,
                        "source_branch": source_branch,
                        "commit_sha": commit_sha,
                    }),
                );
                task.internal_status = InternalStatus::MergeIncomplete;
                task.touch();
                task_repo.update(task).await?;
                if let Ok(history_entry_id) = task_repo
                    .persist_status_change(
                        &task_id,
                        old_status,
                        InternalStatus::MergeIncomplete,
                        "pr_branch_publication_failed",
                    )
                    .await
                {
                    if let Some(notifier) = notifier {
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
                    }
                }
                return Err(error);
            }
        }
    }

    // 1. Append attempt_succeeded event to merge recovery metadata after every
    // required completion gate has passed, including PR branch publication.
    let mut recovery = MergeRecoveryMetadata::from_task_metadata(task.metadata.as_deref())
        .unwrap_or(None)
        .unwrap_or_else(MergeRecoveryMetadata::new);

    // Count total retry attempts
    let attempt_count = recovery
        .events
        .iter()
        .filter(|e| matches!(e.kind, MergeRecoveryEventKind::AutoRetryTriggered))
        .count() as u32
        + 1;

    let success_event = MergeRecoveryEvent::new(
        MergeRecoveryEventKind::AttemptSucceeded,
        MergeRecoverySource::System,
        MergeRecoveryReasonCode::Unknown,
        format!("Merge completed successfully with commit {}", commit_sha),
    )
    .with_attempt(attempt_count);

    recovery.append_event_with_state(success_event, MergeRecoveryState::Succeeded);

    // Update task metadata
    if let Ok(updated_json) = recovery.update_task_metadata(task.metadata.as_deref()) {
        task.metadata = Some(updated_json);
    } else {
        tracing::warn!(
            task_id = task_id_str,
            "Failed to serialize merge recovery metadata on success (non-fatal)"
        );
    }

    // 2. Update task with merge commit SHA, status, and pending_cleanup in ONE write.
    // Combining status + pending_cleanup into a single update eliminates the crash
    // window where status=Merged but pending_cleanup is not yet set.
    task.merge_commit_sha = Some(commit_sha.to_string());
    task.internal_status = InternalStatus::Merged;
    set_pending_cleanup_metadata(task);
    task.touch();

    task_repo.update(task).await.map_err(|e| {
        tracing::error!(error = %e, task_id = task_id_str, "Failed to update task with merge_commit_sha");
        e
    })?;

    // 3. Record status change in history
    if let Err(e) = task_repo
        .persist_status_change(
            &task_id,
            old_status.clone(),
            InternalStatus::Merged,
            "merge_success",
        )
        .await
    {
        tracing::warn!(error = %e, task_id = task_id_str, "Failed to record merge transition (non-fatal)");
    }

    // 4. Emit frontend/runtime events (intentional: no frontend listeners is OK)
    if let Some(sink) = event_sink {
        sink.emit(
            "task:merged",
            serde_json::json!({
                "task_id": task_id_str,
                "commit_sha": commit_sha,
            }),
        );
        sink.emit(
            "task:status_changed",
            serde_json::json!({
                "task_id": task_id_str,
                "old_status": old_status.as_str(),
                "new_status": "merged",
            }),
        );
        sink.emit(
            "merge:completed",
            serde_json::json!({
                "task_id": task_id_str,
                "commit_sha": commit_sha,
            }),
        );
    }

    // 5. External events: merge:completed + task:status_changed
    // Non-fatal: failures must not block merge completion.
    if let (Some(repo), Some(publisher)) = (external_events_repo, webhook_publisher) {
        let project_id_str = project.id.to_string();
        let session_id_str = task
            .ideation_session_id
            .as_ref()
            .map(|id| id.as_str().to_string());
        let category_str = task.category.to_string();

        let ctx = WebhookPresentationContext {
            project_name: Some(project.name.clone()),
            session_title: session_title.clone(),
            task_title: Some(task.title.clone()),
            presentation_kind: Some(PresentationKind::MergeCompleted),
        };

        let mut merge_payload = serde_json::json!({
            "task_id": task_id_str,
            "project_id": project_id_str,
            "session_id": session_id_str,
            "category": category_str,
            "source_branch": source_branch,
            "target_branch": target_branch,
            "commit_sha": commit_sha,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        ctx.inject_into(&mut merge_payload);
        if let Err(e) = emit_external_webhook_event(
            "merge:completed",
            &project_id_str,
            merge_payload,
            repo,
            publisher,
        )
        .await
        {
            tracing::warn!(
                task_id = task_id_str,
                error = %e,
                "complete_merge_internal: merge:completed emit failed (non-fatal)"
            );
        }

        let ctx_sc = WebhookPresentationContext {
            project_name: Some(project.name.clone()),
            session_title: session_title.clone(),
            task_title: Some(task.title.clone()),
            presentation_kind: Some(PresentationKind::TaskStatusChanged),
        };

        let mut sc_payload = serde_json::json!({
            "task_id": task_id_str,
            "project_id": project_id_str,
            "session_id": session_id_str,
            "old_status": old_status.as_str(),
            "new_status": "merged",
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        ctx_sc.inject_into(&mut sc_payload);
        if let Err(e) = emit_external_webhook_event(
            "task:status_changed",
            &project_id_str,
            sc_payload,
            repo,
            publisher,
        )
        .await
        {
            tracing::warn!(
                task_id = task_id_str,
                error = %e,
                "complete_merge_internal: task:status_changed emit failed (non-fatal)"
            );
        }
    }

    // 6. Plan:delivered event for PlanMerge tasks.
    // This is the canonical guaranteed emission point — complete_merge_internal only
    // returns Ok after the DB write is durable and the commit SHA is verified.
    // The on_enter(Merged) path in outcomes.rs is retained as defense-in-depth.
    if task.category == TaskCategory::PlanMerge {
        if let (Some(repo), Some(publisher)) = (external_events_repo, webhook_publisher) {
            if let Some(ref session_id) = task.ideation_session_id {
                let session_id_str = session_id.as_str().to_string();
                let project_id_str = project.id.to_string();

                // IDEMPOTENCY CHECK: fail-safe defaults to true (already delivered)
                // to prevent double-emission when on_enter(Merged) also fires.
                let already_delivered = repo
                    .event_exists("plan:delivered", &project_id_str, &session_id_str)
                    .await
                    .unwrap_or_else(|e| {
                        tracing::error!(
                            task_id = task_id_str,
                            session_id = %session_id_str,
                            error = %e,
                            "plan:delivered idempotency check failed — assuming already delivered (fail-safe)"
                        );
                        true
                    });

                if !already_delivered {
                    let ctx_pd = WebhookPresentationContext {
                        project_name: Some(project.name.clone()),
                        session_title: session_title.clone(),
                        task_title: Some(task.title.clone()),
                        presentation_kind: Some(PresentationKind::PlanDelivered),
                    };
                    let mut payload = serde_json::json!({
                        "session_id": session_id_str,
                        "project_id": project_id_str,
                        "task_id": task_id_str,
                        "commit_sha": commit_sha,
                        "target_branch": target_branch,
                        "timestamp": chrono::Utc::now().to_rfc3339(),
                    });
                    ctx_pd.inject_into(&mut payload);
                    if let Err(e) = emit_external_webhook_event(
                        "plan:delivered",
                        &project_id_str,
                        payload,
                        repo,
                        publisher,
                    )
                    .await
                    {
                        tracing::warn!(
                            task_id = task_id_str,
                            session_id = %session_id_str,
                            error = %e,
                            "plan:delivered emit failed (non-fatal)"
                        );
                    } else {
                        tracing::info!(
                            task_id = task_id_str,
                            session_id = %session_id_str,
                            "plan:delivered emitted from complete_merge_internal"
                        );
                    }
                }
            }
        }
    }

    // Emit finalize success merge progress event
    emit_merge_progress(
        event_sink,
        task_id_str,
        MergePhase::finalize(),
        MergePhaseStatus::Passed,
        format!("Merge finalized successfully: {}", commit_sha),
    );

    // Clean up in-memory merge progress hydration store
    crate::domain::entities::merge_progress_event::clear_merge_progress(task_id_str);

    // Clean up validation log files from disk
    super::merge_validation::cleanup_validation_logs(task_id_str);

    tracing::info!(
        task_id = task_id_str,
        commit_sha = %commit_sha,
        "complete_merge_internal: merge completed successfully"
    );

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn route_pr_branch_publication_conflict(
    task: &mut Task,
    project: &Project,
    conflict: &PrBranchPublicationConflict,
    commit_sha: &str,
    source_branch: &str,
    target_branch: &str,
    task_repo: &Arc<dyn TaskRepository>,
    services: &PlanBranchPrSyncServices,
    old_status: InternalStatus,
) -> AppResult<bool> {
    let repo_path = Path::new(&project.working_directory);
    merge_metadata_into(
        task,
        &serde_json::json!({
            "error": conflict.description(),
            "error_code": "pr_branch_publication_conflict",
            "branch_freshness_conflict": true,
            "freshness_origin_state": "pr_branch_publication",
            "plan_update_conflict": true,
            "pr_branch_update_conflict": true,
            "pr_branch_publication_conflict": true,
            "github_pr_number": conflict.pr_number,
            "pr_branch_update_source": "pr_branch_publication",
            "source_branch": source_branch,
            "target_branch": target_branch,
            "base_branch": conflict.remote_ref,
            "publication_remote_ref": conflict.remote_ref,
            "commit_sha": commit_sha,
            "conflict_files": conflict.conflict_files_as_strings(),
        }),
    );
    task.touch();
    task_repo.update(task).await?;

    let repository = services.branch_update_repo.as_ref().ok_or_else(|| {
        AppError::ExecutionBlocked(
            "Branch update authority repository is unavailable for PR publication recovery"
                .to_string(),
        )
    })?;
    let workflow = services.branch_update_workflow.as_ref().ok_or_else(|| {
        AppError::ExecutionBlocked(
            "Branch update workflow adapter is unavailable for PR publication recovery".to_string(),
        )
    })?;
    let identity = GitService::canonical_target_identity(repo_path, &conflict.branch_name).await?;
    let mut operation = crate::domain::entities::BranchUpdateOperation::new(
        task.id.clone(),
        crate::domain::entities::BranchUpdateDirection::PlanBranch,
        crate::domain::entities::BranchUpdateContinuation::FinalizePostMergePrPublication,
        uuid::Uuid::new_v4().to_string(),
        conflict.remote_ref.clone(),
        conflict.branch_name.clone(),
        crate::domain::entities::BranchUpdateWorkspaceOwnership::OperationWorktree,
        crate::domain::entities::BranchUpdateCapacityOwnership::Inherited,
        identity,
        chrono::Utc::now(),
    );
    operation.workspace_path = Some(PathBuf::from(
        super::merge_helpers::compute_plan_update_worktree_path(project, task.id.as_str()),
    ));
    operation.observed_source_sha =
        Some(GitService::resolve_ref_sha(repo_path, &operation.source_branch).await?);
    operation.observed_target_sha =
        Some(GitService::resolve_ref_sha(repo_path, &operation.target_branch).await?);
    let operation_for_execution = operation.clone();
    let fencing_epoch = match repository
        .activate(crate::domain::repositories::BranchUpdateActivation {
            operation,
            expected_status: old_status,
            update_status: InternalStatus::UpdatingPlanBranch,
            trigger: "pr_branch_publication_conflict".to_string(),
        })
        .await?
    {
        crate::domain::repositories::BranchUpdateActivationOutcome::Applied {
            fencing_epoch,
            ..
        } => fencing_epoch,
        outcome => {
            return Err(AppError::Conflict(format!(
                "PR publication branch-update activation lost authority: {outcome:?}"
            )))
        }
    };
    task.internal_status = InternalStatus::UpdatingPlanBranch;
    let finalized = match workflow
        .execute_programmatic(
            Arc::clone(repository),
            Arc::clone(task_repo),
            repo_path,
            &operation_for_execution,
            InternalStatus::UpdatingPlanBranch,
            fencing_epoch,
        )
        .await?
    {
        crate::domain::state_machine::services::BranchUpdateWorkflowOutcome::ContinuationPending => {
            let pending = repository
                .get_operation(&operation_for_execution.id)
                .await?
                .ok_or_else(|| AppError::Conflict("Publication branch update disappeared".to_string()))?;
            workflow
                .publish_post_merge(
                    Arc::clone(repository),
                    repo_path,
                    &pending,
                    InternalStatus::UpdatingPlanBranch,
                )
                .await?;
            if let Some(plan_branch_repo) = services.plan_branch_repo.as_ref() {
                if let Some(plan_branch) =
                    super::merge_helpers::resolve_task_plan_branch_record(task, plan_branch_repo)
                        .await
                {
                    let _ = plan_branch_repo
                        .update_pr_push_status(
                            &plan_branch.id,
                            crate::domain::entities::plan_branch::PrPushStatus::Pushed,
                        )
                        .await;
                }
            }
            if let Some(stored) = task_repo.get_by_id(&task.id).await? {
                *task = stored;
            }
            true
        }
        crate::domain::state_machine::services::BranchUpdateWorkflowOutcome::NeedsAgent
        | crate::domain::state_machine::services::BranchUpdateWorkflowOutcome::Blocked => false,
        crate::domain::state_machine::services::BranchUpdateWorkflowOutcome::Completed { .. } => {
            return Err(AppError::Conflict(
                "PR publication branch update resumed an invalid continuation".to_string(),
            ));
        }
    };

    Ok(finalized)
}

// NOTE: The old `cleanup_branch_and_worktree_internal` function has been replaced
// by `deferred_merge_cleanup` (Phase 3). The old function ran synchronously during
// `complete_merge_internal`, blocking the merge result. The new function runs as a
// fire-and-forget `tokio::spawn` after the task is already marked Merged.

// ==================
// Pending cleanup metadata helpers
// ==================

/// Set `pending_cleanup` flag in task metadata.
///
/// Called by `complete_merge_internal` (Phase 2) so that Phase 3 cleanup
/// can be deferred and survive app restarts.
pub fn set_pending_cleanup_metadata(task: &mut Task) {
    let mut meta = task
        .metadata
        .as_deref()
        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if let Some(obj) = meta.as_object_mut() {
        obj.insert("pending_cleanup".to_string(), serde_json::json!(true));
    }
    task.metadata = Some(meta.to_string());
}

/// Check if task has `pending_cleanup` metadata flag set.
pub fn has_pending_cleanup_metadata(task: &Task) -> bool {
    task.metadata
        .as_deref()
        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .and_then(|v| v.get("pending_cleanup")?.as_bool())
        .unwrap_or(false)
}

/// Clear `pending_cleanup` flag from task metadata.
pub fn clear_pending_cleanup_metadata(task: &mut Task) {
    let mut meta = task
        .metadata
        .as_deref()
        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if let Some(obj) = meta.as_object_mut() {
        obj.remove("pending_cleanup");
    }
    task.metadata = Some(meta.to_string());
}

// ==================
// No-code-changes metadata helpers
// ==================

/// Set `no_code_changes` flag in task metadata.
///
/// Called by the `complete_review` handler when reviewer uses `approved_no_changes`
/// decision and git diff confirms no code changes. Marks that merge pipeline should
/// be skipped for this task.
pub fn set_no_code_changes_metadata(task: &mut Task) {
    let mut meta = task
        .metadata
        .as_deref()
        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if let Some(obj) = meta.as_object_mut() {
        obj.insert("no_code_changes".to_string(), serde_json::json!(true));
    }
    task.metadata = Some(meta.to_string());
}

/// Check if task has `no_code_changes` metadata flag set.
///
/// Used by `transition_handler/mod.rs` skip check to detect tasks that should
/// bypass the merge pipeline and transition directly from Approved → Merged.
pub fn has_no_code_changes_metadata(task: &Task) -> bool {
    task.metadata
        .as_deref()
        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .and_then(|v| v.get("no_code_changes")?.as_bool())
        .unwrap_or(false)
}

// ==================
// Phase 3: Deferred merge cleanup
// ==================

/// Fire-and-forget cleanup after merge completion (Phase 3).
///
/// Called via `tokio::spawn` after `complete_merge_internal` marks the task Merged.
/// Handles the slow operations that don't need to block the merge result:
/// - Kill worktree processes (SIGKILL)
/// - Delete worktree (rm -rf + prune)
/// - Delete task branch
/// - Clear task_branch/worktree_path fields
/// - Clear `pending_cleanup` metadata
///
/// All operations are non-fatal — the merge is already done. If cleanup fails,
/// it will be retried on app restart via `resume_pending_cleanup`.
pub async fn deferred_merge_cleanup(
    task_id: TaskId,
    task_repo: Arc<dyn TaskRepository>,
    project_working_dir: String,
    task_branch: Option<String>,
    worktree_path: Option<String>,
    plan_branch: Option<String>,
) {
    let task_id_str = task_id.as_str().to_string();
    let repo_path = Path::new(&project_working_dir);

    tracing::info!(
        task_id = %task_id_str,
        task_branch = ?task_branch,
        worktree_path = ?worktree_path,
        "Phase 3: starting deferred merge cleanup"
    );

    if let Some(ref wt_path_str) = worktree_path {
        let wt_path = match validate_absolute_non_root_path(
            Path::new(wt_path_str),
            "deferred merge cleanup worktree",
        ) {
            Ok(path) => path,
            Err(e) => {
                tracing::warn!(
                    task_id = %task_id_str,
                    worktree = %wt_path_str,
                    error = %e,
                    "Phase 3: worktree path failed validation; skipping cleanup to prevent work loss"
                );
                return;
            }
        };
        if wt_path.exists() {
            match GitService::uncommitted_change_summary(&wt_path).await {
                Ok(changes) if !changes.is_empty() => {
                    tracing::error!(
                        task_id = %task_id_str,
                        worktree = %wt_path_str,
                        changes = ?changes,
                        "Phase 3: worktree still has uncommitted changes; skipping cleanup to \
                         prevent work loss"
                    );
                    return;
                }
                Ok(_) => {}
                Err(e) => {
                    let empty_non_git_dir = std::fs::read_dir(&wt_path)
                        .map(|mut entries| entries.next().is_none())
                        .unwrap_or(false);
                    if empty_non_git_dir {
                        tracing::debug!(
                            task_id = %task_id_str,
                            worktree = %wt_path_str,
                            error = %e,
                            "Phase 3: empty non-git worktree path; continuing cleanup"
                        );
                    } else {
                        tracing::warn!(
                            task_id = %task_id_str,
                            worktree = %wt_path_str,
                            error = %e,
                            "Phase 3: could not verify worktree cleanliness; skipping cleanup to \
                             prevent work loss"
                        );
                        return;
                    }
                }
            }
        }
    }

    // Step 1: Kill worktree processes via SIGKILL (instant, no SIGTERM wait)
    // Wrapped in OS-thread timeout to prevent Phase 3 from stalling if kill hangs.
    let kill_step_timed_out = if let Some(ref wt_path_str) = worktree_path {
        let wt_path = PathBuf::from(wt_path_str);
        if wt_path.exists() {
            let lsof_timeout = git_runtime_config().worktree_lsof_timeout_secs;
            let kill_timeout_secs = git_runtime_config().step_0b_kill_timeout_secs;
            match super::cleanup_helpers::os_thread_timeout(
                std::time::Duration::from_secs(kill_timeout_secs),
                crate::domain::services::kill_worktree_processes_async(
                    &wt_path,
                    lsof_timeout,
                    true,
                ),
            )
            .await
            {
                Ok(_) => false,
                Err(_) => {
                    tracing::warn!(
                        task_id = %task_id_str,
                        kill_timeout_secs,
                        "Phase 3: deferred_merge_cleanup worktree kill timed out (OS-thread timeout)"
                    );
                    true
                }
            }
        } else {
            false
        }
    } else {
        false
    };

    // Step 2: Delete worktree via fast path (rm -rf + git worktree prune)
    if let Some(ref wt_path_str) = worktree_path {
        let wt_path = PathBuf::from(wt_path_str);
        if repo_path.exists() {
            if let Err(e) = super::cleanup_helpers::remove_worktree_fast(&wt_path, repo_path).await
            {
                tracing::warn!(
                    task_id = %task_id_str,
                    error = %e,
                    worktree = %wt_path_str,
                    "Phase 3: remove_worktree_fast failed (non-fatal)"
                );
            } else {
                tracing::info!(
                    task_id = %task_id_str,
                    worktree = %wt_path_str,
                    "Phase 3: worktree removed"
                );
            }
        }
    }

    // Step 3: Delete task branch — with merge guard to prevent work loss.
    // If plan_branch is provided, verify task commits landed on it before deleting.
    // Uses shared helper that handles both normal and squash merges.
    if let Some(ref branch) = task_branch {
        if repo_path.exists() {
            let safe_to_delete = match plan_branch.as_deref() {
                Some(pb) => {
                    let (safe, reason) =
                        GitService::is_branch_merged_or_content_equivalent(repo_path, branch, pb)
                            .await;
                    if safe {
                        tracing::debug!(
                            task_id = %task_id_str,
                            task_branch = %branch,
                            plan_branch = %pb,
                            reason = %reason,
                            "Phase 3: branch deletion guard passed"
                        );
                    } else {
                        tracing::warn!(
                            task_id = %task_id_str,
                            task_branch = %branch,
                            plan_branch = %pb,
                            reason = %reason,
                            "Phase 3: branch deletion guard: task content not found in plan HEAD \
                             — skipping deletion to prevent work loss"
                        );
                    }
                    safe
                }
                None => {
                    // No plan branch info — skip merge check (backward compat)
                    tracing::debug!(
                        task_id = %task_id_str,
                        "Phase 3: no plan_branch provided, skipping merge check"
                    );
                    true
                }
            };

            if safe_to_delete {
                match GitService::delete_branch(repo_path, branch, true).await {
                    Ok(_) => {
                        tracing::info!(
                            task_id = %task_id_str,
                            branch = %branch,
                            "Phase 3: task branch deleted"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            task_id = %task_id_str,
                            error = %e,
                            branch = %branch,
                            "Phase 3: failed to delete task branch (non-fatal)"
                        );
                    }
                }
            }
        }
    }

    // Step 4+5: Clear task_branch/worktree_path fields and pending_cleanup metadata.
    // Re-fetch task from DB for freshness (other operations may have updated it).
    match task_repo.get_by_id(&task_id).await {
        Ok(Some(mut fresh_task)) => {
            // Race condition guard: if task status changed from Merged (e.g.,
            // reconciler moved it to a different state), skip field clearing
            // to avoid clobbering a re-execution's branch/worktree assignment.
            if fresh_task.internal_status != InternalStatus::Merged {
                tracing::warn!(
                    task_id = %task_id_str,
                    actual_status = ?fresh_task.internal_status,
                    "Phase 3: task status changed from Merged, skipping field clearing"
                );
                // Still clear pending_cleanup to avoid infinite retry on startup
                clear_pending_cleanup_metadata(&mut fresh_task);
                fresh_task.touch();
                let _ = task_repo.update(&fresh_task).await;
                return;
            }

            fresh_task.task_branch = None;
            fresh_task.worktree_path = None;
            clear_pending_cleanup_metadata(&mut fresh_task);

            // Persist cleanup timeout metadata if the worktree kill step timed out.
            // Recorded on the Merged task for post-mortem visibility.
            if kill_step_timed_out {
                let mut meta: serde_json::Value = fresh_task
                    .metadata
                    .as_deref()
                    .and_then(|m| serde_json::from_str(m).ok())
                    .unwrap_or_else(|| serde_json::json!({}));
                if let Some(obj) = meta.as_object_mut() {
                    obj.insert(
                        "merge_failure_source".to_string(),
                        serde_json::to_value(MergeFailureSource::CleanupTimeout)
                            .unwrap_or_default(),
                    );
                    obj.insert(
                        "cleanup_phase".to_string(),
                        serde_json::to_value(CleanupPhase::DeferredWorktreeKill)
                            .unwrap_or_default(),
                    );
                }
                fresh_task.metadata = Some(meta.to_string());
            }

            fresh_task.touch();

            if let Err(e) = task_repo.update(&fresh_task).await {
                tracing::warn!(
                    task_id = %task_id_str,
                    error = %e,
                    "Phase 3: failed to clear task fields after cleanup (non-fatal)"
                );
            } else {
                tracing::info!(
                    task_id = %task_id_str,
                    "Phase 3: deferred merge cleanup complete"
                );
            }
        }
        Ok(None) => {
            tracing::warn!(
                task_id = %task_id_str,
                "Phase 3: task not found in DB (may have been deleted)"
            );
        }
        Err(e) => {
            tracing::warn!(
                task_id = %task_id_str,
                error = %e,
                "Phase 3: failed to re-fetch task for cleanup (non-fatal)"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::{
        IdeationSessionId, InternalStatus, MergeStrategy, Project, ProjectId, Task, TaskCategory,
    };
    use crate::domain::repositories::external_events_repository::ExternalEventsRepository;
    use crate::domain::state_machine::services::MockWebhookPublisher;
    use crate::infrastructure::memory::{MemoryExternalEventsRepository, MemoryTaskRepository};

    fn make_test_repo() -> (tempfile::TempDir, String) {
        let dir = tempfile::TempDir::new().expect("create temp dir");
        let path = dir.path();
        for args in [
            vec!["init", "-b", "main"],
            vec!["config", "user.email", "t@t.com"],
            vec!["config", "user.name", "T"],
        ] {
            let _ = std::process::Command::new("git")
                .args(&args)
                .current_dir(path)
                .output();
        }
        std::fs::write(path.join("README.md"), "# test").unwrap();
        for args in [vec!["add", "."], vec!["commit", "-m", "init"]] {
            let _ = std::process::Command::new("git")
                .args(&args)
                .current_dir(path)
                .output();
        }
        let path_str = path.to_string_lossy().to_string();
        (dir, path_str)
    }

    /// V2 fix: complete_merge_internal must return Err when git verification fails,
    /// NOT fall through to set Merged status.
    ///
    /// Before the fix: Err from is_commit_on_branch was treated as non-fatal, and
    /// the function proceeded to set task.internal_status = Merged. This allowed
    /// ghost merges when git verification was unavailable or errored.
    ///
    /// After the fix: Err returns AppError::GitOperation — task stays in its prior
    /// state, reconciliation retries, data integrity is preserved.
    #[tokio::test]
    async fn complete_merge_internal_returns_err_when_git_verification_fails() {
        let (_dir, repo_path_str) = make_test_repo();

        let task_repo = Arc::new(MemoryTaskRepository::new());
        let project_id = ProjectId::from_string("proj-v2".to_string());

        let mut task = Task::new(project_id.clone(), "V2 test task".to_string());
        task.internal_status = InternalStatus::PendingMerge;
        let _task_id = task.id.clone();
        task_repo.create(task.clone()).await.unwrap();

        let mut project = Project::new("v2-project".to_string(), repo_path_str);
        project.id = project_id;
        project.base_branch = Some("main".to_string());
        project.merge_strategy = MergeStrategy::Merge;

        // Pass an INVALID commit SHA — git verification will return Err (not Ok(false)),
        // because `git merge-base --is-ancestor invalid_sha main` exits with a non-0/1 code.
        let invalid_sha = "0000000000000000000000000000000000000000";
        let task_repo_arc: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());
        task_repo_arc.create(task.clone()).await.unwrap();

        let result = complete_merge_internal(
            &mut task,
            &project,
            invalid_sha,
            "",
            "main",
            &task_repo_arc,
            None,
            None,
            None,
            None,
        )
        .await;

        // Must return Err — git verification failed
        assert!(
            result.is_err(),
            "complete_merge_internal must return Err when git verification fails (V2 fix). \
             Got Ok(()) which means Merged status was set without confirmation."
        );

        // Task status must NOT have been updated to Merged
        assert_ne!(
            task.internal_status,
            InternalStatus::Merged,
            "Task internal_status must NOT be Merged when git verification fails. \
             Got {:?}",
            task.internal_status,
        );
    }

    #[tokio::test]
    async fn complete_merge_internal_emits_external_plan_events() {
        let (_dir, repo_path_str) = make_test_repo();
        let repo_path = Path::new(&repo_path_str);
        let head_sha_output = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo_path)
            .output()
            .expect("git rev-parse HEAD");
        let head_sha = String::from_utf8_lossy(&head_sha_output.stdout)
            .trim()
            .to_string();

        let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());
        let external_events_repo: Arc<dyn ExternalEventsRepository> =
            Arc::new(MemoryExternalEventsRepository::new());
        let webhook_publisher: Arc<dyn WebhookPublisher> = Arc::new(MockWebhookPublisher);

        let project_id = ProjectId::from_string("proj-plan-events".to_string());
        let session_id = IdeationSessionId::new();
        let mut task = Task::new(project_id.clone(), "Plan merge event task".to_string());
        task.internal_status = InternalStatus::PendingMerge;
        task.category = TaskCategory::PlanMerge;
        task.ideation_session_id = Some(session_id.clone());
        task_repo.create(task.clone()).await.unwrap();

        let mut project = Project::new("Plan Event Project".to_string(), repo_path_str);
        project.id = project_id.clone();
        project.base_branch = Some("main".to_string());
        project.merge_strategy = MergeStrategy::Merge;

        complete_merge_internal(
            &mut task,
            &project,
            &head_sha,
            "task/plan-events",
            "main",
            &task_repo,
            Some(&external_events_repo),
            Some(&webhook_publisher),
            None,
            Some("Plan event session".to_string()),
        )
        .await
        .unwrap();

        let events = external_events_repo
            .get_events_after_cursor(&[project_id.to_string()], 0, 10)
            .await
            .unwrap();
        let event_types: Vec<_> = events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect();

        assert!(event_types.contains(&"merge:completed"));
        assert!(event_types.contains(&"task:status_changed"));
        assert!(event_types.contains(&"plan:delivered"));
        assert!(events
            .iter()
            .any(|event| event.payload.contains(session_id.as_str())));
    }

    /// Fix #3: When pre_merge_cleanup already deleted the worktree,
    /// deferred_merge_cleanup should skip worktree deletion but still
    /// delete the branch and clear task fields.
    #[tokio::test]
    async fn test_deferred_cleanup_skips_already_deleted_worktree() {
        let (_dir, repo_path_str) = make_test_repo();
        let repo_path = Path::new(&repo_path_str);

        // Create a task branch, then return to main so force-delete works
        let branch_name = "task/fix3-test";
        let _ = std::process::Command::new("git")
            .args(["checkout", "-b", branch_name])
            .current_dir(repo_path)
            .output();
        let _ = std::process::Command::new("git")
            .args(["checkout", "main"])
            .current_dir(repo_path)
            .output();

        // Verify branch exists before cleanup
        let branch_check = std::process::Command::new("git")
            .args(["branch", "--list", branch_name])
            .current_dir(repo_path)
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&branch_check.stdout).contains(branch_name),
            "Branch should exist before cleanup"
        );

        let task_repo = Arc::new(MemoryTaskRepository::new());
        let task_repo_dyn: Arc<dyn TaskRepository> =
            Arc::clone(&task_repo) as Arc<dyn TaskRepository>;
        let project_id = ProjectId::from_string("proj-fix3".to_string());

        let mut task = Task::new(project_id.clone(), "Fix3 test".to_string());
        task.internal_status = InternalStatus::Merged;
        task.task_branch = Some(branch_name.to_string());
        // Non-existent worktree path — simulates pre_merge_cleanup already deleted it
        task.worktree_path = Some("/tmp/nonexistent-worktree-fix3-test".to_string());
        set_pending_cleanup_metadata(&mut task);
        let task_id = task.id.clone();
        task_repo.create(task).await.unwrap();

        deferred_merge_cleanup(
            task_id.clone(),
            task_repo_dyn,
            repo_path_str.clone(),
            Some(branch_name.to_string()),
            Some("/tmp/nonexistent-worktree-fix3-test".to_string()),
            None,
        )
        .await;

        // Re-fetch task from repo to check DB state
        let updated_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();

        // Both fields must be cleared after cleanup
        assert!(
            updated_task.worktree_path.is_none(),
            "worktree_path should be None after cleanup (was already deleted)"
        );
        assert!(
            updated_task.task_branch.is_none(),
            "task_branch should be None after cleanup"
        );
        // pending_cleanup must be cleared
        assert!(!has_pending_cleanup_metadata(&updated_task));

        // Branch must be deleted from the real git repo
        let branch_check_after = std::process::Command::new("git")
            .args(["branch", "--list", branch_name])
            .current_dir(repo_path)
            .output()
            .unwrap();
        assert!(
            !String::from_utf8_lossy(&branch_check_after.stdout).contains(branch_name),
            "Branch should be deleted from git after cleanup"
        );
    }

    #[tokio::test]
    async fn test_deferred_cleanup_preserves_dirty_worktree() {
        let (_dir, repo_path_str) = make_test_repo();
        let repo_path = validate_absolute_non_root_path(
            Path::new(&repo_path_str),
            "dirty cleanup test repository",
        )
        .unwrap();
        let worktree_parent = tempfile::TempDir::new().expect("create worktree parent");

        let branch_name = "task/dirty-worktree-test";
        let worktree_path = validate_absolute_non_root_path(
            &worktree_parent.path().join("dirty-worktree"),
            "dirty cleanup test worktree",
        )
        .unwrap();
        let worktree_path_str = worktree_path.to_string_lossy().to_string();

        let _ = std::process::Command::new("git")
            .args(["branch", branch_name])
            .current_dir(&repo_path)
            .output();
        let add_worktree = std::process::Command::new("git")
            .args(["worktree", "add", &worktree_path_str, branch_name])
            .current_dir(&repo_path)
            .output()
            .expect("git worktree add");
        assert!(add_worktree.status.success());

        let dirty_file = validate_absolute_non_root_path(
            &worktree_path.join("uncommitted_source.rs"),
            "dirty cleanup test source",
        )
        .unwrap();
        std::fs::write(dirty_file, "fn work() {}\n").unwrap();

        let task_repo = Arc::new(MemoryTaskRepository::new());
        let task_repo_dyn: Arc<dyn TaskRepository> =
            Arc::clone(&task_repo) as Arc<dyn TaskRepository>;
        let project_id = ProjectId::from_string("proj-dirty-cleanup".to_string());

        let mut task = Task::new(project_id, "Dirty cleanup test".to_string());
        task.internal_status = InternalStatus::Merged;
        task.task_branch = Some(branch_name.to_string());
        task.worktree_path = Some(worktree_path_str.clone());
        set_pending_cleanup_metadata(&mut task);
        let task_id = task.id.clone();
        task_repo.create(task).await.unwrap();

        deferred_merge_cleanup(
            task_id.clone(),
            task_repo_dyn,
            repo_path_str.clone(),
            Some(branch_name.to_string()),
            Some(worktree_path_str.clone()),
            None,
        )
        .await;

        assert!(worktree_path.exists());

        let updated_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
        assert_eq!(
            updated_task.worktree_path.as_deref(),
            Some(worktree_path_str.as_str())
        );
        assert_eq!(updated_task.task_branch.as_deref(), Some(branch_name));
        assert!(has_pending_cleanup_metadata(&updated_task));

        let branch_check = std::process::Command::new("git")
            .args(["branch", "--list", branch_name])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&branch_check.stdout).contains(branch_name),
            "dirty cleanup must not delete task branch"
        );
    }

    #[tokio::test]
    async fn test_deferred_cleanup_preserves_invalid_worktree_path() {
        let (_dir, repo_path_str) = make_test_repo();
        let repo_path = Path::new(&repo_path_str);

        let branch_name = "task/invalid-worktree-path-test";
        let _ = std::process::Command::new("git")
            .args(["branch", branch_name])
            .current_dir(repo_path)
            .output();

        let task_repo = Arc::new(MemoryTaskRepository::new());
        let task_repo_dyn: Arc<dyn TaskRepository> =
            Arc::clone(&task_repo) as Arc<dyn TaskRepository>;
        let project_id = ProjectId::from_string("proj-invalid-cleanup".to_string());

        let mut task = Task::new(project_id, "Invalid cleanup path test".to_string());
        task.internal_status = InternalStatus::Merged;
        task.task_branch = Some(branch_name.to_string());
        task.worktree_path = Some("relative-worktree-path".to_string());
        set_pending_cleanup_metadata(&mut task);
        let task_id = task.id.clone();
        task_repo.create(task).await.unwrap();

        deferred_merge_cleanup(
            task_id.clone(),
            task_repo_dyn,
            repo_path_str.clone(),
            Some(branch_name.to_string()),
            Some("relative-worktree-path".to_string()),
            None,
        )
        .await;

        let updated_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
        assert_eq!(
            updated_task.worktree_path.as_deref(),
            Some("relative-worktree-path")
        );
        assert_eq!(updated_task.task_branch.as_deref(), Some(branch_name));
        assert!(has_pending_cleanup_metadata(&updated_task));

        let branch_check = std::process::Command::new("git")
            .args(["branch", "--list", branch_name])
            .current_dir(repo_path)
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&branch_check.stdout).contains(branch_name),
            "branch should still exist after invalid worktree cleanup path"
        );
    }

    #[tokio::test]
    async fn test_deferred_cleanup_removes_empty_non_git_worktree_dir() {
        let (_dir, repo_path_str) = make_test_repo();
        let repo_path = Path::new(&repo_path_str);
        let worktree_parent = tempfile::TempDir::new().expect("create worktree parent");

        let branch_name = "task/empty-non-git-worktree-test";
        let _ = std::process::Command::new("git")
            .args(["branch", branch_name])
            .current_dir(repo_path)
            .output();
        let worktree_path = worktree_parent.path().join("empty-non-git-worktree");
        std::fs::create_dir_all(&worktree_path).expect("create empty non-git worktree dir");
        let worktree_path_str = worktree_path.to_string_lossy().to_string();

        let task_repo = Arc::new(MemoryTaskRepository::new());
        let task_repo_dyn: Arc<dyn TaskRepository> =
            Arc::clone(&task_repo) as Arc<dyn TaskRepository>;
        let project_id = ProjectId::from_string("proj-empty-non-git-cleanup".to_string());

        let mut task = Task::new(project_id, "Empty non-git cleanup test".to_string());
        task.internal_status = InternalStatus::Merged;
        task.task_branch = Some(branch_name.to_string());
        task.worktree_path = Some(worktree_path_str.clone());
        set_pending_cleanup_metadata(&mut task);
        let task_id = task.id.clone();
        task_repo.create(task).await.unwrap();

        deferred_merge_cleanup(
            task_id.clone(),
            task_repo_dyn,
            repo_path_str.clone(),
            Some(branch_name.to_string()),
            Some(worktree_path_str),
            None,
        )
        .await;

        let updated_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
        assert!(updated_task.worktree_path.is_none());
        assert!(updated_task.task_branch.is_none());
        assert!(!has_pending_cleanup_metadata(&updated_task));
        assert!(
            !worktree_path.exists(),
            "empty non-git worktree dir should be removed"
        );

        let branch_check = std::process::Command::new("git")
            .args(["branch", "--list", branch_name])
            .current_dir(repo_path)
            .output()
            .unwrap();
        assert!(
            !String::from_utf8_lossy(&branch_check.stdout).contains(branch_name),
            "empty non-git cleanup should delete task branch"
        );
    }

    #[tokio::test]
    async fn test_deferred_cleanup_preserves_non_empty_non_git_worktree_dir() {
        let (_dir, repo_path_str) = make_test_repo();
        let repo_path = Path::new(&repo_path_str);
        let worktree_parent = tempfile::TempDir::new().expect("create worktree parent");

        let branch_name = "task/non-empty-non-git-worktree-test";
        let _ = std::process::Command::new("git")
            .args(["branch", branch_name])
            .current_dir(repo_path)
            .output();
        let worktree_path = worktree_parent.path().join("non-empty-non-git-worktree");
        std::fs::create_dir_all(&worktree_path).expect("create non-git worktree dir");
        std::fs::write(worktree_path.join("untracked.txt"), "preserve me\n")
            .expect("write non-git worktree file");
        let worktree_path_str = worktree_path.to_string_lossy().to_string();

        let task_repo = Arc::new(MemoryTaskRepository::new());
        let task_repo_dyn: Arc<dyn TaskRepository> =
            Arc::clone(&task_repo) as Arc<dyn TaskRepository>;
        let project_id = ProjectId::from_string("proj-non-empty-non-git-cleanup".to_string());

        let mut task = Task::new(project_id, "Non-empty non-git cleanup test".to_string());
        task.internal_status = InternalStatus::Merged;
        task.task_branch = Some(branch_name.to_string());
        task.worktree_path = Some(worktree_path_str.clone());
        set_pending_cleanup_metadata(&mut task);
        let task_id = task.id.clone();
        task_repo.create(task).await.unwrap();

        deferred_merge_cleanup(
            task_id.clone(),
            task_repo_dyn,
            repo_path_str.clone(),
            Some(branch_name.to_string()),
            Some(worktree_path_str.clone()),
            None,
        )
        .await;

        let updated_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
        assert_eq!(
            updated_task.worktree_path.as_deref(),
            Some(worktree_path_str.as_str())
        );
        assert_eq!(updated_task.task_branch.as_deref(), Some(branch_name));
        assert!(has_pending_cleanup_metadata(&updated_task));
        assert!(
            worktree_path.exists(),
            "non-empty non-git worktree dir must be preserved"
        );

        let branch_check = std::process::Command::new("git")
            .args(["branch", "--list", branch_name])
            .current_dir(repo_path)
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&branch_check.stdout).contains(branch_name),
            "non-empty non-git cleanup must not delete task branch"
        );
    }

    #[tokio::test]
    async fn test_deferred_cleanup_removes_clean_worktree() {
        let (_dir, repo_path_str) = make_test_repo();
        let repo_path = validate_absolute_non_root_path(
            Path::new(&repo_path_str),
            "clean cleanup test repository",
        )
        .unwrap();
        let worktree_parent = tempfile::TempDir::new().expect("create worktree parent");

        let branch_name = "task/clean-worktree-test";
        let worktree_path = validate_absolute_non_root_path(
            &worktree_parent.path().join("clean-worktree"),
            "clean cleanup test worktree",
        )
        .unwrap();
        let worktree_path_str = worktree_path.to_string_lossy().to_string();

        let _ = std::process::Command::new("git")
            .args(["branch", branch_name])
            .current_dir(&repo_path)
            .output();
        let add_worktree = std::process::Command::new("git")
            .args(["worktree", "add", &worktree_path_str, branch_name])
            .current_dir(&repo_path)
            .output()
            .expect("git worktree add");
        assert!(add_worktree.status.success());

        let task_repo = Arc::new(MemoryTaskRepository::new());
        let task_repo_dyn: Arc<dyn TaskRepository> =
            Arc::clone(&task_repo) as Arc<dyn TaskRepository>;
        let project_id = ProjectId::from_string("proj-clean-cleanup".to_string());

        let mut task = Task::new(project_id, "Clean cleanup test".to_string());
        task.internal_status = InternalStatus::Merged;
        task.task_branch = Some(branch_name.to_string());
        task.worktree_path = Some(worktree_path_str.clone());
        set_pending_cleanup_metadata(&mut task);
        let task_id = task.id.clone();
        task_repo.create(task).await.unwrap();

        deferred_merge_cleanup(
            task_id.clone(),
            task_repo_dyn,
            repo_path_str.clone(),
            Some(branch_name.to_string()),
            Some(worktree_path_str.clone()),
            None,
        )
        .await;

        assert!(!worktree_path.exists());

        let updated_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
        assert!(updated_task.worktree_path.is_none());
        assert!(updated_task.task_branch.is_none());
        assert!(!has_pending_cleanup_metadata(&updated_task));

        let branch_check = std::process::Command::new("git")
            .args(["branch", "--list", branch_name])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        assert!(
            !String::from_utf8_lossy(&branch_check.stdout).contains(branch_name),
            "clean cleanup should delete task branch"
        );
    }

    // ===== No-code-changes metadata helper tests =====

    #[test]
    fn test_set_no_code_changes_metadata_sets_flag() {
        let project_id = ProjectId::from_string("proj-test".to_string());
        let mut task = Task::new(project_id, "test task".to_string());
        assert!(!has_no_code_changes_metadata(&task));

        set_no_code_changes_metadata(&mut task);
        assert!(has_no_code_changes_metadata(&task));
    }

    #[test]
    fn test_has_no_code_changes_metadata_false_for_empty_metadata() {
        let project_id = ProjectId::from_string("proj-test2".to_string());
        let task = Task::new(project_id, "test task".to_string());
        // Task has no metadata set
        assert!(!has_no_code_changes_metadata(&task));
    }

    #[test]
    fn test_has_no_code_changes_metadata_false_for_none_metadata() {
        let project_id = ProjectId::from_string("proj-test3".to_string());
        let mut task = Task::new(project_id, "test task".to_string());
        task.metadata = None;
        assert!(!has_no_code_changes_metadata(&task));
    }

    #[test]
    fn test_set_no_code_changes_metadata_preserves_existing_metadata() {
        let project_id = ProjectId::from_string("proj-test4".to_string());
        let mut task = Task::new(project_id, "test task".to_string());
        // Pre-set some existing metadata
        task.metadata = Some(r#"{"existing_key": "existing_value"}"#.to_string());

        set_no_code_changes_metadata(&mut task);

        assert!(has_no_code_changes_metadata(&task));
        // Existing key should still be there
        let meta: serde_json::Value =
            serde_json::from_str(task.metadata.as_deref().unwrap()).unwrap();
        assert_eq!(meta["existing_key"], "existing_value");
    }

    #[test]
    fn test_set_no_code_changes_and_pending_cleanup_coexist() {
        let project_id = ProjectId::from_string("proj-test5".to_string());
        let mut task = Task::new(project_id, "test task".to_string());

        set_no_code_changes_metadata(&mut task);
        set_pending_cleanup_metadata(&mut task);

        assert!(has_no_code_changes_metadata(&task));
        assert!(has_pending_cleanup_metadata(&task));
    }

    // ===== Ancestor guard tests for branch deletion =====

    /// When the task branch is NOT an ancestor of the plan branch, the branch
    /// must NOT be deleted — it may still contain work not yet on the plan branch.
    #[tokio::test]
    async fn test_deferred_cleanup_skips_deletion_when_not_ancestor() {
        let (_dir, repo_path_str) = make_test_repo();
        let repo_path = Path::new(&repo_path_str);

        let task_branch = "task/not-ancestor-test";
        let plan_branch = "plan/not-ancestor-test";

        // Create task branch from main and add a unique commit not on plan
        let _ = std::process::Command::new("git")
            .args(["checkout", "-b", task_branch])
            .current_dir(repo_path)
            .output();
        std::fs::write(repo_path.join("task_only_file.md"), "task work").unwrap();
        for args in [
            vec!["add", "."],
            vec!["commit", "-m", "task: task-only work"],
        ] {
            let _ = std::process::Command::new("git")
                .args(&args)
                .current_dir(repo_path)
                .output();
        }

        // Checkout main, create plan branch from main (WITHOUT the task commit)
        let _ = std::process::Command::new("git")
            .args(["checkout", "main"])
            .current_dir(repo_path)
            .output();
        let _ = std::process::Command::new("git")
            .args(["checkout", "-b", plan_branch])
            .current_dir(repo_path)
            .output();
        let _ = std::process::Command::new("git")
            .args(["checkout", "main"])
            .current_dir(repo_path)
            .output();

        // Sanity check: task branch should NOT be an ancestor of plan branch
        let task_is_ancestor = std::process::Command::new("git")
            .args(["merge-base", "--is-ancestor", task_branch, plan_branch])
            .current_dir(repo_path)
            .output()
            .unwrap();
        assert!(
            !task_is_ancestor.status.success(),
            "Task should NOT be ancestor of plan for this test"
        );

        let task_repo = Arc::new(MemoryTaskRepository::new());
        let task_repo_dyn: Arc<dyn TaskRepository> =
            Arc::clone(&task_repo) as Arc<dyn TaskRepository>;
        let project_id = ProjectId::from_string("proj-ancestor-guard".to_string());

        let mut task = Task::new(project_id, "Ancestor guard test".to_string());
        task.internal_status = InternalStatus::Merged;
        task.task_branch = Some(task_branch.to_string());
        set_pending_cleanup_metadata(&mut task);
        let task_id = task.id.clone();
        task_repo.create(task).await.unwrap();

        deferred_merge_cleanup(
            task_id.clone(),
            task_repo_dyn,
            repo_path_str.clone(),
            Some(task_branch.to_string()),
            None, // no worktree
            Some(plan_branch.to_string()),
        )
        .await;

        // CRITICAL: task branch must still exist in git (not deleted)
        let branch_check = std::process::Command::new("git")
            .args(["branch", "--list", task_branch])
            .current_dir(repo_path)
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&branch_check.stdout).contains(task_branch),
            "Task branch should NOT be deleted when task is not ancestor of plan branch"
        );
    }

    /// When the task branch IS an ancestor of the plan branch (i.e. it was merged in),
    /// the branch should be deleted as normal.
    #[tokio::test]
    async fn test_deferred_cleanup_deletes_branch_when_ancestor() {
        let (_dir, repo_path_str) = make_test_repo();
        let repo_path = Path::new(&repo_path_str);

        let task_branch = "task/ancestor-yes-test";
        let plan_branch = "plan/ancestor-yes-test";

        // Create plan branch from main
        let _ = std::process::Command::new("git")
            .args(["checkout", "-b", plan_branch])
            .current_dir(repo_path)
            .output();

        // Create task branch from plan and add a commit
        let _ = std::process::Command::new("git")
            .args(["checkout", "-b", task_branch])
            .current_dir(repo_path)
            .output();
        std::fs::write(repo_path.join("task_work.md"), "work").unwrap();
        for args in [vec!["add", "."], vec!["commit", "-m", "task: work"]] {
            let _ = std::process::Command::new("git")
                .args(&args)
                .current_dir(repo_path)
                .output();
        }

        // Merge task into plan (task is now an ancestor of plan)
        let _ = std::process::Command::new("git")
            .args(["checkout", plan_branch])
            .current_dir(repo_path)
            .output();
        let _ = std::process::Command::new("git")
            .args(["merge", "--no-ff", task_branch, "-m", "Merge task"])
            .current_dir(repo_path)
            .output();

        // Return to main so force-delete works
        let _ = std::process::Command::new("git")
            .args(["checkout", "main"])
            .current_dir(repo_path)
            .output();

        let task_repo = Arc::new(MemoryTaskRepository::new());
        let task_repo_dyn: Arc<dyn TaskRepository> =
            Arc::clone(&task_repo) as Arc<dyn TaskRepository>;
        let project_id = ProjectId::from_string("proj-ancestor-yes".to_string());

        let mut task = Task::new(project_id, "Ancestor yes test".to_string());
        task.internal_status = InternalStatus::Merged;
        task.task_branch = Some(task_branch.to_string());
        set_pending_cleanup_metadata(&mut task);
        let task_id = task.id.clone();
        task_repo.create(task).await.unwrap();

        deferred_merge_cleanup(
            task_id.clone(),
            task_repo_dyn,
            repo_path_str.clone(),
            Some(task_branch.to_string()),
            None, // no worktree
            Some(plan_branch.to_string()),
        )
        .await;

        // Task branch SHOULD be deleted (task IS ancestor of plan)
        let branch_check = std::process::Command::new("git")
            .args(["branch", "--list", task_branch])
            .current_dir(repo_path)
            .output()
            .unwrap();
        assert!(
            !String::from_utf8_lossy(&branch_check.stdout).contains(task_branch),
            "Task branch SHOULD be deleted when task is ancestor of plan branch"
        );
    }

    /// Backward compatibility: when no plan_branch is provided, the ancestor check
    /// is skipped and the branch is deleted unconditionally.
    #[tokio::test]
    async fn test_deferred_cleanup_proceeds_when_no_plan_branch() {
        let (_dir, repo_path_str) = make_test_repo();
        let repo_path = Path::new(&repo_path_str);

        let task_branch = "task/no-plan-branch-test";

        // Create task branch and return to main
        let _ = std::process::Command::new("git")
            .args(["checkout", "-b", task_branch])
            .current_dir(repo_path)
            .output();
        let _ = std::process::Command::new("git")
            .args(["checkout", "main"])
            .current_dir(repo_path)
            .output();

        let task_repo = Arc::new(MemoryTaskRepository::new());
        let task_repo_dyn: Arc<dyn TaskRepository> =
            Arc::clone(&task_repo) as Arc<dyn TaskRepository>;
        let project_id = ProjectId::from_string("proj-no-plan".to_string());

        let mut task = Task::new(project_id, "No plan branch test".to_string());
        task.internal_status = InternalStatus::Merged;
        task.task_branch = Some(task_branch.to_string());
        set_pending_cleanup_metadata(&mut task);
        let task_id = task.id.clone();
        task_repo.create(task).await.unwrap();

        deferred_merge_cleanup(
            task_id.clone(),
            task_repo_dyn,
            repo_path_str.clone(),
            Some(task_branch.to_string()),
            None,
            None, // No plan branch — backward compat: skip check, proceed with deletion
        )
        .await;

        // Branch should be deleted (no plan branch = skip check = proceed)
        let branch_check = std::process::Command::new("git")
            .args(["branch", "--list", task_branch])
            .current_dir(repo_path)
            .output()
            .unwrap();
        assert!(
            !String::from_utf8_lossy(&branch_check.stdout).contains(task_branch),
            "Task branch should be deleted when no plan_branch is provided (backward compat)"
        );
    }
}
