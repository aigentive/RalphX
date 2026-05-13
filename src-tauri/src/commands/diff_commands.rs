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
use crate::infrastructure::agents::claude::git_runtime_config;
use crate::infrastructure::tool_paths::resolve_git_cli_path;
use dashmap::DashMap;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tauri::State;
use tracing::{info, warn};

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

#[derive(Clone)]
struct AgentWorkspaceContext {
    working_path: PathBuf,
    base_ref: String,
    /// For plan-branch workspaces, the diff target is the plan branch (not HEAD).
    diff_target: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentWorkspaceReviewResponse {
    pub changes: Vec<FileChange>,
    pub commits: Vec<CommitInfoResponse>,
    pub base_ref: String,
    pub head_ref: String,
}

#[derive(Clone)]
struct AgentWorkspaceReviewSnapshot {
    response: AgentWorkspaceReviewResponse,
}

#[derive(Clone)]
struct AgentWorkspaceContextCacheEntry {
    inserted_at: Instant,
    context: AgentWorkspaceContext,
}

#[derive(Clone)]
struct AgentWorkspaceReviewCacheEntry {
    inserted_at: Instant,
    snapshot: AgentWorkspaceReviewSnapshot,
}

#[derive(Clone, Copy)]
enum AgentWorkspaceDiffCacheStatus {
    Hit,
    Coalesced,
    Miss,
}

impl AgentWorkspaceDiffCacheStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Hit => "hit",
            Self::Coalesced => "coalesced",
            Self::Miss => "miss",
        }
    }
}

fn agent_workspace_review_cache_ttl() -> Duration {
    Duration::from_millis(git_runtime_config().workspace_review_cache_ttl_ms)
}

fn agent_workspace_diff_cache_key(conversation_id: &ChatConversationId) -> Option<String> {
    if conversation_id.as_uuid().is_nil() {
        return None;
    }
    Some(conversation_id.as_str())
}

fn agent_workspace_context_cache() -> &'static DashMap<String, AgentWorkspaceContextCacheEntry> {
    static CACHE: OnceLock<DashMap<String, AgentWorkspaceContextCacheEntry>> = OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

fn agent_workspace_context_locks() -> &'static DashMap<String, Arc<tokio::sync::Mutex<()>>> {
    static LOCKS: OnceLock<DashMap<String, Arc<tokio::sync::Mutex<()>>>> = OnceLock::new();
    LOCKS.get_or_init(DashMap::new)
}

fn agent_workspace_review_cache() -> &'static DashMap<String, AgentWorkspaceReviewCacheEntry> {
    static CACHE: OnceLock<DashMap<String, AgentWorkspaceReviewCacheEntry>> = OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

fn agent_workspace_review_locks() -> &'static DashMap<String, Arc<tokio::sync::Mutex<()>>> {
    static LOCKS: OnceLock<DashMap<String, Arc<tokio::sync::Mutex<()>>>> = OnceLock::new();
    LOCKS.get_or_init(DashMap::new)
}

fn cached_agent_workspace_context(
    conversation_id: &ChatConversationId,
) -> Option<AgentWorkspaceContext> {
    let ttl = agent_workspace_review_cache_ttl();
    if ttl.is_zero() {
        return None;
    }
    let key = agent_workspace_diff_cache_key(conversation_id)?;
    let Some(entry) = agent_workspace_context_cache().get(&key) else {
        return None;
    };
    if entry.inserted_at.elapsed() <= ttl {
        return Some(entry.context.clone());
    }
    drop(entry);
    agent_workspace_context_cache().remove(&key);
    None
}

fn store_agent_workspace_context(
    conversation_id: &ChatConversationId,
    context: &AgentWorkspaceContext,
) {
    if agent_workspace_review_cache_ttl().is_zero() {
        return;
    }
    let Some(key) = agent_workspace_diff_cache_key(conversation_id) else {
        return;
    };
    agent_workspace_context_cache().insert(
        key,
        AgentWorkspaceContextCacheEntry {
            inserted_at: Instant::now(),
            context: context.clone(),
        },
    );
}

