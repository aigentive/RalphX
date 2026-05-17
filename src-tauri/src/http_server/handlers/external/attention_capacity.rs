use super::*;

#[derive(Debug, Serialize)]
pub struct AttentionTaskSummary {
    pub task_id: String,
    pub title: String,
    pub status: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct AttentionItemsResponse {
    pub escalated_reviews: Vec<AttentionTaskSummary>,
    pub failed_tasks: Vec<AttentionTaskSummary>,
    pub merge_conflicts: Vec<AttentionTaskSummary>,
}

/// GET /api/external/attention/:project_id
/// Returns tasks that need human attention grouped by category.
pub async fn get_attention_items_http(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Path(project_id): Path<String>,
) -> Result<Json<AttentionItemsResponse>, StatusCode> {
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

    let tasks = state
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

    let mut escalated_reviews: Vec<AttentionTaskSummary> = Vec::new();
    let mut failed_tasks: Vec<AttentionTaskSummary> = Vec::new();
    let mut merge_conflicts: Vec<AttentionTaskSummary> = Vec::new();

    for task in &tasks {
        let summary = AttentionTaskSummary {
            task_id: task.id.to_string(),
            title: task.title.clone(),
            status: task.internal_status.to_string(),
            updated_at: task.updated_at.to_rfc3339(),
        };
        match task.internal_status {
            InternalStatus::Escalated | InternalStatus::RevisionNeeded => {
                escalated_reviews.push(summary);
            }
            InternalStatus::Failed => {
                failed_tasks.push(summary);
            }
            InternalStatus::MergeConflict | InternalStatus::MergeIncomplete => {
                merge_conflicts.push(summary);
            }
            _ => {}
        }
    }

    Ok(Json(AttentionItemsResponse {
        escalated_reviews,
        failed_tasks,
        merge_conflicts,
    }))
}

#[derive(Debug, Serialize)]
pub struct ExecutionCapacityResponse {
    pub can_start: bool,
    pub project_running: usize,
    pub project_queued: usize,
}

/// GET /api/external/execution_capacity/:project_id
/// Returns project-scoped execution slot counts (no global state exposed).
pub async fn get_execution_capacity_http(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Path(project_id): Path<String>,
) -> Result<Json<ExecutionCapacityResponse>, StatusCode> {
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

    let tasks = state
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

    let project_running = tasks
        .iter()
        .filter(|t| {
            matches!(
                t.internal_status,
                InternalStatus::Executing
                    | InternalStatus::ReExecuting
                    | InternalStatus::QaRefining
                    | InternalStatus::QaTesting
                    | InternalStatus::Reviewing
                    | InternalStatus::Merging
            )
        })
        .count();

    let project_queued = tasks
        .iter()
        .filter(|t| {
            matches!(
                t.internal_status,
                InternalStatus::Ready
                    | InternalStatus::PendingReview
                    | InternalStatus::PendingMerge
            )
        })
        .count();

    // can_start uses the global ExecutionState (shared across all projects)
    let can_start = state.execution_state.can_start_task();

    Ok(Json(ExecutionCapacityResponse {
        can_start,
        project_running,
        project_queued,
    }))
}
