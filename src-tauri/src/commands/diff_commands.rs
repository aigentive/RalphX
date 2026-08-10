//! Diff Commands - Tauri commands for the DiffViewer
//!
//! Provides file change and diff data for reviewing task execution results.

use crate::application::{
    agent_conversation_workspace::{
        resolve_agent_conversation_workspace_path,
        resolve_agent_conversation_workspace_path_for_send,
        resolve_valid_agent_conversation_workspace_path,
    },
    agent_workspace_review::load_agent_workspace_review_context,
    agent_workspace_review_base::resolve_agent_workspace_review_base,
    AppState, ConflictDiff, DiffRefKind, DiffService, DiffSide, FileChange, FileDiff, FileDiffPage,
    GitService, RangeLine,
};
use crate::commands::git_commands::{CommitInfoResponse, TaskCommitsResponse};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentWorkspaceReviewHunkAnnotation, ChatConversationId, PlanBranch,
    Project, Task, TaskId,
};
use crate::domain::services::github_service::{PrAnnotationSourceUnavailable, PrDiffAnnotations};
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::git_runtime_config;
use crate::infrastructure::tool_paths::resolve_git_cli_path;
use chrono::Utc;
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

fn agent_workspace_pr_head_ref(pr_number: i64) -> String {
    format!("refs/ralphx/pr-heads/{pr_number}")
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
    source: AgentWorkspaceContextSource,
    working_path: PathBuf,
    base_ref: String,
    /// For plan-branch or terminal PR workspaces, the diff target is an explicit ref (not HEAD).
    diff_target: Option<String>,
    /// Unified patch fallback when local git refs are no longer available.
    patch_diff: Option<Arc<str>>,
    /// True only when the context points at the agent worktree and can inspect
    /// unstaged/staged changes. Branch-target contexts are read-only history.
    supports_worktree_modes: bool,
    /// Present only for explicit repair-mode contexts.
    repair_state: Option<AgentWorkspaceRepairStateResponse>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentWorkspaceContextSource {
    Worktree,
    LocalBranch,
    PlanBranch,
    PullRequestHead,
    GithubPatch,
    TerminalPullRequestHead,
    RepairWorktree,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentWorkspaceReviewResponse {
    pub changes: Vec<FileChange>,
    pub commits: Vec<CommitInfoResponse>,
    pub base_ref: String,
    pub head_ref: String,
    pub supports_worktree_modes: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentWorkspaceReviewHunkAnnotationsResponse {
    pub artifact_id: Option<String>,
    pub artifact_version: Option<u32>,
    pub target_scope: Option<String>,
    pub head_sha: Option<String>,
    pub diff_fingerprint: Option<String>,
    pub annotations: Vec<AgentWorkspaceReviewHunkAnnotation>,
}

impl AgentWorkspaceReviewHunkAnnotationsResponse {
    fn empty() -> Self {
        Self {
            artifact_id: None,
            artifact_version: None,
            target_scope: None,
            head_sha: None,
            diff_fingerprint: None,
            annotations: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentWorkspaceChangeSummaryBucketResponse {
    pub file_count: usize,
    pub additions: u32,
    pub deletions: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentWorkspaceConflictSummaryResponse {
    pub file_count: usize,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentWorkspaceRepairStateResponse {
    pub expected_branch: String,
    pub checked_out_branch: String,
    pub rebase_in_progress: bool,
    pub merge_in_progress: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentWorkspaceChangeSummaryResponse {
    pub supports_worktree_modes: bool,
    pub staged: AgentWorkspaceChangeSummaryBucketResponse,
    pub unstaged: AgentWorkspaceChangeSummaryBucketResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflicted: Option<AgentWorkspaceConflictSummaryResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repair_state: Option<AgentWorkspaceRepairStateResponse>,
}

#[derive(Clone)]
struct AgentWorkspaceReviewSnapshot {
    response: AgentWorkspaceReviewResponse,
    context_source: AgentWorkspaceContextSource,
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

#[derive(Clone)]
struct AgentWorkspaceRemoteSnapshot<T> {
    inserted_at: Instant,
    captured_at: String,
    cache_version: String,
    context_source: AgentWorkspaceContextSource,
    payload: T,
}

#[derive(Clone)]
struct AgentWorkspacePrAnnotationsCacheEntry {
    inserted_at: Instant,
    payload: PrDiffAnnotations,
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

#[derive(Clone, Copy)]
enum AgentWorkspaceContextMode {
    Strict,
    Repair,
}

impl AgentWorkspaceContextMode {
    fn cache_key_suffix(self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::Repair => "repair",
        }
    }
}

fn agent_workspace_review_cache_ttl() -> Duration {
    Duration::from_millis(git_runtime_config().workspace_review_cache_ttl_ms)
}

fn remote_workspace_snapshot_ttl() -> Duration {
    Duration::from_millis(git_runtime_config().remote_workspace_snapshot_ttl_ms)
}

fn agent_workspace_pr_annotations_cache_ttl() -> Duration {
    Duration::from_millis(git_runtime_config().workspace_pr_annotations_cache_ttl_ms)
}

fn agent_workspace_diff_cache_key(conversation_id: &ChatConversationId) -> Option<String> {
    if conversation_id.as_uuid().is_nil() {
        return None;
    }
    Some(conversation_id.as_str())
}

fn agent_workspace_context_cache_key(
    conversation_id: &ChatConversationId,
    mode: AgentWorkspaceContextMode,
) -> Option<String> {
    agent_workspace_diff_cache_key(conversation_id)
        .map(|key| format!("{key}:{}", mode.cache_key_suffix()))
}

fn agent_workspace_pr_annotations_cache_key(
    conversation_id: &ChatConversationId,
    pr_number: i64,
) -> Option<String> {
    agent_workspace_diff_cache_key(conversation_id).map(|key| format!("{key}:{pr_number}"))
}

fn agent_workspace_context_cache() -> &'static DashMap<String, AgentWorkspaceContextCacheEntry> {
    static CACHE: OnceLock<DashMap<String, AgentWorkspaceContextCacheEntry>> = OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

fn agent_workspace_diff_cache_versions() -> &'static DashMap<String, String> {
    static VERSIONS: OnceLock<DashMap<String, String>> = OnceLock::new();
    VERSIONS.get_or_init(DashMap::new)
}

fn agent_workspace_diff_cache_version(workspace: &AgentConversationWorkspace) -> String {
    format!(
        "{}\0{}\0{}\0{}\0{}\0{}\0{}",
        workspace.updated_at.to_rfc3339(),
        workspace
            .publication_pr_number
            .map(|number| number.to_string())
            .unwrap_or_default(),
        workspace
            .publication_pr_status
            .as_deref()
            .unwrap_or_default(),
        workspace
            .publication_push_status
            .as_deref()
            .unwrap_or_default(),
        workspace.base_ref,
        workspace.base_commit.as_deref().unwrap_or_default(),
        workspace.branch_name,
    )
}

async fn ensure_agent_workspace_diff_cache_current(
    app_state: &AppState,
    conversation_id: &ChatConversationId,
) -> AppResult<()> {
    let version = app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
        .as_ref()
        .map(agent_workspace_diff_cache_version)
        .unwrap_or_default();
    ensure_agent_workspace_diff_cache_matches(conversation_id, version);
    Ok(())
}

fn ensure_agent_workspace_diff_cache_matches(
    conversation_id: &ChatConversationId,
    version: String,
) {
    let Some(key) = agent_workspace_diff_cache_key(conversation_id) else {
        return;
    };
    if agent_workspace_diff_cache_versions()
        .get(&key)
        .is_some_and(|cached| cached.as_str() == version)
    {
        return;
    }
    invalidate_agent_workspace_diff_caches(conversation_id);
    agent_workspace_diff_cache_versions().insert(key, version);
}

fn agent_workspace_context_locks() -> &'static DashMap<String, Arc<tokio::sync::Mutex<()>>> {
    static LOCKS: OnceLock<DashMap<String, Arc<tokio::sync::Mutex<()>>>> = OnceLock::new();
    LOCKS.get_or_init(DashMap::new)
}

fn agent_workspace_review_cache() -> &'static DashMap<String, AgentWorkspaceReviewCacheEntry> {
    static CACHE: OnceLock<DashMap<String, AgentWorkspaceReviewCacheEntry>> = OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

fn agent_workspace_remote_review_cache(
) -> &'static DashMap<String, AgentWorkspaceRemoteSnapshot<AgentWorkspaceReviewResponse>> {
    static CACHE: OnceLock<
        DashMap<String, AgentWorkspaceRemoteSnapshot<AgentWorkspaceReviewResponse>>,
    > = OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

fn agent_workspace_remote_summary_cache(
) -> &'static DashMap<String, AgentWorkspaceRemoteSnapshot<AgentWorkspaceChangeSummaryResponse>> {
    static CACHE: OnceLock<
        DashMap<String, AgentWorkspaceRemoteSnapshot<AgentWorkspaceChangeSummaryResponse>>,
    > = OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

fn agent_workspace_remote_file_diff_cache(
) -> &'static DashMap<String, AgentWorkspaceRemoteSnapshot<FileDiff>> {
    static CACHE: OnceLock<DashMap<String, AgentWorkspaceRemoteSnapshot<FileDiff>>> =
        OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

fn agent_workspace_remote_file_diff_page_cache(
) -> &'static DashMap<String, AgentWorkspaceRemoteSnapshot<FileDiffPage>> {
    static CACHE: OnceLock<DashMap<String, AgentWorkspaceRemoteSnapshot<FileDiffPage>>> =
        OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

fn agent_workspace_file_snapshot_key(
    conversation_id: &ChatConversationId,
    file_path: &str,
    scope: &str,
) -> Option<String> {
    agent_workspace_diff_cache_key(conversation_id)
        .map(|conversation| format!("{conversation}\0{scope}\0{file_path}"))
}

fn agent_workspace_diff_ref_scope(ref_kind: &DiffRefKind) -> String {
    match ref_kind {
        DiffRefKind::Head => "head".to_string(),
        DiffRefKind::Staged => "staged".to_string(),
        DiffRefKind::Unstaged => "unstaged".to_string(),
        DiffRefKind::Commit { sha } => format!("commit:{sha}"),
        DiffRefKind::CumulativeBase => "cumulative_base".to_string(),
        DiffRefKind::CumulativeHead => "cumulative_head".to_string(),
    }
}

fn current_agent_workspace_cache_version(conversation_id: &ChatConversationId) -> Option<String> {
    let key = agent_workspace_diff_cache_key(conversation_id)?;
    agent_workspace_diff_cache_versions()
        .get(&key)
        .map(|version| version.clone())
}

fn store_remote_workspace_snapshot<T: Clone>(
    cache: &DashMap<String, AgentWorkspaceRemoteSnapshot<T>>,
    conversation_id: &ChatConversationId,
    context_source: AgentWorkspaceContextSource,
    payload: &T,
) {
    if remote_workspace_snapshot_ttl().is_zero() {
        return;
    }
    let Some(key) = agent_workspace_diff_cache_key(conversation_id) else {
        return;
    };
    let Some(cache_version) = current_agent_workspace_cache_version(conversation_id) else {
        return;
    };
    cache.insert(
        key,
        AgentWorkspaceRemoteSnapshot {
            inserted_at: Instant::now(),
            captured_at: Utc::now().to_rfc3339(),
            cache_version,
            context_source,
            payload: payload.clone(),
        },
    );
}

fn read_remote_workspace_snapshot<T: Clone>(
    cache: &DashMap<String, AgentWorkspaceRemoteSnapshot<T>>,
    conversation_id: &ChatConversationId,
) -> Option<(T, String, String, AgentWorkspaceContextSource)> {
    let key = agent_workspace_diff_cache_key(conversation_id)?;
    let current_version = current_agent_workspace_cache_version(conversation_id)?;
    let entry = cache.get(&key)?;
    if entry.inserted_at.elapsed() <= remote_workspace_snapshot_ttl()
        && entry.cache_version == current_version
    {
        return Some((
            entry.payload.clone(),
            entry.captured_at.clone(),
            entry.cache_version.clone(),
            entry.context_source,
        ));
    }
    drop(entry);
    cache.remove(&key);
    None
}

fn store_remote_workspace_file_snapshot<T: Clone>(
    cache: &DashMap<String, AgentWorkspaceRemoteSnapshot<T>>,
    conversation_id: &ChatConversationId,
    file_path: &str,
    scope: &str,
    context_source: AgentWorkspaceContextSource,
    payload: &T,
) {
    if remote_workspace_snapshot_ttl().is_zero() {
        return;
    }
    let Some(key) = agent_workspace_file_snapshot_key(conversation_id, file_path, scope) else {
        return;
    };
    let Some(cache_version) = current_agent_workspace_cache_version(conversation_id) else {
        return;
    };
    cache.insert(
        key,
        AgentWorkspaceRemoteSnapshot {
            inserted_at: Instant::now(),
            captured_at: Utc::now().to_rfc3339(),
            cache_version,
            context_source,
            payload: payload.clone(),
        },
    );
}

fn read_remote_workspace_file_snapshot<T: Clone>(
    cache: &DashMap<String, AgentWorkspaceRemoteSnapshot<T>>,
    conversation_id: &ChatConversationId,
    file_path: &str,
    scope: &str,
) -> Option<(T, String, String, AgentWorkspaceContextSource)> {
    let key = agent_workspace_file_snapshot_key(conversation_id, file_path, scope)?;
    let current_version = current_agent_workspace_cache_version(conversation_id)?;
    let entry = cache.get(&key)?;
    if entry.inserted_at.elapsed() <= remote_workspace_snapshot_ttl()
        && entry.cache_version == current_version
    {
        return Some((
            entry.payload.clone(),
            entry.captured_at.clone(),
            entry.cache_version.clone(),
            entry.context_source,
        ));
    }
    drop(entry);
    cache.remove(&key);
    None
}

#[doc(hidden)]
pub fn get_agent_workspace_review_snapshot(
    conversation_id: &ChatConversationId,
) -> Option<(
    AgentWorkspaceReviewResponse,
    String,
    String,
    AgentWorkspaceContextSource,
)> {
    read_remote_workspace_snapshot(agent_workspace_remote_review_cache(), conversation_id)
}

#[doc(hidden)]
pub fn get_agent_workspace_change_summary_snapshot(
    conversation_id: &ChatConversationId,
) -> Option<(
    AgentWorkspaceChangeSummaryResponse,
    String,
    String,
    AgentWorkspaceContextSource,
)> {
    read_remote_workspace_snapshot(agent_workspace_remote_summary_cache(), conversation_id)
}

#[doc(hidden)]
pub fn get_agent_workspace_file_diff_snapshot(
    conversation_id: &ChatConversationId,
    file_path: &str,
    scope: &str,
) -> Option<(FileDiff, String, String, AgentWorkspaceContextSource)> {
    read_remote_workspace_file_snapshot(
        agent_workspace_remote_file_diff_cache(),
        conversation_id,
        file_path,
        scope,
    )
}

#[doc(hidden)]
pub fn get_agent_workspace_file_diff_page_snapshot(
    conversation_id: &ChatConversationId,
    file_path: &str,
    ref_kind: &DiffRefKind,
    offset: usize,
    limit: usize,
) -> Option<(FileDiffPage, String, String, AgentWorkspaceContextSource)> {
    let scope = format!(
        "page:{}:{offset}:{limit}",
        agent_workspace_diff_ref_scope(ref_kind)
    );
    read_remote_workspace_file_snapshot(
        agent_workspace_remote_file_diff_page_cache(),
        conversation_id,
        file_path,
        &scope,
    )
}

fn agent_workspace_review_locks() -> &'static DashMap<String, Arc<tokio::sync::Mutex<()>>> {
    static LOCKS: OnceLock<DashMap<String, Arc<tokio::sync::Mutex<()>>>> = OnceLock::new();
    LOCKS.get_or_init(DashMap::new)
}

fn agent_workspace_pr_annotations_cache(
) -> &'static DashMap<String, AgentWorkspacePrAnnotationsCacheEntry> {
    static CACHE: OnceLock<DashMap<String, AgentWorkspacePrAnnotationsCacheEntry>> =
        OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

fn agent_workspace_pr_annotations_locks() -> &'static DashMap<String, Arc<tokio::sync::Mutex<()>>> {
    static LOCKS: OnceLock<DashMap<String, Arc<tokio::sync::Mutex<()>>>> = OnceLock::new();
    LOCKS.get_or_init(DashMap::new)
}

fn cached_agent_workspace_context(
    conversation_id: &ChatConversationId,
    mode: AgentWorkspaceContextMode,
) -> Option<AgentWorkspaceContext> {
    let ttl = agent_workspace_review_cache_ttl();
    if ttl.is_zero() {
        return None;
    }
    let key = agent_workspace_context_cache_key(conversation_id, mode)?;
    let entry = agent_workspace_context_cache().get(&key)?;
    if entry.inserted_at.elapsed() <= ttl {
        return Some(entry.context.clone());
    }
    drop(entry);
    agent_workspace_context_cache().remove(&key);
    None
}

fn store_agent_workspace_context(
    conversation_id: &ChatConversationId,
    mode: AgentWorkspaceContextMode,
    context: &AgentWorkspaceContext,
) {
    if agent_workspace_review_cache_ttl().is_zero() {
        return;
    }
    let Some(key) = agent_workspace_context_cache_key(conversation_id, mode) else {
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
    let entry = agent_workspace_review_cache().get(&key)?;
    if entry.inserted_at.elapsed() <= ttl {
        return Some(entry.snapshot.clone());
    }
    drop(entry);
    agent_workspace_review_cache().remove(&key);
    agent_workspace_remote_review_cache().remove(&key);
    agent_workspace_remote_summary_cache().remove(&key);
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

fn cached_agent_workspace_pr_annotations(
    conversation_id: &ChatConversationId,
    pr_number: i64,
) -> Option<PrDiffAnnotations> {
    let ttl = agent_workspace_pr_annotations_cache_ttl();
    if ttl.is_zero() {
        return None;
    }
    let key = agent_workspace_pr_annotations_cache_key(conversation_id, pr_number)?;
    let entry = agent_workspace_pr_annotations_cache().get(&key)?;
    if entry.inserted_at.elapsed() <= ttl {
        return Some(entry.payload.clone());
    }
    drop(entry);
    agent_workspace_pr_annotations_cache().remove(&key);
    None
}

fn store_agent_workspace_pr_annotations(
    conversation_id: &ChatConversationId,
    pr_number: i64,
    payload: &PrDiffAnnotations,
) {
    if agent_workspace_pr_annotations_cache_ttl().is_zero() {
        return;
    }
    let Some(key) = agent_workspace_pr_annotations_cache_key(conversation_id, pr_number) else {
        return;
    };
    agent_workspace_pr_annotations_cache().insert(
        key,
        AgentWorkspacePrAnnotationsCacheEntry {
            inserted_at: Instant::now(),
            payload: payload.clone(),
        },
    );
}

#[cfg(test)]
fn clear_agent_workspace_pr_annotations_cache_for_test() {
    agent_workspace_pr_annotations_cache().clear();
    agent_workspace_pr_annotations_locks().clear();
}

pub(crate) fn invalidate_agent_workspace_diff_caches(conversation_id: &ChatConversationId) {
    let Some(key) = agent_workspace_diff_cache_key(conversation_id) else {
        return;
    };
    agent_workspace_context_cache().remove(&format!(
        "{key}:{}",
        AgentWorkspaceContextMode::Strict.cache_key_suffix()
    ));
    agent_workspace_context_cache().remove(&format!(
        "{key}:{}",
        AgentWorkspaceContextMode::Repair.cache_key_suffix()
    ));
    agent_workspace_review_cache().remove(&key);
    agent_workspace_remote_review_cache().remove(&key);
    agent_workspace_remote_summary_cache().remove(&key);
    let file_snapshot_prefix = format!("{key}\0");
    let file_diff_keys = agent_workspace_remote_file_diff_cache()
        .iter()
        .filter_map(|entry| {
            entry
                .key()
                .starts_with(&file_snapshot_prefix)
                .then(|| entry.key().clone())
        })
        .collect::<Vec<_>>();
    for snapshot_key in file_diff_keys {
        agent_workspace_remote_file_diff_cache().remove(&snapshot_key);
    }
    let page_keys = agent_workspace_remote_file_diff_page_cache()
        .iter()
        .filter_map(|entry| {
            entry
                .key()
                .starts_with(&file_snapshot_prefix)
                .then(|| entry.key().clone())
        })
        .collect::<Vec<_>>();
    for snapshot_key in page_keys {
        agent_workspace_remote_file_diff_page_cache().remove(&snapshot_key);
    }
    let annotation_prefix = format!("{key}:");
    let annotation_keys = agent_workspace_pr_annotations_cache()
        .iter()
        .filter_map(|entry| {
            let key = entry.key();
            key.starts_with(&annotation_prefix).then(|| key.clone())
        })
        .collect::<Vec<_>>();
    for key in annotation_keys {
        agent_workspace_pr_annotations_cache().remove(&key);
    }
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
    match get_terminal_agent_workspace_pr_context(&project, &workspace).await {
        Ok(Some(context)) => return Ok(context),
        Ok(None) => {}
        Err(terminal_head_error) => {
            if let Some(base_commit) = workspace.base_commit.as_deref() {
                if let Some(context) = resolve_agent_workspace_github_patch_context(
                    app_state,
                    &project,
                    &workspace,
                    base_commit,
                )
                .await?
                {
                    return Ok(context);
                }
            }
            return Err(terminal_head_error);
        }
    }

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
                source: AgentWorkspaceContextSource::PlanBranch,
                working_path: project_path,
                base_ref: merge_base,
                diff_target: Some(plan_branch.branch_name.clone()),
                patch_diff: None,
                supports_worktree_modes: false,
                repair_state: None,
            });
        }
    }

    let base_commit = workspace.base_commit.clone().ok_or_else(|| {
        AppError::Validation(format!(
            "Agent conversation workspace {} is missing its captured base commit",
            conversation_id
        ))
    })?;
    match resolve_valid_agent_conversation_workspace_path(&project, &workspace).await {
        Ok(worktree_path) => {
            let review_base = resolve_agent_workspace_review_base(
                &worktree_path,
                &workspace,
                "HEAD",
                &base_commit,
            )
            .await?;
            Ok(AgentWorkspaceContext {
                source: AgentWorkspaceContextSource::Worktree,
                working_path: worktree_path,
                base_ref: review_base,
                diff_target: None,
                patch_diff: None,
                supports_worktree_modes: true,
                repair_state: None,
            })
        }
        Err(worktree_error) => {
            if let Some(context) =
                resolve_agent_workspace_local_branch_context(&project, &workspace, &base_commit)
                    .await?
            {
                return Ok(context);
            }
            if let Some(context) =
                resolve_agent_workspace_pr_head_context(&project, &workspace, &base_commit).await?
            {
                return Ok(context);
            }
            if let Some(context) = resolve_agent_workspace_github_patch_context(
                app_state,
                &project,
                &workspace,
                &base_commit,
            )
            .await?
            {
                return Ok(context);
            }
            Err(worktree_error)
        }
    }
}

async fn resolve_agent_workspace_local_branch_context(
    project: &Project,
    workspace: &crate::domain::entities::AgentConversationWorkspace,
    base_commit: &str,
) -> AppResult<Option<AgentWorkspaceContext>> {
    let project_path = PathBuf::from(&project.working_directory);
    let expected_path =
        resolve_agent_conversation_workspace_path(project, &workspace.conversation_id)?;
    let stored_path = PathBuf::from(&workspace.worktree_path);
    if stored_path != expected_path || expected_path == project_path || expected_path.exists() {
        return Ok(None);
    }

    if !GitService::branch_exists(&project_path, &workspace.branch_name).await? {
        return Ok(None);
    }
    let review_base = resolve_agent_workspace_review_base(
        &project_path,
        workspace,
        &workspace.branch_name,
        base_commit,
    )
    .await?;

    Ok(Some(AgentWorkspaceContext {
        source: AgentWorkspaceContextSource::LocalBranch,
        working_path: project_path,
        base_ref: review_base,
        diff_target: Some(workspace.branch_name.clone()),
        patch_diff: None,
        supports_worktree_modes: false,
        repair_state: None,
    }))
}

async fn resolve_agent_workspace_pr_head_context(
    project: &Project,
    workspace: &crate::domain::entities::AgentConversationWorkspace,
    base_commit: &str,
) -> AppResult<Option<AgentWorkspaceContext>> {
    let Some(pr_number) = workspace.publication_pr_number else {
        return Ok(None);
    };
    let project_path = PathBuf::from(&project.working_directory);
    let expected_path =
        resolve_agent_conversation_workspace_path(project, &workspace.conversation_id)?;
    let stored_path = PathBuf::from(&workspace.worktree_path);
    if stored_path != expected_path || expected_path == project_path || expected_path.exists() {
        return Ok(None);
    }

    let Some(pr_head_ref) =
        GitService::fetch_pull_request_head_for_review(&project_path, pr_number).await?
    else {
        return Ok(None);
    };
    let review_base =
        resolve_agent_workspace_review_base(&project_path, workspace, &pr_head_ref, base_commit)
            .await?;

    Ok(Some(AgentWorkspaceContext {
        source: AgentWorkspaceContextSource::PullRequestHead,
        working_path: project_path,
        base_ref: review_base,
        diff_target: Some(pr_head_ref),
        patch_diff: None,
        supports_worktree_modes: false,
        repair_state: None,
    }))
}

async fn resolve_agent_workspace_github_patch_context(
    app_state: &AppState,
    project: &Project,
    workspace: &crate::domain::entities::AgentConversationWorkspace,
    base_commit: &str,
) -> AppResult<Option<AgentWorkspaceContext>> {
    let Some(pr_number) = workspace.publication_pr_number else {
        return Ok(None);
    };
    let Some(github_service) = app_state.github_service.as_ref() else {
        return Ok(None);
    };
    let project_path = PathBuf::from(&project.working_directory);
    let expected_path =
        resolve_agent_conversation_workspace_path(project, &workspace.conversation_id)?;
    let stored_path = PathBuf::from(&workspace.worktree_path);
    if stored_path != expected_path || expected_path == project_path || expected_path.exists() {
        return Ok(None);
    }

    let patch = github_service
        .get_pr_diff_patch(
            &project_path,
            pr_number,
            workspace.publication_pr_url.as_deref(),
        )
        .await?;
    if patch.trim().is_empty() {
        return Ok(None);
    }

    Ok(Some(AgentWorkspaceContext {
        source: AgentWorkspaceContextSource::GithubPatch,
        working_path: project_path,
        base_ref: base_commit.to_string(),
        diff_target: Some(format!("github-pr-diff/{pr_number}")),
        patch_diff: Some(Arc::<str>::from(patch)),
        supports_worktree_modes: false,
        repair_state: None,
    }))
}

async fn get_terminal_agent_workspace_pr_context(
    project: &Project,
    workspace: &AgentConversationWorkspace,
) -> AppResult<Option<AgentWorkspaceContext>> {
    if !workspace.has_terminal_publication_pr_status() {
        return Ok(None);
    }
    let Some(pr_number) = workspace.publication_pr_number else {
        return Ok(None);
    };

    let repo_path = PathBuf::from(&project.working_directory);
    let pr_head_ref = agent_workspace_pr_head_ref(pr_number);
    if !GitService::ref_exists(&repo_path, &pr_head_ref).await?
        && GitService::fetch_pull_request_head_for_review(&repo_path, pr_number)
            .await?
            .is_none()
    {
        return Err(AppError::GitOperation(format!(
            "Terminal agent workspace PR head ref is unavailable: {pr_head_ref}"
        )));
    }
    let head_ref = pr_head_ref;
    let base_ref_source = if workspace.base_ref.trim().is_empty() {
        project.base_branch.as_deref().unwrap_or("main").to_string()
    } else {
        workspace.base_ref.clone()
    };
    let base_ref =
        resolve_merge_base(&repo_path, &base_ref_source, &head_ref).unwrap_or(base_ref_source);
    Ok(Some(AgentWorkspaceContext {
        source: AgentWorkspaceContextSource::TerminalPullRequestHead,
        working_path: repo_path,
        base_ref,
        diff_target: Some(head_ref),
        patch_diff: None,
        supports_worktree_modes: false,
        repair_state: None,
    }))
}

async fn get_agent_workspace_repair_context(
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
    if workspace.publication_push_status.as_deref() != Some("needs_agent") {
        return Err(AppError::Validation(format!(
            "Repair diff context requires agent conversation workspace {} to be in needs_agent publication state",
            conversation_id
        )));
    }

    let project = app_state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await?
        .ok_or_else(|| AppError::ProjectNotFound(workspace.project_id.as_str().to_string()))?;
    let base_commit = workspace.base_commit.clone().ok_or_else(|| {
        AppError::Validation(format!(
            "Agent conversation workspace {} is missing its captured base commit",
            conversation_id
        ))
    })?;
    let worktree_path = resolve_agent_conversation_workspace_path_for_send(&project, &workspace)?;
    let checked_out_branch = GitService::get_current_branch(&worktree_path).await?;
    let rebase_in_progress = GitService::is_rebase_in_progress(&worktree_path);
    let merge_in_progress = GitService::is_merge_in_progress(&worktree_path);
    if checked_out_branch != workspace.branch_name && !rebase_in_progress && !merge_in_progress {
        return Err(AppError::Validation(format!(
            "Agent conversation workspace {} is checked out at '{}' instead of '{}' and is not in a recognized repair state",
            workspace.conversation_id, checked_out_branch, workspace.branch_name
        )));
    }

    Ok(AgentWorkspaceContext {
        source: AgentWorkspaceContextSource::RepairWorktree,
        working_path: worktree_path,
        base_ref: base_commit,
        diff_target: None,
        patch_diff: None,
        supports_worktree_modes: true,
        repair_state: Some(AgentWorkspaceRepairStateResponse {
            expected_branch: workspace.branch_name,
            checked_out_branch,
            rebase_in_progress,
            merge_in_progress,
        }),
    })
}

async fn get_agent_workspace_context_cached(
    app_state: &AppState,
    conversation_id: &ChatConversationId,
) -> AppResult<(AgentWorkspaceContext, AgentWorkspaceDiffCacheStatus)> {
    get_agent_workspace_context_cached_for_mode(
        app_state,
        conversation_id,
        AgentWorkspaceContextMode::Strict,
    )
    .await
}

async fn get_agent_workspace_repair_context_cached(
    app_state: &AppState,
    conversation_id: &ChatConversationId,
) -> AppResult<(AgentWorkspaceContext, AgentWorkspaceDiffCacheStatus)> {
    get_agent_workspace_context_cached_for_mode(
        app_state,
        conversation_id,
        AgentWorkspaceContextMode::Repair,
    )
    .await
}

async fn get_agent_workspace_context_cached_for_mode(
    app_state: &AppState,
    conversation_id: &ChatConversationId,
    mode: AgentWorkspaceContextMode,
) -> AppResult<(AgentWorkspaceContext, AgentWorkspaceDiffCacheStatus)> {
    ensure_agent_workspace_diff_cache_current(app_state, conversation_id).await?;
    if let Some(context) = cached_agent_workspace_context(conversation_id, mode) {
        return Ok((context, AgentWorkspaceDiffCacheStatus::Hit));
    }
    let Some(key) = agent_workspace_context_cache_key(conversation_id, mode) else {
        let context = match mode {
            AgentWorkspaceContextMode::Strict => {
                get_agent_workspace_context(app_state, conversation_id).await?
            }
            AgentWorkspaceContextMode::Repair => {
                get_agent_workspace_repair_context(app_state, conversation_id).await?
            }
        };
        return Ok((context, AgentWorkspaceDiffCacheStatus::Miss));
    };
    let lock = agent_workspace_context_locks()
        .entry(key)
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone();
    let _guard = lock.lock().await;
    if let Some(context) = cached_agent_workspace_context(conversation_id, mode) {
        return Ok((context, AgentWorkspaceDiffCacheStatus::Coalesced));
    }
    let context = match mode {
        AgentWorkspaceContextMode::Strict => {
            get_agent_workspace_context(app_state, conversation_id).await?
        }
        AgentWorkspaceContextMode::Repair => {
            get_agent_workspace_repair_context(app_state, conversation_id).await?
        }
    };
    store_agent_workspace_context(conversation_id, mode, &context);
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
    let result =
        get_agent_conversation_workspace_review_cached(app_state.inner(), &conversation_id).await;
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

fn summarize_agent_workspace_file_changes(
    changes: &[FileChange],
) -> AgentWorkspaceChangeSummaryBucketResponse {
    AgentWorkspaceChangeSummaryBucketResponse {
        file_count: changes.len(),
        additions: changes.iter().map(|change| change.additions).sum(),
        deletions: changes.iter().map(|change| change.deletions).sum(),
    }
}

#[doc(hidden)]
pub async fn get_agent_conversation_workspace_change_summary_for_state(
    app_state: &AppState,
    conversation_id: &ChatConversationId,
) -> AppResult<AgentWorkspaceChangeSummaryResponse> {
    let (ctx, _) = get_agent_workspace_context_cached(app_state, conversation_id).await?;
    let context_source = ctx.source;
    let response = if !ctx.supports_worktree_modes {
        AgentWorkspaceChangeSummaryResponse {
            supports_worktree_modes: false,
            staged: AgentWorkspaceChangeSummaryBucketResponse {
                file_count: 0,
                additions: 0,
                deletions: 0,
            },
            unstaged: AgentWorkspaceChangeSummaryBucketResponse {
                file_count: 0,
                additions: 0,
                deletions: 0,
            },
            conflicted: None,
            repair_state: None,
        }
    } else {
        let working_path = ctx.working_path.to_string_lossy().to_string();
        tokio::task::spawn_blocking(move || {
            let diff_service = DiffService::new();
            let staged = diff_service.get_staged_file_changes(&working_path)?;
            let unstaged = diff_service.get_unstaged_file_changes(&working_path)?;
            Ok::<_, AppError>(AgentWorkspaceChangeSummaryResponse {
                supports_worktree_modes: true,
                staged: summarize_agent_workspace_file_changes(&staged),
                unstaged: summarize_agent_workspace_file_changes(&unstaged),
                conflicted: None,
                repair_state: None,
            })
        })
        .await
        .map_err(|error| {
            AppError::Infrastructure(format!(
                "agent workspace change summary task failed: {error}"
            ))
        })??
    };
    store_remote_workspace_snapshot(
        agent_workspace_remote_summary_cache(),
        conversation_id,
        context_source,
        &response,
    );
    Ok(response)
}

#[tauri::command]
pub async fn get_agent_conversation_workspace_change_summary(
    app_state: State<'_, AppState>,
    conversation_id: String,
) -> AppResult<AgentWorkspaceChangeSummaryResponse> {
    let started = Instant::now();
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let result = get_agent_conversation_workspace_change_summary_for_state(
        app_state.inner(),
        &conversation_id,
    )
    .await;
    match &result {
        Ok(summary) => info!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "change_summary",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            supports_worktree_modes = summary.supports_worktree_modes,
            staged_files = summary.staged.file_count,
            unstaged_files = summary.unstaged.file_count,
            staged_additions = summary.staged.additions,
            staged_deletions = summary.staged.deletions,
            unstaged_additions = summary.unstaged.additions,
            unstaged_deletions = summary.unstaged.deletions,
            "Loaded agent workspace change summary"
        ),
        Err(error) => warn!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "change_summary",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            error = %error,
            "Failed to load agent workspace change summary"
        ),
    }
    result
}

#[doc(hidden)]
pub async fn get_agent_conversation_workspace_repair_change_summary_for_state(
    app_state: &AppState,
    conversation_id: &ChatConversationId,
) -> AppResult<AgentWorkspaceChangeSummaryResponse> {
    let (ctx, _) = get_agent_workspace_repair_context_cached(app_state, conversation_id).await?;
    let repair_state = ctx.repair_state.clone();
    let mut conflicted_files = GitService::get_conflict_files(&ctx.working_path)
        .await?
        .into_iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    conflicted_files.sort();

    let working_path = ctx.working_path.to_string_lossy().to_string();
    let (staged, unstaged) = tokio::task::spawn_blocking(move || {
        let diff_service = DiffService::new();
        let staged = diff_service.get_staged_file_changes(&working_path)?;
        let unstaged = diff_service.get_unstaged_file_changes(&working_path)?;
        Ok::<_, AppError>((staged, unstaged))
    })
    .await
    .map_err(|error| {
        AppError::Infrastructure(format!(
            "agent workspace repair change summary task failed: {error}"
        ))
    })??;

    Ok(AgentWorkspaceChangeSummaryResponse {
        supports_worktree_modes: true,
        staged: summarize_agent_workspace_file_changes(&staged),
        unstaged: summarize_agent_workspace_file_changes(&unstaged),
        conflicted: Some(AgentWorkspaceConflictSummaryResponse {
            file_count: conflicted_files.len(),
            files: conflicted_files,
        }),
        repair_state,
    })
}

#[tauri::command]
pub async fn get_agent_conversation_workspace_repair_change_summary(
    app_state: State<'_, AppState>,
    conversation_id: String,
) -> AppResult<AgentWorkspaceChangeSummaryResponse> {
    let started = Instant::now();
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let result = get_agent_conversation_workspace_repair_change_summary_for_state(
        app_state.inner(),
        &conversation_id,
    )
    .await;
    match &result {
        Ok(summary) => info!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "repair_change_summary",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            staged_files = summary.staged.file_count,
            unstaged_files = summary.unstaged.file_count,
            conflicted_files = summary.conflicted.as_ref().map(|bucket| bucket.file_count).unwrap_or(0),
            "Loaded repair-aware agent workspace change summary"
        ),
        Err(error) => warn!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "repair_change_summary",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            error = %error,
            "Failed to load repair-aware agent workspace change summary"
        ),
    }
    result
}

#[tauri::command]
pub async fn get_agent_conversation_workspace_pr_annotations(
    app_state: State<'_, AppState>,
    conversation_id: String,
) -> AppResult<PrDiffAnnotations> {
    let started = Instant::now();
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let result =
        get_agent_conversation_workspace_pr_annotations_cached(app_state.inner(), &conversation_id)
            .await;
    match &result {
        Ok((payload, cache_status)) => info!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "pr_annotations",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            cache_status = cache_status.as_str(),
            pr_number = payload.pr_number,
            annotations = payload.annotations.len(),
            unavailable_sources = payload.sources_unavailable.len(),
            "Loaded agent workspace PR annotations"
        ),
        Err(error) => warn!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "pr_annotations",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            error = %error,
            "Failed to load agent workspace PR annotations"
        ),
    }
    result.map(|(payload, _)| payload)
}