fn cached_agent_workspace_review(
    conversation_id: &ChatConversationId,
) -> Option<AgentWorkspaceReviewSnapshot> {
    let ttl = agent_workspace_review_cache_ttl();
    if ttl.is_zero() {
        return None;
    }
    let key = agent_workspace_diff_cache_key(conversation_id)?;
    let Some(entry) = agent_workspace_review_cache().get(&key) else {
        return None;
    };
    if entry.inserted_at.elapsed() <= ttl {
        return Some(entry.snapshot.clone());
    }
    drop(entry);
    agent_workspace_review_cache().remove(&key);
    None
}

fn store_agent_workspace_review(
    conversation_id: &ChatConversationId,
    snapshot: &AgentWorkspaceReviewSnapshot,
) {
    if agent_workspace_review_cache_ttl().is_zero() {
        return;
    }
    let Some(key) = agent_workspace_diff_cache_key(conversation_id) else {
        return;
    };
    agent_workspace_review_cache().insert(
        key,
        AgentWorkspaceReviewCacheEntry {
            inserted_at: Instant::now(),
            snapshot: snapshot.clone(),
        },
    );
}

pub(crate) fn invalidate_agent_workspace_diff_caches(conversation_id: &ChatConversationId) {
    let Some(key) = agent_workspace_diff_cache_key(conversation_id) else {
        return;
    };
    agent_workspace_context_cache().remove(&key);
    agent_workspace_review_cache().remove(&key);
}

async fn get_agent_workspace_context(
    app_state: &AppState,
    conversation_id: &ChatConversationId,
) -> AppResult<AgentWorkspaceContext> {
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

    // For ideation workspaces linked to a plan branch, the commits live on the
    // plan branch, not the workspace's own branch. Use the project root and
    // resolve the merge-base so the diff only shows changes introduced by the
    // plan branch (not unrelated base-branch progress).
    if let Some(plan_branch_id) = &workspace.linked_plan_branch_id {
        if let Some(plan_branch) = app_state.plan_branch_repo.get_by_id(plan_branch_id).await? {
            let base_branch = plan_branch_review_base_ref(&plan_branch, &project);
            let project_path = PathBuf::from(&project.working_directory);
            let merge_base =
                resolve_merge_base(&project_path, &base_branch, &plan_branch.branch_name)
                    .unwrap_or(base_branch);
            return Ok(AgentWorkspaceContext {
                working_path: project_path,
                base_ref: merge_base,
                diff_target: Some(plan_branch.branch_name.clone()),
            });
        }
    }

    let worktree_path =
        resolve_valid_agent_conversation_workspace_path(&project, &workspace).await?;
    let base_commit = workspace.base_commit.clone().ok_or_else(|| {
        AppError::Validation(format!(
            "Agent conversation workspace {} is missing its captured base commit",
            conversation_id
        ))
    })?;
    Ok(AgentWorkspaceContext {
        working_path: worktree_path,
        base_ref: base_commit,
        diff_target: None,
    })
}

async fn get_agent_workspace_context_cached(
    app_state: &AppState,
    conversation_id: &ChatConversationId,
) -> AppResult<(AgentWorkspaceContext, AgentWorkspaceDiffCacheStatus)> {
    if let Some(context) = cached_agent_workspace_context(conversation_id) {
        return Ok((context, AgentWorkspaceDiffCacheStatus::Hit));
    }
    let Some(key) = agent_workspace_diff_cache_key(conversation_id) else {
        let context = get_agent_workspace_context(app_state, conversation_id).await?;
        return Ok((context, AgentWorkspaceDiffCacheStatus::Miss));
    };
    let lock = agent_workspace_context_locks()
        .entry(key)
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone();
    let _guard = lock.lock().await;
    if let Some(context) = cached_agent_workspace_context(conversation_id) {
        return Ok((context, AgentWorkspaceDiffCacheStatus::Coalesced));
    }
    let context = get_agent_workspace_context(app_state, conversation_id).await?;
    store_agent_workspace_context(conversation_id, &context);
    Ok((context, AgentWorkspaceDiffCacheStatus::Miss))
}

