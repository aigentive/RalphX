use std::path::{Path, PathBuf};

use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use super::*;
use crate::application::{
    DiffService, FileChange, FileDiff, RunTaskValidationRequest, TaskValidationService,
    TaskValidationSummary,
};
use crate::domain::entities::{Project, Task, TaskId};
use crate::error::{AppError, AppResult};
use crate::utils::path_safety::validate_absolute_non_root_path;

pub async fn run_task_validation_http(
    State(state): State<HttpServerState>,
    Json(req): Json<RunTaskValidationRequest>,
) -> Result<Json<TaskValidationSummary>, StatusCode> {
    TaskValidationService::run_task_validation(&state.app_state, req)
        .await
        .map(Json)
        .map_err(status_from_app_error)
}

pub async fn get_task_validation_summary_http(
    State(state): State<HttpServerState>,
    AxumPath(task_id): AxumPath<String>,
) -> Result<Json<TaskValidationSummary>, StatusCode> {
    let task_id = TaskId::from_string(task_id);
    TaskValidationService::get_task_validation_summary(&state.app_state, &task_id)
        .await
        .map(Json)
        .map_err(status_from_app_error)
}

#[derive(Debug, Clone, Deserialize)]
pub struct ValidationTaskDiffRequest {
    pub task_id: String,
    #[serde(default)]
    pub base_ref: Option<String>,
    #[serde(default)]
    pub file_paths: Vec<String>,
    #[serde(default)]
    pub max_files: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationTaskDiffStatResponse {
    pub task_id: String,
    pub base_ref: String,
    pub files: Vec<FileChange>,
    pub total_files: usize,
    pub total_additions: u32,
    pub total_deletions: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationTaskDiffResponse {
    pub task_id: String,
    pub base_ref: String,
    pub files: Vec<FileChange>,
    pub diffs: Vec<FileDiff>,
    pub truncated: bool,
}

pub async fn get_validation_task_diff_stat_http(
    State(state): State<HttpServerState>,
    Json(req): Json<ValidationTaskDiffRequest>,
) -> Result<Json<ValidationTaskDiffStatResponse>, StatusCode> {
    let (task, _project, repo_path, base_ref) = resolve_task_diff_context(&state, &req.task_id)
        .await
        .map_err(status_from_app_error)?;
    let base_ref = req.base_ref.unwrap_or(base_ref);
    let diff_service = DiffService::new();
    let mut files = diff_service
        .get_worktree_file_changes_from_ref(path_str(&repo_path)?, &base_ref)
        .map_err(status_from_app_error)?;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let total_additions = files.iter().map(|file| file.additions).sum();
    let total_deletions = files.iter().map(|file| file.deletions).sum();

    Ok(Json(ValidationTaskDiffStatResponse {
        task_id: task.id.as_str().to_string(),
        base_ref,
        total_files: files.len(),
        files,
        total_additions,
        total_deletions,
    }))
}

pub async fn get_validation_task_diff_http(
    State(state): State<HttpServerState>,
    Json(req): Json<ValidationTaskDiffRequest>,
) -> Result<Json<ValidationTaskDiffResponse>, StatusCode> {
    let (task, _project, repo_path, base_ref) = resolve_task_diff_context(&state, &req.task_id)
        .await
        .map_err(status_from_app_error)?;
    let base_ref = req.base_ref.unwrap_or(base_ref);
    let diff_service = DiffService::new();
    let mut files = diff_service
        .get_worktree_file_changes_from_ref(path_str(&repo_path)?, &base_ref)
        .map_err(status_from_app_error)?;
    files.sort_by(|a, b| a.path.cmp(&b.path));

    let requested_paths = sanitize_requested_paths(req.file_paths);
    if !requested_paths.is_empty() {
        files.retain(|file| requested_paths.iter().any(|path| path == &file.path));
    }

    let max_files = req.max_files.unwrap_or(20).clamp(1, 50);
    let truncated = files.len() > max_files;
    let selected_files = files.iter().take(max_files).cloned().collect::<Vec<_>>();
    let mut diffs = Vec::new();
    for file in &selected_files {
        if let Ok(diff) = diff_service.get_worktree_file_diff_from_ref(
            &file.path,
            path_str(&repo_path)?,
            &base_ref,
        ) {
            diffs.push(diff);
        }
    }

    Ok(Json(ValidationTaskDiffResponse {
        task_id: task.id.as_str().to_string(),
        base_ref,
        files: selected_files,
        diffs,
        truncated,
    }))
}

async fn resolve_task_diff_context(
    state: &HttpServerState,
    task_id: &str,
) -> AppResult<(Task, Project, PathBuf, String)> {
    let task_id = TaskId::from_string(task_id.to_string());
    let task = state
        .app_state
        .task_repo
        .get_by_id(&task_id)
        .await?
        .ok_or_else(|| AppError::TaskNotFound(task_id.as_str().to_string()))?;
    let project = state
        .app_state
        .project_repo
        .get_by_id(&task.project_id)
        .await?
        .ok_or_else(|| AppError::ProjectNotFound(task.project_id.as_str().to_string()))?;
    let repo_path = task
        .worktree_path
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&project.working_directory));
    let repo_path = validate_absolute_non_root_path(&repo_path, "task diff repo path")?;
    let repo_path = std::fs::canonicalize(&repo_path).map_err(|e| {
        AppError::Validation(format!(
            "task diff repo path is not available: {} ({e})",
            repo_path.display()
        ))
    })?;
    let base_ref = resolve_task_diff_base_ref(state, &task, &project).await;
    Ok((task, project, repo_path, base_ref))
}

async fn resolve_task_diff_base_ref(
    state: &HttpServerState,
    task: &Task,
    project: &Project,
) -> String {
    if let Some(exec_plan_id) = &task.execution_plan_id {
        if let Ok(Some(plan_branch)) = state
            .app_state
            .plan_branch_repo
            .get_by_execution_plan_id(exec_plan_id)
            .await
        {
            return plan_branch.branch_name;
        }
    }
    if let Some(session_id) = &task.ideation_session_id {
        if let Ok(Some(plan_branch)) = state
            .app_state
            .plan_branch_repo
            .get_by_session_id(session_id)
            .await
        {
            return plan_branch.branch_name;
        }
    }
    project.base_branch_or_default().to_string()
}

fn sanitize_requested_paths(paths: Vec<String>) -> Vec<String> {
    paths
        .into_iter()
        .map(|path| path.trim().to_string())
        .filter(|path| {
            !path.is_empty()
                && !path.starts_with('/')
                && !path.split('/').any(|part| part == ".." || part.is_empty())
        })
        .take(100)
        .collect()
}

fn path_str(path: &Path) -> Result<&str, StatusCode> {
    path.to_str().ok_or(StatusCode::BAD_REQUEST)
}

fn status_from_app_error(error: AppError) -> StatusCode {
    match error {
        AppError::TaskNotFound(_) | AppError::ProjectNotFound(_) | AppError::NotFound(_) => {
            StatusCode::NOT_FOUND
        }
        AppError::Validation(_) | AppError::ExecutionBlocked(_) => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
