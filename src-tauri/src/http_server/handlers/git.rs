//! Git HTTP handlers for merge operations
//!
//! Provides HTTP endpoints for the merger agent to signal merge completion or conflicts.
//! Also provides query endpoints for commits and diff statistics.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Emitter;

use super::*;
use crate::application::chat_service::freshness_routing::{
    freshness_return_route, FreshnessRouteResult,
};
use crate::application::{GitService, TaskTransitionService};
use crate::domain::entities::{task_metadata::MergeFailureSource, InternalStatus, TaskId};
use crate::domain::state_machine::resolve_merge_branches;
use crate::domain::state_machine::services::TaskScheduler;
use crate::domain::state_machine::transition_handler::{parse_metadata, set_source_conflict_resolved};

// ============================================================================
// Request/Response Types
// ============================================================================

/// Request to complete a merge after agent resolution
#[derive(Debug, serde::Deserialize)]
pub struct CompleteMergeRequest {
    /// SHA of the merge commit
    pub commit_sha: String,
}

/// Request to report unresolvable conflict
#[derive(Debug, serde::Deserialize)]
pub struct ReportConflictRequest {
    /// List of files with unresolved conflicts
    pub conflict_files: Vec<String>,
    /// Explanation of why conflicts could not be resolved
    pub reason: String,
}

/// Request to report a non-conflict merge failure
#[derive(Debug, serde::Deserialize)]
pub struct ReportIncompleteRequest {
    /// Detailed explanation of why merge failed
    pub reason: String,
    /// Optional diagnostic info (git status, logs, etc.)
    pub diagnostic_info: Option<String>,
}

/// Response for merge operations
#[derive(Debug, serde::Serialize)]
pub struct MergeOperationResponse {
    pub success: bool,
    pub message: String,
    pub new_status: String,
}

/// Commit information for get_task_commits
#[derive(Debug, serde::Serialize)]
pub struct CommitInfoResponse {
    pub sha: String,
    pub short_sha: String,
    pub message: String,
    pub author: String,
    pub timestamp: String,
}

/// Diff statistics for get_task_diff_stats
#[derive(Debug, serde::Serialize)]
pub struct DiffStatsResponse {
    pub files_changed: u32,
    pub insertions: u32,
    pub deletions: u32,
    pub changed_files: Vec<String>,
}

/// Merge target branches for get_merge_target
#[derive(Debug, serde::Serialize)]
pub struct MergeTargetResponse {
    pub source_branch: String,
    pub target_branch: String,
}

// ============================================================================
// Handlers
// ============================================================================

