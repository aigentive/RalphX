// Tauri commands for execution control
// Manages per-project execution state: pause, resume, stop
// Phase 82: Project-scoped execution with optional project_id parameters

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Runtime, State};
use tokio::sync::RwLock;

use crate::application::chat_service::{
    uses_execution_slot, ChatService, SendCallerContext, SendMessageOptions,
};
use crate::application::reconciliation::UserRecoveryAction;
use crate::application::task_restart::{
    build_terminal_ready_restart_plan, classify_failed_restart, FailedRestartClassification,
};
use crate::application::{AppState, ReconciliationRunner, TaskTransitionService};
use crate::domain::entities::{
    app_state::ExecutionHaltMode, task_step::StepProgressSummary, types::IdeationSessionId,
    ChatContextType, ChatConversationId, IdeationSessionStatus, InternalStatus, ProjectId, Task,
    TaskId,
};
use crate::domain::execution::ExecutionSettings;
use crate::domain::execution::{
    build_execution_status_response, build_running_ideation_session,
    build_running_process_with_agent_workspace, build_running_workspace_session,
    elapsed_seconds_for_status, ExecutionStatusInput,
};
use crate::domain::execution::{count_execution_status, ScopedExecutionSubject};
use crate::domain::state_machine::services::TaskScheduler;
use crate::domain::state_machine::transition_handler::get_trigger_origin;

mod state;

pub use state::{
    ActiveProjectState, ExecutionCommandResponse, ExecutionSettingsResponse, ExecutionState,
    ExecutionStatusResponse, GlobalExecutionSettingsResponse, UpdateExecutionSettingsInput,
    UpdateGlobalExecutionSettingsInput, AGENT_ACTIVE_STATUSES, AUTO_TRANSITION_STATES,
};

use state::*;

mod control_helpers;

pub use control_helpers::count_active_ideation_slots;
pub use control_helpers::count_active_workspace_sessions;
pub use control_helpers::project_has_execution_capacity_for_state;
use control_helpers::*;

mod recovery;

use recovery::{
    build_reconciler_for_recovery, build_transition_service_for_recovery,
    restart_transition_target, validate_resume,
};
pub use recovery::{
    categorize_resume_state, CategorizedResume, RestartDisposition, RestartResult, ResumeCategory,
    ResumeValidationResult, ResumeValidationWarning,
};

mod running;

use running::prune_stale_execution_registry_entries;
pub use running::{
    context_matches_running_status_for_gc, ExecutionCapacitySummary, ExecutionLaneUsage,
    RunningIdeationSession, RunningProcess, RunningProcessesResponse, RunningWorkspaceSession,
    DEFAULT_WORKSPACE_MAX_CONCURRENT,
};

mod scheduling;
use scheduling::schedule_ready_tasks_for_project;

mod lifecycle;

pub use lifecycle::{
    __cmd__pause_execution, __cmd__resume_execution, __cmd__stop_execution,
    __tauri_command_name_pause_execution, __tauri_command_name_resume_execution,
    __tauri_command_name_stop_execution, pause_execution, resume_execution, stop_execution,
};
pub(crate) use lifecycle::{
    determine_paused_restore_status, prepare_resumed_task_for_entry_actions,
};

mod settings;

pub use settings::{
    __cmd__get_active_project, __cmd__get_execution_settings, __cmd__get_global_execution_settings,
    __cmd__set_active_project, __cmd__set_max_concurrent, __cmd__update_execution_settings,
    __cmd__update_global_execution_settings, __tauri_command_name_get_active_project,
    __tauri_command_name_get_execution_settings,
    __tauri_command_name_get_global_execution_settings, __tauri_command_name_set_active_project,
    __tauri_command_name_set_max_concurrent, __tauri_command_name_update_execution_settings,
    __tauri_command_name_update_global_execution_settings, get_active_project,
    get_execution_settings, get_global_execution_settings, set_active_project, set_max_concurrent,
    update_execution_settings, update_global_execution_settings,
};

mod status_queries;

pub use status_queries::{
    __cmd__get_execution_status, __cmd__get_running_processes,
    __tauri_command_name_get_execution_status, __tauri_command_name_get_running_processes,
    get_execution_status, get_running_processes,
};