#[tauri::command]
pub async fn get_agent_conversation_workspace_review_hunk_annotations(
    app_state: State<'_, AppState>,
    conversation_id: String,
) -> AppResult<AgentWorkspaceReviewHunkAnnotationsResponse> {
    let started = Instant::now();
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let result = get_agent_conversation_workspace_review_hunk_annotations_for_state(
        app_state.inner(),
        &conversation_id,
    )
    .await;
    match &result {
        Ok(payload) => info!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "workspace_review_hunk_annotations",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            annotations = payload.annotations.len(),
            artifact_id = %payload.artifact_id.as_deref().unwrap_or("none"),
            diff_fingerprint = %payload
                .diff_fingerprint
                .as_deref()
                .map(|value| value.chars().take(12).collect::<String>())
                .unwrap_or_else(|| "none".to_string()),
            "Loaded workspace Review hunk annotations"
        ),
        Err(error) => warn!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "workspace_review_hunk_annotations",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            error = %error,
            "Failed to load workspace Review hunk annotations"
        ),
    }
    result
}

async fn get_agent_conversation_workspace_review_hunk_annotations_for_state(
    app_state: &AppState,
    conversation_id: &ChatConversationId,
) -> AppResult<AgentWorkspaceReviewHunkAnnotationsResponse> {
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
    let context = load_agent_workspace_review_context(app_state, &workspace).await?;
    let Some(target) = context.target.as_ref() else {
        return Ok(AgentWorkspaceReviewHunkAnnotationsResponse::empty());
    };
    if !context.is_current {
        return Ok(AgentWorkspaceReviewHunkAnnotationsResponse {
            artifact_id: context
                .monitor
                .review_artifact_id
                .as_ref()
                .map(|id| id.as_str().to_string()),
            artifact_version: context.monitor.review_artifact_version,
            target_scope: Some(target.scope.to_string()),
            head_sha: target.head_sha.clone(),
            diff_fingerprint: Some(target.diff_fingerprint.clone()),
            annotations: Vec::new(),
        });
    }
    let Some(artifact_id) = context.monitor.review_artifact_id.as_ref() else {
        return Ok(AgentWorkspaceReviewHunkAnnotationsResponse::empty());
    };
    let annotations = app_state
        .agent_conversation_workspace_repo
        .list_workspace_review_hunk_annotations(conversation_id, artifact_id)
        .await?
        .into_iter()
        .filter(|annotation| {
            annotation.target_scope == target.scope
                && annotation.diff_fingerprint == target.diff_fingerprint
                && annotation.artifact_id == *artifact_id
                && annotation.artifact_version
                    == context.monitor.review_artifact_version.unwrap_or(0)
                && (target.scope
                    != crate::domain::entities::AgentWorkspaceReviewTargetScope::SelectedSource
                    || annotation.head_sha.as_deref() == target.head_sha.as_deref())
        })
        .collect();
    Ok(AgentWorkspaceReviewHunkAnnotationsResponse {
        artifact_id: Some(artifact_id.as_str().to_string()),
        artifact_version: context.monitor.review_artifact_version,
        target_scope: Some(target.scope.to_string()),
        head_sha: target.head_sha.clone(),
        diff_fingerprint: Some(target.diff_fingerprint.clone()),
        annotations,
    })
}

