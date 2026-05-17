use super::*;

#[derive(Debug, Deserialize)]
pub struct BatchTaskStatusRequest {
    pub task_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct BatchTaskStatusItem {
    pub id: String,
    pub title: String,
    pub status: String,
    pub project_id: String,
}

#[derive(Debug, Serialize)]
pub struct BatchTaskStatusError {
    pub id: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct BatchTaskStatusResponse {
    pub tasks: Vec<BatchTaskStatusItem>,
    pub errors: Vec<BatchTaskStatusError>,
    pub requested_count: usize,
    pub returned_count: usize,
}

/// POST /api/external/tasks/batch_status
/// Batch lookup up to 50 task IDs.
/// Returns tasks array + errors array with reason: "not_found" | "access_denied"
pub async fn batch_task_status_http(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<BatchTaskStatusRequest>,
) -> Result<Json<BatchTaskStatusResponse>, (StatusCode, String)> {
    if req.task_ids.len() > 50 {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Too many task IDs: {}. Maximum is 50.", req.task_ids.len()),
        ));
    }

    let requested_count = req.task_ids.len();
    let mut tasks = Vec::new();
    let mut errors = Vec::new();

    for raw_id in &req.task_ids {
        let task_id = TaskId::from_string(raw_id.clone());
        match state.app_state.task_repo.get_by_id(&task_id).await {
            Ok(Some(task)) => {
                if task.assert_project_scope(&scope).is_err() {
                    errors.push(BatchTaskStatusError {
                        id: raw_id.clone(),
                        reason: "access_denied".to_string(),
                    });
                } else {
                    tasks.push(BatchTaskStatusItem {
                        id: task.id.to_string(),
                        title: task.title.clone(),
                        status: task.internal_status.to_string(),
                        project_id: task.project_id.to_string(),
                    });
                }
            }
            Ok(None) => {
                errors.push(BatchTaskStatusError {
                    id: raw_id.clone(),
                    reason: "not_found".to_string(),
                });
            }
            Err(e) => {
                error!("Failed to get task {}: {}", raw_id, e);
                errors.push(BatchTaskStatusError {
                    id: raw_id.clone(),
                    reason: "not_found".to_string(),
                });
            }
        }
    }

    let returned_count = tasks.len();
    Ok(Json(BatchTaskStatusResponse {
        tasks,
        errors,
        requested_count,
        returned_count,
    }))
}
