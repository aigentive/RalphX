use super::*;
use crate::application::{
    merge_pipeline_visibility::ArchivedParentMergeVisibility,
    task_diff_base::read_task_diff_stats_from_resolved_base,
    task_restart::build_terminal_ready_restart_plan,
};
use crate::error::AppError;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionAction {
    Pause,
    Cancel,
    Retry,
}

#[derive(Debug, Deserialize)]
pub struct TaskTransitionRequest {
    pub task_id: String,
    pub action: TransitionAction,
}

#[derive(Debug, Serialize)]
pub struct TaskTransitionResponse {
    pub success: bool,
    pub task_id: String,
    pub new_status: String,
}

/// POST /api/external/task_transition
/// Transition a task's state (pause, cancel, retry).
pub async fn external_task_transition_http(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<TaskTransitionRequest>,
) -> Result<Json<TaskTransitionResponse>, HttpError> {
    let task_id = TaskId::from_string(req.task_id.clone());

    // Load task and enforce scope
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

    let feature_action = match &req.action {
        TransitionAction::Pause | TransitionAction::Cancel => {
            crate::domain::ideation::TasksFeatureAction::Quiesce
        }
        TransitionAction::Retry => crate::domain::ideation::TasksFeatureAction::Progress,
    };
    crate::application::tasks_feature_policy::TasksFeaturePolicy::from_state(&state.app_state)
        .authorize_session(task.ideation_session_id.as_ref(), feature_action)
        .await
        .map_err(tasks_feature_http_error)?;

    let is_retry_restart =
        matches!(&req.action, TransitionAction::Retry) && task.internal_status.is_terminal();

    // Map action to target status
    let target_status = match req.action {
        TransitionAction::Pause => InternalStatus::Paused,
        TransitionAction::Cancel => InternalStatus::Cancelled,
        TransitionAction::Retry => {
            if task.internal_status.is_terminal() {
                InternalStatus::Ready
            } else {
                return Err(StatusCode::BAD_REQUEST.into());
            }
        }
    };

    let terminal_restart_plan = if is_retry_restart && target_status == InternalStatus::Ready {
        build_terminal_ready_restart_plan(&state.app_state.task_step_repo, &task)
            .await
            .map_err(|error| {
                error!(
                    task_id = task_id.as_str(),
                    error = %error,
                    "External retry failed to build terminal restart plan"
                );
                StatusCode::INTERNAL_SERVER_ERROR
            })?
    } else {
        None
    };

    let mut transition_service_builder = state
        .app_state
        .build_transition_service_with_execution_state(std::sync::Arc::clone(
            &state.execution_state,
        ));

    if let Some(ref pub_) = state.app_state.webhook_publisher {
        transition_service_builder = transition_service_builder
            .with_webhook_publisher_for_emitter(std::sync::Arc::clone(pub_));
    }

    let transition_service = transition_service_builder
        .with_external_events_repo(std::sync::Arc::clone(&state.app_state.external_events_repo));

    let updated_task = if let Some(plan) = terminal_restart_plan {
        transition_service
            .restart_terminal_task_to_ready(plan, None)
            .await
    } else {
        transition_service
            .transition_task(&task_id, target_status)
            .await
    }
    .map_err(|error| {
        error!("Failed to transition task {}: {}", task_id.as_str(), error);
        tasks_feature_http_error(error)
    })?;

    Ok(Json(TaskTransitionResponse {
        success: true,
        task_id: updated_task.id.to_string(),
        new_status: updated_task.internal_status.to_string(),
    }))
}

fn tasks_feature_http_error(error: AppError) -> HttpError {
    match error {
        AppError::FeatureDisabled(message) => HttpError {
            status: StatusCode::CONFLICT,
            message: Some(message),
        },
        _ => HttpError::from(StatusCode::UNPROCESSABLE_ENTITY),
    }
}

#[derive(Debug, Serialize)]
pub struct TaskStepSummary {
    pub id: String,
    pub title: String,
    pub status: String,
    pub sort_order: i32,
}

