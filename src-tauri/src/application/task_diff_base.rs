use std::path::{Path, PathBuf};

use crate::application::git_service::{DiffStats, GitService};
use crate::application::AppState;
use crate::domain::entities::{Project, Task, TaskCategory};
use crate::error::{AppError, AppResult};
use crate::utils::path_safety::validate_absolute_non_root_path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskDiffBase {
    pub display_base_ref: String,
    pub effective_base_ref: String,
    pub immutable: bool,
}

pub(crate) const EMPTY_TASK_DIFF_MISSING_CAPTURED_BASE_REASON: &str =
    "empty_task_diff_missing_captured_base";

pub(crate) fn captured_task_diff_base(task: &Task) -> Option<TaskDiffBase> {
    let effective_base_ref = task
        .task_branch_base_sha
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let display_base_ref = task
        .task_branch_base_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(effective_base_ref);

    Some(TaskDiffBase {
        display_base_ref: display_base_ref.to_string(),
        effective_base_ref: effective_base_ref.to_string(),
        immutable: true,
    })
}

pub(crate) async fn resolve_task_diff_base(
    state: &AppState,
    task: &Task,
    project: &Project,
) -> TaskDiffBase {
    if let Some(captured) = captured_task_diff_base(task) {
        return captured;
    }

    let display_base_ref = resolve_live_task_base_ref(state, task, project).await;
    TaskDiffBase {
        effective_base_ref: display_base_ref.clone(),
        display_base_ref,
        immutable: false,
    }
}

async fn resolve_live_task_base_ref(state: &AppState, task: &Task, project: &Project) -> String {
    if let Some(exec_plan_id) = &task.execution_plan_id {
        if let Ok(Some(plan_branch)) = state
            .plan_branch_repo
            .get_by_execution_plan_id(exec_plan_id)
            .await
        {
            return plan_branch.branch_name;
        }
    }
    if let Some(session_id) = &task.ideation_session_id {
        if let Ok(Some(plan_branch)) = state.plan_branch_repo.get_by_session_id(session_id).await {
            return plan_branch.branch_name;
        }
    }
    project.base_branch_or_default().to_string()
}

pub(crate) async fn read_task_diff_stats_from_resolved_base(
    state: &AppState,
    task: &Task,
    project: &Project,
    context: &str,
) -> AppResult<DiffStats> {
    let repo_path = task_diff_repo_path(task, project, context)?;
    let task_base = resolve_task_diff_base(state, task, project).await;
    GitService::get_diff_stats(&repo_path, &task_base.effective_base_ref)
        .await
        .map_err(|error| {
            AppError::Validation(format!(
                "failed to read task diff stats for task {} against base '{}' during {}: {}",
                task.id.as_str(),
                task_base.effective_base_ref,
                context,
                error
            ))
        })
}

pub(crate) fn diff_stats_has_changes(stats: &DiffStats) -> bool {
    stats.files_changed > 0
        || stats.insertions > 0
        || stats.deletions > 0
        || !stats.changed_files.is_empty()
}

pub(crate) fn task_allows_empty_captured_diff(task: &Task) -> bool {
    task.category == TaskCategory::PlanMerge || has_no_code_changes_metadata(task)
}

pub(crate) async fn read_captured_task_diff_stats(
    task: &Task,
    project: &Project,
    context: &str,
) -> AppResult<Option<DiffStats>> {
    let Some(base) = captured_task_diff_base(task) else {
        return Ok(None);
    };
    let repo_path = task_diff_repo_path(task, project, context)?;
    ensure_no_worktree_fallback_matches_task_branch(task, &repo_path, context).await?;
    GitService::get_branch_sha(&repo_path, &base.effective_base_ref)
        .await
        .map_err(|error| {
            AppError::ExecutionBlocked(format!(
                "empty_task_diff_guard: captured base '{}' for task {} is not resolvable during {}: {}",
                base.effective_base_ref,
                task.id.as_str(),
                context,
                error
            ))
        })?;
    GitService::get_diff_stats(&repo_path, &base.effective_base_ref)
        .await
        .map(Some)
        .map_err(|error| {
            AppError::ExecutionBlocked(format!(
                "empty_task_diff_guard: failed to read task diff for task {} against captured base '{}' during {}: {}",
                task.id.as_str(),
                base.effective_base_ref,
                context,
                error
            ))
        })
}

pub(crate) async fn ensure_task_has_non_empty_captured_diff(
    task: &Task,
    project: &Project,
    context: &str,
) -> AppResult<()> {
    if task_allows_empty_captured_diff(task) {
        return Ok(());
    }
    let Some(stats) = read_captured_task_diff_stats(task, project, context).await? else {
        let base_ref = task
            .task_branch_base_ref
            .as_deref()
            .unwrap_or("<unknown-base-ref>");
        return Err(AppError::ExecutionBlocked(format!(
            "{}: task {} has no captured base SHA for reported base {} during {}; refusing to advance code-change task without immutable task-diff proof",
            EMPTY_TASK_DIFF_MISSING_CAPTURED_BASE_REASON,
            task.id.as_str(),
            base_ref,
            context
        )));
    };
    if diff_stats_has_changes(&stats) {
        return Ok(());
    }

    let base_ref = task
        .task_branch_base_ref
        .as_deref()
        .unwrap_or("<unknown-base-ref>");
    let base_sha = task
        .task_branch_base_sha
        .as_deref()
        .unwrap_or("<unknown-base-sha>");
    Err(AppError::ExecutionBlocked(format!(
        "empty_task_diff_against_captured_base: task {} has no diff against captured base {} ({}) during {}",
        task.id.as_str(),
        base_ref,
        base_sha,
        context
    )))
}

fn task_diff_repo_path(task: &Task, project: &Project, context: &str) -> AppResult<PathBuf> {
    let repo_path = task
        .worktree_path
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&project.working_directory));
    let repo_path = validate_absolute_non_root_path(&repo_path, context)?;
    std::fs::canonicalize(&repo_path).map_err(|error| {
        AppError::Validation(format!(
            "task diff repo path is not available during {}: {} ({error})",
            context,
            repo_path.display()
        ))
    })
}

async fn ensure_no_worktree_fallback_matches_task_branch(
    task: &Task,
    repo_path: &Path,
    context: &str,
) -> AppResult<()> {
    if task.worktree_path.is_some() {
        return Ok(());
    }
    let Some(expected_branch) = task
        .task_branch
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };

    let current_branch = GitService::get_current_branch(repo_path)
        .await
        .map_err(|error| {
            AppError::ExecutionBlocked(format!(
                "empty_task_diff_guard: failed to verify checkout branch for task {} during {}: {}",
                task.id.as_str(),
                context,
                error
            ))
        })?;
    if current_branch == expected_branch {
        return Ok(());
    }

    Err(AppError::ExecutionBlocked(format!(
        "empty_task_diff_guard: project checkout is on branch '{}' but task {} expects branch '{}' during {}; refusing to verify captured diff from the wrong checkout",
        current_branch,
        task.id.as_str(),
        expected_branch,
        context
    )))
}

fn has_no_code_changes_metadata(task: &Task) -> bool {
    task.metadata
        .as_deref()
        .and_then(|metadata| serde_json::from_str::<serde_json::Value>(metadata).ok())
        .and_then(|value| value.get("no_code_changes")?.as_bool())
        .unwrap_or(false)
}