/// Recover a task execution after a stop request
///
/// Applies the recovery policy:
/// - If run completed → PendingReview
/// - Else → Ready
/// - If evidence conflicts → emit recovery:prompt
#[tauri::command]
pub async fn recover_task_execution(
    task_id: String,
    execution_state: State<'_, Arc<ExecutionState>>,
    app_state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<bool, String> {
    let task_id = crate::domain::entities::TaskId::from_string(task_id);
    let task = match app_state.task_repo.get_by_id(&task_id).await {
        Ok(Some(task)) => task,
        Ok(None) => return Ok(false),
        Err(error) => return Err(error.to_string()),
    };
    crate::application::tasks_feature_policy::TasksFeaturePolicy::from_state(&app_state)
        .authorize_session(
            task.ideation_session_id.as_ref(),
            crate::domain::ideation::TasksFeatureAction::Progress,
        )
        .await
        .map_err(|error| error.to_string())?;
    let reconciler = build_reconciler_for_recovery(&app_state, Arc::clone(&execution_state), app);

    Ok(reconciler.recover_execution_stop(&task_id).await)
}

/// Resolve a recovery prompt by applying the selected action.
#[tauri::command]
pub async fn resolve_recovery_prompt(
    task_id: String,
    action: String,
    execution_state: State<'_, Arc<ExecutionState>>,
    app_state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<bool, String> {
    let task_id = crate::domain::entities::TaskId::from_string(task_id);
    let action = match action.as_str() {
        "restart" => UserRecoveryAction::Restart,
        "cancel" => UserRecoveryAction::Cancel,
        _ => return Err("Invalid recovery action".to_string()),
    };
    let reconciler = build_reconciler_for_recovery(&app_state, Arc::clone(&execution_state), app);

    let task = match app_state.task_repo.get_by_id(&task_id).await {
        Ok(Some(task)) => task,
        Ok(None) => return Ok(false),
        Err(e) => return Err(e.to_string()),
    };

    Ok(reconciler.apply_user_recovery_action(&task, action).await)
}

/// Smart resume for stopped tasks.
///
/// Restarts a task that was stopped mid-execution, using the captured stop metadata
/// to determine the appropriate resume behavior:
///
/// - **Direct**: Resume directly to the original state (Executing, ReExecuting, Reviewing, etc.)
/// - **Validated**: Validate git state before resuming (Merging, PendingMerge, etc.)
/// - **Redirect**: Resume to successor state (QaPassed→PendingReview, RevisionNeeded→ReExecuting)
///
/// # Arguments
/// * `task_id` - The ID of the task to restart
/// * `force` - If true, skip validation (use with caution)
///
/// # Returns
/// * `RestartResult::Success` - Task was restarted successfully
/// * `RestartResult::ValidationFailed` - Validation failed with warnings
#[tauri::command]
pub async fn restart_task(
    task_id: String,
    force: bool,
    note: Option<String>,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
) -> Result<RestartResult, String> {
    use crate::domain::state_machine::transition_handler::metadata_builder::{
        build_restart_metadata, parse_stop_metadata,
    };

    let task_id = TaskId::from_string(task_id);

    // 1. Get the task
    let task = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Task not found: {}", task_id.as_str()))?;
    crate::application::tasks_feature_policy::TasksFeaturePolicy::from_state(&state)
        .authorize_session(
            task.ideation_session_id.as_ref(),
            crate::domain::ideation::TasksFeatureAction::Progress,
        )
        .await
        .map_err(|error| error.to_string())?;

    if task.internal_status == InternalStatus::Failed {
        let classification = classify_failed_restart(&state, &task).await;
        match classification {
            FailedRestartClassification::RecoverToReview(_) => {
                // Repeat the complete proof immediately before the corrective CAS so a
                // worktree/validation change during the first preflight cannot advance review.
                let current_task = state
                    .task_repo
                    .get_by_id(&task_id)
                    .await
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| format!("Task not found: {}", task_id.as_str()))?;
                let FailedRestartClassification::RecoverToReview(evidence) =
                    classify_failed_restart(&state, &current_task).await
                else {
                    return Ok(RestartResult::Blocked {
                        warnings: vec![ResumeValidationWarning {
                            code: "recovery_authority_changed".to_string(),
                            message: "Recovery evidence changed during preflight; no task state was mutated".to_string(),
                        }],
                    });
                };
                let transition_service =
                    build_transition_service_for_recovery(&state, Arc::clone(&execution_state));
                let updated_task = transition_service
                    .recover_failed_completed_task_to_review(&task_id, &evidence)
                    .await
                    .map_err(|error| error.to_string())?;
                return Ok(RestartResult::Success {
                    task: serde_json::to_value(&updated_task).map_err(|error| error.to_string())?,
                    category: ResumeCategory::Redirect,
                    resumed_to_status: InternalStatus::PendingReview.as_str().to_string(),
                    disposition: Some(RestartDisposition::RecoveredToReview),
                });
            }
            FailedRestartClassification::RestartRequired(_) => {
                let plan = build_terminal_ready_restart_plan(&state.task_step_repo, &task)
                    .await
                    .map_err(|error| format!("Failed to prepare task restart: {error}"))?
                    .ok_or_else(|| {
                        "Failed task restart did not produce a terminal plan".to_string()
                    })?;
                let transition_service =
                    build_transition_service_for_recovery(&state, Arc::clone(&execution_state));
                let updated_task = transition_service
                    .restart_terminal_task_to_ready(
                        plan,
                        Some(build_restart_metadata(note.as_deref())),
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                schedule_ready_tasks_for_project(
                    &state,
                    Arc::clone(&execution_state),
                    Some(updated_task.project_id.clone()),
                )
                .await;
                return Ok(RestartResult::Success {
                    task: serde_json::to_value(&updated_task).map_err(|error| error.to_string())?,
                    category: ResumeCategory::Direct,
                    resumed_to_status: InternalStatus::Ready.as_str().to_string(),
                    disposition: Some(RestartDisposition::RestartedToReady),
                });
            }
            FailedRestartClassification::Blocked(warnings) => {
                return Ok(RestartResult::Blocked {
                    warnings: warnings
                        .into_iter()
                        .map(|warning| ResumeValidationWarning {
                            code: warning.code,
                            message: warning.message,
                        })
                        .collect(),
                });
            }
        }
    }

    // 2. Verify task is in Stopped status
    if task.internal_status != InternalStatus::Stopped {
        return Err(format!(
            "Task is not in Stopped status (current: {})",
            task.internal_status.as_str()
        ));
    }

    // 3. Parse stop metadata
    let stop_metadata = parse_stop_metadata(task.metadata.as_deref())
        .ok_or_else(|| "Task has no stop metadata - cannot smart resume".to_string())?;

    let stopped_from_status = stop_metadata.parse_from_status().ok_or_else(|| {
        format!(
            "Invalid stopped_from_status: {}",
            stop_metadata.stopped_from_status
        )
    })?;

    tracing::info!(
        task_id = task_id.as_str(),
        stopped_from = stopped_from_status.as_str(),
        reason = ?stop_metadata.stop_reason,
        "Smart restarting task"
    );

    // 4. Categorize the resume state
    let categorized = categorize_resume_state(stopped_from_status);

    // 5. For Validated category, run validation (unless forced)
    if categorized.category == ResumeCategory::Validated && !force {
        let validation_result = validate_resume(&task, &state).await;
        if !validation_result.passed {
            return Ok(RestartResult::ValidationFailed {
                warnings: validation_result.warnings,
                stopped_from_status: stopped_from_status.as_str().to_string(),
            });
        }
    }

    // 6. Build transition service
    let transition_service =
        build_transition_service_for_recovery(&state, Arc::clone(&execution_state));

    let transition_target = restart_transition_target(stopped_from_status);
    if !task.internal_status.can_transition_to(transition_target) {
        return Ok(RestartResult::ValidationFailed {
            warnings: vec![ResumeValidationWarning {
                code: "unsupported_restart_target".to_string(),
                message: format!(
                    "Stopped task from '{}' cannot safely restart directly to '{}'",
                    stopped_from_status.as_str(),
                    transition_target.as_str()
                ),
            }],
            stopped_from_status: stopped_from_status.as_str().to_string(),
        });
    }
    let terminal_restart_plan =
        if transition_target == InternalStatus::Ready && task.internal_status.is_terminal() {
            build_terminal_ready_restart_plan(&state.task_step_repo, &task)
                .await
                .map_err(|e| format!("Failed to prepare task restart: {e}"))?
        } else {
            None
        };

    // 7. Transition to target status: clear stop metadata and optionally store restart_note
    let restart_metadata = build_restart_metadata(note.as_deref());
    let updated_task = if let Some(plan) = terminal_restart_plan {
        transition_service
            .restart_terminal_task_to_ready(plan, Some(restart_metadata))
            .await
            .map_err(|e| e.to_string())?
    } else {
        transition_service
            .transition_task_with_metadata(&task_id, transition_target, Some(restart_metadata))
            .await
            .map_err(|e| e.to_string())?
    };

    if transition_target == InternalStatus::Ready {
        schedule_ready_tasks_for_project(
            &state,
            Arc::clone(&execution_state),
            Some(updated_task.project_id.clone()),
        )
        .await;
    }

    tracing::info!(
        task_id = task_id.as_str(),
        category = ?categorized.category,
        target = transition_target.as_str(),
        stopped_from = stopped_from_status.as_str(),
        "Task restarted successfully"
    );

    // 8. Emit lifecycle event
    if let Some(ref app) = state.app_handle {
        let _ = app.emit(
            "task:restarted",
            serde_json::json!({
                "taskId": updated_task.id.as_str(),
                "projectId": updated_task.project_id.as_str(),
                "resumedToStatus": transition_target.as_str(),
                "stoppedFromStatus": stopped_from_status.as_str(),
                "category": categorized.category,
                "stopReason": stop_metadata.stop_reason,
                "timestamp": chrono::Utc::now().to_rfc3339(),
            }),
        );
    }

    // 9. Return success result
    // Serialize task to JSON Value for flexible response
    let task_json = serde_json::to_value(&updated_task).map_err(|e| e.to_string())?;

    Ok(RestartResult::Success {
        task: task_json,
        category: categorized.category,
        resumed_to_status: transition_target.as_str().to_string(),
        disposition: None,
    })
}

#[cfg(test)]
mod tests;