async fn get_agent_conversation_workspace_pr_annotations_cached(
    app_state: &AppState,
    conversation_id: &ChatConversationId,
) -> AppResult<(PrDiffAnnotations, AgentWorkspaceDiffCacheStatus)> {
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
    ensure_agent_workspace_diff_cache_matches(
        conversation_id,
        agent_workspace_diff_cache_version(&workspace),
    );
    let Some(pr_number) = workspace.publication_pr_number else {
        return Ok((
            PrDiffAnnotations::empty(0),
            AgentWorkspaceDiffCacheStatus::Miss,
        ));
    };
    if let Some(payload) = cached_agent_workspace_pr_annotations(conversation_id, pr_number) {
        return Ok((payload, AgentWorkspaceDiffCacheStatus::Hit));
    }
    let Some(key) = agent_workspace_pr_annotations_cache_key(conversation_id, pr_number) else {
        let payload = get_agent_conversation_workspace_pr_annotations_for_state(
            app_state,
            conversation_id,
            pr_number,
        )
        .await?;
        return Ok((payload, AgentWorkspaceDiffCacheStatus::Miss));
    };
    let lock = agent_workspace_pr_annotations_locks()
        .entry(key)
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone();
    let _guard = lock.lock().await;
    if let Some(payload) = cached_agent_workspace_pr_annotations(conversation_id, pr_number) {
        return Ok((payload, AgentWorkspaceDiffCacheStatus::Coalesced));
    }
    let payload = get_agent_conversation_workspace_pr_annotations_for_state(
        app_state,
        conversation_id,
        pr_number,
    )
    .await?;
    store_agent_workspace_pr_annotations(conversation_id, pr_number, &payload);
    Ok((payload, AgentWorkspaceDiffCacheStatus::Miss))
}