/// POST /api/git/tasks/{id}/complete-merge
///
/// Called by merger agent when conflicts have been successfully resolved.
/// Transitions task from Merging → Merged and triggers cleanup.
pub async fn complete_merge(
    State(state): State<HttpServerState>,
    Path(task_id): Path<String>,
    Json(req): Json<CompleteMergeRequest>,
) -> Result<Json<MergeOperationResponse>, JsonError> {
    // 1. Validate SHA format (must be 40 hex characters)
    if !is_valid_git_sha(&req.commit_sha) {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "commit_sha must be a full 40-character SHA (use `git rev-parse HEAD`)",
            None,
        ));
    }

    let task_id = TaskId::from_string(task_id);

    // 2. Get task
    let mut task = state
        .app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .map_err(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string(), None))?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Task not found", None))?;

    // 3. Idempotent: if already merged, return success
    if task.internal_status == InternalStatus::Merged {
        return Ok(Json(MergeOperationResponse {
            success: true,
            message: "Merge already completed".to_string(),
            new_status: "already_merged".to_string(),
        }));
    }

    // 4. Validate state is Merging
    if task.internal_status != InternalStatus::Merging {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            format!(
                "Task must be in 'merging' status to complete merge. Current status: {}",
                task.internal_status.as_str()
            ),
            None,
        ));
    }

    // 5. Get project for cleanup info and verification
    let project = state
        .app_state
        .project_repo
        .get_by_id(&task.project_id)
        .await
        .map_err(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string(), None))?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Project not found", None))?;

    // Hoist transition service construction — reused by all early-return paths below
    // (freshness intercept, rebase retry, source-update retry, and normal Merged path).
    let transition_service = build_transition_service(&state);

    // 5a. Freshness intercept — MUST fire before step 6 (merge_commit_sha) and before
    //     the rebase check. If plan_update_conflict=true, the merger resolved a plan←main
    //     freshness conflict (not the actual task merge) and we must route back to the
    //     origin state instead of completing the merge and losing the task's work.
    match freshness_return_route(
        &task,
        Arc::clone(&state.app_state.task_repo),
        &transition_service,
        &project,
        Some(&*state.app_state.interactive_process_registry),
    )
    .await
    .map_err(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string(), None))?
    {
        FreshnessRouteResult::FreshnessRouted(origin_state) => {
            return Ok(Json(MergeOperationResponse {
                success: true,
                message: "Freshness conflict resolved, routing back to origin state".to_string(),
                new_status: origin_state,
            }));
        }
        FreshnessRouteResult::NormalMerge => {}
    }

    // 5b. Check for rebase conflict resolution — if so, transition back to PendingMerge
    //     so attempt_programmatic_merge re-runs (source is now rebased → merge fast-forwards)
    let merge_worktree_path = task
        .worktree_path
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&project.working_directory));
    let is_rebase_conflict = GitService::is_rebase_in_progress(&merge_worktree_path);
    tracing::info!(
        task_id = task_id.as_str(),
        is_rebase_conflict = is_rebase_conflict,
        worktree_path = %merge_worktree_path.display(),
        "is_rebase_conflict detected from git state: {}",
        is_rebase_conflict
    );

    if is_rebase_conflict {
        tracing::info!(
            task_id = task_id.as_str(),
            "Rebase conflict resolved by agent, transitioning back to PendingMerge for merge step"
        );

        // Delete rebase worktree
        let repo_path = PathBuf::from(&project.working_directory);
        if let Some(ref worktree_path) = task.worktree_path {
            let wt_path = PathBuf::from(worktree_path);
            if wt_path.exists() {
                if let Err(e) = GitService::delete_worktree(&repo_path, &wt_path).await {
                    tracing::warn!(
                        task_id = task_id.as_str(),
                        error = %e,
                        worktree = %wt_path.display(),
                        "Failed to delete rebase worktree — pre_merge_cleanup will handle it"
                    );
                }
            }
        }

        // Clear all debris metadata keys
        if let Some(ref meta_str) = task.metadata {
            if let Ok(mut meta) = serde_json::from_str::<serde_json::Value>(meta_str) {
                if let Some(obj) = meta.as_object_mut() {
                    obj.remove("conflict_type");
                    obj.remove("merge_failure_source");
                    obj.remove("source_conflict_resolved");
                    obj.remove("plan_update_conflict");
                    obj.remove("merge_error");
                    obj.remove("source_update_conflict");
                    obj.remove("conflict_files");
                    obj.remove("error");
                }
                task.metadata = Some(meta.to_string());
            }
        }
        task.worktree_path = None;
        task.touch();
        state
            .app_state
            .task_repo
            .update(&task)
            .await
            .map_err(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string(), None))?;

        // Transition Merging → PendingMerge (via Retry event)
        transition_service
            .transition_task_corrective_with_exit(
                &task_id,
                InternalStatus::PendingMerge,
                None,
                "complete_merge",
            )
            .await
            .map_err(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string(), None))?;

        // Notify completion signal then close stdin via IPR (rebase retry path)
        {
            use crate::application::interactive_process_registry::InteractiveProcessKey;
            let key = InteractiveProcessKey::new("merge", task_id.as_str());
            if let Some(signal) = state.app_state.interactive_process_registry.get_completion_signal(&key).await {
                signal.notify_one();
            }
            if state.app_state.interactive_process_registry.remove(&key).await.is_some() {
                tracing::info!("IPR removed for merger on task {} (complete_merge rebase retry)", task_id.as_str());
            }
        }

        return Ok(Json(MergeOperationResponse {
            success: true,
            message: "Rebase conflicts resolved, retrying merge".to_string(),
            new_status: "pending_merge".to_string(),
        }));
    }

    // 6. Verify commit is on target branch (resolved via plan branch or base branch)
    let plan_branch_repo = Some(Arc::clone(&state.app_state.plan_branch_repo));
    let (_, target_branch) = resolve_merge_branches(&task, &project, &plan_branch_repo).await;
    let repo_path = PathBuf::from(&project.working_directory);

    if !GitService::is_commit_on_branch(&repo_path, &req.commit_sha, &target_branch)
        .await
        .map_err(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string(), None))?
    {
        // Check if this is a source_update_conflict completion — the commit is on the
        // source branch (agent merged target INTO source), not the target branch.
        let is_source_update = parse_metadata(&task)
            .and_then(|v| v.get("source_update_conflict")?.as_bool())
            .unwrap_or(false);

        if is_source_update {
            tracing::info!(
                task_id = task_id.as_str(),
                commit_sha = %req.commit_sha,
                "Source update conflict resolved by agent, transitioning back to PendingMerge"
            );

            // Delete merge worktree created for source_update_conflict resolution
            if let Some(ref worktree_path) = task.worktree_path {
                let wt_path = PathBuf::from(worktree_path);
                if let Err(e) = GitService::delete_worktree(&repo_path, &wt_path).await {
                    tracing::warn!(
                        task_id = task_id.as_str(),
                        error = %e,
                        worktree = %wt_path.display(),
                        "Failed to delete source_update worktree — pre_merge_cleanup will handle it"
                    );
                }
            }
            task.worktree_path = None;

            // Clear source_update_conflict and set source_conflict_resolved for squash-only retry
            {
                let mut meta = parse_metadata(&task).unwrap_or_else(|| serde_json::json!({}));
                if let Some(obj) = meta.as_object_mut() {
                    obj.remove("source_update_conflict");
                    obj.remove("conflict_files");
                    obj.remove("error");
                    obj.remove("conflict_type");
                }
                task.metadata = Some(meta.to_string());
                set_source_conflict_resolved(&mut task);
            }
            task.touch();
            state
                .app_state
                .task_repo
                .update(&task)
                .await
                .map_err(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string(), None))?;

            // Transition Merging → PendingMerge
            transition_service
                .transition_task_corrective_with_exit(
                    &task_id,
                    InternalStatus::PendingMerge,
                    None,
                    "complete_merge",
                )
                .await
                .map_err(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string(), None))?;

            // Notify completion signal then close stdin via IPR (source update conflict resolved)
            {
                use crate::application::interactive_process_registry::InteractiveProcessKey;
                let key = InteractiveProcessKey::new("merge", task_id.as_str());
                if let Some(signal) = state.app_state.interactive_process_registry.get_completion_signal(&key).await {
                    signal.notify_one();
                }
                if state.app_state.interactive_process_registry.remove(&key).await.is_some() {
                    tracing::info!("IPR removed for merger on task {} (source update conflict resolved)", task_id.as_str());
                }
            }

            return Ok(Json(MergeOperationResponse {
                success: true,
                message: "Source update completed, retrying merge".to_string(),
                new_status: "pending_merge".to_string(),
            }));
        }

        return Err(json_error(
            StatusCode::BAD_REQUEST,
            format!(
                "Commit {} is not on {} branch. The merge may not have completed successfully.",
                req.commit_sha, target_branch
            ),
            Some(format!(
                "Ensure you merged the task branch INTO {} and obtained the SHA from {} (git rev-parse HEAD on {})",
                target_branch, target_branch, target_branch
            )),
        ));
    }

    // 7. Set merge_commit_sha on task
    task.merge_commit_sha = Some(req.commit_sha.clone());
    state
        .app_state
        .task_repo
        .update(&task)
        .await
        .map_err(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string(), None))?;

    // 8. Persist merge commit SHA before transitioning status
    if task.merge_commit_sha.as_deref() != Some(&req.commit_sha) {
        let mut updated_task = task.clone();
        updated_task.merge_commit_sha = Some(req.commit_sha.clone());
        updated_task.touch();
        state
            .app_state
            .task_repo
            .update(&updated_task)
            .await
            .map_err(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string(), None))?;
    }

    // 9. Transition to Merged
    transition_service
        .transition_task(&task_id, InternalStatus::Merged)
        .await
        .map_err(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string(), None))?;

    // 10. Cleanup branch/worktree — with merge guard to prevent work loss.
    // Worktree deletion is always safe (no commits lost). Branch deletion is guarded
    // by the shared helper that handles both normal and squash merges.
    // CRITICAL: HTTP handler is the ONLY cleanup for HTTP-path merges — no deferred cleanup.
    if let Some(task_branch) = &task.task_branch {
        // Delete worktree unconditionally if it exists (Worktree mode)
        if let Some(worktree_path) = &task.worktree_path {
            match GitService::delete_worktree(&repo_path, &PathBuf::from(worktree_path)).await {
                Ok(_) => {
                    tracing::info!(
                        task_id = %task_id.as_str(),
                        worktree = %worktree_path,
                        "HTTP cleanup: worktree removed"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        task_id = %task_id.as_str(),
                        error = %e,
                        worktree = %worktree_path,
                        "HTTP cleanup: delete_worktree failed (non-fatal)"
                    );
                }
            }
        }

        // Guard branch deletion with shared helper (handles squash merges too)
        let (safe_to_delete, reason) =
            GitService::is_branch_merged_or_content_equivalent(&repo_path, task_branch, &target_branch)
                .await;

        if safe_to_delete {
            match GitService::delete_branch(&repo_path, task_branch, true).await {
                Ok(_) => {
                    tracing::info!(
                        task_id = %task_id.as_str(),
                        branch = %task_branch,
                        reason = %reason,
                        "HTTP cleanup: task branch deleted"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        task_id = %task_id.as_str(),
                        error = %e,
                        branch = %task_branch,
                        "HTTP cleanup: delete_branch failed (non-fatal)"
                    );
                }
            }
        } else {
            tracing::warn!(
                task_id = %task_id.as_str(),
                task_branch = %task_branch,
                plan_branch = %target_branch,
                reason = %reason,
                "HTTP cleanup: branch deletion guard: task content not found in plan HEAD \
                 — skipping deletion to prevent work loss"
            );
        }
    }

    // 11. Emit events
    if let Some(app_handle) = &state.app_state.app_handle {
        let _ = app_handle.emit(
            "merge:completed",
            serde_json::json!({
                "task_id": task_id.as_str(),
                "commit_sha": req.commit_sha,
            }),
        );
        let _ = app_handle.emit(
            "task:status_changed",
            serde_json::json!({
                "task_id": task_id.as_str(),
                "old_status": "merging",
                "new_status": "merged",
            }),
        );
    }

    // 11b. Dual-channel emission: external_events table (SSE/poll) + webhook publisher
    {
        let project_id_str = project.id.to_string();
        let merge_payload = serde_json::json!({
            "task_id": task_id.as_str(),
            "project_id": project_id_str,
            "source_branch": task.task_branch.as_deref().unwrap_or(""),
            "target_branch": target_branch,
            "commit_sha": req.commit_sha,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        if let Err(e) = state
            .app_state
            .external_events_repo
            .insert_event("merge:completed", &project_id_str, &merge_payload.to_string())
            .await
        {
            tracing::warn!(
                error = %e,
                task_id = task_id.as_str(),
                "complete_merge: failed to insert external event (non-fatal)"
            );
        }
        if let Some(ref publisher) = state.app_state.webhook_publisher {
            publisher
                .publish(
                    ralphx_domain::entities::EventType::MergeCompleted,
                    &project_id_str,
                    merge_payload,
                )
                .await;
        }
    }

    // 12. Notify completion signal then close stdin via IPR (success path)
    {
        use crate::application::interactive_process_registry::InteractiveProcessKey;
        let key = InteractiveProcessKey::new("merge", task_id.as_str());
        if let Some(signal) = state.app_state.interactive_process_registry.get_completion_signal(&key).await {
            signal.notify_one();
        }
        if state.app_state.interactive_process_registry.remove(&key).await.is_some() {
            tracing::info!("IPR removed for merger on task {} (complete_merge)", task_id.as_str());
        }
    }

    Ok(Json(MergeOperationResponse {
        success: true,
        message: "Merge completed successfully".to_string(),
        new_status: "merged".to_string(),
    }))
}

/// POST /api/git/tasks/{id}/report-conflict
///
/// Called by merger agent when it cannot resolve conflicts.
/// Transitions task from Merging → MergeConflict for manual resolution.
pub async fn report_conflict(
    State(state): State<HttpServerState>,
    Path(task_id): Path<String>,
    Json(req): Json<ReportConflictRequest>,
) -> Result<Json<MergeOperationResponse>, JsonError> {
    let task_id = TaskId::from_string(task_id);

    // 1. Get task and validate state is Merging
    let mut task = state
        .app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .map_err(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string(), None))?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Task not found", None))?;

    if task.internal_status != InternalStatus::Merging {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            format!(
                "Task must be in 'merging' status to report conflict. Current status: {}",
                task.internal_status.as_str()
            ),
            None,
        ));
    }

    // 2. Store conflict metadata for historical navigation
    // This ensures conflict_files are persisted even when navigating historical tasks
    crate::domain::state_machine::transition_handler::set_conflict_metadata(
        &mut task,
        &req.conflict_files,
        "agent",
    );
    // Mark this conflict as agent-reported so reconciler knows not to auto-retry.
    // Smart retry guard: agent made a deliberate decision — requires human action.
    {
        let mut json: serde_json::Value = task
            .metadata
            .as_deref()
            .and_then(|m| serde_json::from_str(m).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        if let Some(obj) = json.as_object_mut() {
            obj.insert(
                "merge_failure_source".to_string(),
                serde_json::to_value(MergeFailureSource::AgentReported)
                    .unwrap_or(serde_json::json!("agent_reported")),
            );
            obj.insert(
                "conflict_reason".to_string(),
                serde_json::Value::String(req.reason.clone()),
            );
        }
        task.metadata = Some(json.to_string());
    }
    task.touch();

    if let Err(e) = state.app_state.task_repo.update(&task).await {
        tracing::error!(
            task_id = task_id.as_str(),
            error = %e,
            "Failed to update task with conflict metadata"
        );
    }

    // 3. Transition to MergeConflict
    let transition_service = state
        .app_state
        .build_transition_service_with_execution_state(Arc::clone(&state.execution_state));

    transition_service
        .transition_task(&task_id, InternalStatus::MergeConflict)
        .await
        .map_err(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string(), None))?;

    // 4. Emit events
    if let Some(app_handle) = &state.app_state.app_handle {
        let _ = app_handle.emit(
            "merge:conflict",
            serde_json::json!({
                "task_id": task_id.as_str(),
                "conflict_files": req.conflict_files,
                "reason": req.reason,
            }),
        );
        let _ = app_handle.emit(
            "task:status_changed",
            serde_json::json!({
                "task_id": task_id.as_str(),
                "old_status": "merging",
                "new_status": "merge_conflict",
            }),
        );
    }

    // 4b. Dual-channel emission: external_events table (SSE/poll) + webhook publisher
    {
        let project_id_str = task.project_id.to_string();
        let conflict_payload = serde_json::json!({
            "task_id": task_id.as_str(),
            "project_id": project_id_str,
            "source_branch": task.task_branch.as_deref().unwrap_or(""),
            "conflict_files": req.conflict_files,
            "reason": req.reason,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        if let Err(e) = state
            .app_state
            .external_events_repo
            .insert_event("merge:conflict", &project_id_str, &conflict_payload.to_string())
            .await
        {
            tracing::warn!(
                error = %e,
                task_id = task_id.as_str(),
                "report_conflict: failed to insert external event (non-fatal)"
            );
        }
        if let Some(ref publisher) = state.app_state.webhook_publisher {
            publisher
                .publish(
                    ralphx_domain::entities::EventType::MergeConflict,
                    &project_id_str,
                    conflict_payload,
                )
                .await;
        }
    }

    // 5. Notify completion signal then close stdin via IPR
    {
        use crate::application::interactive_process_registry::InteractiveProcessKey;
        let key = InteractiveProcessKey::new("merge", task_id.as_str());
        if let Some(signal) = state.app_state.interactive_process_registry.get_completion_signal(&key).await {
            signal.notify_one();
        }
        if state.app_state.interactive_process_registry.remove(&key).await.is_some() {
            tracing::info!("IPR removed for merger on task {} (conflict)", task_id.as_str());
        }
    }

    Ok(Json(MergeOperationResponse {
        success: true,
        message: "Conflict reported. Task requires manual resolution.".to_string(),
        new_status: "merge_conflict".to_string(),
    }))
}

/// POST /api/git/tasks/{id}/report-incomplete
///
/// Called by merger agent when merge cannot be completed due to non-conflict errors
/// (e.g., git operation failures, missing configuration).
/// Transitions task from Merging → MergeIncomplete.
pub async fn report_incomplete(
    State(state): State<HttpServerState>,
    Path(task_id): Path<String>,
    Json(req): Json<ReportIncompleteRequest>,
) -> Result<Json<MergeOperationResponse>, JsonError> {
    let task_id = TaskId::from_string(task_id);

    // 1. Get task and validate state is Merging
    let mut task = state
        .app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .map_err(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string(), None))?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Task not found", None))?;

    if task.internal_status != InternalStatus::Merging {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            format!(
                "Task must be in 'merging' status to report incomplete. Current status: {}",
                task.internal_status.as_str()
            ),
            None,
        ));
    }

    // 2. Persist error context to task metadata.
    // Mark as agent-reported so reconciler skips auto-retry (agent made a deliberate decision).
    task.metadata = Some(
        serde_json::json!({
            "error": req.reason,
            "diagnostic_info": req.diagnostic_info,
            "merge_failure_source": serde_json::to_value(MergeFailureSource::AgentReported)
                .unwrap_or(serde_json::json!("agent_reported")),
        })
        .to_string(),
    );
    state
        .app_state
        .task_repo
        .update(&task)
        .await
        .map_err(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string(), None))?;

    // 3. Transition to MergeIncomplete
    let transition_service = state
        .app_state
        .build_transition_service_with_execution_state(Arc::clone(&state.execution_state));

    transition_service
        .transition_task(&task_id, InternalStatus::MergeIncomplete)
        .await
        .map_err(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string(), None))?;

    // 4. Emit events
    if let Some(app_handle) = &state.app_state.app_handle {
        let _ = app_handle.emit(
            "merge:incomplete",
            serde_json::json!({
                "task_id": task_id.as_str(),
                "reason": req.reason,
                "diagnostic_info": req.diagnostic_info,
            }),
        );
        let _ = app_handle.emit(
            "task:status_changed",
            serde_json::json!({
                "task_id": task_id.as_str(),
                "old_status": "merging",
                "new_status": "merge_incomplete",
            }),
        );
    }

    // 5. Notify completion signal then close stdin via IPR
    {
        use crate::application::interactive_process_registry::InteractiveProcessKey;
        let key = InteractiveProcessKey::new("merge", task_id.as_str());
        if let Some(signal) = state.app_state.interactive_process_registry.get_completion_signal(&key).await {
            signal.notify_one();
        }
        if state.app_state.interactive_process_registry.remove(&key).await.is_some() {
            tracing::info!("IPR removed for merger on task {} (incomplete)", task_id.as_str());
        }
    }

    Ok(Json(MergeOperationResponse {
        success: true,
        message: "Merge incomplete reported. Task marked for investigation.".to_string(),
        new_status: "merge_incomplete".to_string(),
    }))
}

