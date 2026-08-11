use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use super::execution_plan_control_service::{
    ExecutionPlanControlOutcome, ExecutionPlanControlScope, ExecutionPlanControlService,
};
use super::types::{ExecutionPlanControlInput, ExecutionPlanControlResponse};
use crate::application::AppState;
use crate::commands::ExecutionState;
use crate::domain::entities::{ExecutionPlanId, IdeationSessionId, ProjectId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExecutionPlanControlAction {
    Pause,
    Resume,
    Stop,
}

fn control_scope(input: &ExecutionPlanControlInput) -> ExecutionPlanControlScope {
    ExecutionPlanControlScope {
        project_id: ProjectId::from_string(input.project_id.clone()),
        session_id: IdeationSessionId::from_string(input.session_id.clone()),
        execution_plan_id: input
            .execution_plan_id
            .as_ref()
            .map(|id| ExecutionPlanId::from_string(id.clone())),
    }
}

fn response(outcome: ExecutionPlanControlOutcome) -> ExecutionPlanControlResponse {
    ExecutionPlanControlResponse {
        execution_plan_id: outcome.execution_plan_id.as_str().to_string(),
        affected_count: outcome.affected_count,
    }
}

fn emit_plan_task_refresh(app: &AppHandle, project_id: &str) {
    let _ = app.emit(
        "task:list_changed",
        serde_json::json!({
            "projectId": project_id,
        }),
    );
}

pub(super) async fn execute_execution_plan_control(
    input: &ExecutionPlanControlInput,
    state: &AppState,
    execution_state: Arc<ExecutionState>,
    app: Option<&AppHandle>,
    action: ExecutionPlanControlAction,
) -> Result<ExecutionPlanControlResponse, String> {
    let scope = control_scope(input);
    let service = ExecutionPlanControlService::new(state, execution_state, app.cloned());
    let outcome = match action {
        ExecutionPlanControlAction::Pause => service.pause_plan(scope).await,
        ExecutionPlanControlAction::Resume => service.resume_plan(scope).await,
        ExecutionPlanControlAction::Stop => service.stop_plan(scope).await,
    }
    .map_err(|e| e.to_string())?;

    if let Some(app) = app {
        emit_plan_task_refresh(app, &input.project_id);
    }

    Ok(response(outcome))
}

#[tauri::command]
pub async fn pause_execution_plan(
    input: ExecutionPlanControlInput,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: AppHandle,
) -> Result<ExecutionPlanControlResponse, String> {
    execute_execution_plan_control(
        &input,
        &state,
        Arc::clone(execution_state.inner()),
        Some(&app),
        ExecutionPlanControlAction::Pause,
    )
    .await
}

#[tauri::command]
pub async fn resume_execution_plan(
    input: ExecutionPlanControlInput,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: AppHandle,
) -> Result<ExecutionPlanControlResponse, String> {
    execute_execution_plan_control(
        &input,
        &state,
        Arc::clone(execution_state.inner()),
        Some(&app),
        ExecutionPlanControlAction::Resume,
    )
    .await
}

#[tauri::command]
pub async fn stop_execution_plan(
    input: ExecutionPlanControlInput,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: AppHandle,
) -> Result<ExecutionPlanControlResponse, String> {
    execute_execution_plan_control(
        &input,
        &state,
        Arc::clone(execution_state.inner()),
        Some(&app),
        ExecutionPlanControlAction::Stop,
    )
    .await
}