async fn get_agent_conversation_workspace_pr_annotations_for_state(
    app_state: &AppState,
    conversation_id: &ChatConversationId,
    pr_number: i64,
) -> AppResult<PrDiffAnnotations> {
    let Some(github) = app_state.github_service.as_ref() else {
        let mut payload = PrDiffAnnotations::empty(pr_number);
        payload
            .sources_unavailable
            .push(PrAnnotationSourceUnavailable {
                source: "github".to_string(),
                reason: "GitHub service is unavailable".to_string(),
            });
        return Ok(payload);
    };
    let (ctx, _) = get_agent_workspace_context_cached(app_state, conversation_id).await?;
    github
        .fetch_pr_diff_annotations(&ctx.working_path, pr_number)
        .await
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
    ensure_agent_workspace_diff_cache_current(app_state, conversation_id).await?;
    if let Some(snapshot) = cached_agent_workspace_review(conversation_id) {
        store_remote_workspace_snapshot(
            agent_workspace_remote_review_cache(),
            conversation_id,
            snapshot.context_source,
            &snapshot.response,
        );
        return Ok((snapshot, AgentWorkspaceDiffCacheStatus::Hit));
    }
    let Some(key) = agent_workspace_diff_cache_key(conversation_id) else {
        let snapshot =
            get_agent_conversation_workspace_review_for_state(app_state, conversation_id).await?;
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
    store_remote_workspace_snapshot(
        agent_workspace_remote_review_cache(),
        conversation_id,
        snapshot.context_source,
        &snapshot.response,
    );
    Ok((snapshot, AgentWorkspaceDiffCacheStatus::Miss))
}

async fn get_agent_workspace_review_for_context(
    ctx: AgentWorkspaceContext,
) -> AppResult<AgentWorkspaceReviewSnapshot> {
    let context_source = ctx.source;
    let working_path = ctx.working_path.to_string_lossy().to_string();
    let base_ref = ctx.base_ref.clone();
    let head_ref = ctx
        .diff_target
        .clone()
        .unwrap_or_else(|| "HEAD".to_string());

    if let Some(patch) = ctx.patch_diff {
        let diff_service = DiffService::new();
        let mut changes = diff_service.get_file_changes_from_unified_diff(&patch);
        let flags = {
            let path_strs: Vec<&str> = changes.iter().map(|c| c.path.as_str()).collect();
            diff_service.compute_generated_flags(ctx.working_path.as_path(), &path_strs)?
        };
        for change in &mut changes {
            if let Some(&is_gen) = flags.get(&change.path) {
                change.is_generated = is_gen;
            }
        }
        return Ok(AgentWorkspaceReviewSnapshot {
            context_source,
            response: AgentWorkspaceReviewResponse {
                changes,
                commits: Vec::new(),
                base_ref,
                head_ref,
                supports_worktree_modes: ctx.supports_worktree_modes,
            },
        });
    }

    let changes_path = working_path.clone();
    let changes_base_ref = base_ref.clone();
    let changes_target = ctx.diff_target.clone();
    let changes_supports_worktree_modes = ctx.supports_worktree_modes;
    let changes_fut = async move {
        tokio::task::spawn_blocking(move || {
            let diff_service = DiffService::new();
            let mut changes = get_workspace_file_changes_for_context(
                &diff_service,
                &changes_path,
                &changes_base_ref,
                changes_target.as_deref(),
                changes_supports_worktree_modes,
            )?;
            let flags = {
                let path_strs: Vec<&str> = changes.iter().map(|c| c.path.as_str()).collect();
                diff_service.compute_generated_flags(Path::new(&changes_path), &path_strs)?
            };
            for change in &mut changes {
                if let Some(&is_gen) = flags.get(&change.path) {
                    change.is_generated = is_gen;
                }
            }
            Ok(changes)
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
        context_source,
        response: AgentWorkspaceReviewResponse {
            changes,
            commits: commits.into_iter().map(CommitInfoResponse::from).collect(),
            base_ref,
            head_ref,
            supports_worktree_modes: ctx.supports_worktree_modes,
        },
    })
}

fn get_workspace_file_changes_for_context(
    diff_service: &DiffService,
    working_path: &str,
    base_ref: &str,
    diff_target: Option<&str>,
    supports_worktree_modes: bool,
) -> AppResult<Vec<FileChange>> {
    if supports_worktree_modes {
        return diff_service.get_worktree_file_changes_from_ref(working_path, base_ref);
    }

    if let Some(target) = diff_target {
        return diff_service.get_file_changes_between_refs(working_path, base_ref, target);
    }

    diff_service.get_worktree_file_changes_from_ref(working_path, base_ref)
}

fn get_workspace_file_diff_for_context(
    diff_service: &DiffService,
    file_path: &str,
    working_path: &str,
    base_ref: &str,
    diff_target: Option<&str>,
    supports_worktree_modes: bool,
) -> AppResult<FileDiff> {
    if supports_worktree_modes {
        return diff_service.get_file_diff(file_path, working_path, base_ref);
    }

    if let Some(target) = diff_target {
        return diff_service.get_file_diff_between_refs(file_path, working_path, base_ref, target);
    }

    diff_service.get_file_diff(file_path, working_path, base_ref)
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
        let context_source = ctx.source;
        let working_path = ctx.working_path.to_string_lossy().to_string();
        let diff_service = DiffService::new();
        let diff = if let Some(patch) = &ctx.patch_diff {
            diff_service.get_file_diff_from_unified_diff(patch, &file_path)
        } else {
            get_workspace_file_diff_for_context(
                &diff_service,
                &file_path,
                &working_path,
                &ctx.base_ref,
                ctx.diff_target.as_deref(),
                ctx.supports_worktree_modes,
            )
        }?;
        store_remote_workspace_file_snapshot(
            agent_workspace_remote_file_diff_cache(),
            &conversation_id,
            &file_path,
            "head",
            context_source,
            &diff,
        );
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
            hunk_count = diff.hunks.len(),
            old_lines = diff.old_total_lines,
            new_lines = diff.new_total_lines,
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
        let mut changes = if diff_service.is_merge_commit(&working_path, &commit_sha) {
            diff_service.get_file_changes_between_refs(&working_path, &ctx.base_ref, &commit_sha)
        } else {
            diff_service.get_commit_file_changes(&commit_sha, &working_path)
        }?;
        let flags = {
            let path_strs: Vec<&str> = changes.iter().map(|c| c.path.as_str()).collect();
            diff_service.compute_generated_flags(Path::new(&working_path), &path_strs)?
        };
        for change in &mut changes {
            if let Some(&is_gen) = flags.get(&change.path) {
                change.is_generated = is_gen;
            }
        }
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
        let context_source = ctx.source;
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
        store_remote_workspace_file_snapshot(
            agent_workspace_remote_file_diff_cache(),
            &conversation_id,
            &file_path,
            &format!("commit:{commit_sha}"),
            context_source,
            &diff,
        );
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
            hunk_count = diff.hunks.len(),
            old_lines = diff.old_total_lines,
            new_lines = diff.new_total_lines,
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

// =========================================================================
// Extension A — Staged / Unstaged workspace diffs
// =========================================================================

#[doc(hidden)]
pub async fn get_agent_conversation_workspace_staged_file_changes_for_state(
    app_state: &AppState,
    conversation_id: &ChatConversationId,
) -> AppResult<Vec<FileChange>> {
    let (ctx, _) = get_agent_workspace_context_cached(app_state, conversation_id).await?;
    let working_path = ctx.working_path.to_string_lossy().to_string();
    tokio::task::spawn_blocking(move || {
        let diff_service = DiffService::new();
        let mut changes = diff_service.get_staged_file_changes(&working_path)?;
        let flags = {
            let path_strs: Vec<&str> = changes.iter().map(|c| c.path.as_str()).collect();
            diff_service.compute_generated_flags(Path::new(&working_path), &path_strs)?
        };
        for change in &mut changes {
            if let Some(&is_gen) = flags.get(&change.path) {
                change.is_generated = is_gen;
            }
        }
        Ok(changes)
    })
    .await
    .map_err(|e| AppError::Infrastructure(format!("staged file changes task failed: {e}")))?
}

#[doc(hidden)]
pub async fn get_agent_conversation_workspace_unstaged_file_changes_for_state(
    app_state: &AppState,
    conversation_id: &ChatConversationId,
) -> AppResult<Vec<FileChange>> {
    let (ctx, _) = get_agent_workspace_context_cached(app_state, conversation_id).await?;
    let working_path = ctx.working_path.to_string_lossy().to_string();
    tokio::task::spawn_blocking(move || {
        let diff_service = DiffService::new();
        let mut changes = diff_service.get_unstaged_file_changes(&working_path)?;
        let flags = {
            let path_strs: Vec<&str> = changes.iter().map(|c| c.path.as_str()).collect();
            diff_service.compute_generated_flags(Path::new(&working_path), &path_strs)?
        };
        for change in &mut changes {
            if let Some(&is_gen) = flags.get(&change.path) {
                change.is_generated = is_gen;
            }
        }
        Ok(changes)
    })
    .await
    .map_err(|e| AppError::Infrastructure(format!("unstaged file changes task failed: {e}")))?
}

#[doc(hidden)]
pub async fn get_agent_conversation_workspace_staged_file_diff_for_state(
    app_state: &AppState,
    conversation_id: &ChatConversationId,
    file_path: String,
) -> AppResult<FileDiff> {
    let (ctx, _) = get_agent_workspace_context_cached(app_state, conversation_id).await?;
    let working_path = ctx.working_path.to_string_lossy().to_string();
    tokio::task::spawn_blocking(move || {
        DiffService::new().get_staged_file_diff(&file_path, &working_path)
    })
    .await
    .map_err(|e| AppError::Infrastructure(format!("staged file diff task failed: {e}")))?
}

#[doc(hidden)]
pub async fn get_agent_conversation_workspace_unstaged_file_diff_for_state(
    app_state: &AppState,
    conversation_id: &ChatConversationId,
    file_path: String,
) -> AppResult<FileDiff> {
    let (ctx, _) = get_agent_workspace_context_cached(app_state, conversation_id).await?;
    let working_path = ctx.working_path.to_string_lossy().to_string();
    tokio::task::spawn_blocking(move || {
        DiffService::new().get_unstaged_file_diff(&file_path, &working_path)
    })
    .await
    .map_err(|e| AppError::Infrastructure(format!("unstaged file diff task failed: {e}")))?
}

#[doc(hidden)]
pub async fn get_agent_conversation_workspace_repair_staged_file_changes_for_state(
    app_state: &AppState,
    conversation_id: &ChatConversationId,
) -> AppResult<Vec<FileChange>> {
    let (ctx, _) = get_agent_workspace_repair_context_cached(app_state, conversation_id).await?;
    let working_path = ctx.working_path.to_string_lossy().to_string();
    tokio::task::spawn_blocking(move || {
        let diff_service = DiffService::new();
        let mut changes = diff_service.get_staged_file_changes(&working_path)?;
        let flags = {
            let path_strs: Vec<&str> = changes.iter().map(|c| c.path.as_str()).collect();
            diff_service.compute_generated_flags(Path::new(&working_path), &path_strs)?
        };
        for change in &mut changes {
            if let Some(&is_gen) = flags.get(&change.path) {
                change.is_generated = is_gen;
            }
        }
        Ok(changes)
    })
    .await
    .map_err(|e| AppError::Infrastructure(format!("repair staged file changes task failed: {e}")))?
}

#[doc(hidden)]
pub async fn get_agent_conversation_workspace_repair_unstaged_file_changes_for_state(
    app_state: &AppState,
    conversation_id: &ChatConversationId,
) -> AppResult<Vec<FileChange>> {
    let (ctx, _) = get_agent_workspace_repair_context_cached(app_state, conversation_id).await?;
    let working_path = ctx.working_path.to_string_lossy().to_string();
    tokio::task::spawn_blocking(move || {
        let diff_service = DiffService::new();
        let mut changes = diff_service.get_unstaged_file_changes(&working_path)?;
        let flags = {
            let path_strs: Vec<&str> = changes.iter().map(|c| c.path.as_str()).collect();
            diff_service.compute_generated_flags(Path::new(&working_path), &path_strs)?
        };
        for change in &mut changes {
            if let Some(&is_gen) = flags.get(&change.path) {
                change.is_generated = is_gen;
            }
        }
        Ok(changes)
    })
    .await
    .map_err(|e| {
        AppError::Infrastructure(format!("repair unstaged file changes task failed: {e}"))
    })?
}

#[doc(hidden)]
pub async fn get_agent_conversation_workspace_repair_staged_file_diff_for_state(
    app_state: &AppState,
    conversation_id: &ChatConversationId,
    file_path: String,
) -> AppResult<FileDiff> {
    let (ctx, _) = get_agent_workspace_repair_context_cached(app_state, conversation_id).await?;
    let working_path = ctx.working_path.to_string_lossy().to_string();
    tokio::task::spawn_blocking(move || {
        DiffService::new().get_staged_file_diff(&file_path, &working_path)
    })
    .await
    .map_err(|e| AppError::Infrastructure(format!("repair staged file diff task failed: {e}")))?
}

#[doc(hidden)]
pub async fn get_agent_conversation_workspace_repair_unstaged_file_diff_for_state(
    app_state: &AppState,
    conversation_id: &ChatConversationId,
    file_path: String,
) -> AppResult<FileDiff> {
    let (ctx, _) = get_agent_workspace_repair_context_cached(app_state, conversation_id).await?;
    let working_path = ctx.working_path.to_string_lossy().to_string();
    tokio::task::spawn_blocking(move || {
        DiffService::new().get_unstaged_file_diff(&file_path, &working_path)
    })
    .await
    .map_err(|e| AppError::Infrastructure(format!("repair unstaged file diff task failed: {e}")))?
}

#[doc(hidden)]
pub async fn get_agent_conversation_workspace_repair_conflict_file_diff_for_state(
    app_state: &AppState,
    conversation_id: &ChatConversationId,
    file_path: String,
) -> AppResult<ConflictDiff> {
    let (ctx, _) = get_agent_workspace_repair_context_cached(app_state, conversation_id).await?;
    let working_path = ctx.working_path.to_string_lossy().to_string();
    tokio::task::spawn_blocking(move || {
        DiffService::new().get_index_conflict_diff(&file_path, &working_path)
    })
    .await
    .map_err(|e| AppError::Infrastructure(format!("repair conflict file diff task failed: {e}")))?
}

#[doc(hidden)]
pub async fn get_agent_conversation_workspace_cumulative_file_changes_for_state(
    app_state: &AppState,
    conversation_id: &ChatConversationId,
) -> AppResult<Vec<FileChange>> {
    let (ctx, _) = get_agent_workspace_context_cached(app_state, conversation_id).await?;
    if let Some(patch) = ctx.patch_diff {
        let diff_service = DiffService::new();
        let mut changes = diff_service.get_file_changes_from_unified_diff(&patch);
        let flags = {
            let path_strs: Vec<&str> = changes.iter().map(|c| c.path.as_str()).collect();
            diff_service.compute_generated_flags(ctx.working_path.as_path(), &path_strs)?
        };
        for change in &mut changes {
            if let Some(&is_gen) = flags.get(&change.path) {
                change.is_generated = is_gen;
            }
        }
        return Ok(changes);
    }
    let working_path = ctx.working_path.to_string_lossy().to_string();
    let base_ref = ctx.base_ref.clone();
    let head_ref = ctx
        .diff_target
        .clone()
        .unwrap_or_else(|| "HEAD".to_string());
    tokio::task::spawn_blocking(move || {
        let diff_service = DiffService::new();
        let mut changes =
            diff_service.get_file_changes_between_refs(&working_path, &base_ref, &head_ref)?;
        let flags = {
            let path_strs: Vec<&str> = changes.iter().map(|c| c.path.as_str()).collect();
            diff_service.compute_generated_flags(Path::new(&working_path), &path_strs)?
        };
        for change in &mut changes {
            if let Some(&is_gen) = flags.get(&change.path) {
                change.is_generated = is_gen;
            }
        }
        Ok(changes)
    })
    .await
    .map_err(|e| AppError::Infrastructure(format!("cumulative file changes task failed: {e}")))?
}

#[doc(hidden)]
pub async fn get_agent_conversation_workspace_cumulative_file_diff_for_state(
    app_state: &AppState,
    conversation_id: &ChatConversationId,
    file_path: String,
) -> AppResult<FileDiff> {
    let (ctx, _) = get_agent_workspace_context_cached(app_state, conversation_id).await?;
    let context_source = ctx.source;
    if let Some(patch) = ctx.patch_diff {
        let diff = DiffService::new().get_file_diff_from_unified_diff(&patch, &file_path)?;
        store_remote_workspace_file_snapshot(
            agent_workspace_remote_file_diff_cache(),
            conversation_id,
            &file_path,
            "cumulative_head",
            context_source,
            &diff,
        );
        return Ok(diff);
    }
    let working_path = ctx.working_path.to_string_lossy().to_string();
    let base_ref = ctx.base_ref.clone();
    let head_ref = ctx
        .diff_target
        .clone()
        .unwrap_or_else(|| "HEAD".to_string());
    let snapshot_file_path = file_path.clone();
    let diff = tokio::task::spawn_blocking(move || {
        let diff_service = DiffService::new();
        diff_service.get_file_diff_between_refs(&file_path, &working_path, &base_ref, &head_ref)
    })
    .await
    .map_err(|e| AppError::Infrastructure(format!("cumulative file diff task failed: {e}")))??;
    store_remote_workspace_file_snapshot(
        agent_workspace_remote_file_diff_cache(),
        conversation_id,
        &snapshot_file_path,
        "cumulative_head",
        context_source,
        &diff,
    );
    Ok(diff)
}

#[doc(hidden)]
pub async fn get_agent_conversation_workspace_file_diff_page_for_state(
    app_state: &AppState,
    conversation_id: &ChatConversationId,
    file_path: String,
    ref_kind: DiffRefKind,
    offset: usize,
    limit: usize,
) -> AppResult<FileDiffPage> {
    let (ctx, _) = get_agent_workspace_context_cached(app_state, conversation_id).await?;
    let context_source = ctx.source;

    if matches!(ref_kind, DiffRefKind::CumulativeBase) {
        return Err(AppError::Validation(
            "CumulativeBase is a file-content ref, not a diff page target".to_string(),
        ));
    }

    if matches!(ref_kind, DiffRefKind::Staged | DiffRefKind::Unstaged)
        && !ctx.supports_worktree_modes
    {
        return Err(AppError::Validation(
            "Staged and unstaged diff pages are unavailable for read-only workspaces".to_string(),
        ));
    }

    if let DiffRefKind::Commit { sha } = &ref_kind {
        let head_ref = ctx.diff_target.as_deref().unwrap_or("HEAD");
        ensure_agent_workspace_commit_in_range(
            conversation_id,
            &ctx.working_path,
            &ctx.base_ref,
            head_ref,
            sha,
        )
        .await?;
    }

    let working_path = ctx.working_path.to_string_lossy().to_string();
    let base_ref = ctx.base_ref.clone();
    let diff_target = ctx.diff_target.clone();
    let patch_diff = ctx.patch_diff.clone();
    let supports_worktree_modes = ctx.supports_worktree_modes;
    let snapshot_file_path = file_path.clone();
    let snapshot_scope = format!(
        "page:{}:{offset}:{limit}",
        agent_workspace_diff_ref_scope(&ref_kind)
    );

    let page = tokio::task::spawn_blocking(move || {
        let diff_service = DiffService::new();
        let diff = match ref_kind {
            DiffRefKind::Head => {
                if let Some(patch) = patch_diff.as_deref() {
                    diff_service.get_file_diff_from_unified_diff(patch, &file_path)
                } else {
                    get_workspace_file_diff_for_context(
                        &diff_service,
                        &file_path,
                        &working_path,
                        &base_ref,
                        diff_target.as_deref(),
                        supports_worktree_modes,
                    )
                }
            }
            DiffRefKind::Staged => diff_service.get_staged_file_diff(&file_path, &working_path),
            DiffRefKind::Unstaged => diff_service.get_unstaged_file_diff(&file_path, &working_path),
            DiffRefKind::Commit { sha } => {
                if diff_service.is_merge_commit(&working_path, &sha) {
                    diff_service.get_file_diff_between_refs(
                        &file_path,
                        &working_path,
                        &base_ref,
                        &sha,
                    )
                } else {
                    diff_service.get_commit_file_diff(&sha, &file_path, &working_path)
                }
            }
            DiffRefKind::CumulativeHead => {
                if let Some(patch) = patch_diff.as_deref() {
                    diff_service.get_file_diff_from_unified_diff(patch, &file_path)
                } else {
                    let head_ref = diff_target.unwrap_or_else(|| "HEAD".to_string());
                    diff_service.get_file_diff_between_refs(
                        &file_path,
                        &working_path,
                        &base_ref,
                        &head_ref,
                    )
                }
            }
            DiffRefKind::CumulativeBase => unreachable!("validated before blocking task"),
        }?;

        DiffService::page_file_diff(diff, offset, limit)
    })
    .await
    .map_err(|e| AppError::Infrastructure(format!("file diff page task failed: {e}")))??;
    store_remote_workspace_file_snapshot(
        agent_workspace_remote_file_diff_page_cache(),
        conversation_id,
        &snapshot_file_path,
        &snapshot_scope,
        context_source,
        &page,
    );
    Ok(page)
}

// =========================================================================
// Extension B — Cumulative (base..HEAD) workspace diffs
// =========================================================================

#[tauri::command]
pub async fn get_agent_conversation_workspace_staged_file_changes(
    app_state: State<'_, AppState>,
    conversation_id: String,
) -> AppResult<Vec<FileChange>> {
    let started = Instant::now();
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let result = get_agent_conversation_workspace_staged_file_changes_for_state(
        app_state.inner(),
        &conversation_id,
    )
    .await;
    match &result {
        Ok(changes) => info!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "staged_file_changes",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            files = changes.len(),
            "Loaded agent workspace staged file changes"
        ),
        Err(error) => warn!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "staged_file_changes",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            error = %error,
            "Failed to load agent workspace staged file changes"
        ),
    }
    result
}

#[tauri::command]
pub async fn get_agent_conversation_workspace_unstaged_file_changes(
    app_state: State<'_, AppState>,
    conversation_id: String,
) -> AppResult<Vec<FileChange>> {
    let started = Instant::now();
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let result = get_agent_conversation_workspace_unstaged_file_changes_for_state(
        app_state.inner(),
        &conversation_id,
    )
    .await;
    match &result {
        Ok(changes) => info!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "unstaged_file_changes",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            files = changes.len(),
            "Loaded agent workspace unstaged file changes"
        ),
        Err(error) => warn!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "unstaged_file_changes",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            error = %error,
            "Failed to load agent workspace unstaged file changes"
        ),
    }
    result
}

#[tauri::command]
pub async fn get_agent_conversation_workspace_staged_file_diff(
    app_state: State<'_, AppState>,
    conversation_id: String,
    file_path: String,
) -> AppResult<FileDiff> {
    let started = Instant::now();
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let result = get_agent_conversation_workspace_staged_file_diff_for_state(
        app_state.inner(),
        &conversation_id,
        file_path,
    )
    .await;
    match &result {
        Ok(diff) => info!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "staged_file_diff",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            hunk_count = diff.hunks.len(),
            old_lines = diff.old_total_lines,
            new_lines = diff.new_total_lines,
            "Loaded agent workspace staged file diff"
        ),
        Err(error) => warn!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "staged_file_diff",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            error = %error,
            "Failed to load agent workspace staged file diff"
        ),
    }
    result
}

#[tauri::command]
pub async fn get_agent_conversation_workspace_unstaged_file_diff(
    app_state: State<'_, AppState>,
    conversation_id: String,
    file_path: String,
) -> AppResult<FileDiff> {
    let started = Instant::now();
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let result = get_agent_conversation_workspace_unstaged_file_diff_for_state(
        app_state.inner(),
        &conversation_id,
        file_path,
    )
    .await;
    match &result {
        Ok(diff) => info!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "unstaged_file_diff",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            hunk_count = diff.hunks.len(),
            old_lines = diff.old_total_lines,
            new_lines = diff.new_total_lines,
            "Loaded agent workspace unstaged file diff"
        ),
        Err(error) => warn!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "unstaged_file_diff",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            error = %error,
            "Failed to load agent workspace unstaged file diff"
        ),
    }
    result
}

#[tauri::command]
pub async fn get_agent_conversation_workspace_repair_staged_file_changes(
    app_state: State<'_, AppState>,
    conversation_id: String,
) -> AppResult<Vec<FileChange>> {
    let started = Instant::now();
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let result = get_agent_conversation_workspace_repair_staged_file_changes_for_state(
        app_state.inner(),
        &conversation_id,
    )
    .await;
    match &result {
        Ok(changes) => info!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "repair_staged_file_changes",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            files = changes.len(),
            "Loaded repair-aware agent workspace staged file changes"
        ),
        Err(error) => warn!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "repair_staged_file_changes",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            error = %error,
            "Failed to load repair-aware agent workspace staged file changes"
        ),
    }
    result
}

#[tauri::command]
pub async fn get_agent_conversation_workspace_repair_unstaged_file_changes(
    app_state: State<'_, AppState>,
    conversation_id: String,
) -> AppResult<Vec<FileChange>> {
    let started = Instant::now();
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let result = get_agent_conversation_workspace_repair_unstaged_file_changes_for_state(
        app_state.inner(),
        &conversation_id,
    )
    .await;
    match &result {
        Ok(changes) => info!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "repair_unstaged_file_changes",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            files = changes.len(),
            "Loaded repair-aware agent workspace unstaged file changes"
        ),
        Err(error) => warn!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "repair_unstaged_file_changes",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            error = %error,
            "Failed to load repair-aware agent workspace unstaged file changes"
        ),
    }
    result
}

#[tauri::command]
pub async fn get_agent_conversation_workspace_repair_staged_file_diff(
    app_state: State<'_, AppState>,
    conversation_id: String,
    file_path: String,
) -> AppResult<FileDiff> {
    let started = Instant::now();
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let result = get_agent_conversation_workspace_repair_staged_file_diff_for_state(
        app_state.inner(),
        &conversation_id,
        file_path,
    )
    .await;
    match &result {
        Ok(diff) => info!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "repair_staged_file_diff",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            hunk_count = diff.hunks.len(),
            old_lines = diff.old_total_lines,
            new_lines = diff.new_total_lines,
            "Loaded repair-aware agent workspace staged file diff"
        ),
        Err(error) => warn!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "repair_staged_file_diff",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            error = %error,
            "Failed to load repair-aware agent workspace staged file diff"
        ),
    }
    result
}

#[tauri::command]
pub async fn get_agent_conversation_workspace_repair_unstaged_file_diff(
    app_state: State<'_, AppState>,
    conversation_id: String,
    file_path: String,
) -> AppResult<FileDiff> {
    let started = Instant::now();
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let result = get_agent_conversation_workspace_repair_unstaged_file_diff_for_state(
        app_state.inner(),
        &conversation_id,
        file_path,
    )
    .await;
    match &result {
        Ok(diff) => info!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "repair_unstaged_file_diff",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            hunk_count = diff.hunks.len(),
            old_lines = diff.old_total_lines,
            new_lines = diff.new_total_lines,
            "Loaded repair-aware agent workspace unstaged file diff"
        ),
        Err(error) => warn!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "repair_unstaged_file_diff",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            error = %error,
            "Failed to load repair-aware agent workspace unstaged file diff"
        ),
    }
    result
}

#[tauri::command]
pub async fn get_agent_conversation_workspace_repair_conflict_file_diff(
    app_state: State<'_, AppState>,
    conversation_id: String,
    file_path: String,
) -> AppResult<ConflictDiff> {
    let started = Instant::now();
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let result = get_agent_conversation_workspace_repair_conflict_file_diff_for_state(
        app_state.inner(),
        &conversation_id,
        file_path,
    )
    .await;
    match &result {
        Ok(diff) => info!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "repair_conflict_file_diff",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            file_path = diff.file_path.as_str(),
            "Loaded repair conflict file diff"
        ),
        Err(error) => warn!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "repair_conflict_file_diff",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            error = %error,
            "Failed to load repair conflict file diff"
        ),
    }
    result
}

#[tauri::command]
pub async fn get_agent_conversation_workspace_cumulative_file_changes(
    app_state: State<'_, AppState>,
    conversation_id: String,
) -> AppResult<Vec<FileChange>> {
    let started = Instant::now();
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let result = get_agent_conversation_workspace_cumulative_file_changes_for_state(
        app_state.inner(),
        &conversation_id,
    )
    .await;
    match &result {
        Ok(changes) => info!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "cumulative_file_changes",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            files = changes.len(),
            "Loaded agent workspace cumulative file changes"
        ),
        Err(error) => warn!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "cumulative_file_changes",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            error = %error,
            "Failed to load agent workspace cumulative file changes"
        ),
    }
    result
}

