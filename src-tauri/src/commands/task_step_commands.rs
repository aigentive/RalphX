// Tauri commands for TaskStep CRUD operations
// Thin layer that delegates to TaskStepRepository

use ralphx_events::emit_serialized;
use tauri::State;

use crate::application::AppState;
use crate::domain::entities::{StepProgressSummary, TaskId, TaskStep, TaskStepId};
use crate::error::AppResult;

// Re-export types for external use
pub use super::task_step_commands_types::{
    CreateTaskStepInput, TaskStepResponse, UpdateTaskStepInput,
};

/// Emit step:updated event to frontend
fn emit_step_updated(state: &AppState, step: &TaskStep) {
    let response = TaskStepResponse::from(step.clone());
    let _ = emit_serialized(
        state.events.as_ref(),
        "step:updated",
        &serde_json::json!({
            "step": response,
            "task_id": step.task_id.as_str()
        }),
    );
}

/// Create a new task step
#[tauri::command]
pub async fn create_task_step(
    task_id: String,
    input: CreateTaskStepInput,
    state: State<'_, AppState>,
) -> AppResult<TaskStepResponse> {
    let task_id = TaskId::from_string(task_id);

    // Determine sort_order: use provided, or default to 0
    let sort_order = input.sort_order.unwrap_or(0);

    // Create new step
    let mut step = TaskStep::new(task_id, input.title, sort_order, "user".to_string());

    // Set description if provided
    if let Some(desc) = input.description {
        step.description = Some(desc);
    }

    // Save to repository
    let step = state.task_step_repo.create(step).await?;

    Ok(TaskStepResponse::from(step))
}

/// Get all steps for a task
#[tauri::command]
pub async fn get_task_steps(
    task_id: String,
    state: State<'_, AppState>,
) -> AppResult<Vec<TaskStepResponse>> {
    let task_id = TaskId::from_string(task_id);
    let steps = state.task_step_repo.get_by_task(&task_id).await?;
    Ok(steps.into_iter().map(TaskStepResponse::from).collect())
}

/// Update a task step
#[tauri::command]
pub async fn update_task_step(
    step_id: String,
    input: UpdateTaskStepInput,
    state: State<'_, AppState>,
) -> AppResult<TaskStepResponse> {
    let step_id = TaskStepId::from_string(step_id);

    // Get existing step
    let mut step = state
        .task_step_repo
        .get_by_id(&step_id)
        .await?
        .ok_or_else(|| {
            crate::error::AppError::NotFound(format!("Step {} not found", step_id.as_str()))
        })?;

    // Update fields
    if let Some(title) = input.title {
        step.title = title;
    }
    if let Some(description) = input.description {
        step.description = Some(description);
    }
    if let Some(sort_order) = input.sort_order {
        step.sort_order = sort_order;
    }

    // Update timestamp
    step.touch();

    // Save
    state.task_step_repo.update(&step).await?;

    Ok(TaskStepResponse::from(step))
}

/// Delete a task step
#[tauri::command]
pub async fn delete_task_step(step_id: String, state: State<'_, AppState>) -> AppResult<()> {
    let step_id = TaskStepId::from_string(step_id);
    state.task_step_repo.delete(&step_id).await
}

/// Reorder task steps
#[tauri::command]
pub async fn reorder_task_steps(
    task_id: String,
    step_ids: Vec<String>,
    state: State<'_, AppState>,
) -> AppResult<Vec<TaskStepResponse>> {
    let task_id = TaskId::from_string(task_id);
    let step_ids: Vec<TaskStepId> = step_ids.into_iter().map(TaskStepId::from_string).collect();

    state.task_step_repo.reorder(&task_id, step_ids).await?;

    // Return updated steps
    let steps = state.task_step_repo.get_by_task(&task_id).await?;
    Ok(steps.into_iter().map(TaskStepResponse::from).collect())
}

/// Get step progress summary for a task
#[tauri::command]
pub async fn get_step_progress(
    task_id: String,
    state: State<'_, AppState>,
) -> AppResult<StepProgressSummary> {
    let task_id = TaskId::from_string(task_id);
    let steps = state.task_step_repo.get_by_task(&task_id).await?;
    Ok(StepProgressSummary::from_steps(&task_id, &steps))
}

