//! Diff Commands - Tauri commands for the DiffViewer
//!
//! Provides file change and diff data for reviewing task execution results.

use crate::application::{
    agent_conversation_workspace::resolve_valid_agent_conversation_workspace_path, AppState,
    ConflictDiff, DiffService, FileChange, FileDiff, GitService,
};
use crate::commands::git_commands::{CommitInfoResponse, TaskCommitsResponse};
use crate::domain::entities::{ChatConversationId, PlanBranch, Project, Task, TaskId};
use crate::error::{AppError, AppResult};
use std::path::{Path, PathBuf};
use tauri::State;

fn resolve_merge_base(repo: &Path, base: &str, target: &str) -> Result<String, String> {
    let output = Command::new(resolve_git_cli_path())
        .args(["merge-base", base, target])
        .current_dir(repo)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Determine the working path for a task.
///
/// Uses task.worktree_path if available and exists, falls back to project.working_directory.
/// Also returns the project for access to base_branch.
async fn get_task_context(
    app_state: &AppState,
    task_id: &TaskId,
) -> AppResult<(Task, PathBuf, String, Project)> {
    // Get task
    let task = app_state
        .task_repo
        .get_by_id(task_id)
        .await?
        .ok_or_else(|| AppError::TaskNotFound(task_id.as_str().to_string()))?;

    // Get project
    let project = app_state
        .project_repo
        .get_by_id(&task.project_id)
        .await?
        .ok_or_else(|| AppError::ProjectNotFound(task.project_id.as_str().to_string()))?;

    // Determine working path — worktree path if available, else project dir
    let working_path = task
        .worktree_path
        .as_ref()
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from(&project.working_directory));

    let working_path_str = working_path.to_string_lossy().to_string();
    Ok((task, working_path, working_path_str, project))
}

async fn get_branchless_plan_branch(
    app_state: &AppState,
    task: &Task,
) -> AppResult<Option<PlanBranch>> {
    if task.task_branch.is_some() {
        return Ok(None);
    }

    app_state
        .plan_branch_repo
        .get_by_merge_task_id(&task.id)
        .await
}

fn plan_branch_review_base_ref(plan_branch: &PlanBranch, project: &Project) -> String {
    plan_branch
        .base_branch_override
        .as_deref()
        .filter(|branch| !branch.is_empty())
        .or_else(|| {
            (!plan_branch.source_branch.is_empty()).then_some(plan_branch.source_branch.as_str())
        })
        .or(project.base_branch.as_deref())
        .unwrap_or("main")
        .to_string()
}

fn plan_branch_review_diff_base_ref(
    repo: &Path,
    plan_branch: &PlanBranch,
    project: &Project,
) -> String {
    let base_ref = plan_branch_review_base_ref(plan_branch, project);
    resolve_merge_base(repo, &base_ref, &plan_branch.branch_name).unwrap_or(base_ref)
}

/// Get all files changed by the agent for a task
#[tauri::command]
pub async fn get_task_file_changes(
    app_state: State<'_, AppState>,
    task_id: String,
) -> AppResult<Vec<FileChange>> {
    get_task_file_changes_for_state(app_state.inner(), TaskId::from_string(task_id)).await
}

#[doc(hidden)]
pub async fn get_task_file_changes_for_state(
    app_state: &AppState,
    task_id: TaskId,
) -> AppResult<Vec<FileChange>> {
    // Get the correct working path and project for this task
    let (task, _, working_path_str, project) = get_task_context(app_state, &task_id).await?;
    let base_branch = project.base_branch.as_deref().unwrap_or("main");

    let diff_service = DiffService::new();
    let plan_branch = get_branchless_plan_branch(app_state, &task).await?;
    let repo_path = Path::new(&working_path_str);
    if task.internal_status == crate::domain::entities::InternalStatus::Merged {
        let merge_sha = task.merge_commit_sha.as_deref().or_else(|| {
            plan_branch
                .as_ref()
                .and_then(|branch| branch.merge_commit_sha.as_deref())
        });
        if let Some(merge_sha) = merge_sha {
            let base_ref = plan_branch
                .as_ref()
                .map(|branch| plan_branch_review_base_ref(branch, &project))
                .unwrap_or_else(|| base_branch.to_string());
            return diff_service.get_merged_task_file_changes(
                &working_path_str,
                &base_ref,
                merge_sha,
            );
        }
    }

    if let Some(plan_branch) = plan_branch {
        let base_ref = plan_branch_review_diff_base_ref(repo_path, &plan_branch, &project);
        return diff_service.get_file_changes_between_refs(
            &working_path_str,
            &base_ref,
            &plan_branch.branch_name,
        );
    }

    diff_service
        .get_task_file_changes(&task_id, &working_path_str, base_branch)
        .await
}