#[tauri::command]
pub async fn get_agent_conversation_workspace_cumulative_file_diff(
    app_state: State<'_, AppState>,
    conversation_id: String,
    file_path: String,
) -> AppResult<FileDiff> {
    let started = Instant::now();
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let result = get_agent_conversation_workspace_cumulative_file_diff_for_state(
        app_state.inner(),
        &conversation_id,
        file_path,
    )
    .await;
    match &result {
        Ok(diff) => info!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "cumulative_file_diff",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            hunk_count = diff.hunks.len(),
            old_lines = diff.old_total_lines,
            new_lines = diff.new_total_lines,
            "Loaded agent workspace cumulative file diff"
        ),
        Err(error) => warn!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "cumulative_file_diff",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            error = %error,
            "Failed to load agent workspace cumulative file diff"
        ),
    }
    result
}

#[tauri::command]
pub async fn get_agent_conversation_workspace_file_diff_page(
    app_state: State<'_, AppState>,
    conversation_id: String,
    file_path: String,
    ref_kind: DiffRefKind,
    offset: usize,
    limit: usize,
) -> AppResult<FileDiffPage> {
    let started = Instant::now();
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let result = get_agent_conversation_workspace_file_diff_page_for_state(
        app_state.inner(),
        &conversation_id,
        file_path,
        ref_kind,
        offset,
        limit,
    )
    .await;
    match &result {
        Ok(page) => info!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "file_diff_page",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            file_path = %page.file_path,
            offset = page.offset,
            limit = page.limit,
            rows = page.rows.len(),
            total_rows = page.total_rows,
            "Loaded agent workspace file diff page"
        ),
        Err(error) => warn!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "file_diff_page",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            error = %error,
            "Failed to load agent workspace file diff page"
        ),
    }
    result
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

/// Shared implementation used by both the Tauri command and the HTTP handler.
#[doc(hidden)]
pub async fn get_agent_conversation_workspace_file_content_range_for_state(
    app_state: &AppState,
    conversation_id: &ChatConversationId,
    side: DiffSide,
    file_path: String,
    ref_kind: DiffRefKind,
    from: u32,
    to: u32,
) -> AppResult<Vec<RangeLine>> {
    let (ctx, _) = get_agent_workspace_context_cached(app_state, conversation_id).await?;
    if ctx.patch_diff.is_some() {
        return Err(AppError::Validation(
            "File content range is unavailable for patch-backed agent workspace diffs".to_string(),
        ));
    }
    let workspace_path = ctx.working_path.to_string_lossy().to_string();

    // Resolve workspace-specific ref_kind variants to concrete refs that match
    // the same old/new pair used by get_agent_conversation_workspace_file_diff.
    let live_worktree_head_range = ctx.supports_worktree_modes && ctx.patch_diff.is_none();
    let resolved_ref_kind = match ref_kind {
        DiffRefKind::CumulativeBase => DiffRefKind::Commit {
            sha: ctx.base_ref.clone(),
        },
        DiffRefKind::CumulativeHead => {
            let head = ctx
                .diff_target
                .clone()
                .unwrap_or_else(|| "HEAD".to_string());
            DiffRefKind::Commit { sha: head }
        }
        DiffRefKind::Head if live_worktree_head_range => match &side {
            DiffSide::Old => DiffRefKind::Commit {
                sha: ctx.base_ref.clone(),
            },
            DiffSide::New => DiffRefKind::Unstaged,
        },
        other => other,
    };

    tokio::task::spawn_blocking(move || {
        DiffService::new().get_file_content_range(
            &workspace_path,
            &side,
            &file_path,
            &resolved_ref_kind,
            from,
            to,
        )
    })
    .await
    .map_err(|e| AppError::Infrastructure(format!("file content range task failed: {e}")))?
}

