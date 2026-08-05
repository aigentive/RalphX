// Tauri commands for execution control
// Manages per-project execution state: pause, resume, stop
// Phase 82: Project-scoped execution with optional project_id parameters

use std::sync::Arc;
use tauri::{Emitter, State};

use crate::application::chat_service::ChatService;
use crate::application::reconciliation::UserRecoveryAction;
pub(crate) use crate::application::task_restart::restart_task_for_state;
use crate::application::AppState;
use crate::domain::entities::{app_state::ExecutionHaltMode, InternalStatus, ProjectId};
use crate::domain::execution::ExecutionSettings;
use crate::domain::state_machine::services::TaskScheduler;

mod state;

pub use state::{
    ActiveProjectState, ExecutionCommandResponse, ExecutionSettingsResponse, ExecutionState,
    ExecutionStatusResponse, GlobalExecutionSettingsResponse, UpdateExecutionSettingsInput,
    UpdateGlobalExecutionSettingsInput, AGENT_ACTIVE_STATUSES, AUTO_TRANSITION_STATES,
};

use state::sync_quota_from_project;

mod control_helpers;

pub use control_helpers::count_active_ideation_slots;
pub use control_helpers::count_active_workspace_sessions;
pub use control_helpers::project_has_execution_capacity_for_state;
use control_helpers::*;

mod recovery;

use crate::application::execution_recovery::build_reconciler_for_recovery;
pub use crate::application::execution_recovery::{
    categorize_resume_state, CategorizedResume, RestartDisposition, RestartResult, ResumeCategory,
    ResumeValidationResult, ResumeValidationWarning,
};

mod running;

pub use crate::application::execution_running::{
    context_matches_running_status_for_gc, ExecutionCapacitySummary, ExecutionLaneUsage,
    RunningIdeationSession, RunningProcess, RunningProcessesResponse, RunningWorkspaceSession,
    DEFAULT_WORKSPACE_MAX_CONCURRENT,
};

mod scheduling;
use scheduling::schedule_ready_tasks_for_project;

mod lifecycle;

pub(crate) use crate::application::execution_resume::determine_paused_restore_status;
pub(crate) use crate::application::task_resume_execution::prepare_resumed_task_for_entry_actions;
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
    __tauri_command_name_get_execution_settings,
    __tauri_command_name_get_global_execution_settings, __tauri_command_name_set_active_project,
    __tauri_command_name_set_max_concurrent, __tauri_command_name_update_execution_settings,
    __tauri_command_name_update_global_execution_settings, get_active_project,
    get_execution_settings, get_global_execution_settings, set_active_project, set_max_concurrent,
    update_execution_settings, update_global_execution_settings,
};

mod status_queries;

pub(crate) use crate::application::execution_status::{
    compute_execution_status, IdeationWaitingErrorPolicy,
};

/// Get current execution status
/// Phase 82: Optional project_id for per-project scoping.
/// If project_id is None, falls back to active project or aggregates across all projects.
#[tauri::command]
pub async fn get_execution_status(
    project_id: Option<String>,
    active_project_state: State<'_, Arc<ActiveProjectState>>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app_state: State<'_, AppState>,
) -> Result<crate::domain::execution::ExecutionStatusResponse, String> {
    crate::application::execution_status::get_execution_status_for_state(
        project_id,
        active_project_state.inner(),
        execution_state.inner(),
        app_state.inner(),
    )
    .await
}

/// List running agent processes with enriched data.
#[tauri::command]
pub async fn get_running_processes(
    project_id: Option<String>,
    active_project_state: State<'_, Arc<ActiveProjectState>>,
    execution_state: State<'_, Arc<ExecutionState>>,
    state: State<'_, AppState>,
) -> Result<crate::domain::execution::RunningProcessesResponse, String> {
    crate::application::execution_status::get_running_processes_for_state(
        project_id,
        active_project_state.inner(),
        execution_state.inner(),
        state.inner(),
    )
    .await
}

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
    restart_task_for_state(task_id, force, note, &state, &execution_state).await
}

#[cfg(test)]
mod tests;