/// Get the diff content for a specific file
#[tauri::command]
pub async fn get_file_diff(
    app_state: State<'_, AppState>,
    task_id: String,
    file_path: String,
) -> AppResult<FileDiff> {
    get_file_diff_for_state(app_state.inner(), TaskId::from_string(task_id), file_path).await
}

#[doc(hidden)]
pub async fn get_file_diff_for_state(
    app_state: &AppState,
    task_id: TaskId,
    file_path: String,
) -> AppResult<FileDiff> {
    // Get the correct working path and project for this task
    let (task, _, working_path_str, project) = get_task_context(app_state, &task_id).await?;
    let base_branch = project.base_branch.as_deref().unwrap_or("main");

    let diff_service = DiffService::new();
    let plan_branch = get_branchless_plan_branch(app_state, &task).await?;
    let repo_path = Path::new(&working_path_str);
    if task.internal_status == crate::domain::entities::InternalStatus::Merged {
        let merge_sha = task.merge_commit_sha.as_deref().or_else(|| {
            plan_branch
                .as_ref()
                .and_then(|branch| branch.merge_commit_sha.as_deref())
        });
        if let Some(merge_sha) = merge_sha {
            let base_ref = plan_branch
                .as_ref()
                .map(|branch| plan_branch_review_base_ref(branch, &project))
                .unwrap_or_else(|| base_branch.to_string());
            return diff_service.get_merged_task_file_diff(
                &file_path,
                &working_path_str,
                &base_ref,
                merge_sha,
            );
        }
    }

    if let Some(plan_branch) = plan_branch {
        let base_ref = plan_branch_review_diff_base_ref(repo_path, &plan_branch, &project);
        return diff_service.get_file_diff_between_refs(
            &file_path,
            &working_path_str,
            &base_ref,
            &plan_branch.branch_name,
        );
    }

    diff_service.get_file_diff(&file_path, &working_path_str, base_branch)
}

async fn get_agent_workspace_context(
    app_state: &AppState,
    conversation_id: &ChatConversationId,
) -> AppResult<(PathBuf, String)> {
    let workspace = app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
        .ok_or_else(|| {
            AppError::Validation(format!(
                "Agent conversation workspace not found for conversation {}",
                conversation_id
            ))
        })?;
    let project = app_state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await?
        .ok_or_else(|| AppError::ProjectNotFound(workspace.project_id.as_str().to_string()))?;
    let worktree_path =
        resolve_valid_agent_conversation_workspace_path(&project, &workspace).await?;
    let base_commit = workspace.base_commit.clone().ok_or_else(|| {
        AppError::Validation(format!(
            "Agent conversation workspace {} is missing its captured base commit",
            conversation_id
        ))
    })?;
    Ok((worktree_path, base_commit))
}

async fn ensure_agent_workspace_commit_in_range(
    worktree_path: &Path,
    base_ref: &str,
    commit_sha: &str,
) -> AppResult<()> {
    let commits = GitService::get_commits_between(worktree_path, base_ref, "HEAD").await?;
    if commits
        .iter()
        .any(|commit| commit.sha == commit_sha || commit.short_sha == commit_sha)
    {
        return Ok(());
    }

    Err(AppError::Validation(format!(
        "Commit {} is not part of this agent workspace branch",
        commit_sha
    )))
}

#[tauri::command]
pub async fn get_agent_conversation_workspace_file_changes(
    app_state: State<'_, AppState>,
    conversation_id: String,
) -> AppResult<Vec<FileChange>> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let (worktree_path, base_ref) =
        get_agent_workspace_context(app_state.inner(), &conversation_id).await?;
    let worktree_path = worktree_path.to_string_lossy().to_string();
    DiffService::new().get_worktree_file_changes_from_ref(&worktree_path, &base_ref)
}