/// Fetch a line range from a file at a specific ref in the agent workspace.
///
/// `side` — "old" | "new"
/// `ref_kind` — `{ "kind": "head" }` | `{ "kind": "staged" }` | `{ "kind": "unstaged" }` |
///              `{ "kind": "commit", "sha": "…" }` | `{ "kind": "cumulative_base" }` |
///              `{ "kind": "cumulative_head" }`
///
/// `from` and `to` are 1-indexed inclusive.  Maximum range: 5 000 lines.
#[tauri::command]
pub async fn get_agent_conversation_workspace_file_content_range(
    app_state: State<'_, AppState>,
    conversation_id: String,
    side: DiffSide,
    file_path: String,
    ref_kind: DiffRefKind,
    from: u32,
    to: u32,
) -> AppResult<Vec<RangeLine>> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let started = Instant::now();

    let result = get_agent_conversation_workspace_file_content_range_for_state(
        app_state.inner(),
        &conversation_id,
        side,
        file_path,
        ref_kind,
        from,
        to,
    )
    .await;

    match &result {
        Ok(lines) => info!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "file_content_range",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            line_count = lines.len(),
            "Loaded agent workspace file content range"
        ),
        Err(error) => warn!(
            target: "ralphx_lib::commands::agent_workspace_diff",
            operation = "file_content_range",
            conversation_id = %conversation_id,
            elapsed_ms = started.elapsed().as_millis(),
            error = %error,
            "Failed to load agent workspace file content range"
        ),
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::agent_conversation_workspace::{
        prepare_agent_conversation_workspace, AgentConversationWorkspaceBaseSelection,
    };
    use crate::application::{DiffPageRow, FileChangeStatus};
    use crate::domain::entities::{
        AgentConversationWorkspaceMode, IdeationAnalysisBaseRefKind, Project,
    };
    use crate::domain::services::GithubServiceTrait;
    use crate::tests::mock_github_service::MockGithubService;
    use std::sync::Arc;
    use tauri::test::{mock_builder, mock_context, noop_assets};
    use tauri::Manager;
    use tempfile::TempDir;

    fn test_conversation_id() -> ChatConversationId {
        ChatConversationId::from_string(uuid::Uuid::new_v4().to_string())
    }

    fn run_git(repo: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("git command should run");
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn diff_page_contains_line(page: &FileDiffPage, needle: &str) -> bool {
        page.rows.iter().any(|row| {
            matches!(
                row,
                DiffPageRow::Line { line } if line.content.contains(needle)
            )
        })
    }

    fn create_review_repo() -> (TempDir, PathBuf, String) {
        let temp_dir = TempDir::new().expect("temp repo should be created");
        let repo = temp_dir.path().to_path_buf();
        run_git(&repo, &["init", "-b", "main"]);
        run_git(&repo, &["config", "user.email", "test@example.com"]);
        run_git(&repo, &["config", "user.name", "Test User"]);

        std::fs::write(repo.join("README.md"), "initial\n").unwrap();
        run_git(&repo, &["add", "."]);
        run_git(&repo, &["commit", "-m", "Initial commit"]);
        let base = run_git(&repo, &["rev-parse", "HEAD"]);

        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(
            repo.join("src").join("lib.rs"),
            "pub fn answer() -> u8 { 42 }\n",
        )
        .unwrap();
        run_git(&repo, &["add", "."]);
        run_git(&repo, &["commit", "-m", "Add source file"]);

        (temp_dir, repo, base)
    }

    async fn create_agent_workspace_command_state(
    ) -> (TempDir, AppState, ChatConversationId, PathBuf, String) {
        let temp_dir = TempDir::new().expect("temp repo should be created");
        let repo = temp_dir.path().join("repo");
        std::fs::create_dir_all(&repo).expect("repo root should be created");
        run_git(&repo, &["init", "-b", "main"]);
        run_git(&repo, &["config", "user.email", "test@example.com"]);
        run_git(&repo, &["config", "user.name", "Test User"]);
        std::fs::write(repo.join("README.md"), "base\n").unwrap();
        run_git(&repo, &["add", "."]);
        run_git(&repo, &["commit", "-m", "Initial commit"]);

        let mut project = Project::new(
            "Agent Workspace Diff".to_string(),
            repo.display().to_string(),
        );
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory =
            Some(temp_dir.path().join("worktrees").display().to_string());
        let conversation_id = test_conversation_id();
        let workspace = prepare_agent_conversation_workspace(
            &project,
            &conversation_id,
            AgentConversationWorkspaceMode::Edit,
            AgentConversationWorkspaceBaseSelection {
                kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
                branch_mode: None,
                base_ref: Some("main".to_string()),
                display_name: None,
                source_pull_request: None,
            },
        )
        .await
        .expect("agent workspace should be prepared");
        let worktree_path = PathBuf::from(&workspace.worktree_path);
        std::fs::create_dir_all(worktree_path.join("src")).unwrap();
        std::fs::write(
            worktree_path.join("src").join("lib.rs"),
            "pub fn answer() -> u8 { 42 }\n",
        )
        .unwrap();
        run_git(&worktree_path, &["add", "."]);
        run_git(&worktree_path, &["commit", "-m", "Add workspace change"]);
        let commit_sha = run_git(&worktree_path, &["rev-parse", "HEAD"]);

        let state = AppState::new_test();
        state
            .project_repo
            .create(project)
            .await
            .expect("project should be persisted");
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace should be persisted");

        (temp_dir, state, conversation_id, worktree_path, commit_sha)
    }

    fn sample_review_snapshot(sha: &str) -> AgentWorkspaceReviewSnapshot {
        AgentWorkspaceReviewSnapshot {
            context_source: AgentWorkspaceContextSource::Worktree,
            response: AgentWorkspaceReviewResponse {
                changes: vec![FileChange {
                    path: "src/lib.rs".to_string(),
                    status: FileChangeStatus::Modified,
                    additions: 3,
                    deletions: 1,
                    is_generated: false,
                }],
                commits: vec![CommitInfoResponse {
                    sha: sha.to_string(),
                    short_sha: sha.chars().take(7).collect(),
                    message: "Improve review cache".to_string(),
                    author: "Test User".to_string(),
                    timestamp: "2026-05-13T10:00:00Z".to_string(),
                }],
                base_ref: "base-sha".to_string(),
                head_ref: "HEAD".to_string(),
                supports_worktree_modes: true,
            },
        }
    }

    #[test]
    fn agent_workspace_diff_cache_status_labels_are_stable() {
        assert_eq!(AgentWorkspaceDiffCacheStatus::Hit.as_str(), "hit");
        assert_eq!(
            AgentWorkspaceDiffCacheStatus::Coalesced.as_str(),
            "coalesced"
        );
        assert_eq!(AgentWorkspaceDiffCacheStatus::Miss.as_str(), "miss");
    }

    #[test]
    fn agent_workspace_diff_cache_keys_skip_nil_ids() {
        let nil_id = ChatConversationId::from_string(uuid::Uuid::nil().to_string());
        assert!(agent_workspace_diff_cache_key(&nil_id).is_none());

        let conversation_id = test_conversation_id();
        assert_eq!(
            agent_workspace_diff_cache_key(&conversation_id),
            Some(conversation_id.as_str())
        );
    }

    #[test]
    fn agent_workspace_diff_caches_store_hit_and_invalidate_by_conversation() {
        let conversation_id = test_conversation_id();
        invalidate_agent_workspace_diff_caches(&conversation_id);

        let context = AgentWorkspaceContext {
            source: AgentWorkspaceContextSource::Worktree,
            working_path: PathBuf::from("/tmp/agent-workspace-review-cache"),
            base_ref: "base-sha".to_string(),
            diff_target: Some("feature/review-cache".to_string()),
            patch_diff: None,
            supports_worktree_modes: false,
            repair_state: None,
        };
        let snapshot = sample_review_snapshot("abcdef0123456789abcdef0123456789abcdef01");
        let mut annotations = PrDiffAnnotations::empty(68);
        annotations.head_sha = Some("head-sha".to_string());

        store_agent_workspace_context(
            &conversation_id,
            AgentWorkspaceContextMode::Strict,
            &context,
        );
        store_agent_workspace_review(&conversation_id, &snapshot);
        store_agent_workspace_pr_annotations(&conversation_id, 68, &annotations);

        let cached_context =
            cached_agent_workspace_context(&conversation_id, AgentWorkspaceContextMode::Strict)
                .expect("context should hit");
        assert_eq!(cached_context.working_path, context.working_path);
        assert_eq!(cached_context.base_ref, "base-sha");
        assert_eq!(
            cached_context.diff_target.as_deref(),
            Some("feature/review-cache")
        );

        let cached_review =
            cached_agent_workspace_review(&conversation_id).expect("review should hit");
        assert_eq!(cached_review.response.changes.len(), 1);
        assert_eq!(
            cached_review.response.commits[0].sha,
            snapshot.response.commits[0].sha
        );
        assert_eq!(
            cached_agent_workspace_pr_annotations(&conversation_id, 68)
                .expect("annotations should hit")
                .head_sha
                .as_deref(),
            Some("head-sha")
        );

        invalidate_agent_workspace_diff_caches(&conversation_id);
        assert!(cached_agent_workspace_context(
            &conversation_id,
            AgentWorkspaceContextMode::Strict
        )
        .is_none());
        assert!(cached_agent_workspace_review(&conversation_id).is_none());
        assert!(cached_agent_workspace_pr_annotations(&conversation_id, 68).is_none());
    }

    #[test]
    fn agent_workspace_diff_cache_stores_skip_uncacheable_ids() {
        let conversation_id = ChatConversationId::from_string(uuid::Uuid::nil().to_string());
        let context = AgentWorkspaceContext {
            source: AgentWorkspaceContextSource::Worktree,
            working_path: PathBuf::from("/tmp/uncacheable-agent-workspace"),
            base_ref: "base-sha".to_string(),
            diff_target: None,
            patch_diff: None,
            supports_worktree_modes: true,
            repair_state: None,
        };
        let snapshot = sample_review_snapshot("abcdef0123456789abcdef0123456789abcdef01");
        let annotations = PrDiffAnnotations::empty(68);

        store_agent_workspace_context(
            &conversation_id,
            AgentWorkspaceContextMode::Strict,
            &context,
        );
        store_agent_workspace_review(&conversation_id, &snapshot);
        store_agent_workspace_pr_annotations(&conversation_id, 68, &annotations);

        assert!(cached_agent_workspace_context(
            &conversation_id,
            AgentWorkspaceContextMode::Strict
        )
        .is_none());
        assert!(cached_agent_workspace_review(&conversation_id).is_none());
        assert!(cached_agent_workspace_pr_annotations(&conversation_id, 68).is_none());
        invalidate_agent_workspace_diff_caches(&conversation_id);
    }

    #[tokio::test]
    async fn commit_range_validation_uses_cached_review_full_and_short_sha() {
        let conversation_id = test_conversation_id();
        let sha = "abcdef0123456789abcdef0123456789abcdef01";
        invalidate_agent_workspace_diff_caches(&conversation_id);
        store_agent_workspace_review(&conversation_id, &sample_review_snapshot(sha));

        ensure_agent_workspace_commit_in_range(
            &conversation_id,
            Path::new("/path/that/does/not/exist"),
            "missing-base",
            "missing-head",
            sha,
        )
        .await
        .expect("full cached sha should validate without hitting git");

        ensure_agent_workspace_commit_in_range(
            &conversation_id,
            Path::new("/path/that/does/not/exist"),
            "missing-base",
            "missing-head",
            "abcdef0",
        )
        .await
        .expect("short cached sha should validate without hitting git");

        invalidate_agent_workspace_diff_caches(&conversation_id);
    }

    #[tokio::test]
    async fn cached_context_loader_returns_hit_for_current_workspace_version() {
        let conversation_id = test_conversation_id();
        let state = AppState::new_test();
        let context = AgentWorkspaceContext {
            source: AgentWorkspaceContextSource::Worktree,
            working_path: PathBuf::from("/tmp/pre-resolved-agent-workspace"),
            base_ref: "base-sha".to_string(),
            diff_target: None,
            patch_diff: None,
            supports_worktree_modes: true,
            repair_state: None,
        };
        invalidate_agent_workspace_diff_caches(&conversation_id);
        ensure_agent_workspace_diff_cache_matches(&conversation_id, String::new());
        store_agent_workspace_context(
            &conversation_id,
            AgentWorkspaceContextMode::Strict,
            &context,
        );

        let (cached, status) = get_agent_workspace_context_cached(&state, &conversation_id)
            .await
            .expect("current-version cached context should be returned");

        assert_eq!(status.as_str(), "hit");
        assert_eq!(cached.working_path, context.working_path);
        assert_eq!(cached.base_ref, context.base_ref);

        invalidate_agent_workspace_diff_caches(&conversation_id);
    }

    #[tokio::test]
    async fn cached_review_loader_returns_hit_for_current_workspace_version() {
        let conversation_id = test_conversation_id();
        let state = AppState::new_test();
        let snapshot = sample_review_snapshot("abcdef0123456789abcdef0123456789abcdef01");
        invalidate_agent_workspace_diff_caches(&conversation_id);
        ensure_agent_workspace_diff_cache_matches(&conversation_id, String::new());
        store_agent_workspace_review(&conversation_id, &snapshot);

        let (cached, status) =
            get_agent_conversation_workspace_review_cached(&state, &conversation_id)
                .await
                .expect("current-version cached review should be returned");

        assert_eq!(status.as_str(), "hit");
        assert_eq!(cached.response.commits.len(), 1);
        assert_eq!(cached.response.commits[0].short_sha, "abcdef0");

        invalidate_agent_workspace_diff_caches(&conversation_id);
    }

    #[tokio::test]
    async fn pr_annotation_loader_coalesces_and_caches_by_published_pr() {
        clear_agent_workspace_pr_annotations_cache_for_test();
        let (_temp_dir, mut state, conversation_id, _worktree_path, _commit_sha) =
            create_agent_workspace_command_state().await;
        let mut workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        workspace.publication_pr_number = Some(68);
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace should be updated");

        let github = Arc::new(MockGithubService::new());
        let mut payload = PrDiffAnnotations::empty(68);
        payload.head_sha = Some("head-sha".to_string());
        github.will_return_pr_diff_annotations(payload);
        github.with_pr_diff_annotations_delay_ms(50);
        let github_trait: Arc<dyn crate::domain::services::github_service::GithubServiceTrait> =
            github.clone();
        state.github_service = Some(github_trait);

        let first =
            get_agent_conversation_workspace_pr_annotations_cached(&state, &conversation_id);
        let second =
            get_agent_conversation_workspace_pr_annotations_cached(&state, &conversation_id);
        let (first, second) = tokio::join!(first, second);
        let first = first.expect("first annotations call should load");
        let second = second.expect("second annotations call should coalesce");
        let statuses = [first.1.as_str(), second.1.as_str()];

        assert!(statuses.contains(&"miss"));
        assert!(statuses.contains(&"coalesced"));
        assert_eq!(first.0.head_sha.as_deref(), Some("head-sha"));
        assert_eq!(second.0.head_sha.as_deref(), Some("head-sha"));
        assert_eq!(github.state().fetch_pr_diff_annotations_calls, 1);

        let (cached, status) =
            get_agent_conversation_workspace_pr_annotations_cached(&state, &conversation_id)
                .await
                .expect("cached annotations should load");
        assert_eq!(status.as_str(), "hit");
        assert_eq!(cached.head_sha.as_deref(), Some("head-sha"));
        assert_eq!(github.state().fetch_pr_diff_annotations_calls, 1);

        invalidate_agent_workspace_diff_caches(&conversation_id);
        clear_agent_workspace_pr_annotations_cache_for_test();
    }

    #[tokio::test]
    async fn pr_annotation_loader_returns_empty_without_published_pr() {
        let (_temp_dir, mut state, conversation_id, _worktree_path, _commit_sha) =
            create_agent_workspace_command_state().await;
        let github = Arc::new(MockGithubService::new());
        let github_trait: Arc<dyn crate::domain::services::github_service::GithubServiceTrait> =
            github.clone();
        state.github_service = Some(github_trait);

        let (payload, status) =
            get_agent_conversation_workspace_pr_annotations_cached(&state, &conversation_id)
                .await
                .expect("annotations should return an empty unpublished payload");

        assert_eq!(status.as_str(), "miss");
        assert_eq!(payload.pr_number, 0);
        assert!(payload.annotations.is_empty());
        assert!(payload.sources_unavailable.is_empty());
        assert_eq!(github.state().fetch_pr_diff_annotations_calls, 0);
    }

    #[tokio::test]
    async fn pr_annotation_loader_reports_unavailable_github_service() {
        let (_temp_dir, state, conversation_id, _worktree_path, _commit_sha) =
            create_agent_workspace_command_state().await;
        let mut workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        workspace.publication_pr_number = Some(68);
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace should be updated");

        let (payload, status) =
            get_agent_conversation_workspace_pr_annotations_cached(&state, &conversation_id)
                .await
                .expect("annotations should return partial unavailable payload");

        assert_eq!(status.as_str(), "miss");
        assert_eq!(payload.pr_number, 68);
        assert!(payload.annotations.is_empty());
        assert_eq!(payload.sources_unavailable.len(), 1);
        assert_eq!(payload.sources_unavailable[0].source, "github");
        assert_eq!(
            payload.sources_unavailable[0].reason,
            "GitHub service is unavailable"
        );
    }

    #[tokio::test]
    async fn agent_workspace_diff_commands_use_shared_review_cache() {
        let (_temp_dir, state, conversation_id, worktree_path, commit_sha) =
            create_agent_workspace_command_state().await;
        std::fs::create_dir_all(worktree_path.join("docs")).unwrap();
        std::fs::write(
            worktree_path.join("docs").join("untracked.md"),
            "draft\nnotes\n",
        )
        .unwrap();
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let review =
            get_agent_conversation_workspace_review(app.state(), conversation_id.as_str()).await;
        let review = review.expect("review payload should load");
        assert_eq!(review.head_ref, "HEAD");
        assert!(review
            .changes
            .iter()
            .any(|change| change.path == "src/lib.rs"));
        assert!(review
            .changes
            .iter()
            .any(|change| change.path == "docs/untracked.md"
                && matches!(change.status, FileChangeStatus::Added)));
        assert!(review.commits.iter().any(|commit| commit.sha == commit_sha));

        let changes =
            get_agent_conversation_workspace_file_changes(app.state(), conversation_id.as_str())
                .await
                .expect("cached file changes should load");
        assert!(changes.iter().any(|change| change.path == "src/lib.rs"));
        assert!(changes
            .iter()
            .any(|change| change.path == "docs/untracked.md"));

        let unstaged_changes = get_agent_conversation_workspace_unstaged_file_changes(
            app.state(),
            conversation_id.as_str(),
        )
        .await
        .expect("unstaged file changes should load");
        assert!(unstaged_changes
            .iter()
            .any(|change| change.path == "docs/untracked.md"));

        let commits =
            get_agent_conversation_workspace_commits(app.state(), conversation_id.as_str())
                .await
                .expect("cached commits should load");
        let commit = commits
            .commits
            .iter()
            .find(|commit| commit.sha == commit_sha)
            .expect("workspace commit should be present");

        let file_diff = get_agent_conversation_workspace_file_diff(
            app.state(),
            conversation_id.as_str(),
            "src/lib.rs".to_string(),
        )
        .await
        .expect("workspace file diff should load");
        // Hunk-based: the committed "answer" function appears as additions in the diff hunks
        assert!(
            file_diff
                .hunks
                .iter()
                .flat_map(|h| h.lines.iter())
                .any(|l| l.content.contains("answer")),
            "file diff hunks should contain the 'answer' function"
        );

        let untracked_file_diff = get_agent_conversation_workspace_file_diff(
            app.state(),
            conversation_id.as_str(),
            "docs/untracked.md".to_string(),
        )
        .await
        .expect("untracked workspace file diff should load");
        assert_eq!(untracked_file_diff.old_total_lines, 0);
        assert_eq!(untracked_file_diff.new_total_lines, 2);
        assert!(untracked_file_diff
            .hunks
            .iter()
            .flat_map(|hunk| hunk.lines.iter())
            .any(|line| line.content == "draft"));

        let file_diff_page = get_agent_conversation_workspace_file_diff_page(
            app.state(),
            conversation_id.as_str(),
            "src/lib.rs".to_string(),
            DiffRefKind::Head,
            0,
            1,
        )
        .await
        .expect("workspace file diff page should load");
        assert_eq!(file_diff_page.file_path, "src/lib.rs");
        assert_eq!(file_diff_page.offset, 0);
        assert_eq!(file_diff_page.limit, 1);
        assert!(file_diff_page.rows.len() <= 1);
        assert!(
            file_diff_page.total_rows > file_diff_page.rows.len(),
            "page response should report more rows than it sends"
        );
        assert_eq!(file_diff_page.next_offset, Some(file_diff_page.rows.len()));

        let commit_changes = get_agent_conversation_workspace_commit_file_changes(
            app.state(),
            conversation_id.as_str(),
            commit.short_sha.clone(),
        )
        .await
        .expect("commit file changes should load through cached commit validation");
        assert!(commit_changes
            .iter()
            .any(|change| change.path == "src/lib.rs"));

        let commit_diff = get_agent_conversation_workspace_commit_file_diff(
            app.state(),
            conversation_id.as_str(),
            commit.sha.clone(),
            "src/lib.rs".to_string(),
        )
        .await
        .expect("commit file diff should load through cached commit validation");
        assert!(
            commit_diff
                .hunks
                .iter()
                .flat_map(|h| h.lines.iter())
                .any(|l| l.content.contains("answer")),
            "commit diff hunks should contain the 'answer' function"
        );

        invalidate_agent_workspace_diff_caches(&conversation_id);
    }

    #[tokio::test]
    async fn diff_target_editable_workspace_includes_untracked_files() {
        let (_temp_dir, state, conversation_id, worktree_path, _commit_sha) =
            create_agent_workspace_command_state().await;
        let workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        std::fs::create_dir_all(worktree_path.join("docs")).unwrap();
        std::fs::write(
            worktree_path.join("docs").join("untracked.md"),
            "draft\nnotes\n",
        )
        .unwrap();
        store_agent_workspace_context(
            &conversation_id,
            AgentWorkspaceContextMode::Strict,
            &AgentWorkspaceContext {
                source: AgentWorkspaceContextSource::Worktree,
                working_path: worktree_path,
                base_ref: workspace
                    .base_commit
                    .expect("workspace should have captured base commit"),
                diff_target: Some("HEAD".to_string()),
                patch_diff: None,
                supports_worktree_modes: true,
                repair_state: None,
            },
        );
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let review =
            get_agent_conversation_workspace_review(app.state(), conversation_id.as_str()).await;
        let review = review.expect("diff-target review should load");
        assert!(review
            .changes
            .iter()
            .any(|change| change.path == "docs/untracked.md"));

        let file_diff = get_agent_conversation_workspace_file_diff(
            app.state(),
            conversation_id.as_str(),
            "docs/untracked.md".to_string(),
        )
        .await
        .expect("diff-target untracked file diff should load");
        assert_eq!(file_diff.old_total_lines, 0);
        assert_eq!(file_diff.new_total_lines, 2);
        assert!(file_diff
            .hunks
            .iter()
            .flat_map(|hunk| hunk.lines.iter())
            .any(|line| line.content == "draft"));

        let file_diff_page = get_agent_conversation_workspace_file_diff_page(
            app.state(),
            conversation_id.as_str(),
            "docs/untracked.md".to_string(),
            DiffRefKind::Head,
            0,
            20,
        )
        .await
        .expect("diff-target untracked file diff page should load");
        assert!(diff_page_contains_line(&file_diff_page, "draft"));

        let content_range = get_agent_conversation_workspace_file_content_range(
            app.state(),
            conversation_id.as_str(),
            DiffSide::New,
            "docs/untracked.md".to_string(),
            DiffRefKind::Head,
            1,
            2,
        )
        .await
        .expect("diff-target untracked file content range should load");
        assert_eq!(content_range[0].content, "draft");

        invalidate_agent_workspace_diff_caches(&conversation_id);
    }

    #[tokio::test]
    async fn file_diff_page_command_loads_staged_and_unstaged_refs() {
        let (_tmp, state, conversation_id, worktree_path) =
            create_staged_unstaged_workspace_state().await;
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app");

        std::fs::write(worktree_path.join("base.txt"), "base\nstaged\n").unwrap();
        run_git(&worktree_path, &["add", "base.txt"]);

        let staged_page = get_agent_conversation_workspace_file_diff_page(
            app.state(),
            conversation_id.as_str(),
            "base.txt".to_string(),
            DiffRefKind::Staged,
            0,
            20,
        )
        .await
        .expect("staged diff page should load");
        assert!(
            diff_page_contains_line(&staged_page, "staged"),
            "staged diff page should include staged content"
        );

        std::fs::write(worktree_path.join("base.txt"), "base\nstaged\nunstaged\n").unwrap();

        let unstaged_page = get_agent_conversation_workspace_file_diff_page(
            app.state(),
            conversation_id.as_str(),
            "base.txt".to_string(),
            DiffRefKind::Unstaged,
            0,
            20,
        )
        .await
        .expect("unstaged diff page should load");
        assert!(
            diff_page_contains_line(&unstaged_page, "unstaged"),
            "unstaged diff page should include unstaged content"
        );

        invalidate_agent_workspace_diff_caches(&conversation_id);
    }

    #[tokio::test]
    async fn file_diff_page_command_resolves_commit_and_cumulative_refs() {
        let (_temp_dir, state, conversation_id, _worktree_path, commit_sha) =
            create_agent_workspace_command_state().await;
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app");

        let commit_page = get_agent_conversation_workspace_file_diff_page(
            app.state(),
            conversation_id.as_str(),
            "src/lib.rs".to_string(),
            DiffRefKind::Commit {
                sha: commit_sha.clone(),
            },
            0,
            20,
        )
        .await
        .expect("commit diff page should load");
        assert!(
            diff_page_contains_line(&commit_page, "answer"),
            "commit diff page should include selected commit content"
        );

        let cumulative_page = get_agent_conversation_workspace_file_diff_page(
            app.state(),
            conversation_id.as_str(),
            "src/lib.rs".to_string(),
            DiffRefKind::CumulativeHead,
            0,
            20,
        )
        .await
        .expect("cumulative head diff page should load");
        assert!(
            diff_page_contains_line(&cumulative_page, "answer"),
            "cumulative head diff page should include workspace content"
        );

        let cumulative_base = get_agent_conversation_workspace_file_diff_page(
            app.state(),
            conversation_id.as_str(),
            "src/lib.rs".to_string(),
            DiffRefKind::CumulativeBase,
            0,
            20,
        )
        .await;
        assert!(
            cumulative_base
                .expect_err("cumulative base is not a diff page target")
                .to_string()
                .contains("CumulativeBase"),
            "cumulative base error should explain the unsupported ref"
        );

        invalidate_agent_workspace_diff_caches(&conversation_id);
    }

    #[tokio::test]
    async fn file_diff_page_command_rejects_staged_refs_for_read_only_context() {
        let (_tmp, state, conversation_id, worktree_path) =
            create_staged_unstaged_workspace_state().await;
        ensure_agent_workspace_diff_cache_current(&state, &conversation_id)
            .await
            .expect("workspace cache version should load");
        store_agent_workspace_context(
            &conversation_id,
            AgentWorkspaceContextMode::Strict,
            &AgentWorkspaceContext {
                source: AgentWorkspaceContextSource::Worktree,
                working_path: worktree_path,
                base_ref: "HEAD".to_string(),
                diff_target: Some("agent-branch".to_string()),
                patch_diff: None,
                supports_worktree_modes: false,
                repair_state: None,
            },
        );
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app");

        let result = get_agent_conversation_workspace_file_diff_page(
            app.state(),
            conversation_id.as_str(),
            "base.txt".to_string(),
            DiffRefKind::Staged,
            0,
            20,
        )
        .await;
        assert!(
            result
                .expect_err("staged pages should be unavailable for read-only workspaces")
                .to_string()
                .contains("read-only workspaces"),
            "read-only staged error should explain why staged pages are unavailable"
        );

        invalidate_agent_workspace_diff_caches(&conversation_id);
    }

    #[tokio::test]
    async fn agent_workspace_review_uses_local_branch_after_worktree_cleanup() {
        let (temp_dir, state, conversation_id, worktree_path, commit_sha) =
            create_agent_workspace_command_state().await;
        let repo = temp_dir.path().join("repo");
        let workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        let branch_name = workspace.branch_name.clone();
        let worktree_arg = worktree_path
            .to_str()
            .expect("test worktree path should be utf-8");
        run_git(&repo, &["worktree", "remove", worktree_arg]);
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let review =
            get_agent_conversation_workspace_review(app.state(), conversation_id.as_str()).await;
        let review = review.expect("review payload should load from the local branch");
        assert_eq!(review.head_ref, branch_name);
        assert!(!review.supports_worktree_modes);
        assert!(review
            .changes
            .iter()
            .any(|change| change.path == "src/lib.rs"));
        assert!(review.commits.iter().any(|commit| commit.sha == commit_sha));

        let file_diff = get_agent_conversation_workspace_file_diff(
            app.state(),
            conversation_id.as_str(),
            "src/lib.rs".to_string(),
        )
        .await
        .expect("workspace file diff should load from branch target");
        assert!(
            file_diff
                .hunks
                .iter()
                .flat_map(|h| h.lines.iter())
                .any(|line| line.content.contains("answer")),
            "file diff hunks should contain the branch change"
        );

        invalidate_agent_workspace_diff_caches(&conversation_id);
    }

    #[tokio::test]
    async fn agent_workspace_review_fetches_pr_head_after_worktree_and_branch_cleanup() {
        let (temp_dir, state, conversation_id, worktree_path, commit_sha) =
            create_agent_workspace_command_state().await;
        let repo = temp_dir.path().join("repo");
        let remote = temp_dir.path().join("origin.git");
        std::fs::create_dir_all(&remote).expect("remote dir should be created");
        run_git(&remote, &["init", "--bare"]);
        let remote_arg = remote.to_str().expect("test remote path should be utf-8");
        run_git(&repo, &["remote", "add", "origin", remote_arg]);
        run_git(
            &worktree_path,
            &["push", "origin", "HEAD:refs/pull/123/head"],
        );

        let mut workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        let branch_name = workspace.branch_name.clone();
        workspace.publication_pr_number = Some(123);
        workspace.publication_pr_status = Some("merged".to_string());
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace should be updated with publication metadata");

        let worktree_arg = worktree_path
            .to_str()
            .expect("test worktree path should be utf-8");
        run_git(&repo, &["worktree", "remove", worktree_arg]);
        run_git(&repo, &["branch", "-D", &branch_name]);

        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let review =
            get_agent_conversation_workspace_review(app.state(), conversation_id.as_str()).await;
        let review = review.expect("review payload should load from fetched PR head");
        assert_eq!(review.head_ref, "refs/ralphx/pr-heads/123");
        assert!(!review.supports_worktree_modes);
        assert!(review
            .changes
            .iter()
            .any(|change| change.path == "src/lib.rs"));
        assert!(review.commits.iter().any(|commit| commit.sha == commit_sha));
        assert!(
            !GitService::branch_exists(&repo, &branch_name)
                .await
                .expect("branch existence check should succeed"),
            "review fallback should not recreate the deleted workspace branch"
        );

        invalidate_agent_workspace_diff_caches(&conversation_id);
    }

    #[tokio::test]
    async fn agent_workspace_review_uses_github_patch_after_git_refs_are_unavailable() {
        let (temp_dir, mut state, conversation_id, worktree_path, _commit_sha) =
            create_agent_workspace_command_state().await;
        let repo = temp_dir.path().join("repo");
        let patch = "\
diff --git a/src/lib.rs b/src/lib.rs
new file mode 100644
--- /dev/null
+++ b/src/lib.rs
@@ -0,0 +1,1 @@
+pub fn answer() -> u8 { 42 }
";
        let github = Arc::new(MockGithubService::new());
        github.state().get_pr_diff_patch_result = Some(Ok(patch.to_string()));
        let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
        state.github_service = Some(github_trait);

        let mut workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        let branch_name = workspace.branch_name.clone();
        workspace.publication_pr_number = Some(123);
        workspace.publication_pr_url = Some("https://github.com/mock/project/pull/123".to_string());
        workspace.publication_pr_status = Some("merged".to_string());
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace should be updated with publication metadata");

        let worktree_arg = worktree_path
            .to_str()
            .expect("test worktree path should be utf-8");
        run_git(&repo, &["worktree", "remove", worktree_arg]);
        run_git(&repo, &["branch", "-D", &branch_name]);

        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let review =
            get_agent_conversation_workspace_review(app.state(), conversation_id.as_str()).await;
        let review = review.expect("review payload should load from GitHub patch");
        assert_eq!(review.head_ref, "github-pr-diff/123");
        assert!(!review.supports_worktree_modes);
        assert!(review.commits.is_empty());
        let change = review
            .changes
            .iter()
            .find(|change| change.path == "src/lib.rs")
            .expect("patch-backed change should be present");
        assert!(matches!(change.status, FileChangeStatus::Added));
        assert_eq!(change.additions, 1);

        let file_diff = get_agent_conversation_workspace_file_diff(
            app.state(),
            conversation_id.as_str(),
            "src/lib.rs".to_string(),
        )
        .await
        .expect("workspace file diff should load from GitHub patch");
        assert!(file_diff
            .hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .any(|line| line.content.contains("answer")));

        let range_result = get_agent_conversation_workspace_file_content_range(
            app.state(),
            conversation_id.as_str(),
            DiffSide::New,
            "src/lib.rs".to_string(),
            DiffRefKind::CumulativeHead,
            1,
            1,
        )
        .await;
        assert!(
            range_result
                .unwrap_err()
                .to_string()
                .contains("patch-backed"),
            "patch-backed diffs do not have a full local file source for lazy context ranges"
        );

        let state = github.state();
        assert_eq!(state.get_pr_diff_patch_calls, 1);
        assert_eq!(state.last_get_pr_diff_patch_number, Some(123));
        assert_eq!(
            state.last_get_pr_diff_patch_url.as_deref(),
            Some("https://github.com/mock/project/pull/123")
        );

        invalidate_agent_workspace_diff_caches(&conversation_id);
    }

    #[tokio::test]
    async fn agent_workspace_review_for_context_collects_changes_and_commits_from_head() {
        let (_temp_dir, repo, base) = create_review_repo();

        let snapshot = get_agent_workspace_review_for_context(AgentWorkspaceContext {
            source: AgentWorkspaceContextSource::Worktree,
            working_path: repo,
            base_ref: base,
            diff_target: None,
            patch_diff: None,
            supports_worktree_modes: true,
            repair_state: None,
        })
        .await
        .expect("review payload should load");

        assert_eq!(snapshot.response.base_ref.len(), 40);
        assert_eq!(snapshot.response.head_ref, "HEAD");
        assert_eq!(snapshot.response.commits.len(), 1);
        assert_eq!(
            snapshot.response.commits[0].message,
            "Add source file".to_string()
        );
        let source_change = snapshot
            .response
            .changes
            .iter()
            .find(|change| change.path == "src/lib.rs")
            .expect("source file change should be present");
        assert!(matches!(source_change.status, FileChangeStatus::Added));
        assert_eq!(source_change.additions, 1);
    }

    #[tokio::test]
    async fn agent_workspace_review_for_context_uses_explicit_diff_target_branch() {
        let (_temp_dir, repo, base) = create_review_repo();
        run_git(&repo, &["checkout", "main"]);
        run_git(&repo, &["checkout", "-b", "feature/target-review"]);
        std::fs::write(repo.join("README.md"), "initial\nfeature\n").unwrap();
        run_git(&repo, &["add", "."]);
        run_git(&repo, &["commit", "-m", "Update readme on target branch"]);
        run_git(&repo, &["checkout", "main"]);

        let snapshot = get_agent_workspace_review_for_context(AgentWorkspaceContext {
            source: AgentWorkspaceContextSource::Worktree,
            working_path: repo,
            base_ref: base,
            diff_target: Some("feature/target-review".to_string()),
            patch_diff: None,
            supports_worktree_modes: false,
            repair_state: None,
        })
        .await
        .expect("targeted review payload should load");

        assert_eq!(snapshot.response.head_ref, "feature/target-review");
        assert!(snapshot
            .response
            .changes
            .iter()
            .any(|change| change.path == "README.md"));
        assert!(snapshot
            .response
            .commits
            .iter()
            .any(|commit| commit.message == "Update readme on target branch"));
    }

    // =========================================================================
    // Extension A — Staged / Unstaged command tests
    // =========================================================================

    async fn create_staged_unstaged_workspace_state(
    ) -> (TempDir, AppState, ChatConversationId, PathBuf) {
        use crate::application::agent_conversation_workspace::{
            prepare_agent_conversation_workspace, AgentConversationWorkspaceBaseSelection,
        };
        use crate::domain::entities::{
            AgentConversationWorkspaceMode, IdeationAnalysisBaseRefKind, Project,
        };

        let temp_dir = TempDir::new().expect("temp dir");
        let repo = temp_dir.path().join("repo");
        std::fs::create_dir_all(&repo).expect("create repo dir");
        run_git(&repo, &["init", "-b", "main"]);
        run_git(&repo, &["config", "user.email", "test@example.com"]);
        run_git(&repo, &["config", "user.name", "Test User"]);
        std::fs::write(repo.join("base.txt"), "base\n").unwrap();
        run_git(&repo, &["add", "."]);
        run_git(&repo, &["commit", "-m", "Initial commit"]);

        let mut project = Project::new(
            "Staged Unstaged Test".to_string(),
            repo.display().to_string(),
        );
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory =
            Some(temp_dir.path().join("worktrees").display().to_string());

        let conversation_id = test_conversation_id();
        let workspace = prepare_agent_conversation_workspace(
            &project,
            &conversation_id,
            AgentConversationWorkspaceMode::Edit,
            AgentConversationWorkspaceBaseSelection {
                kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
                branch_mode: None,
                base_ref: Some("main".to_string()),
                display_name: None,
                source_pull_request: None,
            },
        )
        .await
        .expect("workspace should be prepared");

        let worktree_path = PathBuf::from(&workspace.worktree_path);
        let state = AppState::new_test();
        state
            .project_repo
            .create(project)
            .await
            .expect("seed project");
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("seed workspace");

        (temp_dir, state, conversation_id, worktree_path)
    }

    fn git_dir_for_test(worktree_path: &Path) -> PathBuf {
        let git_path = worktree_path.join(".git");
        if git_path.is_file() {
            let content = std::fs::read_to_string(&git_path).expect(".git file should be readable");
            if let Some(path) = content.strip_prefix("gitdir: ") {
                let path = PathBuf::from(path.trim());
                return if path.is_absolute() {
                    path
                } else {
                    worktree_path.join(path)
                };
            }
        }
        git_path
    }

    fn create_rebase_marker_for_test(worktree_path: &Path) {
        let rebase_dir = git_dir_for_test(worktree_path).join("rebase-merge");
        std::fs::create_dir_all(rebase_dir).expect("rebase marker should be created");
    }

    async fn mark_workspace_needs_agent(
        state: &AppState,
        conversation_id: &ChatConversationId,
    ) -> AgentConversationWorkspace {
        let mut workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        workspace.publication_push_status = Some("needs_agent".to_string());
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("workspace should be updated");
        workspace
    }

    fn create_agent_workspace_merge_conflict(temp_dir: &TempDir, worktree_path: &Path) {
        let repo = temp_dir.path().join("repo");

        std::fs::write(worktree_path.join("base.txt"), "base\nours\n").unwrap();
        run_git(worktree_path, &["add", "base.txt"]);
        run_git(worktree_path, &["commit", "-m", "Update workspace side"]);

        std::fs::write(repo.join("base.txt"), "base\ntheirs\n").unwrap();
        run_git(&repo, &["add", "base.txt"]);
        run_git(&repo, &["commit", "-m", "Update base side"]);

        let output = Command::new("git")
            .args(["merge", "main"])
            .current_dir(worktree_path)
            .output()
            .expect("git merge should run");
        assert!(
            !output.status.success(),
            "merge should leave base.txt conflicted"
        );
    }

    #[tokio::test]
    async fn repair_change_summary_allows_needs_agent_detached_rebase_state() {
        let (_tmp, state, conversation_id, worktree_path) =
            create_staged_unstaged_workspace_state().await;
        invalidate_agent_workspace_diff_caches(&conversation_id);
        mark_workspace_needs_agent(&state, &conversation_id).await;
        run_git(&worktree_path, &["checkout", "--detach"]);
        create_rebase_marker_for_test(&worktree_path);

        std::fs::write(worktree_path.join("staged.txt"), "one\ntwo\n").unwrap();
        run_git(&worktree_path, &["add", "staged.txt"]);
        std::fs::write(worktree_path.join("base.txt"), "base\nunstaged\n").unwrap();

        let normal_summary =
            get_agent_conversation_workspace_change_summary_for_state(&state, &conversation_id)
                .await;
        assert!(
            normal_summary
                .expect_err("normal summary should remain strict while detached")
                .to_string()
                .contains("checked out at 'HEAD'"),
            "normal summary should preserve strict branch validation"
        );

        let repair_summary = get_agent_conversation_workspace_repair_change_summary_for_state(
            &state,
            &conversation_id,
        )
        .await
        .expect("repair summary should load for needs_agent rebase state");

        assert!(repair_summary.supports_worktree_modes);
        assert_eq!(repair_summary.staged.file_count, 1);
        assert_eq!(repair_summary.staged.additions, 2);
        assert_eq!(repair_summary.unstaged.file_count, 1);
        assert_eq!(repair_summary.unstaged.additions, 1);
        let repair_state = repair_summary
            .repair_state
            .expect("repair summary should include repair state");
        assert_eq!(repair_state.checked_out_branch, "HEAD");
        assert!(repair_state.rebase_in_progress);
        assert!(!repair_state.merge_in_progress);

        let staged = get_agent_conversation_workspace_repair_staged_file_changes_for_state(
            &state,
            &conversation_id,
        )
        .await
        .expect("repair staged files should load");
        assert!(staged.iter().any(|file| file.path == "staged.txt"));

        let unstaged_diff = get_agent_conversation_workspace_repair_unstaged_file_diff_for_state(
            &state,
            &conversation_id,
            "base.txt".to_string(),
        )
        .await
        .expect("repair unstaged diff should load");
        assert!(
            unstaged_diff
                .hunks
                .iter()
                .flat_map(|hunk| hunk.lines.iter())
                .any(|line| line.content.contains("unstaged")),
            "repair unstaged diff should include disk-only content"
        );

        invalidate_agent_workspace_diff_caches(&conversation_id);
    }

    #[tokio::test]
    async fn repair_conflict_file_diff_command_reads_unmerged_index_stages() {
        let (tmp, state, conversation_id, worktree_path) =
            create_staged_unstaged_workspace_state().await;
        invalidate_agent_workspace_diff_caches(&conversation_id);
        mark_workspace_needs_agent(&state, &conversation_id).await;
        create_agent_workspace_merge_conflict(&tmp, &worktree_path);

        let summary = get_agent_conversation_workspace_repair_change_summary_for_state(
            &state,
            &conversation_id,
        )
        .await
        .expect("repair summary should load");
        assert_eq!(
            summary
                .conflicted
                .as_ref()
                .expect("conflict summary should be present")
                .files,
            vec!["base.txt".to_string()]
        );

        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app");

        let conflict_diff = get_agent_conversation_workspace_repair_conflict_file_diff(
            app.state(),
            conversation_id.as_str(),
            "base.txt".to_string(),
        )
        .await
        .expect("repair conflict diff command should load");

        assert_eq!(conflict_diff.file_path, "base.txt");
        assert_eq!(conflict_diff.base_content, "base\n");
        assert_eq!(conflict_diff.ours_content, "base\nours\n");
        assert_eq!(conflict_diff.theirs_content, "base\ntheirs\n");
        assert!(conflict_diff.merged_with_markers.contains("<<<<<<<"));

        invalidate_agent_workspace_diff_caches(&conversation_id);
    }

    #[tokio::test]
    async fn repair_commands_return_summary_lists_and_diffs() {
        let (_tmp, state, conversation_id, worktree_path) =
            create_staged_unstaged_workspace_state().await;
        invalidate_agent_workspace_diff_caches(&conversation_id);
        mark_workspace_needs_agent(&state, &conversation_id).await;

        std::fs::write(worktree_path.join("staged.txt"), "one\ntwo\n").unwrap();
        run_git(&worktree_path, &["add", "staged.txt"]);
        std::fs::write(worktree_path.join("base.txt"), "base\nunstaged\n").unwrap();

        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app");

        let summary = get_agent_conversation_workspace_repair_change_summary(
            app.state(),
            conversation_id.as_str(),
        )
        .await
        .expect("repair summary command should load");
        assert!(summary.supports_worktree_modes);
        assert_eq!(summary.staged.file_count, 1);
        assert_eq!(summary.unstaged.file_count, 1);

        let staged = get_agent_conversation_workspace_repair_staged_file_changes(
            app.state(),
            conversation_id.as_str(),
        )
        .await
        .expect("repair staged changes command should load");
        assert!(staged.iter().any(|file| file.path == "staged.txt"));

        let unstaged = get_agent_conversation_workspace_repair_unstaged_file_changes(
            app.state(),
            conversation_id.as_str(),
        )
        .await
        .expect("repair unstaged changes command should load");
        assert!(unstaged.iter().any(|file| file.path == "base.txt"));

        let staged_diff = get_agent_conversation_workspace_repair_staged_file_diff(
            app.state(),
            conversation_id.as_str(),
            "staged.txt".to_string(),
        )
        .await
        .expect("repair staged diff command should load");
        assert!(staged_diff
            .hunks
            .iter()
            .flat_map(|hunk| hunk.lines.iter())
            .any(|line| line.content.contains("two")));

        let unstaged_diff = get_agent_conversation_workspace_repair_unstaged_file_diff(
            app.state(),
            conversation_id.as_str(),
            "base.txt".to_string(),
        )
        .await
        .expect("repair unstaged diff command should load");
        assert!(unstaged_diff
            .hunks
            .iter()
            .flat_map(|hunk| hunk.lines.iter())
            .any(|line| line.content.contains("unstaged")));

        invalidate_agent_workspace_diff_caches(&conversation_id);
    }

    #[tokio::test]
    async fn repair_change_summary_rejects_detached_rebase_without_needs_agent() {
        let (_tmp, state, conversation_id, worktree_path) =
            create_staged_unstaged_workspace_state().await;
        invalidate_agent_workspace_diff_caches(&conversation_id);
        run_git(&worktree_path, &["checkout", "--detach"]);
        create_rebase_marker_for_test(&worktree_path);

        let result = get_agent_conversation_workspace_repair_change_summary_for_state(
            &state,
            &conversation_id,
        )
        .await;

        assert!(
            result
                .expect_err("repair summary should require needs_agent")
                .to_string()
                .contains("requires agent conversation workspace"),
            "repair summary should explain the needs_agent gate"
        );

        invalidate_agent_workspace_diff_caches(&conversation_id);
    }

    #[tokio::test]
    async fn repair_change_summary_rejects_detached_without_transient_repair_state() {
        let (_tmp, state, conversation_id, worktree_path) =
            create_staged_unstaged_workspace_state().await;
        invalidate_agent_workspace_diff_caches(&conversation_id);
        mark_workspace_needs_agent(&state, &conversation_id).await;
        run_git(&worktree_path, &["checkout", "--detach"]);

        let result = get_agent_conversation_workspace_repair_change_summary_for_state(
            &state,
            &conversation_id,
        )
        .await;

        assert!(
            result
                .expect_err("detached worktree without repair state should reject")
                .to_string()
                .contains("recognized repair state"),
            "repair summary should reject unrecognized detached state"
        );

        invalidate_agent_workspace_diff_caches(&conversation_id);
    }

    #[tokio::test]
    async fn repair_change_summary_rejects_mismatched_workspace_path() {
        let (tmp, state, conversation_id, _worktree_path) =
            create_staged_unstaged_workspace_state().await;
        invalidate_agent_workspace_diff_caches(&conversation_id);
        let mut workspace = mark_workspace_needs_agent(&state, &conversation_id).await;
        workspace.worktree_path = tmp
            .path()
            .join("wrong-worktree")
            .to_string_lossy()
            .to_string();
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace should be updated with mismatched path");

        let result = get_agent_conversation_workspace_repair_change_summary_for_state(
            &state,
            &conversation_id,
        )
        .await;

        assert!(
            result
                .expect_err("mismatched path should reject")
                .to_string()
                .contains("path mismatch"),
            "repair summary should validate the canonical workspace path"
        );

        invalidate_agent_workspace_diff_caches(&conversation_id);
    }

    #[tokio::test]
    async fn staged_file_changes_command_returns_staged_files() {
        let (_tmp, state, conversation_id, worktree_path) =
            create_staged_unstaged_workspace_state().await;
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app");

        // Stage a new file
        std::fs::write(worktree_path.join("staged.txt"), "staged\n").unwrap();
        run_git(&worktree_path, &["add", "staged.txt"]);

        // Write an unstaged file (not added)
        std::fs::write(worktree_path.join("unstaged.txt"), "unstaged\n").unwrap();

        let changes = get_agent_conversation_workspace_staged_file_changes(
            app.state(),
            conversation_id.as_str(),
        )
        .await
        .expect("staged changes should load");

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].path, "staged.txt");
        assert!(matches!(changes[0].status, FileChangeStatus::Added));
    }

    #[tokio::test]
    async fn unstaged_file_changes_command_returns_unstaged_files() {
        let (_tmp, state, conversation_id, worktree_path) =
            create_staged_unstaged_workspace_state().await;
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app");

        // Modify committed file without staging
        std::fs::write(worktree_path.join("base.txt"), "base\nmodified\n").unwrap();

        // Stage a separate file (should not appear in unstaged)
        std::fs::write(worktree_path.join("staged.txt"), "staged\n").unwrap();
        run_git(&worktree_path, &["add", "staged.txt"]);

        let changes = get_agent_conversation_workspace_unstaged_file_changes(
            app.state(),
            conversation_id.as_str(),
        )
        .await
        .expect("unstaged changes should load");

        assert!(
            changes.iter().any(|c| c.path == "base.txt"),
            "Modified base.txt should appear in unstaged changes"
        );
        assert!(
            !changes.iter().any(|c| c.path == "staged.txt"),
            "staged.txt should not appear in unstaged changes"
        );
    }

    #[tokio::test]
    async fn change_summary_command_returns_compact_staged_and_unstaged_totals() {
        let (_tmp, state, conversation_id, worktree_path) =
            create_staged_unstaged_workspace_state().await;
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app");

        std::fs::write(worktree_path.join("staged.txt"), "one\ntwo\n").unwrap();
        run_git(&worktree_path, &["add", "staged.txt"]);
        std::fs::write(worktree_path.join("base.txt"), "base\nunstaged\n").unwrap();

        let summary =
            get_agent_conversation_workspace_change_summary(app.state(), conversation_id.as_str())
                .await
                .expect("change summary should load");

        assert!(summary.supports_worktree_modes);
        assert_eq!(summary.staged.file_count, 1);
        assert_eq!(summary.staged.additions, 2);
        assert_eq!(summary.staged.deletions, 0);
        assert_eq!(summary.unstaged.file_count, 1);
        assert_eq!(summary.unstaged.additions, 1);
        assert_eq!(summary.unstaged.deletions, 0);
    }

    #[tokio::test]
    async fn change_summary_command_returns_empty_for_branch_backed_context() {
        let (_tmp, state, conversation_id, worktree_path) =
            create_staged_unstaged_workspace_state().await;
        ensure_agent_workspace_diff_cache_current(&state, &conversation_id)
            .await
            .expect("workspace cache version should load");
        store_agent_workspace_context(
            &conversation_id,
            AgentWorkspaceContextMode::Strict,
            &AgentWorkspaceContext {
                source: AgentWorkspaceContextSource::Worktree,
                working_path: worktree_path,
                base_ref: "HEAD".to_string(),
                diff_target: Some("agent-branch".to_string()),
                patch_diff: None,
                supports_worktree_modes: false,
                repair_state: None,
            },
        );
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app");

        let summary =
            get_agent_conversation_workspace_change_summary(app.state(), conversation_id.as_str())
                .await
                .expect("change summary should load");

        assert!(!summary.supports_worktree_modes);
        assert_eq!(summary.staged.file_count, 0);
        assert_eq!(summary.staged.additions, 0);
        assert_eq!(summary.staged.deletions, 0);
        assert_eq!(summary.unstaged.file_count, 0);
        assert_eq!(summary.unstaged.additions, 0);
        assert_eq!(summary.unstaged.deletions, 0);

        invalidate_agent_workspace_diff_caches(&conversation_id);
    }

    #[tokio::test]
    async fn staged_file_diff_command_returns_head_vs_index_content() {
        let (_tmp, state, conversation_id, worktree_path) =
            create_staged_unstaged_workspace_state().await;
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app");

        // Stage a modification to base.txt
        std::fs::write(worktree_path.join("base.txt"), "base\nstaged line\n").unwrap();
        run_git(&worktree_path, &["add", "base.txt"]);

        // Make a further unstaged change (should not appear in staged diff)
        std::fs::write(
            worktree_path.join("base.txt"),
            "base\nstaged line\nfurther\n",
        )
        .unwrap();

        let diff = get_agent_conversation_workspace_staged_file_diff(
            app.state(),
            conversation_id.as_str(),
            "base.txt".to_string(),
        )
        .await
        .expect("staged file diff should load");

        assert_eq!(diff.file_path, "base.txt");
        // Hunk-based: staged diff HEAD→index; "staged line" appears as an addition
        assert!(
            diff.hunks
                .iter()
                .flat_map(|h| h.lines.iter())
                .any(|l| l.content.contains("staged line")),
            "staged diff hunks should contain the staged addition"
        );
        // The unstaged disk change ("further") must NOT appear in the staged diff
        assert!(
            !diff
                .hunks
                .iter()
                .flat_map(|h| h.lines.iter())
                .any(|l| l.content.contains("further")),
            "staged diff should not include disk-only change"
        );
        // old_total_lines = HEAD version (1 line: "base\n"), new_total_lines = index (2 lines)
        assert_eq!(diff.old_total_lines, 1, "HEAD has 1 line");
        assert_eq!(diff.new_total_lines, 2, "index has 2 lines");
    }

    #[tokio::test]
    async fn unstaged_file_diff_command_returns_index_vs_disk_content() {
        let (_tmp, state, conversation_id, worktree_path) =
            create_staged_unstaged_workspace_state().await;
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app");

        // Stage a modification
        std::fs::write(worktree_path.join("base.txt"), "base\nstaged\n").unwrap();
        run_git(&worktree_path, &["add", "base.txt"]);

        // Further disk change (unstaged)
        std::fs::write(worktree_path.join("base.txt"), "base\nstaged\ndisk\n").unwrap();

        let diff = get_agent_conversation_workspace_unstaged_file_diff(
            app.state(),
            conversation_id.as_str(),
            "base.txt".to_string(),
        )
        .await
        .expect("unstaged file diff should load");

        assert_eq!(diff.file_path, "base.txt");
        // Hunk-based: unstaged diff index→disk; "disk" line appears as an addition
        assert!(
            diff.hunks
                .iter()
                .flat_map(|h| h.lines.iter())
                .any(|l| l.content.contains("disk")),
            "unstaged diff hunks should contain the disk-only addition"
        );
        // old_total_lines = index (2 lines), new_total_lines = disk (3 lines)
        assert_eq!(diff.old_total_lines, 2, "index has 2 lines");
        assert_eq!(diff.new_total_lines, 3, "disk has 3 lines");
    }

    // =========================================================================
    // Extension B — Cumulative command tests
    // =========================================================================

    #[tokio::test]
    async fn cumulative_file_changes_command_shows_all_commits_since_base() {
        let (_temp_dir, state, conversation_id, _worktree_path, _) =
            create_agent_workspace_command_state().await;
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app");

        let changes = get_agent_conversation_workspace_cumulative_file_changes(
            app.state(),
            conversation_id.as_str(),
        )
        .await
        .expect("cumulative file changes should load");

        assert!(
            changes.iter().any(|c| c.path == "src/lib.rs"),
            "Committed file should appear in cumulative changes"
        );
    }

    #[tokio::test]
    async fn cumulative_file_diff_command_shows_base_to_head_content() {
        let (_temp_dir, state, conversation_id, _worktree_path, _) =
            create_agent_workspace_command_state().await;
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app");

        let diff = get_agent_conversation_workspace_cumulative_file_diff(
            app.state(),
            conversation_id.as_str(),
            "src/lib.rs".to_string(),
        )
        .await
        .expect("cumulative file diff should load");

        assert_eq!(diff.file_path, "src/lib.rs");
        // Hunk-based: cumulative diff base→HEAD; "answer" fn appears as additions
        assert!(
            diff.hunks
                .iter()
                .flat_map(|h| h.lines.iter())
                .any(|l| l.content.contains("answer")),
            "cumulative diff hunks should contain the committed change"
        );
        // File did not exist at base, so old_total_lines = 0
        assert_eq!(
            diff.old_total_lines, 0,
            "File did not exist in base, so old_total_lines is 0"
        );
    }

    #[tokio::test]
    async fn cumulative_file_changes_for_merged_workspace_use_pr_head_merge_base() {
        let (temp_dir, state, conversation_id, worktree_path, _commit_sha) =
            create_agent_workspace_command_state().await;
        let repo = temp_dir.path().join("repo");
        let mut workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        let pr_head_ref = agent_workspace_pr_head_ref(243);
        let pr_head = run_git(&worktree_path, &["rev-parse", "HEAD"]);
        run_git(&worktree_path, &["update-ref", &pr_head_ref, &pr_head]);

        run_git(&repo, &["merge", "--squash", &pr_head_ref]);
        run_git(&repo, &["commit", "-m", "Squash PR 243"]);
        let squash_commit = run_git(&repo, &["rev-parse", "HEAD"]);
        run_git(
            &repo,
            &[
                "worktree",
                "remove",
                "--force",
                worktree_path.to_str().unwrap(),
            ],
        );

        workspace.base_commit = Some(squash_commit);
        workspace.publication_pr_number = Some(243);
        workspace.publication_pr_status = Some("merged".to_string());
        workspace.publication_push_status = Some("pushed".to_string());
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace should update");

        let review = get_agent_conversation_workspace_review_for_state(&state, &conversation_id)
            .await
            .expect("merged workspace review should use preserved PR head");
        assert_eq!(review.response.head_ref, pr_head_ref);
        assert!(
            review
                .response
                .changes
                .iter()
                .any(|change| change.path == "src/lib.rs"),
            "review changes should still show the published PR file"
        );

        let changes = get_agent_conversation_workspace_cumulative_file_changes_for_state(
            &state,
            &conversation_id,
        )
        .await
        .expect("merged workspace cumulative changes should use preserved PR head");

        assert!(
            changes.iter().any(|change| change.path == "src/lib.rs"),
            "cumulative changes should still show the published PR file"
        );

        let diff = get_agent_conversation_workspace_cumulative_file_diff_for_state(
            &state,
            &conversation_id,
            "src/lib.rs".to_string(),
        )
        .await
        .expect("merged workspace cumulative diff should use preserved PR head");

        assert!(
            diff.hunks
                .iter()
                .flat_map(|h| h.lines.iter())
                .any(|l| l.content.contains("answer")),
            "cumulative diff should still show the published PR hunk"
        );
    }

    // =========================================================================
    // Coverage: error branches in the 6 new workspace Tauri wrapper commands
    // =========================================================================

    /// Exercises the `Err` (warn!) branch in each of the 6 new workspace diff
    /// Tauri wrappers by passing a conversation_id that has no registered workspace.
    #[tokio::test]
    async fn workspace_diff_commands_error_branches_are_exercised() {
        let state = AppState::new_test();
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app");
        let unknown_id = test_conversation_id();

        // Each call should return Err (no workspace registered) — exercises warn! branch
        assert!(
            get_agent_conversation_workspace_staged_file_changes(app.state(), unknown_id.as_str())
                .await
                .is_err(),
            "staged_file_changes should error for unknown workspace"
        );
        assert!(
            get_agent_conversation_workspace_unstaged_file_changes(
                app.state(),
                unknown_id.as_str()
            )
            .await
            .is_err(),
            "unstaged_file_changes should error for unknown workspace"
        );
        assert!(
            get_agent_conversation_workspace_staged_file_diff(
                app.state(),
                unknown_id.as_str(),
                "any.rs".to_string(),
            )
            .await
            .is_err(),
            "staged_file_diff should error for unknown workspace"
        );
        assert!(
            get_agent_conversation_workspace_unstaged_file_diff(
                app.state(),
                unknown_id.as_str(),
                "any.rs".to_string(),
            )
            .await
            .is_err(),
            "unstaged_file_diff should error for unknown workspace"
        );
        assert!(
            get_agent_conversation_workspace_cumulative_file_changes(
                app.state(),
                unknown_id.as_str()
            )
            .await
            .is_err(),
            "cumulative_file_changes should error for unknown workspace"
        );
        assert!(
            get_agent_conversation_workspace_cumulative_file_diff(
                app.state(),
                unknown_id.as_str(),
                "any.rs".to_string(),
            )
            .await
            .is_err(),
            "cumulative_file_diff should error for unknown workspace"
        );
    }

    // =========================================================================
    // Coverage: file_content_range command + _for_state helper
    // =========================================================================

    #[tokio::test]
    async fn file_content_range_command_returns_lines_for_valid_request() {
        let (_tmp, state, conversation_id, worktree_path) =
            create_staged_unstaged_workspace_state().await;
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app");

        // base.txt was committed as "base\n" in the base repo; worktree has a copy
        // Stage a modification so HEAD → index differs
        std::fs::write(worktree_path.join("base.txt"), "base\nranged\n").unwrap();
        run_git(&worktree_path, &["add", "base.txt"]);

        // Range command reading new side of staged diff (reads from git index)
        let lines = get_agent_conversation_workspace_file_content_range(
            app.state(),
            conversation_id.as_str(),
            DiffSide::New,
            "base.txt".to_string(),
            DiffRefKind::Staged,
            1,
            2,
        )
        .await
        .expect("file content range should load");

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].line_num, 1);
        assert_eq!(lines[0].content, "base");
        assert_eq!(lines[1].line_num, 2);
        assert_eq!(lines[1].content, "ranged");

        let old_lines = get_agent_conversation_workspace_file_content_range(
            app.state(),
            conversation_id.as_str(),
            DiffSide::Old,
            "base.txt".to_string(),
            DiffRefKind::Staged,
            1,
            1,
        )
        .await
        .expect("staged old-side range should load from HEAD");

        assert_eq!(old_lines.len(), 1);
        assert_eq!(old_lines[0].line_num, 1);
        assert_eq!(old_lines[0].content, "base");
    }

    #[tokio::test]
    async fn file_content_range_command_reads_workspace_head_sides_from_base_and_disk() {
        let (_tmp, state, conversation_id, worktree_path) =
            create_staged_unstaged_workspace_state().await;
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app");

        std::fs::write(worktree_path.join("base.txt"), "disk-first\ndisk-only\n").unwrap();

        let old_lines = get_agent_conversation_workspace_file_content_range(
            app.state(),
            conversation_id.as_str(),
            DiffSide::Old,
            "base.txt".to_string(),
            DiffRefKind::Head,
            1,
            1,
        )
        .await
        .expect("workspace head old-side range should load from captured base");

        assert_eq!(old_lines.len(), 1);
        assert_eq!(old_lines[0].line_num, 1);
        assert_eq!(old_lines[0].content, "base");

        let lines = get_agent_conversation_workspace_file_content_range(
            app.state(),
            conversation_id.as_str(),
            DiffSide::New,
            "base.txt".to_string(),
            DiffRefKind::Head,
            2,
            2,
        )
        .await
        .expect("file content range should load");

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].line_num, 2);
        assert_eq!(lines[0].content, "disk-only");
    }

    #[tokio::test]
    async fn file_content_range_command_reads_unstaged_sides_from_index_and_disk() {
        let (_tmp, state, conversation_id, worktree_path) =
            create_staged_unstaged_workspace_state().await;
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app");

        std::fs::write(worktree_path.join("base.txt"), "base\nindex\n").unwrap();
        run_git(&worktree_path, &["add", "base.txt"]);
        std::fs::write(worktree_path.join("base.txt"), "base\nindex\ndisk\n").unwrap();

        let old_lines = get_agent_conversation_workspace_file_content_range(
            app.state(),
            conversation_id.as_str(),
            DiffSide::Old,
            "base.txt".to_string(),
            DiffRefKind::Unstaged,
            2,
            2,
        )
        .await
        .expect("unstaged old-side range should load from index");
        let new_lines = get_agent_conversation_workspace_file_content_range(
            app.state(),
            conversation_id.as_str(),
            DiffSide::New,
            "base.txt".to_string(),
            DiffRefKind::Unstaged,
            3,
            3,
        )
        .await
        .expect("unstaged new-side range should load from disk");

        assert_eq!(old_lines.len(), 1);
        assert_eq!(old_lines[0].content, "index");
        assert_eq!(new_lines.len(), 1);
        assert_eq!(new_lines[0].content, "disk");
    }

    #[tokio::test]
    async fn file_content_range_command_resolves_commit_and_cumulative_refs() {
        let (_temp_dir, state, conversation_id, _worktree_path, commit_sha) =
            create_agent_workspace_command_state().await;
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app");

        let base_lines = get_agent_conversation_workspace_file_content_range(
            app.state(),
            conversation_id.as_str(),
            DiffSide::New,
            "README.md".to_string(),
            DiffRefKind::CumulativeBase,
            1,
            1,
        )
        .await
        .expect("cumulative base range should resolve to base commit");
        let head_lines = get_agent_conversation_workspace_file_content_range(
            app.state(),
            conversation_id.as_str(),
            DiffSide::New,
            "src/lib.rs".to_string(),
            DiffRefKind::CumulativeHead,
            1,
            1,
        )
        .await
        .expect("cumulative head range should resolve to workspace HEAD");
        let commit_lines = get_agent_conversation_workspace_file_content_range(
            app.state(),
            conversation_id.as_str(),
            DiffSide::New,
            "src/lib.rs".to_string(),
            DiffRefKind::Commit { sha: commit_sha },
            1,
            1,
        )
        .await
        .expect("commit range should resolve to selected commit");

        assert_eq!(base_lines.len(), 1);
        assert_eq!(base_lines[0].content, "base");
        assert_eq!(head_lines.len(), 1);
        assert!(head_lines[0].content.contains("answer"));
        assert_eq!(commit_lines.len(), 1);
        assert!(commit_lines[0].content.contains("answer"));
    }

    #[tokio::test]
    async fn file_content_range_command_reads_cumulative_head_from_branch_target_context() {
        let (temp_dir, state, conversation_id, worktree_path, _commit_sha) =
            create_agent_workspace_command_state().await;
        let repo = temp_dir.path().join("repo");
        let worktree_arg = worktree_path
            .to_str()
            .expect("test worktree path should be utf-8");
        run_git(&repo, &["worktree", "remove", worktree_arg]);
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let lines = get_agent_conversation_workspace_file_content_range(
            app.state(),
            conversation_id.as_str(),
            DiffSide::New,
            "src/lib.rs".to_string(),
            DiffRefKind::CumulativeHead,
            1,
            1,
        )
        .await
        .expect("cumulative head range should load from branch target");

        assert_eq!(lines.len(), 1);
        assert!(lines[0].content.contains("answer"));
    }

    #[tokio::test]
    async fn file_content_range_command_error_branch_for_unknown_workspace() {
        let state = AppState::new_test();
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app");
        let unknown_id = test_conversation_id();

        // Should hit the warn! branch in the Tauri wrapper
        let result = get_agent_conversation_workspace_file_content_range(
            app.state(),
            unknown_id.as_str(),
            DiffSide::New,
            "any.rs".to_string(),
            DiffRefKind::Head,
            1,
            10,
        )
        .await;
        assert!(
            result.is_err(),
            "range command should error for unknown workspace"
        );
    }
}
