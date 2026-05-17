use super::*;

#[derive(Debug, Deserialize)]
pub struct GetSessionTasksParams {
    pub changed_since: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SessionTask {
    pub id: String,
    pub title: String,
    pub status: String,
    pub proposal_id: Option<String>,
    pub category: String,
    pub priority: i32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct SessionTasksResponse {
    pub session_id: String,
    pub tasks: Vec<SessionTask>,
    pub delivery_status: String,
    pub task_count: usize,
}

/// Derive an aggregate delivery status from a slice of tasks linked to a session.
///
/// Rules (in priority order):
/// 1. `not_scheduled` — 0 tasks
/// 2. `delivered`     — all tasks are Merged
/// 3. `in_progress`   — any task is still actively executing / queued / in merge pipeline
/// 4. `pending_review`— no active tasks, some are in review states
/// 5. `partial`       — some Merged + rest are terminal (Cancelled / Failed / Stopped / Paused)
/// 6. `in_progress`   — fallback
pub(super) fn derive_delivery_status(tasks: &[Task]) -> String {
    if tasks.is_empty() {
        return "not_scheduled".to_string();
    }

    let mut all_merged = true;
    let mut has_merged = false;
    let mut has_active = false;
    let mut has_terminal = false;
    let mut has_review = false;

    for task in tasks {
        match task.internal_status {
            InternalStatus::Merged => {
                has_merged = true;
            }
            InternalStatus::Cancelled
            | InternalStatus::Failed
            | InternalStatus::Stopped
            | InternalStatus::Paused => {
                has_terminal = true;
                all_merged = false;
            }
            InternalStatus::PendingReview
            | InternalStatus::Reviewing
            | InternalStatus::ReviewPassed
            | InternalStatus::Escalated
            | InternalStatus::RevisionNeeded
            | InternalStatus::Approved => {
                has_review = true;
                all_merged = false;
            }
            _ => {
                // Backlog, Ready, Executing, ReExecuting, QaTesting, QaRefining, QaPassed,
                // QaFailed, Blocked, PendingMerge, Merging, MergeIncomplete, MergeConflict
                has_active = true;
                all_merged = false;
            }
        }
    }

    if all_merged {
        return "delivered".to_string();
    }
    if has_active {
        return "in_progress".to_string();
    }
    if has_review {
        return "pending_review".to_string();
    }
    if has_merged && has_terminal {
        return "partial".to_string();
    }
    "in_progress".to_string()
}

#[derive(Debug, Serialize)]
pub struct SessionSummary {
    pub id: String,
    pub title: Option<String>,
    pub status: String,
    pub proposal_count: u32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct ListSessionsResponse {
    pub sessions: Vec<SessionSummary>,
}

#[derive(Debug, Deserialize)]
pub struct ListSessionsParams {
    pub status: Option<String>,
    pub limit: Option<u32>,
    pub updated_after: Option<String>,
}
/// GET /api/external/sessions/:session_id/tasks
/// Get all tasks created from an ideation session with aggregate delivery_status.
pub async fn get_session_tasks_http(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Path(session_id): Path<String>,
    Query(params): Query<GetSessionTasksParams>,
) -> Result<Json<SessionTasksResponse>, HttpError> {
    let session_id_obj = IdeationSessionId::from_string(session_id.clone());

    // Load session to verify it exists and enforce scope
    let session = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id_obj)
        .await
        .map_err(|e| {
            error!("Failed to get ideation session {}: {}", session_id, e);
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("Failed to get ideation session".to_string()),
            }
        })?
        .ok_or(HttpError {
            status: StatusCode::NOT_FOUND,
            message: Some("Session not found".to_string()),
        })?;

    // Enforce project scope
    session
        .assert_project_scope(&scope)
        .map_err(|e| HttpError {
            status: e.status,
            message: e.message,
        })?;

    // Fetch all tasks linked to this session
    let tasks = state
        .app_state
        .task_repo
        .get_by_ideation_session(&session_id_obj)
        .await
        .map_err(|e| {
            error!("Failed to get tasks for session {}: {}", session_id, e);
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("Failed to get session tasks".to_string()),
            }
        })?;

    // Parse changed_since filter if provided (400 on invalid RFC3339)
    let since_cutoff = if let Some(ref cs) = params.changed_since {
        let dt = chrono::DateTime::parse_from_rfc3339(cs).map_err(|_| HttpError {
            status: StatusCode::BAD_REQUEST,
            message: Some(format!(
                "Invalid changed_since value '{}': must be ISO 8601 / RFC3339",
                cs
            )),
        })?;
        Some(dt.with_timezone(&chrono::Utc))
    } else {
        None
    };

