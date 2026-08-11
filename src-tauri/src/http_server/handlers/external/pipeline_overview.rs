use super::*;

#[derive(Debug, Serialize)]
pub struct PipelineStages {
    pub pending: usize,
    pub executing: usize,
    pub reviewing: usize,
    pub pending_merge: usize,
    pub merging: usize,
    pub merged: usize,
    pub blocked: usize,
    pub cancelled: usize,
    pub stopped: usize,
}

#[derive(Debug, Serialize)]
pub struct ChangedTask {
    pub id: String,
    pub title: String,
    pub status: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct GetPipelineParams {
    pub since: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PipelineOverviewResponse {
    pub project_id: String,
    pub stages: PipelineStages,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_tasks: Option<Vec<ChangedTask>>,
}

#[derive(Debug, Deserialize)]
pub struct PollEventsQuery {
    pub project_id: String,
    pub cursor: Option<i64>,
    pub limit: Option<i64>,
    pub event_type: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ExternalEvent {
    pub id: i64,
    pub event_type: String,
    pub project_id: String,
    pub payload: serde_json::Value,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct PollEventsResponse {
    pub events: Vec<ExternalEvent>,
    pub next_cursor: Option<i64>,
    pub has_more: bool,
}
/// GET /api/external/pipeline/:project_id
/// Get pipeline overview — task counts per stage.
pub async fn get_pipeline_overview_http(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Path(project_id): Path<String>,
    Query(params): Query<GetPipelineParams>,
) -> Result<Json<PipelineOverviewResponse>, HttpError> {
    let project_id = ProjectId::from_string(project_id);

    // Validate project exists and is in scope
    let project = state
        .app_state
        .project_repo
        .get_by_id(&project_id)
        .await
        .map_err(|e| {
            error!("Failed to get project {}: {}", project_id.as_str(), e);
            HttpError::from(StatusCode::INTERNAL_SERVER_ERROR)
        })?
        .ok_or_else(|| HttpError::from(StatusCode::NOT_FOUND))?;

    project
        .assert_project_scope(&scope)
        .map_err(|e| HttpError::from(e.status))?;

    // Load all tasks
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
            HttpError::from(StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    let mut stages = PipelineStages {
        pending: 0,
        executing: 0,
        reviewing: 0,
        pending_merge: 0,
        merging: 0,
        merged: 0,
        blocked: 0,
        cancelled: 0,
        stopped: 0,
    };

    // Stage counts over ALL tasks (regardless of `since` filter)
    for task in &tasks {
        match task.internal_status {
            InternalStatus::Backlog | InternalStatus::Ready => stages.pending += 1,
            InternalStatus::Executing
            | InternalStatus::QaRefining
            | InternalStatus::QaTesting
            | InternalStatus::QaPassed
            | InternalStatus::QaFailed
            | InternalStatus::ReExecuting
            | InternalStatus::UpdatingPlanBranch
            | InternalStatus::UpdatingTaskBranch => stages.executing += 1,
            InternalStatus::PendingReview
            | InternalStatus::Reviewing
            | InternalStatus::ReviewPassed
            | InternalStatus::Escalated
            | InternalStatus::RevisionNeeded => stages.reviewing += 1,
            InternalStatus::Approved | InternalStatus::PendingMerge => stages.pending_merge += 1,
            InternalStatus::Merging
            | InternalStatus::WaitingOnPr
            | InternalStatus::MergeIncomplete
            | InternalStatus::MergeConflict => stages.merging += 1,
            InternalStatus::Merged => stages.merged += 1,
            InternalStatus::Blocked | InternalStatus::BranchUpdateBlocked => stages.blocked += 1,
            InternalStatus::Cancelled | InternalStatus::Failed => stages.cancelled += 1,
            InternalStatus::Paused | InternalStatus::Stopped => stages.stopped += 1,
        }
    }

    // If `since` is provided, compute changed_tasks (tasks with updated_at > since)
    let changed_tasks = if let Some(since_str) = params.since {
        let since = chrono::DateTime::parse_from_rfc3339(&since_str).map_err(|_| {
            HttpError::validation(format!("Invalid `since` timestamp: {since_str}"))
        })?;
        let filtered: Vec<ChangedTask> = tasks
            .iter()
            .filter(|t| t.updated_at > since.with_timezone(&chrono::Utc))
            .map(|t| ChangedTask {
                id: t.id.to_string(),
                title: t.title.clone(),
                status: t.internal_status.to_string(),
                updated_at: t.updated_at.to_rfc3339(),
            })
            .collect();
        Some(filtered)
    } else {
        None
    };

    Ok(Json(PipelineOverviewResponse {
        project_id: project.id.to_string(),
        stages,
        changed_tasks,
    }))
}

/// GET /api/external/events/poll
/// Poll external events for a project with cursor-based pagination.
pub async fn poll_events_http(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Query(params): Query<PollEventsQuery>,
) -> Result<Json<PollEventsResponse>, StatusCode> {
    let project_id = ProjectId::from_string(params.project_id.clone());

    // Validate project exists and is in scope
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

    let cursor = params.cursor.unwrap_or(0);
    let limit = params.limit.unwrap_or(50).clamp(1, 200);
    let project_id_str = project_id.to_string();
    let event_type_filter = params.event_type.clone();

    // Query external_events via the shared db connection
    let events = state
        .app_state
        .db
        .run(move |conn| {
            let mut sql = "SELECT id, event_type, project_id, payload, created_at \
                 FROM external_events \
                 WHERE project_id = ?1 AND id > ?2"
                .to_string();
            let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> =
                vec![Box::new(project_id_str.clone()), Box::new(cursor)];
            if let Some(ref et) = event_type_filter {
                sql.push_str(" AND event_type = ?3");
                params_vec.push(Box::new(et.clone()));
                sql.push_str(" ORDER BY id ASC LIMIT ?4");
                params_vec.push(Box::new(limit + 1));
            } else {
                sql.push_str(" ORDER BY id ASC LIMIT ?3");
                params_vec.push(Box::new(limit + 1));
            }
            let params_refs: Vec<&dyn rusqlite::types::ToSql> =
                params_vec.iter().map(|p| p.as_ref()).collect();
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params_refs.as_slice(), |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?;
            let mut result = Vec::new();
            for row in rows {
                result.push(row.map_err(|e| crate::error::AppError::Database(e.to_string()))?);
            }
            Ok(result)
        })
        .await
        .map_err(|e| {
            error!("Failed to query external_events: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let has_more = events.len() as i64 > limit;
    let events_page: Vec<_> = events.into_iter().take(limit as usize).collect();
    let next_cursor = if has_more {
        events_page.last().map(|(id, _, _, _, _)| *id)
    } else {
        None
    };

    let event_responses: Vec<ExternalEvent> = events_page
        .into_iter()
        .map(|(id, event_type, proj_id, payload, created_at)| {
            let payload_json: serde_json::Value =
                serde_json::from_str(&payload).unwrap_or(serde_json::json!({}));
            ExternalEvent {
                id,
                event_type,
                project_id: proj_id,
                payload: payload_json,
                created_at,
            }
        })
        .collect();

    Ok(Json(PollEventsResponse {
        events: event_responses,
        next_cursor,
        has_more,
    }))
}