#[tauri::command]
pub async fn get_agent_conversation_workspace_file_diff(
    app_state: State<'_, AppState>,
    conversation_id: String,
    file_path: String,
) -> AppResult<FileDiff> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let (worktree_path, base_ref) =
        get_agent_workspace_context(app_state.inner(), &conversation_id).await?;
    let worktree_path = worktree_path.to_string_lossy().to_string();
    DiffService::new().get_file_diff(&file_path, &worktree_path, &base_ref)
}

#[tauri::command]
pub async fn get_agent_conversation_workspace_commits(
    app_state: State<'_, AppState>,
    conversation_id: String,
) -> AppResult<TaskCommitsResponse> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let (worktree_path, base_ref) =
        get_agent_workspace_context(app_state.inner(), &conversation_id).await?;
    let commits = GitService::get_commits_between(&worktree_path, &base_ref, "HEAD").await?;
    Ok(TaskCommitsResponse {
        commits: commits.into_iter().map(CommitInfoResponse::from).collect(),
    })
}

#[tauri::command]
pub async fn get_agent_conversation_workspace_commit_file_changes(
    app_state: State<'_, AppState>,
    conversation_id: String,
    commit_sha: String,
) -> AppResult<Vec<FileChange>> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let (worktree_path, base_ref) =
        get_agent_workspace_context(app_state.inner(), &conversation_id).await?;
    ensure_agent_workspace_commit_in_range(&worktree_path, &base_ref, &commit_sha).await?;
    let worktree_path = worktree_path.to_string_lossy().to_string();
    let diff_service = DiffService::new();
    if diff_service.is_merge_commit(&worktree_path, &commit_sha) {
        return diff_service.get_file_changes_between_refs(&worktree_path, &base_ref, &commit_sha);
    }
    diff_service.get_commit_file_changes(&commit_sha, &worktree_path)
}

#[tauri::command]
pub async fn get_agent_conversation_workspace_commit_file_diff(
    app_state: State<'_, AppState>,
    conversation_id: String,
    commit_sha: String,
    file_path: String,
) -> AppResult<FileDiff> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let (worktree_path, base_ref) =
        get_agent_workspace_context(app_state.inner(), &conversation_id).await?;
    ensure_agent_workspace_commit_in_range(&worktree_path, &base_ref, &commit_sha).await?;
    let worktree_path = worktree_path.to_string_lossy().to_string();
    let diff_service = DiffService::new();
    if diff_service.is_merge_commit(&worktree_path, &commit_sha) {
        return diff_service.get_file_diff_between_refs(
            &file_path,
            &worktree_path,
            &base_ref,
            &commit_sha,
        );
    }
    diff_service.get_commit_file_diff(&commit_sha, &file_path, &worktree_path)
}

/// Get files changed in a specific commit
#[tauri::command]
pub async fn get_commit_file_changes(
    app_state: State<'_, AppState>,
    task_id: String,
    commit_sha: String,
) -> AppResult<Vec<FileChange>> {
    let task_id = TaskId::from_string(task_id);

    // Get the correct working path for this task
    let (task, _, working_path_str, project) = get_task_context(&app_state, &task_id).await?;
    let base_branch = project.base_branch.as_deref().unwrap_or("main");

    let diff_service = DiffService::new();
    if task.internal_status == crate::domain::entities::InternalStatus::Merged {
        if let Some(ref merge_sha) = task.merge_commit_sha {
            if merge_sha == &commit_sha
                && diff_service.is_merge_commit(&working_path_str, merge_sha)
            {
                let from_ref =
                    diff_service.get_merged_base_ref(&working_path_str, base_branch, merge_sha);
                return diff_service.get_file_changes_between_refs(
                    &working_path_str,
                    &from_ref,
                    merge_sha,
                );
            }
        }
    }

    diff_service.get_commit_file_changes(&commit_sha, &working_path_str)
}

/// Get diff for a file in a specific commit (comparing to its parent)
#[tauri::command]
pub async fn get_commit_file_diff(
    app_state: State<'_, AppState>,
    task_id: String,
    commit_sha: String,
    file_path: String,
) -> AppResult<FileDiff> {
    let task_id = TaskId::from_string(task_id);

    // Get the correct working path for this task
    let (task, _, working_path_str, project) = get_task_context(&app_state, &task_id).await?;
    let base_branch = project.base_branch.as_deref().unwrap_or("main");

    let diff_service = DiffService::new();
    if task.internal_status == crate::domain::entities::InternalStatus::Merged {
        if let Some(ref merge_sha) = task.merge_commit_sha {
            if merge_sha == &commit_sha
                && diff_service.is_merge_commit(&working_path_str, merge_sha)
            {
                let from_ref =
                    diff_service.get_merged_base_ref(&working_path_str, base_branch, merge_sha);
                return diff_service.get_file_diff_between_refs(
                    &file_path,
                    &working_path_str,
                    &from_ref,
                    merge_sha,
                );
            }
        }
    }

    diff_service.get_commit_file_diff(&commit_sha, &file_path, &working_path_str)
}