async fn ensure_agent_workspace_commit_in_range(
    conversation_id: &ChatConversationId,
    worktree_path: &Path,
    base_ref: &str,
    head_ref: &str,
    commit_sha: &str,
) -> AppResult<()> {
    if cached_agent_workspace_review(conversation_id).is_some_and(|snapshot| {
        snapshot
            .response
            .commits
            .iter()
            .any(|commit| commit.sha == commit_sha || commit.short_sha == commit_sha)
    }) {
        return Ok(());
    }

    let commits = GitService::get_commits_between(worktree_path, base_ref, head_ref).await?;
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
pub async fn get_agent_conversation_workspace_review(
    app_state: State<'_, AppState>,
    conversation_id: String,
) -> AppResult<AgentWorkspaceReviewResponse> {
    let started = Instant::now();
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let result = get_agent_conversation_workspace_review_cached(
        app_state.inner(),
        &conversation_id,
    )
    .await;
    match &result {
        Ok((snapshot, cache_status)) => info!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "review",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            cache_status = cache_status.as_str(),
            files = snapshot.response.changes.len(),
            commits = snapshot.response.commits.len(),
            base_ref = snapshot.response.base_ref.as_str(),
            head_ref = snapshot.response.head_ref.as_str(),
            "Loaded agent workspace review payload"
        ),
        Err(error) => warn!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "review",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            error = %error,
            "Failed to load agent workspace review payload"
        ),
    }
    result.map(|(snapshot, _)| snapshot.response)
}

async fn get_agent_conversation_workspace_review_for_state(
    app_state: &AppState,
    conversation_id: &ChatConversationId,
) -> AppResult<AgentWorkspaceReviewSnapshot> {
    let (ctx, _) = get_agent_workspace_context_cached(app_state, conversation_id).await?;
    get_agent_workspace_review_for_context(ctx).await
}

async fn get_agent_conversation_workspace_review_cached(
    app_state: &AppState,
    conversation_id: &ChatConversationId,
) -> AppResult<(AgentWorkspaceReviewSnapshot, AgentWorkspaceDiffCacheStatus)> {
    if let Some(snapshot) = cached_agent_workspace_review(conversation_id) {
        return Ok((snapshot, AgentWorkspaceDiffCacheStatus::Hit));
    }
    let Some(key) = agent_workspace_diff_cache_key(conversation_id) else {
        let snapshot = get_agent_conversation_workspace_review_for_state(app_state, conversation_id)
            .await?;
        return Ok((snapshot, AgentWorkspaceDiffCacheStatus::Miss));
    };
    let lock = agent_workspace_review_locks()
        .entry(key)
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone();
    let _guard = lock.lock().await;
    if let Some(snapshot) = cached_agent_workspace_review(conversation_id) {
        return Ok((snapshot, AgentWorkspaceDiffCacheStatus::Coalesced));
    }
    let snapshot =
        get_agent_conversation_workspace_review_for_state(app_state, conversation_id).await?;
    store_agent_workspace_review(conversation_id, &snapshot);
    Ok((snapshot, AgentWorkspaceDiffCacheStatus::Miss))
}

async fn get_agent_workspace_review_for_context(
    ctx: AgentWorkspaceContext,
) -> AppResult<AgentWorkspaceReviewSnapshot> {
    let working_path = ctx.working_path.to_string_lossy().to_string();
    let base_ref = ctx.base_ref.clone();
    let head_ref = ctx
        .diff_target
        .clone()
        .unwrap_or_else(|| "HEAD".to_string());

    let changes_path = working_path.clone();
    let changes_base_ref = base_ref.clone();
    let changes_target = ctx.diff_target.clone();
    let changes_fut = async move {
        tokio::task::spawn_blocking(move || {
            let diff_service = DiffService::new();
            if let Some(target) = changes_target {
                diff_service.get_file_changes_between_refs(
                    &changes_path,
                    &changes_base_ref,
                    &target,
                )
            } else {
                diff_service.get_worktree_file_changes_from_ref(&changes_path, &changes_base_ref)
            }
        })
        .await
        .map_err(|error| {
            AppError::Infrastructure(format!(
                "agent workspace review file change task failed: {error}"
            ))
        })?
    };

    let commits_path = ctx.working_path.clone();
    let commits_base_ref = base_ref.clone();
    let commits_head_ref = head_ref.clone();
    let commits_fut =
        GitService::get_commits_between(&commits_path, &commits_base_ref, &commits_head_ref);

    let (changes, commits) = tokio::try_join!(changes_fut, commits_fut)?;
    Ok(AgentWorkspaceReviewSnapshot {
        response: AgentWorkspaceReviewResponse {
            changes,
            commits: commits.into_iter().map(CommitInfoResponse::from).collect(),
            base_ref,
            head_ref,
        },
    })
}