#[derive(Debug, Serialize)]
pub struct TaskDetailResponse {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub project_id: String,
    pub task_branch: Option<String>,
    pub worktree_path: Option<String>,
    pub steps: Vec<TaskStepSummary>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct TaskDiffResponse {
    pub task_id: String,
    pub files_changed: u32,
    pub insertions: u32,
    pub deletions: u32,
    pub changed_files: Vec<String>,
    pub task_branch: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ReviewNoteSummary {
    pub id: String,
    pub reviewer: String,
    pub outcome: String,
    pub notes: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct ReviewSummaryResponse {
    pub task_id: String,
    pub task_status: String,
    pub review_notes: Vec<ReviewNoteSummary>,
    pub revision_count: u32,
}

#[derive(Debug, Serialize)]
pub struct MergePipelineTask {
    pub id: String,
    pub title: String,
    pub status: String,
    pub task_branch: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct MergePipelineResponse {
    pub project_id: String,
    pub tasks: Vec<MergePipelineTask>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewActionType {
    ApproveReview,
    RequestChanges,
    ResolveEscalation,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EscalationResolution {
    Approve,
    RequestChanges,
    Cancel,
}

#[derive(Debug, Deserialize)]
pub struct ReviewActionRequest {
    pub task_id: String,
    pub action: ReviewActionType,
    // feedback is part of the public API; reserved for future use (audit log, reviewer note).
    #[allow(dead_code)]
    pub feedback: Option<String>,
    pub resolution: Option<EscalationResolution>,
}

#[derive(Debug, Serialize)]
pub struct ReviewActionResponse {
    pub success: bool,
    pub task_id: String,
    pub new_status: String,
}

// ============================================================================
// Phase 5: Pipeline supervision handlers
// ============================================================================

/// GET /api/external/task/:id
/// Get full task details + steps.
pub async fn get_task_detail_http(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Path(id): Path<String>,
) -> Result<Json<TaskDetailResponse>, StatusCode> {
    let task_id = crate::domain::entities::TaskId::from_string(id);

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

    let steps = state
        .app_state
        .task_step_repo
        .get_by_task(&task_id)
        .await
        .map_err(|e| {
            error!("Failed to get steps for task {}: {}", task_id.as_str(), e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let step_summaries: Vec<TaskStepSummary> = steps
        .into_iter()
        .map(|s| TaskStepSummary {
            id: s.id.to_string(),
            title: s.title.clone(),
            status: format!("{:?}", s.status),
            sort_order: s.sort_order,
        })
        .collect();

    Ok(Json(TaskDetailResponse {
        id: task.id.to_string(),
        title: task.title.clone(),
        description: task.description.clone(),
        status: task.internal_status.to_string(),
        project_id: task.project_id.to_string(),
        task_branch: task.task_branch.clone(),
        worktree_path: task.worktree_path.clone(),
        steps: step_summaries,
        created_at: task.created_at.to_rfc3339(),
        updated_at: task.updated_at.to_rfc3339(),
    }))
}

/// GET /api/external/task/:id/diff
/// Get git diff stats for a task branch.
pub async fn get_task_diff_http(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Path(id): Path<String>,
) -> Result<Json<TaskDiffResponse>, StatusCode> {
    let task_id = crate::domain::entities::TaskId::from_string(id);

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

    let task_branch = task.task_branch.clone();

    // If no branch, return empty diff
    if task_branch.is_none() {
        return Ok(Json(TaskDiffResponse {
            task_id: task.id.to_string(),
            files_changed: 0,
            insertions: 0,
            deletions: 0,
            changed_files: vec![],
            task_branch: None,
        }));
    }

    let project = state
        .app_state
        .project_repo
        .get_by_id(&task.project_id)
        .await
        .map_err(|e| {
            error!("Failed to get project {}: {}", task.project_id.as_str(), e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    let stats = read_task_diff_stats_from_resolved_base(
        &state.app_state,
        &task,
        &project,
        "external task diff stats",
    )
    .await
    .map_err(|e| {
        error!(
            "Failed to get diff stats for task {}: {}",
            task_id.as_str(),
            e
        );
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(TaskDiffResponse {
        task_id: task.id.to_string(),
        files_changed: stats.files_changed,
        insertions: stats.insertions,
        deletions: stats.deletions,
        changed_files: stats.changed_files,
        task_branch,
    }))
}

/// GET /api/external/task/:id/review_summary
/// Get review notes and findings for a task.
pub async fn get_task_review_summary_http(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Path(id): Path<String>,
) -> Result<Json<ReviewSummaryResponse>, StatusCode> {
    use crate::domain::entities::review::ReviewOutcome;

    let task_id = crate::domain::entities::TaskId::from_string(id);

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

    let notes = state
        .app_state
        .review_repo
        .get_notes_by_task_id(&task_id)
        .await
        .map_err(|e| {
            error!(
                "Failed to get review notes for task {}: {}",
                task_id.as_str(),
                e
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let revision_count = notes
        .iter()
        .filter(|n| n.outcome == ReviewOutcome::ChangesRequested)
        .count() as u32;

    let note_summaries: Vec<ReviewNoteSummary> = notes
        .into_iter()
        .map(|n| ReviewNoteSummary {
            id: n.id.to_string(),
            reviewer: n.reviewer.to_string(),
            outcome: n.outcome.to_string(),
            notes: n.notes,
            created_at: n.created_at.to_rfc3339(),
        })
        .collect();

    Ok(Json(ReviewSummaryResponse {
        task_id: task.id.to_string(),
        task_status: task.internal_status.to_string(),
        review_notes: note_summaries,
        revision_count,
    }))
}

/// GET /api/external/merge_pipeline/:project_id
/// Get all tasks in merge-related states for a project.
pub async fn get_merge_pipeline_http(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Path(project_id): Path<String>,
) -> Result<Json<MergePipelineResponse>, StatusCode> {
    let project_id = ProjectId::from_string(project_id);

    let project = state
        .app_state
        .project_repo
        .get_by_id(&project_id)
        .await
        .map_err(|e| {
            error!("Failed to get project {}: {}", project_id.as_str(), e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    project.assert_project_scope(&scope).map_err(|e| e.status)?;

    let all_tasks = state
        .app_state
        .task_repo
        .get_by_project(&project_id)
        .await
        .map_err(|e| {
            error!(
                "Failed to get tasks for project {}: {}",
                project_id.as_str(),
                e
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let plan_branches = state
        .app_state
        .plan_branch_repo
        .get_by_project_id(&project_id)
        .await
        .map_err(|e| {
            error!(
                "Failed to get plan branches for project {}: {}",
                project_id.as_str(),
                e
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let agent_workspaces = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_project_id(&project_id)
        .await
        .map_err(|e| {
            error!(
                "Failed to get agent workspaces for project {}: {}",
                project_id.as_str(),
                e
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let archived_parent_visibility =
        ArchivedParentMergeVisibility::from_workspaces(&agent_workspaces);

    // Filter to tasks in merge-related states
    let merge_tasks: Vec<MergePipelineTask> = all_tasks
        .into_iter()
        .filter(|t| {
            t.archived_at.is_none()
                && matches!(
                    t.internal_status,
                    InternalStatus::PendingMerge
                        | InternalStatus::Merging
                        | InternalStatus::MergeIncomplete
                        | InternalStatus::MergeConflict
                        | InternalStatus::Merged
                )
                && !archived_parent_visibility.hides_task(t, &plan_branches)
        })
        .map(|t| MergePipelineTask {
            id: t.id.to_string(),
            title: t.title.clone(),
            status: t.internal_status.to_string(),
            task_branch: t.task_branch.clone(),
            updated_at: t.updated_at.to_rfc3339(),
        })
        .collect();

    Ok(Json(MergePipelineResponse {
        project_id: project.id.to_string(),
        tasks: merge_tasks,
    }))
}

/// POST /api/external/review_action
/// Approve a review, request changes, or resolve an escalation.
/// All operations go through TaskTransitionService for state machine enforcement.
pub async fn review_action_http(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<ReviewActionRequest>,
) -> Result<Json<ReviewActionResponse>, HttpError> {
    let task_id = crate::domain::entities::TaskId::from_string(req.task_id.clone());

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

    let action_allowed = match &req.action {
        ReviewActionType::ApproveReview | ReviewActionType::RequestChanges => matches!(
            task.internal_status,
            InternalStatus::ReviewPassed | InternalStatus::Escalated
        ),
        ReviewActionType::ResolveEscalation => {
            matches!(task.internal_status, InternalStatus::Escalated)
        }
    };

    if !action_allowed {
        tracing::warn!(
            task_id = task_id.as_str(),
            action = ?req.action,
            current_status = task.internal_status.as_str(),
            "Rejected external review action due to invalid task status"
        );
        return Err(StatusCode::UNPROCESSABLE_ENTITY.into());
    }

    let target_status = match &req.action {
        ReviewActionType::ApproveReview => InternalStatus::Approved,
        ReviewActionType::RequestChanges => InternalStatus::RevisionNeeded,
        ReviewActionType::ResolveEscalation => {
            match req.resolution.as_ref().ok_or(StatusCode::BAD_REQUEST)? {
                EscalationResolution::Approve => InternalStatus::Approved,
                EscalationResolution::RequestChanges => InternalStatus::RevisionNeeded,
                EscalationResolution::Cancel => InternalStatus::Cancelled,
            }
        }
    };

    let mut transition_service_builder = state
        .app_state
        .build_transition_service_with_execution_state(std::sync::Arc::clone(
            &state.execution_state,
        ));

    if let Some(ref pub_) = state.app_state.webhook_publisher {
        transition_service_builder = transition_service_builder
            .with_webhook_publisher_for_emitter(std::sync::Arc::clone(pub_));
    }

    let transition_service = transition_service_builder
        .with_external_events_repo(std::sync::Arc::clone(&state.app_state.external_events_repo));

    let updated_task = transition_service
        .transition_task(&task_id, target_status)
        .await
        .map_err(|error| {
            error!(
                "Failed to transition task {} for review action: {}",
                task_id.as_str(),
                error
            );
            tasks_feature_http_error(error)
        })?;

    Ok(Json(ReviewActionResponse {
        success: true,
        task_id: updated_task.id.to_string(),
        new_status: updated_task.internal_status.to_string(),
    }))
}
#[derive(Debug, Deserialize)]
pub struct CreateTaskNoteRequest {
    pub task_id: String,
    pub note: String,
}

#[derive(Debug, Serialize)]
pub struct TaskNoteResponse {
    pub task_id: String,
    pub success: bool,
}

/// POST /api/external/task-note
/// Add a progress note to a task. Proxies to add_task_note logic.
pub async fn create_task_note_http(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<CreateTaskNoteRequest>,
) -> Result<Json<TaskNoteResponse>, HttpError> {
    let task_id = crate::domain::entities::TaskId::from_string(req.task_id);

    let mut task = state
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

    crate::application::tasks_feature_policy::TasksFeaturePolicy::from_state(&state.app_state)
        .authorize_session(
            task.ideation_session_id.as_ref(),
            crate::domain::ideation::TasksFeatureAction::Progress,
        )
        .await
        .map_err(tasks_feature_http_error)?;

    let note_text = format!("\n\n---\n**Note:** {}", req.note);
    task.description = Some(match task.description {
        Some(existing) => format!("{}{}", existing, note_text),
        None => note_text,
    });

    state.app_state.task_repo.update(&task).await.map_err(|e| {
        error!("Failed to update task {}: {}", task_id.as_str(), e);
        tasks_feature_http_error(e)
    })?;

    Ok(Json(TaskNoteResponse {
        task_id: task.id.to_string(),
        success: true,
    }))
}