    let delivery_status = derive_delivery_status(&tasks);

    // Apply changed_since filter in-memory after loading
    let filtered_tasks: Vec<_> = if let Some(cutoff) = since_cutoff {
        tasks
            .into_iter()
            .filter(|t| t.updated_at > cutoff)
            .collect()
    } else {
        tasks.into_iter().collect()
    };

    let task_count = filtered_tasks.len();

    let session_tasks: Vec<SessionTask> = filtered_tasks
        .into_iter()
        .map(|t| SessionTask {
            id: t.id.to_string(),
            title: t.title.clone(),
            status: t.internal_status.to_string(),
            proposal_id: t.source_proposal_id.as_ref().map(|p| p.to_string()),
            category: t.category.to_string(),
            priority: t.priority,
            created_at: t.created_at.to_rfc3339(),
            updated_at: t.updated_at.to_rfc3339(),
        })
        .collect();

    Ok(Json(SessionTasksResponse {
        session_id,
        tasks: session_tasks,
        delivery_status,
        task_count,
    }))
}

/// GET /api/external/sessions/:project_id?status=active&limit=20
/// List ideation sessions for a project, optionally filtered by status.
pub async fn list_ideation_sessions_http(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Path(project_id): Path<String>,
    Query(params): Query<ListSessionsParams>,
) -> Result<Json<ListSessionsResponse>, (StatusCode, Json<serde_json::Value>)> {
    let pid = ProjectId::from_string(project_id.clone());

    // Validate project exists and enforce scope
    let project = state
        .app_state
        .project_repo
        .get_by_id(&pid)
        .await
        .map_err(|e| {
            error!("Failed to get project {}: {}", project_id, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Internal server error"})),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Project not found"})),
            )
        })?;

    project
        .assert_project_scope(&scope)
        .map_err(|e| (e.status, Json(serde_json::json!({"error": "Forbidden"}))))?;

    let limit = params.limit.unwrap_or(20).clamp(1, 100);

    // Fetch sessions based on status filter
    let sessions = match params.status.as_deref() {
        None | Some("all") => {
            // Return all sessions for the project (up to limit, ordered by updated_at DESC)
            let all = state
                .app_state
                .ideation_session_repo
                .get_by_project(&pid)
                .await
                .map_err(|e| {
                    error!("Failed to list sessions for project {}: {}", project_id, e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({"error": "Internal server error"})),
                    )
                })?;
            all.into_iter().take(limit as usize).collect::<Vec<_>>()
        }
        Some(s @ ("active" | "accepted" | "archived")) => {
            let status_str = s.to_string();
            state
                .app_state
                .ideation_session_repo
                .get_by_project_and_status(pid.as_str(), &status_str, limit)
                .await
                .map_err(|e| {
                    error!(
                        "Failed to list sessions by status for project {}: {}",
                        project_id, e
                    );
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({"error": "Internal server error"})),
                    )
                })?
        }
        Some(_) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "Invalid status filter. Valid values: active, accepted, archived, all"
                })),
            ));
        }
    };

    // Apply updated_after filter before proposal-count loop
    let sessions = if let Some(ref updated_after_str) = params.updated_after {
        let cutoff = chrono::DateTime::parse_from_rfc3339(updated_after_str).map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid updated_after format. Expected ISO 8601 (RFC 3339)."})),
            )
        })?;
        sessions
            .into_iter()
            .filter(|s| s.updated_at > cutoff.with_timezone(&chrono::Utc))
            .collect::<Vec<_>>()
    } else {
        sessions
    };

    // Build summaries with proposal counts
    let mut summaries = Vec::with_capacity(sessions.len());
    for session in &sessions {
        let proposal_count = state
            .app_state
            .task_proposal_repo
            .count_by_session(&session.id)
            .await
            .map_err(|e| {
                error!(
                    "Failed to count proposals for session {}: {}",
                    session.id.as_str(),
                    e
                );
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "Internal server error"})),
                )
            })?;
        summaries.push(SessionSummary {
            id: session.id.to_string(),
            title: session.title.clone(),
            status: session.status.to_string(),
            proposal_count,
            created_at: session.created_at.to_rfc3339(),
            updated_at: session.updated_at.to_rfc3339(),
        });
    }

    Ok(Json(ListSessionsResponse {
        sessions: summaries,
    }))
}
