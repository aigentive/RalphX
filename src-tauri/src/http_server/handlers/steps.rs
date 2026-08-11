use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use tracing::error;

use super::*;
use crate::application::git_service::GitService;
use crate::application::interactive_process_registry::InteractiveProcessKey;
use crate::application::task_diff_base::ensure_task_has_non_empty_captured_diff;
use crate::application::validation_service::TaskValidationService;
use crate::domain::entities::{
    ExecutionFailureSource, StepProgressSummary, Task, TaskId, TaskStep, TaskStepId, TaskStepStatus,
};
use crate::error::AppError;
use crate::http_server::project_scope::{ProjectScope, ProjectScopeGuard};
use crate::http_server::types::HttpError;
use crate::utils::path_safety::validate_absolute_non_root_path;

pub async fn get_task_steps_http(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Path(task_id): Path<String>,
) -> Result<Json<Vec<StepResponse>>, StatusCode> {
    let task_id = TaskId::from_string(task_id);

    // If a project scope header is present, verify the task belongs to the scoped project.
    // If absent (internal agent call), skip the check.
    if scope.0.is_some() {
        let task = state
            .app_state
            .task_repo
            .get_by_id(&task_id)
            .await
            .map_err(|e| {
                error!("Failed to get task {}: {}", task_id.as_str(), e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?
            .ok_or(StatusCode::NOT_FOUND)?;
        task.assert_project_scope(&scope).map_err(|e| e.status)?;
    }

    let steps = state
        .app_state
        .task_step_repo
        .get_by_task(&task_id)
        .await
        .map_err(|e| {
            error!("Failed to get steps for task {}: {}", task_id.as_str(), e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(steps.into_iter().map(StepResponse::from).collect()))
}

pub async fn start_step_http(
    State(state): State<HttpServerState>,
    Json(req): Json<StartStepRequest>,
) -> Result<Json<StepResponse>, HttpError> {
    let step_id = TaskStepId::from_string(req.step_id);

    // Get existing step
    let mut step = state
        .app_state
        .task_step_repo
        .get_by_id(&step_id)
        .await
        .map_err(|e| {
            error!("Failed to get step {}: {}", step_id.as_str(), e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    authorize_step_mutation(&state, &step.task_id).await?;

    // Validate step is Pending
    if step.status != TaskStepStatus::Pending {
        return Err(StatusCode::BAD_REQUEST.into());
    }

    // Update status
    step.status = TaskStepStatus::InProgress;
    step.started_at = Some(chrono::Utc::now());
    step.touch();

    // Save
    state
        .app_state
        .task_step_repo
        .update(&step)
        .await
        .map_err(|error| step_mutation_http_error(error, "update"))?;

    let response = StepResponse::from(step);

    crate::http_server::emit_http_event(
        &state,
        "step:updated",
        serde_json::json!({
            "step": &response,
            "task_id": &response.task_id
        }),
    );

    Ok(Json(response))
}

pub async fn complete_step_http(
    State(state): State<HttpServerState>,
    Json(req): Json<CompleteStepRequest>,
) -> Result<Json<StepResponse>, HttpError> {
    let step_id = TaskStepId::from_string(req.step_id);

    // Get existing step
    let mut step = state
        .app_state
        .task_step_repo
        .get_by_id(&step_id)
        .await
        .map_err(|e| {
            error!("Failed to get step {}: {}", step_id.as_str(), e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    authorize_step_mutation(&state, &step.task_id).await?;

    // Validate step is InProgress
    if step.status != TaskStepStatus::InProgress {
        return Err(StatusCode::BAD_REQUEST.into());
    }

    // Update status
    step.status = TaskStepStatus::Completed;
    step.completed_at = Some(chrono::Utc::now());
    step.completion_note = req.note;
    step.touch();

    // Save
    state
        .app_state
        .task_step_repo
        .update(&step)
        .await
        .map_err(|error| step_mutation_http_error(error, "update"))?;

    let response = StepResponse::from(step);

    crate::http_server::emit_http_event(
        &state,
        "step:updated",
        serde_json::json!({
            "step": &response,
            "task_id": &response.task_id
        }),
    );

    // Fallback: if all steps for this task are now done, close the worker's IPR to signal EOF
    {
        let task_id_for_check = TaskId::from_string(response.task_id.clone());
        match state
            .app_state
            .task_step_repo
            .get_by_task(&task_id_for_check)
            .await
        {
            Ok(all_steps) if !all_steps.is_empty() => {
                let all_done = all_steps.iter().all(|s| {
                    matches!(
                        s.status,
                        TaskStepStatus::Completed | TaskStepStatus::Skipped
                    )
                });
                if all_done {
                    tracing::info!(
                        "All {} steps complete for task {} — triggering execution completion fallback",
                        all_steps.len(),
                        response.task_id
                    );
                    let key = InteractiveProcessKey::new("task_execution", &response.task_id);
                    if state
                        .app_state
                        .interactive_process_registry
                        .remove(&key)
                        .await
                        .is_some()
                    {
                        tracing::info!(
                            "IPR removed for worker on task {} (all-steps-done fallback)",
                            response.task_id
                        );
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to check step completion for task {}: {}",
                    response.task_id,
                    e
                );
            }
            _ => {}
        }
    }

    Ok(Json(response))
}

pub async fn skip_step_http(
    State(state): State<HttpServerState>,
    Json(req): Json<SkipStepRequest>,
) -> Result<Json<StepResponse>, HttpError> {
    let step_id = TaskStepId::from_string(req.step_id);

    // Get existing step
    let mut step = state
        .app_state
        .task_step_repo
        .get_by_id(&step_id)
        .await
        .map_err(|e| {
            error!("Failed to get step {}: {}", step_id.as_str(), e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    authorize_step_mutation(&state, &step.task_id).await?;

    // Validate step is Pending or InProgress
    if step.status != TaskStepStatus::Pending && step.status != TaskStepStatus::InProgress {
        return Err(StatusCode::BAD_REQUEST.into());
    }

    // Update status
    step.status = TaskStepStatus::Skipped;
    step.completed_at = Some(chrono::Utc::now());
    step.completion_note = Some(req.reason);
    step.touch();

    // Save
    state
        .app_state
        .task_step_repo
        .update(&step)
        .await
        .map_err(|error| step_mutation_http_error(error, "update"))?;

    let response = StepResponse::from(step);

    crate::http_server::emit_http_event(
        &state,
        "step:updated",
        serde_json::json!({
            "step": &response,
            "task_id": &response.task_id
        }),
    );

    // Fallback: if all steps for this task are now done, close the worker's IPR to signal EOF
    {
        let task_id_for_check = TaskId::from_string(response.task_id.clone());
        match state
            .app_state
            .task_step_repo
            .get_by_task(&task_id_for_check)
            .await
        {
            Ok(all_steps) if !all_steps.is_empty() => {
                let all_done = all_steps.iter().all(|s| {
                    matches!(
                        s.status,
                        TaskStepStatus::Completed | TaskStepStatus::Skipped
                    )
                });
                if all_done {
                    tracing::info!(
                        "All {} steps complete for task {} — triggering execution completion fallback",
                        all_steps.len(),
                        response.task_id
                    );
                    let key = InteractiveProcessKey::new("task_execution", &response.task_id);
                    if state
                        .app_state
                        .interactive_process_registry
                        .remove(&key)
                        .await
                        .is_some()
                    {
                        tracing::info!(
                            "IPR removed for worker on task {} (all-steps-done fallback)",
                            response.task_id
                        );
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to check step completion for task {}: {}",
                    response.task_id,
                    e
                );
            }
            _ => {}
        }
    }

    Ok(Json(response))
}

pub async fn fail_step_http(
    State(state): State<HttpServerState>,
    Json(req): Json<FailStepRequest>,
) -> Result<Json<StepResponse>, HttpError> {
    let step_id = TaskStepId::from_string(req.step_id);

    // Get existing step
    let mut step = state
        .app_state
        .task_step_repo
        .get_by_id(&step_id)
        .await
        .map_err(|e| {
            error!("Failed to get step {}: {}", step_id.as_str(), e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    authorize_step_mutation(&state, &step.task_id).await?;

    // Validate step is InProgress
    if step.status != TaskStepStatus::InProgress {
        return Err(StatusCode::BAD_REQUEST.into());
    }

    // Update status
    step.status = TaskStepStatus::Failed;
    step.completed_at = Some(chrono::Utc::now());
    step.completion_note = Some(req.error);
    step.touch();

    // Save
    state
        .app_state
        .task_step_repo
        .update(&step)
        .await
        .map_err(|error| step_mutation_http_error(error, "update"))?;

    let response = StepResponse::from(step);

    crate::http_server::emit_http_event(
        &state,
        "step:updated",
        serde_json::json!({
            "step": &response,
            "task_id": &response.task_id
        }),
    );

    Ok(Json(response))
}

pub async fn add_step_http(
    State(state): State<HttpServerState>,
    Json(req): Json<AddStepRequest>,
) -> Result<Json<StepResponse>, HttpError> {
    let task_id = TaskId::from_string(req.task_id);

    authorize_step_mutation(&state, &task_id).await?;

    // Determine sort_order
    let sort_order = if let Some(after_step_id_str) = req.after_step_id {
        // Insert after specified step
        let after_step_id = TaskStepId::from_string(after_step_id_str);
        let after_step = state
            .app_state
            .task_step_repo
            .get_by_id(&after_step_id)
            .await
            .map_err(|e| {
                error!("Failed to get step {}: {}", after_step_id.as_str(), e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?
            .ok_or(StatusCode::NOT_FOUND)?;
        after_step.sort_order + 1
    } else {
        // Append to end - find max sort_order
        let steps = state
            .app_state
            .task_step_repo
            .get_by_task(&task_id)
            .await
            .map_err(|e| {
                error!("Failed to get steps for task {}: {}", task_id.as_str(), e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        steps.iter().map(|s| s.sort_order).max().unwrap_or(-1) + 1
    };

    // Create new step
    let mut step = TaskStep::new(task_id.clone(), req.title, sort_order, "agent".to_string());
    step.description = req.description;
    step.parent_step_id = req.parent_step_id.map(TaskStepId::from_string);
    step.scope_context = req.scope_context;

    // Save to repository
    let step = state
        .app_state
        .task_step_repo
        .create(step)
        .await
        .map_err(|error| step_mutation_http_error(error, "create"))?;

    let response = StepResponse::from(step);

    crate::http_server::emit_http_event(
        &state,
        "step:created",
        serde_json::json!({
            "step": &response,
            "task_id": &response.task_id
        }),
    );

    Ok(Json(response))
}

fn step_mutation_http_error(error: AppError, operation: &str) -> HttpError {
    error!("Failed to {operation} task step: {error}");
    match error {
        AppError::FeatureDisabled(message) => HttpError {
            status: StatusCode::CONFLICT,
            message: Some(message),
        },
        _ => HttpError::from(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn authorize_step_mutation(
    state: &HttpServerState,
    task_id: &TaskId,
) -> Result<(), HttpError> {
    let task = state
        .app_state
        .task_repo
        .get_by_id(task_id)
        .await
        .map_err(|error| step_mutation_http_error(error, "load task"))?
        .ok_or(StatusCode::NOT_FOUND)?;

    crate::application::tasks_feature_policy::TasksFeaturePolicy::from_state(&state.app_state)
        .authorize_session(
            task.ideation_session_id.as_ref(),
            crate::domain::ideation::TasksFeatureAction::Progress,
        )
        .await
        .map_err(|error| step_mutation_http_error(error, "authorize"))
}

pub async fn get_step_progress_http(
    State(state): State<HttpServerState>,
    Path(task_id): Path<String>,
) -> Result<Json<StepProgressSummary>, StatusCode> {
    let task_id = TaskId::from_string(task_id);
    let steps = state
        .app_state
        .task_step_repo
        .get_by_task(&task_id)
        .await
        .map_err(|e| {
            error!("Failed to get steps for task {}: {}", task_id.as_str(), e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(StepProgressSummary::from_steps(&task_id, &steps)))
}

pub async fn get_sub_steps_http(
    State(state): State<HttpServerState>,
    Path(parent_step_id): Path<String>,
) -> Result<Json<Vec<StepResponse>>, StatusCode> {
    let parent_step_id_typed = TaskStepId::from_string(parent_step_id.clone());

    // First verify the parent step exists
    let parent_step = state
        .app_state
        .task_step_repo
        .get_by_id(&parent_step_id_typed)
        .await
        .map_err(|e| {
            error!("Failed to get parent step {}: {}", parent_step_id, e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Get all steps for the task
    let all_steps = state
        .app_state
        .task_step_repo
        .get_by_task(&parent_step.task_id)
        .await
        .map_err(|e| {
            error!(
                "Failed to get steps for task {}: {}",
                parent_step.task_id.as_str(),
                e
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Filter to sub-steps of this parent, ordered by sort_order
    let mut sub_steps: Vec<TaskStep> = all_steps
        .into_iter()
        .filter(|s| s.parent_step_id.as_ref() == Some(&parent_step_id_typed))
        .collect();
    sub_steps.sort_by_key(|s| s.sort_order);

    Ok(Json(
        sub_steps.into_iter().map(StepResponse::from).collect(),
    ))
}

fn build_step_context_hints(step: &TaskStep, task: &Task, siblings: &[TaskStep]) -> Vec<String> {
    let mut hints = Vec::new();
    if step.scope_context.is_some() {
        hints.push("STRICT SCOPE assigned — only modify files listed in scope_context".to_string());
    }
    hints.push(format!(
        "SCOPE: You are executing step \"{}\" for task \"{}\". Do not work on other steps.",
        step.title, task.title
    ));
    if !siblings.is_empty() {
        let names: Vec<_> = siblings.iter().map(|s| s.title.as_str()).collect();
        hints.push(format!(
            "Parallel sibling steps (other coders — DO NOT do their work): {}",
            names.join(", ")
        ));
    }
    hints
}

pub async fn get_step_context_http(
    State(state): State<HttpServerState>,
    Path(step_id): Path<String>,
) -> Result<Json<StepContextResponse>, StatusCode> {
    let step_id = TaskStepId::from_string(step_id);

    // Fetch the step
    let step = state
        .app_state
        .task_step_repo
        .get_by_id(&step_id)
        .await
        .map_err(|e| {
            error!("Failed to get step {}: {}", step_id.as_str(), e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Fetch parent step if this is a sub-step
    let parent_step = if let Some(ref parent_id) = step.parent_step_id {
        state
            .app_state
            .task_step_repo
            .get_by_id(parent_id)
            .await
            .map_err(|e| {
                error!("Failed to get parent step {}: {}", parent_id.as_str(), e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?
    } else {
        None
    };

    // Fetch the task
    let task = state
        .app_state
        .task_repo
        .get_by_id(&step.task_id)
        .await
        .map_err(|e| {
            error!("Failed to get task {}: {}", step.task_id.as_str(), e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Fetch sibling steps (same parent_step_id)
    let all_steps = state
        .app_state
        .task_step_repo
        .get_by_task(&step.task_id)
        .await
        .map_err(|e| {
            error!(
                "Failed to get steps for task {}: {}",
                step.task_id.as_str(),
                e
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let sibling_steps: Vec<TaskStep> = all_steps
        .into_iter()
        .filter(|s| s.id != step.id && s.parent_step_id == step.parent_step_id)
        .collect();

    // Compute step progress for the task
    let all_task_steps = state
        .app_state
        .task_step_repo
        .get_by_task(&step.task_id)
        .await
        .map_err(|e| {
            error!("Failed to get steps for progress: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let step_progress = StepProgressSummary::from_steps(&step.task_id, &all_task_steps);

    // Build context hints
    let context_hints = build_step_context_hints(&step, &task, &sibling_steps);

    // Build response
    let response = StepContextResponse {
        step: StepResponse::from(step.clone()),
        parent_step: parent_step.map(StepResponse::from),
        task_summary: TaskSummaryForStep {
            id: task.id.as_str().to_string(),
            title: task.title.clone(),
            description: task.description.clone(),
            internal_status: task.internal_status.as_str().to_string(),
        },
        scope_context: step.scope_context.clone(),
        sibling_steps: sibling_steps.into_iter().map(StepResponse::from).collect(),
        step_progress,
        context_hints,
    };

    Ok(Json(response))
}

/// Signal that task execution is complete.
///
/// Called by the worker agent via MCP `execution_complete` tool after all steps are done.
/// Closes the agent's stdin via IPR so it receives EOF and exits gracefully.
/// The `handle_stream_success` callback handles the actual state transition when the process exits.
pub async fn execution_complete_http(
    State(state): State<HttpServerState>,
    Path(task_id_str): Path<String>,
    Json(req): Json<ExecutionCompleteRequest>,
) -> Result<Json<ExecutionCompleteResponse>, HttpError> {
    let task_id = TaskId::from_string(task_id_str.clone());

    // Fetch task (needed for worktree_path and metadata update)
    let task = state
        .app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .map_err(|e| {
            error!("Failed to get task {}: {}", task_id_str, e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    crate::application::tasks_feature_policy::TasksFeaturePolicy::from_state(&state.app_state)
        .authorize_session(
            task.ideation_session_id.as_ref(),
            crate::domain::ideation::TasksFeatureAction::Progress,
        )
        .await
        .map_err(|error| step_mutation_http_error(error, "authorize execution completion"))?;

    let project = state
        .app_state
        .project_repo
        .get_by_id(&task.project_id)
        .await
        .map_err(|e| {
            error!("Failed to get project for task {}: {}", task_id_str, e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    tracing::info!(
        "execution_complete received for task {}: summary={:?}",
        task_id_str,
        req.summary
    );

    if let Some(ref worktree_path) = task.worktree_path {
        let path = validate_absolute_non_root_path(
            std::path::Path::new(worktree_path),
            "execution complete worktree",
        )
        .map_err(|e| {
            tracing::error!(
                task_id = %task_id_str,
                worktree_path = %worktree_path,
                error = %e,
                "Rejecting execution_complete because worktree path failed validation"
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
        let dirty_summary = GitService::uncommitted_change_summary(&path)
            .await
            .map_err(|e| {
                tracing::error!(
                    task_id = %task_id_str,
                    worktree_path = %worktree_path,
                    error = %e,
                    "Rejecting execution_complete because worktree status could not be checked"
                );
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        if !dirty_summary.is_empty() {
            tracing::warn!(
                task_id = %task_id_str,
                worktree_path = %worktree_path,
                dirty_files = ?dirty_summary,
                "Rejecting execution_complete because task worktree has uncommitted changes"
            );
            return Err(HttpError::from(StatusCode::CONFLICT));
        }
    }

    ensure_task_has_non_empty_captured_diff(&task, &project, "execution_complete")
        .await
        .map_err(|e| {
            tracing::warn!(
                task_id = %task_id_str,
                error = %e,
                "Rejecting execution_complete because task-owned diff is empty or unavailable"
            );
            StatusCode::CONFLICT
        })?;

    // Agent-reported failed tests are terminal, but successful reports are not review evidence.
    if let Some(ref test_result) = req.test_result {
        if test_result.tests_ran && !test_result.tests_passed {
            let summary = test_result
                .test_summary
                .as_deref()
                .unwrap_or("reported tests failed")
                .trim();
            let message = if summary.is_empty() {
                "Validation failed: reported tests failed".to_string()
            } else {
                format!("Validation failed: {summary}")
            };
            persist_execution_complete_validation_rejection(&state, &task, &message).await?;
            tracing::warn!(
                task_id = %task_id_str,
                summary = %message,
                "Rejecting execution_complete because reported validation failed"
            );
            return Err(HttpError {
                status: StatusCode::CONFLICT,
                message: Some(
                    serde_json::json!({
                        "error": crate::application::chat_service::VALIDATION_FAILED_ERROR_CODE,
                        "details": message,
                    })
                    .to_string(),
                ),
            });
        }
    }

    if let Some(worktree_path) = task.worktree_path.as_deref() {
        let path = validate_absolute_non_root_path(
            std::path::Path::new(worktree_path),
            "execution complete validation promotion worktree",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let commit_sha = GitService::get_head_sha(&path)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if let Err(error) = TaskValidationService::promote_matching_validation_to_commit(
            &state.app_state,
            &task_id,
            &path,
            &commit_sha,
        )
        .await
        {
            tracing::warn!(task_id = %task_id_str, %error, "Could not promote validation evidence");
        }
    }

    // Close stdin via IPR — agent gets EOF and exits gracefully.
    // State transition happens in handle_stream_success when the process exits.
    let key = InteractiveProcessKey::new("task_execution", task_id_str.as_str());
    if state
        .app_state
        .interactive_process_registry
        .remove(&key)
        .await
        .is_some()
    {
        tracing::info!(
            "IPR entry removed for task {} — agent will receive EOF and exit",
            task_id_str
        );
    } else {
        tracing::warn!(
            "No IPR entry found for task {} — agent may have already exited",
            task_id_str
        );
    }

    Ok(Json(ExecutionCompleteResponse {
        success: true,
        message: format!("Execution complete for task {}", task_id_str),
    }))
}

async fn persist_execution_complete_validation_rejection(
    state: &HttpServerState,
    task: &Task,
    message: &str,
) -> Result<(), HttpError> {
    let mut metadata = task
        .metadata
        .as_deref()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if let Some(obj) = metadata.as_object_mut() {
        obj.insert(
            "failure_error".to_string(),
            serde_json::Value::String(message.to_string()),
        );
        obj.insert(
            "failure_source".to_string(),
            serde_json::to_value(ExecutionFailureSource::ValidationFailed)
                .unwrap_or_else(|_| serde_json::json!("validation_failed")),
        );
        obj.insert(
            "failed_at".to_string(),
            serde_json::Value::String(chrono::Utc::now().to_rfc3339()),
        );
    }
    state
        .app_state
        .task_repo
        .update_metadata(&task.id, Some(metadata.to_string()))
        .await
        .map_err(|error| {
            tracing::error!(
                task_id = %task.id.as_str(),
                error = %error,
                "Failed to persist validation rejection metadata"
            );
            HttpError::from(StatusCode::INTERNAL_SERVER_ERROR)
        })
}

#[cfg(test)]
#[path = "steps_tests.rs"]
mod steps_tests;