/// Detect merge conflicts for a task.
///
/// Uses two strategies based on the current git state:
/// 1. **Active merge** (MERGE_HEAD exists): Returns files with conflict markers.
/// 2. **Pre-merge preview** (no active merge): Simulates merge using `git merge-tree --write-tree`.
///
/// Returns an empty vector if no conflicts are detected.
///
/// # Arguments
/// * `task_id` - The task to check for conflicts
///
/// # Returns
/// * `Vec<String>` - List of file paths with merge conflicts
#[tauri::command]
pub async fn detect_merge_conflicts(
    app_state: State<'_, AppState>,
    task_id: String,
) -> AppResult<Vec<String>> {
    let task_id = TaskId::from_string(task_id);

    // Get task context (task, working_path, project)
    let (task, _, working_path_str, project) = get_task_context(&app_state, &task_id).await?;

    // Get the task branch - required for conflict detection
    let task_branch = task
        .task_branch
        .as_deref()
        .ok_or_else(|| AppError::Validation("Task has no branch assigned".to_string()))?;

    let base_branch = project.base_branch.as_deref().unwrap_or("main");

    let diff_service = DiffService::new();
    diff_service
        .detect_conflicts(&working_path_str, task_branch, base_branch)
        .await
}

/// Get 3-way diff data for a file with merge conflicts.
///
/// Returns the content from all three sides of the merge (base, ours, theirs)
/// plus the current file content with conflict markers for inline rendering.
///
/// # Arguments
/// * `task_id` - The task with conflicts
/// * `file_path` - Path to the conflicting file (relative to project root)
///
/// # Returns
/// * `ConflictDiff` - 3-way diff data with conflict markers
#[tauri::command]
pub async fn get_conflict_file_diff(
    app_state: State<'_, AppState>,
    task_id: String,
    file_path: String,
) -> AppResult<ConflictDiff> {
    let task_id = TaskId::from_string(task_id);

    // Get task context (task, working_path, project)
    let (task, _, working_path_str, project) = get_task_context(&app_state, &task_id).await?;

    // Get the task branch - required for 3-way diff
    let task_branch = task
        .task_branch
        .as_deref()
        .ok_or_else(|| AppError::Validation("Task has no branch assigned".to_string()))?;

    let base_branch = project.base_branch.as_deref().unwrap_or("main");

    // Parse metadata to get actual merge branches and conflict type
    let metadata: Option<serde_json::Value> = task
        .metadata
        .as_ref()
        .and_then(|m| serde_json::from_str(m).ok());

    let (ours_ref, theirs_ref) = if let Some(ref meta) = metadata {
        let is_plan_update = meta
            .get("plan_update_conflict")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let is_source_update = meta
            .get("source_update_conflict")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let merge_source = meta
            .get("merge_source_branch")
            .and_then(|v| v.as_str())
            .unwrap_or(task_branch);
        let merge_target = meta
            .get("merge_target_branch")
            .and_then(|v| v.as_str())
            .unwrap_or(base_branch);

        if is_plan_update {
            // Plan branch (target) checked out, merging main in
            // ours = target (plan branch), theirs = base (main)
            (merge_target.to_string(), base_branch.to_string())
        } else if is_source_update {
            // Source branch checked out, merging target in
            // ours = source, theirs = target
            (merge_source.to_string(), merge_target.to_string())
        } else {
            // Normal merge: target ← source
            // ours = target, theirs = source
            (merge_target.to_string(), merge_source.to_string())
        }
    } else {
        // Fallback: original behavior
        (base_branch.to_string(), task_branch.to_string())
    };

    let diff_service = DiffService::new();
    // get_conflict_diff params: (file_path, project_path, task_branch=theirs, base_branch=ours)
    diff_service.get_conflict_diff(&file_path, &working_path_str, &theirs_ref, &ours_ref)
}