/// Start a step (mark as in-progress)
#[tauri::command]
pub async fn start_step(
    step_id: String,
    state: State<'_, AppState>,
) -> AppResult<TaskStepResponse> {
    let step_id = TaskStepId::from_string(step_id);

    // Get existing step
    let mut step = state
        .task_step_repo
        .get_by_id(&step_id)
        .await?
        .ok_or_else(|| {
            crate::error::AppError::NotFound(format!("Step {} not found", step_id.as_str()))
        })?;

    // Validate step is Pending
    if step.status != crate::domain::entities::TaskStepStatus::Pending {
        return Err(crate::error::AppError::Validation(format!(
            "Step must be Pending to start, but is {:?}",
            step.status
        )));
    }

    // Update status
    step.status = crate::domain::entities::TaskStepStatus::InProgress;
    step.started_at = Some(chrono::Utc::now());
    step.touch();

    // Save
    state.task_step_repo.update(&step).await?;

    // Emit event to frontend
    emit_step_updated(&state, &step);

    Ok(TaskStepResponse::from(step))
}

/// Complete a step
#[tauri::command]
pub async fn complete_step(
    step_id: String,
    note: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<TaskStepResponse> {
    let step_id = TaskStepId::from_string(step_id);

    // Get existing step
    let mut step = state
        .task_step_repo
        .get_by_id(&step_id)
        .await?
        .ok_or_else(|| {
            crate::error::AppError::NotFound(format!("Step {} not found", step_id.as_str()))
        })?;

    // Validate step is InProgress
    if step.status != crate::domain::entities::TaskStepStatus::InProgress {
        return Err(crate::error::AppError::Validation(format!(
            "Step must be InProgress to complete, but is {:?}",
            step.status
        )));
    }

    // Update status
    step.status = crate::domain::entities::TaskStepStatus::Completed;
    step.completed_at = Some(chrono::Utc::now());
    step.completion_note = note;
    step.touch();

    // Save
    state.task_step_repo.update(&step).await?;

    // Emit event to frontend
    emit_step_updated(&state, &step);

    Ok(TaskStepResponse::from(step))
}

/// Skip a step with a reason
#[tauri::command]
pub async fn skip_step(
    step_id: String,
    reason: String,
    state: State<'_, AppState>,
) -> AppResult<TaskStepResponse> {
    let step_id = TaskStepId::from_string(step_id);

    // Get existing step
    let mut step = state
        .task_step_repo
        .get_by_id(&step_id)
        .await?
        .ok_or_else(|| {
            crate::error::AppError::NotFound(format!("Step {} not found", step_id.as_str()))
        })?;

    // Validate step is Pending or InProgress
    if step.status != crate::domain::entities::TaskStepStatus::Pending
        && step.status != crate::domain::entities::TaskStepStatus::InProgress
    {
        return Err(crate::error::AppError::Validation(format!(
            "Step must be Pending or InProgress to skip, but is {:?}",
            step.status
        )));
    }

    // Update status
    step.status = crate::domain::entities::TaskStepStatus::Skipped;
    step.completed_at = Some(chrono::Utc::now());
    step.completion_note = Some(reason);
    step.touch();

    // Save
    state.task_step_repo.update(&step).await?;

    // Emit event to frontend
    emit_step_updated(&state, &step);

    Ok(TaskStepResponse::from(step))
}

/// Fail a step with an error message
#[tauri::command]
pub async fn fail_step(
    step_id: String,
    error: String,
    state: State<'_, AppState>,
) -> AppResult<TaskStepResponse> {
    let step_id = TaskStepId::from_string(step_id);

    // Get existing step
    let mut step = state
        .task_step_repo
        .get_by_id(&step_id)
        .await?
        .ok_or_else(|| {
            crate::error::AppError::NotFound(format!("Step {} not found", step_id.as_str()))
        })?;

    // Validate step is InProgress
    if step.status != crate::domain::entities::TaskStepStatus::InProgress {
        return Err(crate::error::AppError::Validation(format!(
            "Step must be InProgress to fail, but is {:?}",
            step.status
        )));
    }

    // Update status
    step.status = crate::domain::entities::TaskStepStatus::Failed;
    step.completed_at = Some(chrono::Utc::now());
    step.completion_note = Some(error);
    step.touch();

    // Save
    state.task_step_repo.update(&step).await?;

    // Emit event to frontend
    emit_step_updated(&state, &step);

    Ok(TaskStepResponse::from(step))
}