#[tauri::command]
pub async fn get_agent_conversation_workspace_file_changes(
    app_state: State<'_, AppState>,
    conversation_id: String,
) -> AppResult<Vec<FileChange>> {
    let started = Instant::now();
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let result: AppResult<(Vec<FileChange>, AgentWorkspaceDiffCacheStatus)> = async {
        let (snapshot, cache_status) =
            get_agent_conversation_workspace_review_cached(app_state.inner(), &conversation_id)
                .await?;
        Ok((snapshot.response.changes, cache_status))
    }
    .await;
    match &result {
        Ok((changes, cache_status)) => info!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "file_changes",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            cache_status = cache_status.as_str(),
            files = changes.len(),
            "Loaded agent workspace file changes"
        ),
        Err(error) => warn!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "file_changes",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            error = %error,
            "Failed to load agent workspace file changes"
        ),
    }
    result.map(|(changes, _)| changes)
}

#[tauri::command]
pub async fn get_agent_conversation_workspace_file_diff(
    app_state: State<'_, AppState>,
    conversation_id: String,
    file_path: String,
) -> AppResult<FileDiff> {
    let started = Instant::now();
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let result: AppResult<(FileDiff, AgentWorkspaceDiffCacheStatus)> = async {
        let (ctx, cache_status) =
            get_agent_workspace_context_cached(app_state.inner(), &conversation_id).await?;
        let working_path = ctx.working_path.to_string_lossy().to_string();
        let diff_service = DiffService::new();
        let diff = if let Some(target) = &ctx.diff_target {
            diff_service.get_file_diff_between_refs(
                &file_path,
                &working_path,
                &ctx.base_ref,
                target,
            )
        } else {
            diff_service.get_file_diff(&file_path, &working_path, &ctx.base_ref)
        }?;
        Ok((diff, cache_status))
    }
    .await;
    match &result {
        Ok((diff, cache_status)) => info!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "file_diff",
            conversation_id = %conversation_id,
            file_path,
            elapsed_ms = started.elapsed().as_millis(),
            cache_status = cache_status.as_str(),
            old_chars = diff.old_content.chars().count(),
            new_chars = diff.new_content.chars().count(),
            "Loaded agent workspace file diff"
        ),
        Err(error) => warn!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "file_diff",
            conversation_id = %conversation_id,
            file_path,
            elapsed_ms = started.elapsed().as_millis(),
            error = %error,
            "Failed to load agent workspace file diff"
        ),
    }
    result.map(|(diff, _)| diff)
}

#[tauri::command]
pub async fn get_agent_conversation_workspace_commits(
    app_state: State<'_, AppState>,
    conversation_id: String,
) -> AppResult<TaskCommitsResponse> {
    let started = Instant::now();
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let result: AppResult<(TaskCommitsResponse, AgentWorkspaceDiffCacheStatus)> = async {
        let (snapshot, cache_status) =
            get_agent_conversation_workspace_review_cached(app_state.inner(), &conversation_id)
                .await?;
        Ok((
            TaskCommitsResponse {
                commits: snapshot.response.commits,
            },
            cache_status,
        ))
    }
    .await;
    match &result {
        Ok((response, cache_status)) => info!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "commits",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            cache_status = cache_status.as_str(),
            commits = response.commits.len(),
            "Loaded agent workspace commits"
        ),
        Err(error) => warn!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "commits",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            error = %error,
            "Failed to load agent workspace commits"
        ),
    }
    result.map(|(response, _)| response)
}

