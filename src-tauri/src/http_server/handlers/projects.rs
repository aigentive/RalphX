use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use super::*;
use crate::application::git_service::GitBootstrapRequest;
use crate::application::GitService;
use crate::domain::entities::{InternalStatus, Project, ProjectId, TaskId};
use crate::domain::services::validate_project_path;
use crate::error::AppError;
use crate::infrastructure::git_auth::{inspect_repository_capability, RepositoryCapability};
use crate::infrastructure::sqlite::sqlite_project_repo::insert_project_row;

pub async fn list_tasks(
    State(state): State<HttpServerState>,
    Json(req): Json<ListTasksRequest>,
) -> Result<Json<ListTasksResponse>, StatusCode> {
    let project_id = ProjectId::from_string(req.project_id);

    // Parse optional status filter
    let status_filter = req
        .status
        .as_ref()
        .map(|s| parse_internal_status(s.as_str()))
        .transpose()
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    // Get all tasks for project
    let mut tasks = state
        .app_state
        .task_repo
        .get_by_project(&project_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Apply status filter if provided
    if let Some(status) = status_filter {
        tasks.retain(|t| t.internal_status == status);
    }

    // Convert to response
    let task_responses: Vec<_> = tasks.iter().map(task_to_response).collect();

    Ok(Json(ListTasksResponse {
        tasks: task_responses,
    }))
}

pub async fn suggest_task(
    State(state): State<HttpServerState>,
    Json(req): Json<SuggestTaskRequest>,
) -> Result<Json<SuggestTaskResponse>, StatusCode> {
    let project_id = ProjectId::from_string(req.project_id);

    // Get all backlog tasks for the project
    let tasks = state
        .app_state
        .task_repo
        .get_by_project(&project_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let backlog_tasks: Vec<_> = tasks
        .into_iter()
        .filter(|t| t.internal_status == InternalStatus::Backlog)
        .collect();

    if backlog_tasks.is_empty() {
        return Err(StatusCode::NOT_FOUND);
    }

    // Find highest priority task (higher i32 = higher priority)
    let suggested = backlog_tasks
        .iter()
        .max_by_key(|t| t.priority)
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(SuggestTaskResponse {
        task: task_to_response(suggested),
    }))
}

// ============================================================================
// Project Analysis Endpoints
// ============================================================================

/// Single analysis entry (path-scoped build/validation commands)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisEntry {
    pub path: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub validate: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub worktree_setup: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct AnalysisResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_secs: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entries: Option<Vec<AnalysisEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analyzed_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AnalysisQueryParams {
    pub task_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SaveAnalysisRequest {
    pub entries: Vec<AnalysisEntry>,
}

/// GET /api/projects/:id/analysis?task_id=
///
/// Returns project analysis data with resolved template variables.
/// If no analysis exists, returns { status: "analyzing", retry_after_secs: 30 }.
pub async fn get_project_analysis(
    State(state): State<HttpServerState>,
    Path(project_id): Path<String>,
    Query(params): Query<AnalysisQueryParams>,
) -> Result<Json<AnalysisResponse>, StatusCode> {
    let project_id = ProjectId::from_string(project_id);

    let project = state
        .app_state
        .project_repo
        .get_by_id(&project_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Check if analysis has been run — lazy spawn if not
    if project.analyzed_at.is_none() && project.custom_analysis.is_none() {
        crate::commands::project_commands::spawn_project_analyzer(
            &state.app_state,
            project_id.as_str(),
            &project.working_directory,
            state.app_state.app_handle.clone(),
        )
        .await;
        return Ok(Json(AnalysisResponse {
            status: "analyzing".to_string(),
            retry_after_secs: Some(30),
            entries: None,
            analyzed_at: None,
        }));
    }

    // Merge strategy: custom_analysis wins if set, else detected_analysis
    let analysis_json = project
        .custom_analysis
        .as_ref()
        .or(project.detected_analysis.as_ref());

    let entries: Vec<AnalysisEntry> = match analysis_json {
        Some(json) => serde_json::from_str(json).unwrap_or_default(),
        None => Vec::new(),
    };

    // Resolve template variables if task_id provided
    let resolved_entries = if let Some(task_id_str) = &params.task_id {
        let task_id = TaskId::from_string(task_id_str.to_string());
        let task = state
            .app_state
            .task_repo
            .get_by_id(&task_id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let worktree_path = task
            .as_ref()
            .and_then(|t| t.worktree_path.as_deref())
            .unwrap_or("");
        let task_branch = task
            .as_ref()
            .and_then(|t| t.task_branch.as_deref())
            .unwrap_or("");

        resolve_template_vars(
            entries,
            &project.working_directory,
            worktree_path,
            task_branch,
        )
    } else {
        resolve_template_vars(entries, &project.working_directory, "", "")
    };

    Ok(Json(AnalysisResponse {
        status: "ready".to_string(),
        retry_after_secs: None,
        entries: Some(resolved_entries),
        analyzed_at: project.analyzed_at,
    }))
}

/// POST /api/projects/:id/analysis
///
/// Saves detected analysis data. Updates detected_analysis + analyzed_at.
/// Never touches custom_analysis.
pub async fn save_project_analysis(
    State(state): State<HttpServerState>,
    Path(project_id): Path<String>,
    Json(req): Json<SaveAnalysisRequest>,
) -> Result<Json<SuccessResponse>, StatusCode> {
    let project_id = ProjectId::from_string(project_id);

    let mut project = state
        .app_state
        .project_repo
        .get_by_id(&project_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Update detected_analysis (never touch custom_analysis)
    let entries_json = serde_json::to_string(&req.entries).map_err(|_| StatusCode::BAD_REQUEST)?;
    project.detected_analysis = Some(entries_json);
    project.analyzed_at = Some(chrono::Utc::now().to_rfc3339());
    project.touch();

    state
        .app_state
        .project_repo
        .update(&project)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    crate::http_server::emit_http_event(
        &state,
        "project:analysis_complete",
        serde_json::json!({
            "project_id": project_id.as_str(),
            "detected_analysis": project.detected_analysis,
            "analyzed_at": project.analyzed_at,
        }),
    );

    Ok(Json(SuccessResponse {
        success: true,
        message: format!("Saved {} analysis entries", req.entries.len()),
    }))
}

// ============================================================================
// External Project Registration
// ============================================================================

/// Response for POST /api/external/projects
#[derive(Debug, Serialize)]
pub struct RegisterProjectExternalResponse {
    pub id: String,
    pub name: String,
    pub working_directory: String,
    pub base_branch: String,
    pub worktree_parent_directory: Option<String>,
    pub github_pr_enabled: bool,
    pub repository_capability: RepositoryCapability,
    pub created_at: String,
}

/// POST /api/external/projects
///
/// Registers a directory as a RalphX project. Handles all directory/git states:
/// - Path doesn't exist → create directory + git init + empty commit
/// - Path exists, no .git → git init + empty commit
/// - Path exists, .git exists, no commits → empty commit
/// - Path exists, .git + commits → project record only
///
/// Requires CREATE_PROJECT permission (enforced by ValidatedExternalKey extractor).
/// Auto-adds the creating key to the new project's scope.
pub async fn register_project_external(
    State(state): State<HttpServerState>,
    validated_key: ValidatedExternalKey,
    Json(req): Json<RegisterProjectExternalRequest>,
) -> Result<Json<RegisterProjectExternalResponse>, HttpError> {
    let RegisterProjectExternalRequest {
        working_directory,
        name,
        base_branch,
        worktree_parent_directory,
    } = req;

    // 1-3. Validate path: canonicalize, blocklist, home directory check (shared helper)
    let canonical = validate_project_path(&working_directory).map_err(|e| HttpError {
        status: StatusCode::UNPROCESSABLE_ENTITY,
        message: Some(e.to_string()),
    })?;

    let canonical_str = canonical.to_string_lossy().to_string();

    // 4. Duplicate check → 409 Conflict
    let existing = state
        .app_state
        .project_repo
        .get_by_working_directory(&canonical_str)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if existing.is_some() {
        return Err(HttpError {
            status: StatusCode::CONFLICT,
            message: Some(format!("Project already exists at: {canonical_str}")),
        });
    }

    // 5. Create directory if it doesn't exist
    let created_dir = !canonical.exists();
    if created_dir {
        std::fs::create_dir_all(&canonical).map_err(|e| HttpError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: Some(format!("Failed to create directory: {e}")),
        })?;
    }

    // 6. Ensure git is initialized (git-first: before DB to avoid zombie records)
    let ran_git_init = !canonical.join(".git").exists();
    let bootstrap =
        GitService::bootstrap_project_repository(&canonical, GitBootstrapRequest::new(base_branch))
            .await
            .map_err(|error| HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some(format!("Git initialization failed: {error}")),
            })?;
    let repository_capability = inspect_repository_capability(&canonical).await;

    // 7. Construct project with domain defaults (Project::new handles UUID, timestamps, etc.)
    let name = name.unwrap_or_else(|| {
        canonical
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unnamed Project".to_string())
    });
    let mut project = Project::new(name, canonical_str.clone());
    project.base_branch = Some(bootstrap.base_branch.clone());
    project.worktree_parent_directory = worktree_parent_directory;
    project.github_pr_enabled =
        matches!(repository_capability, RepositoryCapability::Github { .. });

    // Extract response data before moving project into transaction closure
    let response_id = project.id.as_str().to_string();
    let response_name = project.name.clone();
    let response_created_at = project.created_at.to_rfc3339();
    let response_worktree_parent_directory = project.worktree_parent_directory.clone();
    let response_github_pr_enabled = project.github_pr_enabled;
    let cached_repository_capability =
        crate::infrastructure::git_auth::cached_repository_capability(
            &project,
            &repository_capability,
        );
    let key_id = validated_key.key_id.clone();

    // 8. Atomic DB transaction: INSERT project + INSERT OR IGNORE api_key_projects
    state
        .app_state
        .db
        .run_transaction(move |conn| {
            insert_project_row(conn, &project)
                .map_err(|e| AppError::Database(format!("Insert project failed: {e}")))?;

            // Auto-add creating key to new project's scope (INSERT OR IGNORE handles races)
            conn.execute(
                "INSERT OR IGNORE INTO api_key_projects (api_key_id, project_id) VALUES (?1, ?2)",
                rusqlite::params![key_id, project.id.as_str()],
            )
            .map_err(|e| AppError::Database(format!("Insert api_key_projects failed: {e}")))?;

            Ok(())
        })
        .await
        .map_err(|e| {
            tracing::error!("DB transaction failed for register_project_external: {e}");
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("Failed to save project".to_string()),
            }
        })?;

    if let Err(error) = state
        .app_state
        .project_repository_capability_repo
        .upsert(&cached_repository_capability)
        .await
    {
        tracing::warn!(project_id = %response_id, %error, "failed to cache repository capability");
    }

    // 9. Audit log
    tracing::info!(
        key_id = %validated_key.key_id,
        project_id = %response_id,
        path = %canonical_str,
        dir_created = %created_dir,
        git_initialized = %ran_git_init,
        "External project registered"
    );

    // 10. Fire-and-forget: spawn project analyzer (non-blocking)
    crate::commands::project_commands::spawn_project_analyzer(
        &state.app_state,
        &response_id,
        &canonical_str,
        state.app_state.app_handle.clone(),
    )
    .await;

    Ok(Json(RegisterProjectExternalResponse {
        id: response_id,
        name: response_name,
        working_directory: canonical_str,
        base_branch: bootstrap.base_branch,
        worktree_parent_directory: response_worktree_parent_directory,
        github_pr_enabled: response_github_pr_enabled,
        repository_capability,
        created_at: response_created_at,
    }))
}

/// Resolve template variables in analysis entries
fn resolve_template_vars(
    entries: Vec<AnalysisEntry>,
    project_root: &str,
    worktree_path: &str,
    task_branch: &str,
) -> Vec<AnalysisEntry> {
    entries
        .into_iter()
        .map(|entry| {
            let resolve = |s: String| -> String {
                s.replace("{project_root}", project_root)
                    .replace("{worktree_path}", worktree_path)
                    .replace("{task_branch}", task_branch)
            };
            AnalysisEntry {
                path: resolve(entry.path),
                label: entry.label,
                install: entry.install.map(resolve),
                validate: entry.validate.into_iter().map(resolve).collect(),
                worktree_setup: entry.worktree_setup.into_iter().map(resolve).collect(),
            }
        })
        .collect()
}