/// GET /api/git/tasks/{id}/commits
///
/// Get commits scoped to the selected task.
pub async fn get_task_commits(
    State(state): State<HttpServerState>,
    Path(task_id): Path<String>,
) -> Result<Json<Vec<CommitInfoResponse>>, (StatusCode, String)> {
    let task_id = TaskId::from_string(task_id);

    let commits = crate::commands::git_commands::get_task_commits_for_state(
        task_id,
        state.app_state.as_ref(),
    )
        .await
        .map_err(|e| {
            if e == "Task not found" || e == "Project not found" {
                (StatusCode::NOT_FOUND, e)
            } else if e == "Task has no branch assigned" {
                (StatusCode::BAD_REQUEST, e)
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, e)
            }
        })?;

    let response: Vec<CommitInfoResponse> = commits
        .commits
        .into_iter()
        .map(|c| CommitInfoResponse {
            sha: c.sha,
            short_sha: c.short_sha,
            message: c.message,
            author: c.author,
            timestamp: c.timestamp,
        })
        .collect();

    Ok(Json(response))
}

/// GET /api/git/tasks/{id}/diff-stats
///
/// Get diff statistics for the task branch compared to base.
pub async fn get_task_diff_stats(
    State(state): State<HttpServerState>,
    Path(task_id): Path<String>,
) -> Result<Json<DiffStatsResponse>, (StatusCode, String)> {
    let task_id = TaskId::from_string(task_id);

    // 1. Get task
    let task = state
        .app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Task not found".to_string()))?;

    // 2. Check task has a branch (we need to verify it exists, but don't need to use it)
    task.task_branch.as_ref().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "Task does not have a git branch".to_string(),
        )
    })?;

    // 3. Get project for base branch and working directory
    let project = state
        .app_state
        .project_repo
        .get_by_id(&task.project_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Project not found".to_string()))?;

    let base_branch = project.base_branch.as_deref().unwrap_or("main");

    // 4. Determine working path (worktree or main repo)
    let working_path = task
        .worktree_path
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&project.working_directory));

    // 5. Get diff stats from GitService
    let stats = GitService::get_diff_stats(&working_path, base_branch)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(DiffStatsResponse {
        files_changed: stats.files_changed,
        insertions: stats.insertions,
        deletions: stats.deletions,
        changed_files: stats.changed_files,
    }))
}