#[tauri::command]
pub async fn get_agent_conversation_workspace_commit_file_changes(
    app_state: State<'_, AppState>,
    conversation_id: String,
    commit_sha: String,
) -> AppResult<Vec<FileChange>> {
    let started = Instant::now();
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let result: AppResult<(Vec<FileChange>, AgentWorkspaceDiffCacheStatus)> = async {
        let (ctx, cache_status) =
            get_agent_workspace_context_cached(app_state.inner(), &conversation_id).await?;
        let head_ref = ctx.diff_target.as_deref().unwrap_or("HEAD");
        ensure_agent_workspace_commit_in_range(
            &conversation_id,
            &ctx.working_path,
            &ctx.base_ref,
            head_ref,
            &commit_sha,
        )
        .await?;
        let working_path = ctx.working_path.to_string_lossy().to_string();
        let diff_service = DiffService::new();
        let changes = if diff_service.is_merge_commit(&working_path, &commit_sha) {
            diff_service.get_file_changes_between_refs(
                &working_path,
                &ctx.base_ref,
                &commit_sha,
            )
        } else {
            diff_service.get_commit_file_changes(&commit_sha, &working_path)
        }?;
        Ok((changes, cache_status))
    }
    .await;
    match &result {
        Ok((changes, cache_status)) => info!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "commit_file_changes",
            conversation_id = %conversation_id,
            commit_sha,
            elapsed_ms = started.elapsed().as_millis(),
            cache_status = cache_status.as_str(),
            files = changes.len(),
            "Loaded agent workspace commit file changes"
        ),
        Err(error) => warn!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "commit_file_changes",
            conversation_id = %conversation_id,
            commit_sha,
            elapsed_ms = started.elapsed().as_millis(),
            error = %error,
            "Failed to load agent workspace commit file changes"
        ),
    }
    result.map(|(changes, _)| changes)
}

#[tauri::command]
pub async fn get_agent_conversation_workspace_commit_file_diff(
    app_state: State<'_, AppState>,
    conversation_id: String,
    commit_sha: String,
    file_path: String,
) -> AppResult<FileDiff> {
    let started = Instant::now();
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let result: AppResult<(FileDiff, AgentWorkspaceDiffCacheStatus)> = async {
        let (ctx, cache_status) =
            get_agent_workspace_context_cached(app_state.inner(), &conversation_id).await?;
        let head_ref = ctx.diff_target.as_deref().unwrap_or("HEAD");
        ensure_agent_workspace_commit_in_range(
            &conversation_id,
            &ctx.working_path,
            &ctx.base_ref,
            head_ref,
            &commit_sha,
        )
        .await?;
        let working_path = ctx.working_path.to_string_lossy().to_string();
        let diff_service = DiffService::new();
        let diff = if diff_service.is_merge_commit(&working_path, &commit_sha) {
            diff_service.get_file_diff_between_refs(
                &file_path,
                &working_path,
                &ctx.base_ref,
                &commit_sha,
            )
        } else {
            diff_service.get_commit_file_diff(&commit_sha, &file_path, &working_path)
        }?;
        Ok((diff, cache_status))
    }
    .await;
    match &result {
        Ok((diff, cache_status)) => info!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "commit_file_diff",
            conversation_id = %conversation_id,
            commit_sha,
            file_path,
            elapsed_ms = started.elapsed().as_millis(),
            cache_status = cache_status.as_str(),
            old_chars = diff.old_content.chars().count(),
            new_chars = diff.new_content.chars().count(),
            "Loaded agent workspace commit file diff"
        ),
        Err(error) => warn!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "commit_file_diff",
            conversation_id = %conversation_id,
            commit_sha,
            file_path,
            elapsed_ms = started.elapsed().as_millis(),
            error = %error,
            "Failed to load agent workspace commit file diff"
        ),
    }
    result.map(|(diff, _)| diff)
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
