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
    context: &str,
) -> AppResult<Option<DiffStats>> {
    let Some(base) = captured_task_diff_base(task) else {
        return Ok(None);
    };
    let worktree_path = task.worktree_path.as_deref().ok_or_else(|| {
        AppError::ExecutionBlocked(format!(
            "empty_task_diff_guard: task {} has captured base '{}' but no worktree path during {}",
            task.id.as_str(),
            base.effective_base_ref,
            context
        ))
    })?;
    let worktree_path = validate_task_worktree_path(worktree_path, context)?;
    GitService::get_branch_sha(&worktree_path, &base.effective_base_ref)
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
    GitService::get_diff_stats(&worktree_path, &base.effective_base_ref)
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
    context: &str,
) -> AppResult<()> {
    if task_allows_empty_captured_diff(task) {
        return Ok(());
    }
    let Some(stats) = read_captured_task_diff_stats(task, context).await? else {
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

fn validate_task_worktree_path(worktree_path: &str, context: &str) -> AppResult<PathBuf> {
    let path = validate_absolute_non_root_path(Path::new(worktree_path), context)?;
    std::fs::canonicalize(&path).map_err(|error| {
        AppError::Validation(format!(
            "task worktree path is not available during {}: {} ({error})",
            context,
            path.display()
        ))
    })
}

fn has_no_code_changes_metadata(task: &Task) -> bool {
    task.metadata
        .as_deref()
        .and_then(|metadata| serde_json::from_str::<serde_json::Value>(metadata).ok())
        .and_then(|value| value.get("no_code_changes")?.as_bool())
        .unwrap_or(false)
}
