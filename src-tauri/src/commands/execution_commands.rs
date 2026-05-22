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

use crate::application::chat_service::{uses_execution_slot, ChatService, SendMessageOptions};
use crate::application::reconciliation::UserRecoveryAction;
use crate::application::team_state_tracker::TeamStateTracker;
use crate::application::{AppState, ReconciliationRunner, TaskTransitionService};
use crate::domain::entities::{
    app_state::ExecutionHaltMode, task_step::StepProgressSummary, types::IdeationSessionId,
    ChatContextType, ChatConversationId, IdeationSessionStatus, InternalStatus, ProjectId, Task,
    TaskId,
};
use crate::domain::execution::ExecutionSettings;
use crate::domain::execution::{
    build_execution_status_response, build_running_ideation_session, build_running_process,
    elapsed_seconds_for_status, ExecutionStatusInput,
};
use crate::domain::execution::{count_execution_status, ScopedExecutionSubject};
use crate::domain::services::QueueKey;
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
pub use control_helpers::project_has_execution_capacity_for_state;
use control_helpers::*;

mod recovery;

use recovery::{
    build_reconciler_for_recovery, build_transition_service_for_recovery, validate_resume,
};
pub use recovery::{
    categorize_resume_state, CategorizedResume, RestartResult, ResumeCategory,
    ResumeValidationResult, ResumeValidationWarning,
};

mod running;

use running::prune_stale_execution_registry_entries;
pub use running::{
    context_matches_running_status_for_gc, RunningIdeationSession, RunningProcess,
    RunningProcessesResponse,
};

mod scheduling;
use scheduling::schedule_ready_tasks_for_project;

mod lifecycle;

pub(crate) use lifecycle::prepare_resumed_task_for_entry_actions;
pub use lifecycle::{
    __cmd__pause_execution, __cmd__resume_execution, __cmd__stop_execution,
    __tauri_command_name_pause_execution, __tauri_command_name_resume_execution,
    __tauri_command_name_stop_execution, pause_execution, resume_execution, stop_execution,
};

mod settings;

pub use settings::{
    __cmd__get_active_project, __cmd__get_execution_settings, __cmd__get_global_execution_settings,
    __cmd__set_active_project, __cmd__set_max_concurrent, __cmd__update_execution_settings,
    __cmd__update_global_execution_settings, __tauri_command_name_get_active_project,
    __tauri_command_name_get_execution_settings, __tauri_command_name_get_global_execution_settings,
    __tauri_command_name_set_active_project, __tauri_command_name_set_max_concurrent,
    __tauri_command_name_update_execution_settings,
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

    // 7. Transition to target status: clear stop metadata and optionally store restart_note
    let restart_metadata = build_restart_metadata(note.as_deref());
    let updated_task = transition_service
        .transition_task_with_metadata(&task_id, categorized.target_status, Some(restart_metadata))
        .await
        .map_err(|e| e.to_string())?;

    tracing::info!(
        task_id = task_id.as_str(),
        category = ?categorized.category,
        target = categorized.target_status.as_str(),
        "Task restarted successfully"
    );

    // 8. Emit lifecycle event
    if let Some(ref app) = state.app_handle {
        let _ = app.emit(
            "task:restarted",
            serde_json::json!({
                "taskId": updated_task.id.as_str(),
                "projectId": updated_task.project_id.as_str(),
                "resumedToStatus": categorized.target_status.as_str(),
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
        resumed_to_status: categorized.target_status.as_str().to_string(),
    })
}

#[cfg(test)]
mod tests;