/// GET /api/git/tasks/{id}/merge-target
///
/// Get the resolved merge target branches for a task.
/// Returns source_branch (task's branch) and target_branch (where to merge INTO).
/// The target may be a plan feature branch instead of main.
pub async fn get_merge_target(
    State(state): State<HttpServerState>,
    Path(task_id): Path<String>,
) -> Result<Json<MergeTargetResponse>, (StatusCode, String)> {
    let task_id = TaskId::from_string(task_id);

    // 1. Get task
    let task = state
        .app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Task not found".to_string()))?;

    // 2. Get project
    let project = state
        .app_state
        .project_repo
        .get_by_id(&task.project_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Project not found".to_string()))?;

    // 3. Resolve merge branches
    let plan_branch_repo = Some(Arc::clone(&state.app_state.plan_branch_repo));
    let (source_branch, target_branch) =
        resolve_merge_branches(&task, &project, &plan_branch_repo).await;

    Ok(Json(MergeTargetResponse {
        source_branch,
        target_branch,
    }))
}

// ============================================================================
// Helpers
// ============================================================================

/// JSON error response type for git handlers
pub type JsonError = (StatusCode, Json<serde_json::Value>);

/// Create a JSON error response with an error message and optional details
#[doc(hidden)]
pub fn json_error(
    status: StatusCode,
    error: impl Into<String>,
    details: Option<String>,
) -> JsonError {
    let mut body = serde_json::json!({ "error": error.into() });
    if let Some(d) = details {
        body["details"] = serde_json::Value::String(d);
    }
    (status, Json(body))
}

/// Validates that a string is a valid full git SHA (40 hexadecimal characters)
#[doc(hidden)]
pub fn is_valid_git_sha(sha: &str) -> bool {
    sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit())
}

/// Build a `TaskTransitionService` from HTTP server state.
///
/// Encapsulates the boilerplate of constructing `TaskSchedulerService` (with
/// self-ref) and wiring it into `TaskTransitionService`. All complete_merge
/// paths (rebase retry, source-update retry, normal Merged, freshness routing)
/// use the same service instance, avoiding duplicate construction.
fn build_transition_service(state: &HttpServerState) -> TaskTransitionService<tauri::Wry> {
    let scheduler_concrete = std::sync::Arc::new(
        state.app_state.build_task_scheduler_for_runtime(
            std::sync::Arc::clone(&state.execution_state),
            state.app_state.app_handle.as_ref().cloned(),
        ),
    );
    scheduler_concrete
        .set_self_ref(std::sync::Arc::clone(&scheduler_concrete) as std::sync::Arc<dyn TaskScheduler>);
    let task_scheduler: std::sync::Arc<dyn TaskScheduler> = scheduler_concrete;

    state
        .app_state
        .build_transition_service_with_execution_state(std::sync::Arc::clone(&state.execution_state))
        .with_task_scheduler(task_scheduler)
}

// ============================================================================
// Tests
// ============================================================================
