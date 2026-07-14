use super::*;

#[derive(Debug, Serialize)]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub created_at: String,
    pub task_count: u32,
}

#[derive(Debug, Serialize)]
pub struct ListProjectsResponse {
    pub projects: Vec<ProjectSummary>,
}

#[derive(Debug, Serialize)]
pub struct ProjectStatusTaskCounts {
    pub total: usize,
    pub backlog: usize,
    pub ready: usize,
    pub executing: usize,
    pub reviewing: usize,
    pub merging: usize,
    pub merged: usize,
    pub cancelled: usize,
    pub stopped: usize,
    pub blocked: usize,
    pub pending_review: usize,
    pub pending_merge: usize,
    pub other: usize,
}

#[derive(Debug, Serialize)]
pub struct ProjectStatusProjectInfo {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct ProjectStatusResponse {
    pub project: ProjectStatusProjectInfo,
    pub task_counts: ProjectStatusTaskCounts,
    pub running_agents: usize,
}
/// GET /api/external/projects
/// List all projects, filtered by ProjectScope if present.
pub async fn list_projects_http(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
) -> Result<Json<ListProjectsResponse>, StatusCode> {
    let projects = state.app_state.project_repo.get_all().await.map_err(|e| {
        error!("Failed to list projects: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let mut summaries = Vec::new();
    for project in &projects {
        // Filter by scope if restricted
        if let Some(ref allowed) = scope.0 {
            if !allowed.contains(&project.id) {
                continue;
            }
        }

        let task_count = state
            .app_state
            .task_repo
            .count_tasks(&project.id, false, None, None)
            .await
            .unwrap_or(0);

        summaries.push(ProjectSummary {
            id: project.id.to_string(),
            name: project.name.clone(),
            description: None,
            created_at: project.created_at.to_rfc3339(),
            task_count,
        });
    }

    Ok(Json(ListProjectsResponse {
        projects: summaries,
    }))
}

/// GET /api/external/project/:id/status
/// Get project status including task counts and running agents.
pub async fn get_project_status_http(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Path(id): Path<String>,
) -> Result<Json<ProjectStatusResponse>, StatusCode> {
    let project_id = ProjectId::from_string(id);

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

    // Enforce project scope
    project.assert_project_scope(&scope).map_err(|e| e.status)?;

    // Load all tasks for project
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

    let mut counts = ProjectStatusTaskCounts {
        total: tasks.len(),
        backlog: 0,
        ready: 0,
        executing: 0,
        reviewing: 0,
        merging: 0,
        merged: 0,
        cancelled: 0,
        stopped: 0,
        blocked: 0,
        pending_review: 0,
        pending_merge: 0,
        other: 0,
    };

    for task in &tasks {
        match task.internal_status {
            InternalStatus::Backlog => counts.backlog += 1,
            InternalStatus::Ready => counts.ready += 1,
            InternalStatus::Executing
            | InternalStatus::QaRefining
            | InternalStatus::QaTesting
            | InternalStatus::QaPassed
            | InternalStatus::QaFailed
            | InternalStatus::ReExecuting
            | InternalStatus::UpdatingPlanBranch
            | InternalStatus::UpdatingTaskBranch => counts.executing += 1,
            InternalStatus::PendingReview => counts.pending_review += 1,
            InternalStatus::Reviewing
            | InternalStatus::ReviewPassed
            | InternalStatus::Escalated
            | InternalStatus::RevisionNeeded => counts.reviewing += 1,
            InternalStatus::Approved => counts.other += 1,
            InternalStatus::PendingMerge => counts.pending_merge += 1,
            InternalStatus::Merging
            | InternalStatus::WaitingOnPr
            | InternalStatus::MergeIncomplete
            | InternalStatus::MergeConflict => counts.merging += 1,
            InternalStatus::Merged => counts.merged += 1,
            InternalStatus::Failed => counts.other += 1,
            InternalStatus::Cancelled => counts.cancelled += 1,
            InternalStatus::Paused => counts.stopped += 1,
            InternalStatus::Stopped => counts.stopped += 1,
            InternalStatus::Blocked | InternalStatus::BranchUpdateBlocked => counts.blocked += 1,
        }
    }

    // Count running agents for this project by iterating the registry
    let all_running = state.app_state.running_agent_registry.list_all().await;
    let running_agents = all_running
        .iter()
        .filter(|(key, _)| {
            // task_execution:{task_id} — check if the task belongs to this project
            // We check by seeing if any task in our list matches
            if key.context_type == "task_execution" {
                tasks.iter().any(|t| t.id.as_str() == key.context_id)
            } else {
                false
            }
        })
        .count();

    Ok(Json(ProjectStatusResponse {
        project: ProjectStatusProjectInfo {
            id: project.id.to_string(),
            name: project.name.clone(),
        },
        task_counts: counts,
        running_agents,
    }))
}
