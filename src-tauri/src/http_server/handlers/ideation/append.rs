use std::sync::Arc;

use axum::{extract::State, Json};

use crate::application::spawn_ready_task_scheduler_if_needed;
use crate::commands::ideation_commands::{
    append_ideation_plan_task_core, AppendIdeationPlanTaskInput, AppendIdeationPlanTaskResult,
};
use crate::error::AppError;
use crate::http_server::types::{HttpError, HttpServerState};

pub async fn append_ideation_plan_task_http(
    State(state): State<HttpServerState>,
    Json(req): Json<AppendIdeationPlanTaskInput>,
) -> Result<Json<AppendIdeationPlanTaskResult>, HttpError> {
    let result = append_ideation_plan_task_core(&state.app_state, req)
        .await
        .map_err(append_error_to_http)?;

    spawn_ready_task_scheduler_if_needed(
        &state.app_state,
        Arc::clone(&state.execution_state),
        state.app_state.app_handle.as_ref().cloned(),
        result.any_ready_tasks,
    );

    Ok(Json(result))
}

pub(crate) fn append_error_to_http(error: AppError) -> HttpError {
    match error {
        AppError::NotFound(_) | AppError::TaskNotFound(_) | AppError::ProjectNotFound(_) => {
            HttpError {
                status: axum::http::StatusCode::NOT_FOUND,
                message: Some(error.to_string()),
            }
        }
        AppError::Validation(_) | AppError::Conflict(_) => HttpError::validation(error.to_string()),
        _ => HttpError {
            status: axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            message: Some(error.to_string()),
        },
    }
}
