use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use chrono::{Duration as ChronoDuration, Utc};

use crate::application::agent_conversation_workspace::{
    expand_worktree_parent_public, resolve_agent_conversation_project_workspace_dir,
};
use crate::application::git_service::git_cmd::{self, GitCommandLane};
use crate::application::git_service::GitService;
use crate::domain::entities::{Project, ProjectId};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, OrphanWorktreeCleanupMarker,
    OrphanWorktreeCleanupMarkerKey, OrphanWorktreeCleanupMarkerRepository, ProjectRepository,
};
use crate::domain::services::RunningAgentRegistry;
use crate::infrastructure::agents::claude::git_runtime_config;

const RALPHX_BRANCH_PREFIX: &str = "ralphx/";
const AGENT_CONVERSATION_DIR_PREFIX: &str = "agent-conversation-";
const CLEANUP_STATUS_DIRTY: &str = "dirty";
const CLEANUP_STATUS_UNSAFE: &str = "unsafe";

#[derive(Debug, Default)]
pub(crate) struct OrphanCleanupStats {
    pub projects_seen: usize,
    pub projects_skipped_blocked: usize,
    pub worktrees_scanned: usize,
    pub directories_scanned: usize,
    pub db_matches: usize,
    pub db_missing_candidates: usize,
    pub unique_candidates: usize,
    pub duplicate_candidate_skips: usize,
    pub contained_removals: usize,
    pub dirty_skips: usize,
    pub unsafe_skips: usize,
    pub marker_skips: usize,
    pub dirty_markers_written: usize,
    pub unsafe_markers_written: usize,
    pub marker_read_failures: usize,
    pub marker_write_failures: usize,
    pub non_ralphx_skips: usize,
    pub branch_deletions: usize,
}

impl OrphanCleanupStats {
    pub(super) fn log_summary(&self, started_at: Instant, paused: bool) {
        tracing::info!(
            cleanup_scope = "orphan_agent_workspace_cleanup",
            paused,
            projects_seen = self.projects_seen,
            projects_skipped_blocked = self.projects_skipped_blocked,
            worktrees_scanned = self.worktrees_scanned,
            directories_scanned = self.directories_scanned,
            db_matches = self.db_matches,
            db_missing_candidates = self.db_missing_candidates,
            unique_candidates = self.unique_candidates,
            duplicate_candidate_skips = self.duplicate_candidate_skips,
            contained_removals = self.contained_removals,
            dirty_skips = self.dirty_skips,
            unsafe_skips = self.unsafe_skips,
            marker_skips = self.marker_skips,
            dirty_markers_written = self.dirty_markers_written,
            unsafe_markers_written = self.unsafe_markers_written,
            marker_read_failures = self.marker_read_failures,
            marker_write_failures = self.marker_write_failures,
            non_ralphx_skips = self.non_ralphx_skips,
            branch_deletions = self.branch_deletions,
            elapsed_ms = started_at.elapsed().as_millis(),
            "Orphan cleanup: startup local worktree cleanup summary"
        );
    }
}

pub(super) fn is_ralphx_owned_branch(branch: &str) -> bool {
    branch.starts_with(RALPHX_BRANCH_PREFIX)
}

pub(super) async fn resolve_target_ref_for_orphan(repo_path: &Path) -> String {
    if GitService::ref_exists(repo_path, "origin/main")
        .await
        .unwrap_or(false)
    {
        return "origin/main".to_string();
    }
    if GitService::ref_exists(repo_path, "origin/master")
        .await
        .unwrap_or(false)
    {
        return "origin/master".to_string();
    }
    if GitService::ref_exists(repo_path, "main")
        .await
        .unwrap_or(false)
    {
        return "main".to_string();
    }
    "master".to_string()
}

pub(crate) async fn cleanup_orphan_agent_worktrees_on_startup(
    project_repo: Arc<dyn ProjectRepository>,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    marker_repo: Arc<dyn OrphanWorktreeCleanupMarkerRepository>,
    blocked_git_project_ids: Arc<HashSet<ProjectId>>,
    running_agent_registry: Arc<dyn RunningAgentRegistry>,
) {
    let started_at = Instant::now();
    let mut stats = OrphanCleanupStats::default();

    let projects = match project_repo.get_all().await {
        Ok(projects) => projects,
        Err(error) => {
            tracing::warn!(
                error = %error,
                "Orphan cleanup: failed to list projects"
            );
            stats.log_summary(started_at, false);
            return;
        }
    };

    for project in projects {
        stats.projects_seen += 1;

        if blocked_git_project_ids.contains(&project.id) {
            stats.projects_skipped_blocked += 1;
            continue;
        }

        cleanup_project_orphan_worktrees(
            &project,
            &workspace_repo,
            &marker_repo,
            &running_agent_registry,
            &mut stats,
        )
        .await;
    }

    stats.log_summary(started_at, false);
}

pub(crate) async fn run_periodic_orphan_agent_worktree_cleanup(
    project_repo: Arc<dyn ProjectRepository>,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    marker_repo: Arc<dyn OrphanWorktreeCleanupMarkerRepository>,
    blocked_git_project_ids: Arc<HashSet<ProjectId>>,
    running_agent_registry: Arc<dyn RunningAgentRegistry>,
) {
    run_orphan_agent_worktree_cleanup_pass(
        Arc::clone(&project_repo),
        Arc::clone(&workspace_repo),
        Arc::clone(&marker_repo),
        Arc::clone(&blocked_git_project_ids),
        Arc::clone(&running_agent_registry),
    )
    .await;

    let interval_secs = git_runtime_config().orphan_worktree_cleanup_interval_secs;
    if interval_secs == 0 {
        return;
    }
    let interval = std::time::Duration::from_secs(interval_secs);
    loop {
        tokio::time::sleep(interval).await;
        run_orphan_agent_worktree_cleanup_pass(
            Arc::clone(&project_repo),
            Arc::clone(&workspace_repo),
            Arc::clone(&marker_repo),
            Arc::clone(&blocked_git_project_ids),
            Arc::clone(&running_agent_registry),
        )
        .await;
    }
}

pub(super) async fn run_orphan_agent_worktree_cleanup_pass(
    project_repo: Arc<dyn ProjectRepository>,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    marker_repo: Arc<dyn OrphanWorktreeCleanupMarkerRepository>,
    blocked_git_project_ids: Arc<HashSet<ProjectId>>,
    running_agent_registry: Arc<dyn RunningAgentRegistry>,
) {
    git_cmd::with_git_command_lane(GitCommandLane::Background, async move {
        cleanup_orphan_agent_worktrees_on_startup(
            project_repo,
            workspace_repo,
            marker_repo,
            blocked_git_project_ids,
            running_agent_registry,
        )
        .await;
    })
    .await;
}

pub(super) async fn cleanup_project_orphan_worktrees(
    project: &Project,
    workspace_repo: &Arc<dyn AgentConversationWorkspaceRepository>,
    marker_repo: &Arc<dyn OrphanWorktreeCleanupMarkerRepository>,
    running_agent_registry: &Arc<dyn RunningAgentRegistry>,
    stats: &mut OrphanCleanupStats,
) {
    let repo_path = Path::new(&project.working_directory);

    let worktree_parent = match expand_worktree_parent_public(project.worktree_parent_or_default())
    {
        Ok(p) => p,
        Err(error) => {
            tracing::debug!(
                project_id = project.id.as_str(),
                error = %error,
                "Orphan cleanup: failed to expand worktree parent"
            );
            return;
        }
    };

    let project_dir = match resolve_agent_conversation_project_workspace_dir(project) {
        Ok(path) => path,
        Err(error) => {
            tracing::debug!(
                project_id = project.id.as_str(),
                error = %error,
                "Orphan cleanup: failed to resolve canonical project directory"
            );
            return;
        }
    };

    if !is_under_worktree_parent(&project_dir, &worktree_parent) {
        tracing::warn!(
            project_id = project.id.as_str(),
            project_dir = %project_dir.display(),
            worktree_parent = %worktree_parent.display(),
            "Orphan cleanup: canonical project directory escaped worktree parent"
        );
        return;
    }

    if !project_dir.is_dir() {
        tracing::debug!(
            project_id = project.id.as_str(),
            project_dir = %project_dir.display(),
            "Orphan cleanup: canonical project directory is absent"
        );
        return;
    }

    let worktrees = match GitService::list_worktrees(repo_path).await {
        Ok(w) => w,
        Err(error) => {
            tracing::debug!(
                project_id = project.id.as_str(),
                error = %error,
                "Orphan cleanup: failed to list worktrees"
            );
            return;
        }
    };

    let local_branches = GitService::list_local_branch_names(repo_path)
        .await
        .unwrap_or_default();
    let target_ref = resolve_target_ref_for_orphan(repo_path).await;
    let mut processed_candidate_paths = HashSet::new();

    let known_workspace_paths: HashSet<String> = match workspace_repo
        .list_worktree_paths_by_project_id(&project.id)
        .await
    {
        Ok(paths) => paths
            .into_iter()
            .map(PathBuf::from)
            .map(|path| candidate_path_key(&path))
            .collect(),
        Err(error) => {
            tracing::debug!(
                project_id = project.id.as_str(),
                error = %error,
                "Orphan cleanup: failed to list workspace paths from DB"
            );
            return;
        }
    };

    for worktree in &worktrees {
        stats.worktrees_scanned += 1;

        let Some(branch) = worktree.branch.as_deref() else {
            continue;
        };

        if !is_ralphx_owned_branch(branch) {
            stats.non_ralphx_skips += 1;
            continue;
        }

        let worktree_path = PathBuf::from(&worktree.path);
        if candidate_is_busy(running_agent_registry, &worktree_path).await {
            continue;
        }
        if !is_current_project_agent_conversation_worktree(&worktree_path, &project_dir) {
            stats.non_ralphx_skips += 1;
            continue;
        }

        if known_workspace_paths.contains(&candidate_path_key(&worktree_path)) {
            stats.db_matches += 1;
            continue;
        }
        stats.db_missing_candidates += 1;
        if !record_candidate_path(&worktree_path, &mut processed_candidate_paths, stats) {
            continue;
        }

        try_cleanup_orphan_worktree(
            project,
            repo_path,
            &worktree_path,
            branch,
            worktree.head.as_deref(),
            &target_ref,
            &local_branches,
            marker_repo,
            stats,
        )
        .await;
    }

    scan_canonical_directories_with_seen(
        project,
        repo_path,
        &worktree_parent,
        &known_workspace_paths,
        &target_ref,
        &local_branches,
        running_agent_registry,
        marker_repo,
        &mut processed_candidate_paths,
        stats,
    )
    .await;
}

fn is_current_project_agent_conversation_worktree(
    worktree_path: &Path,
    project_dir: &Path,
) -> bool {
    let Some(parent) = worktree_path.parent() else {
        return false;
    };
    if !same_existing_path(parent, project_dir) {
        return false;
    }

    worktree_path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(AGENT_CONVERSATION_DIR_PREFIX))
}

#[cfg(test)]
pub(super) async fn scan_canonical_directories(
    project: &Project,
    repo_path: &Path,
    worktree_parent: &Path,
    known_workspace_paths: &HashSet<String>,
    target_ref: &str,
    local_branches: &HashSet<String>,
    running_agent_registry: &Arc<dyn RunningAgentRegistry>,
    marker_repo: &Arc<dyn OrphanWorktreeCleanupMarkerRepository>,
    stats: &mut OrphanCleanupStats,
) {
    let mut processed_candidate_paths = HashSet::new();
    scan_canonical_directories_with_seen(
        project,
        repo_path,
        worktree_parent,
        known_workspace_paths,
        target_ref,
        local_branches,
        running_agent_registry,
        marker_repo,
        &mut processed_candidate_paths,
        stats,
    )
    .await;
}

async fn scan_canonical_directories_with_seen(
    project: &Project,
    repo_path: &Path,
    worktree_parent: &Path,
    known_workspace_paths: &HashSet<String>,
    target_ref: &str,
    local_branches: &HashSet<String>,
    running_agent_registry: &Arc<dyn RunningAgentRegistry>,
    marker_repo: &Arc<dyn OrphanWorktreeCleanupMarkerRepository>,
    processed_candidate_paths: &mut HashSet<String>,
    stats: &mut OrphanCleanupStats,
) {
    let project_dir = match resolve_agent_conversation_project_workspace_dir(project) {
        Ok(path) => path,
        Err(error) => {
            tracing::debug!(
                project_id = project.id.as_str(),
                error = %error,
                "Orphan cleanup: failed to resolve canonical project directory"
            );
            return;
        }
    };

    if !is_under_worktree_parent(&project_dir, worktree_parent) {
        tracing::warn!(
            project_id = project.id.as_str(),
            project_dir = %project_dir.display(),
            worktree_parent = %worktree_parent.display(),
            "Orphan cleanup: canonical project directory escaped worktree parent"
        );
        return;
    }

    if !project_dir.is_dir() {
        tracing::debug!(
            project_id = project.id.as_str(),
            project_dir = %project_dir.display(),
            "Orphan cleanup: canonical project directory is absent"
        );
        return;
    }

    let conversation_dirs = match std::fs::read_dir(&project_dir) {
        Ok(entries) => entries,
        Err(error) => {
            tracing::debug!(
                project_id = project.id.as_str(),
                project_dir = %project_dir.display(),
                error = %error,
                "Orphan cleanup: failed to read canonical project directory"
            );
            return;
        }
    };

    for conv_entry in conversation_dirs.flatten() {
        let conv_name = conv_entry.file_name();
        let conv_name_str = conv_name.to_string_lossy();
        if !conv_name_str.starts_with(AGENT_CONVERSATION_DIR_PREFIX) {
            continue;
        }

        let conv_path = conv_entry.path();
        if !conv_path.is_dir() {
            continue;
        }

        stats.directories_scanned += 1;

        if candidate_is_busy(running_agent_registry, &conv_path).await {
            continue;
        }
        if known_workspace_paths.contains(&candidate_path_key(&conv_path)) {
            stats.db_matches += 1;
            continue;
        }

        let conv_path_key = candidate_path_key(&conv_path);
        if processed_candidate_paths.contains(&conv_path_key) {
            stats.db_missing_candidates += 1;
            stats.duplicate_candidate_skips += 1;
            continue;
        }

        let (branch, head_sha) = match detect_worktree_branch_and_head(&conv_path).await {
            Some(result) => result,
            None => continue,
        };

        if !is_ralphx_owned_branch(&branch) {
            stats.non_ralphx_skips += 1;
            continue;
        }

        stats.db_missing_candidates += 1;
        if !record_candidate_path(&conv_path, processed_candidate_paths, stats) {
            continue;
        }

        try_cleanup_orphan_worktree(
            project,
            repo_path,
            &conv_path,
            &branch,
            head_sha.as_deref(),
            target_ref,
            local_branches,
            marker_repo,
            stats,
        )
        .await;
    }
}

pub(super) async fn try_cleanup_orphan_worktree(
    project: &Project,
    repo_path: &Path,
    worktree_path: &Path,
    branch: &str,
    head_sha: Option<&str>,
    target_ref: &str,
    local_branches: &HashSet<String>,
    marker_repo: &Arc<dyn OrphanWorktreeCleanupMarkerRepository>,
    stats: &mut OrphanCleanupStats,
) {
    if !worktree_path.exists() {
        return;
    }

    let dirty_marker_key = cleanup_marker_key(
        &project.id,
        worktree_path,
        branch,
        head_sha,
        CLEANUP_STATUS_DIRTY,
        None,
    );
    if has_recent_cleanup_marker(marker_repo, &dirty_marker_key, stats).await {
        stats.dirty_skips += 1;
        stats.marker_skips += 1;
        tracing::debug!(
            path = %worktree_path.display(),
            branch,
            head_sha = ?head_sha,
            "Orphan cleanup: skipped recently dirty worktree"
        );
        return;
    }

    let unsafe_marker_key = cleanup_marker_key(
        &project.id,
        worktree_path,
        branch,
        head_sha,
        CLEANUP_STATUS_UNSAFE,
        Some(target_ref),
    );
    if has_recent_cleanup_marker(marker_repo, &unsafe_marker_key, stats).await {
        stats.unsafe_skips += 1;
        stats.marker_skips += 1;
        tracing::debug!(
            path = %worktree_path.display(),
            branch,
            head_sha = ?head_sha,
            target_ref,
            "Orphan cleanup: skipped recently unsafe worktree"
        );
        return;
    }

    if GitService::has_uncommitted_changes(worktree_path)
        .await
        .unwrap_or(true)
    {
        stats.dirty_skips += 1;
        mark_cleanup_marker(marker_repo, dirty_marker_key, stats).await;
        tracing::debug!(
            path = %worktree_path.display(),
            branch,
            "Orphan cleanup: skipped dirty worktree"
        );
        return;
    }

    let (is_contained, reason) =
        GitService::is_branch_merged_or_content_equivalent(repo_path, branch, target_ref).await;

    if !is_contained {
        stats.unsafe_skips += 1;
        mark_cleanup_marker(marker_repo, unsafe_marker_key, stats).await;
        tracing::debug!(
            path = %worktree_path.display(),
            branch,
            target_ref,
            reason,
            "Orphan cleanup: skipped non-contained branch"
        );
        return;
    }

    clear_cleanup_marker(marker_repo, &project.id, worktree_path, branch).await;

    let safe_path = match crate::utils::path_safety::validate_absolute_non_root_path(
        worktree_path,
        "orphan worktree cleanup",
    ) {
        Ok(p) => p,
        Err(error) => {
            tracing::warn!(
                path = %worktree_path.display(),
                error = %error,
                "Orphan cleanup: path safety validation failed"
            );
            return;
        }
    };

    if let Err(error) = GitService::delete_worktree(repo_path, &safe_path).await {
        tracing::warn!(
            path = %worktree_path.display(),
            branch,
            error = %error,
            "Orphan cleanup: failed to remove worktree"
        );
        return;
    }
    stats.contained_removals += 1;
    tracing::info!(
        path = %worktree_path.display(),
        branch,
        "Orphan cleanup: removed contained orphan worktree"
    );

    if local_branches.contains(branch) {
        if let Err(error) = GitService::delete_branch(repo_path, branch, true).await {
            tracing::warn!(
                branch,
                error = %error,
                "Orphan cleanup: failed to delete branch after worktree removal"
            );
        } else {
            stats.branch_deletions += 1;
            tracing::info!(branch, "Orphan cleanup: deleted contained orphan branch");
        }
    }
}

fn candidate_path_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}

fn same_existing_path(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn record_candidate_path(
    path: &Path,
    processed_candidate_paths: &mut HashSet<String>,
    stats: &mut OrphanCleanupStats,
) -> bool {
    if processed_candidate_paths.insert(candidate_path_key(path)) {
        stats.unique_candidates += 1;
        true
    } else {
        stats.duplicate_candidate_skips += 1;
        false
    }
}

#[cfg(test)]
pub(super) async fn detect_worktree_branch(worktree_path: &Path) -> Option<String> {
    detect_worktree_branch_and_head(worktree_path)
        .await
        .map(|(branch, _)| branch)
}

pub(super) async fn detect_worktree_branch_and_head(
    worktree_path: &Path,
) -> Option<(String, Option<String>)> {
    let head_file = worktree_path.join(".git");
    if !head_file.exists() {
        return None;
    }

    let branch_output = crate::application::git_service::git_cmd::run(
        &["rev-parse", "--abbrev-ref", "HEAD"],
        worktree_path,
    )
    .await
    .ok()?;

    if !branch_output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&branch_output.stdout);
    let branch = stdout.lines().next().unwrap_or_default().trim().to_string();
    if branch.is_empty() || branch == "HEAD" {
        None
    } else {
        let head = crate::application::git_service::git_cmd::run(
            &["rev-parse", "--verify", "HEAD"],
            worktree_path,
        )
        .await
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .map(str::trim)
                .filter(|head| !head.is_empty())
                .map(str::to_string)
        });
        Some((branch, head))
    }
}

fn cleanup_marker_key(
    project_id: &ProjectId,
    worktree_path: &Path,
    branch: &str,
    head_sha: Option<&str>,
    cleanup_status: &str,
    target_ref: Option<&str>,
) -> OrphanWorktreeCleanupMarkerKey {
    OrphanWorktreeCleanupMarkerKey {
        project_id: project_id.clone(),
        worktree_path: worktree_path.to_string_lossy().to_string(),
        branch_name: branch.to_string(),
        cleanup_status: cleanup_status.to_string(),
        head_sha: head_sha.map(str::to_string),
        target_ref: target_ref.map(str::to_string),
    }
}

fn cleanup_marker_retry_after() -> chrono::DateTime<Utc> {
    let retry_secs = i64::try_from(git_runtime_config().orphan_worktree_cleanup_marker_retry_secs)
        .unwrap_or(i64::MAX);
    Utc::now() - ChronoDuration::seconds(retry_secs)
}

async fn has_recent_cleanup_marker(
    marker_repo: &Arc<dyn OrphanWorktreeCleanupMarkerRepository>,
    key: &OrphanWorktreeCleanupMarkerKey,
    stats: &mut OrphanCleanupStats,
) -> bool {
    match marker_repo
        .has_recent_marker(key, cleanup_marker_retry_after())
        .await
    {
        Ok(found) => found,
        Err(error) => {
            stats.marker_read_failures += 1;
            tracing::debug!(
                branch = %key.branch_name,
                path = %key.worktree_path,
                status = %key.cleanup_status,
                error = %error,
                "Orphan cleanup: failed to read cleanup marker"
            );
            false
        }
    }
}

async fn mark_cleanup_marker(
    marker_repo: &Arc<dyn OrphanWorktreeCleanupMarkerRepository>,
    key: OrphanWorktreeCleanupMarkerKey,
    stats: &mut OrphanCleanupStats,
) {
    let cleanup_status = key.cleanup_status.clone();
    if let Err(error) = marker_repo
        .mark(OrphanWorktreeCleanupMarker {
            key: key.clone(),
            checked_at: Utc::now(),
        })
        .await
    {
        stats.marker_write_failures += 1;
        tracing::debug!(
            branch = %key.branch_name,
            path = %key.worktree_path,
            status = %key.cleanup_status,
            error = %error,
            "Orphan cleanup: failed to write cleanup marker"
        );
    } else {
        match cleanup_status.as_str() {
            CLEANUP_STATUS_DIRTY => stats.dirty_markers_written += 1,
            CLEANUP_STATUS_UNSAFE => stats.unsafe_markers_written += 1,
            _ => {}
        }
    }
}

async fn clear_cleanup_marker(
    marker_repo: &Arc<dyn OrphanWorktreeCleanupMarkerRepository>,
    project_id: &ProjectId,
    worktree_path: &Path,
    branch: &str,
) {
    let worktree_path = worktree_path.to_string_lossy().to_string();
    if let Err(error) = marker_repo
        .clear_for_worktree(project_id, &worktree_path, branch)
        .await
    {
        tracing::debug!(
            branch,
            path = %worktree_path,
            error = %error,
            "Orphan cleanup: failed to clear cleanup marker"
        );
    }
}

pub(super) fn is_under_worktree_parent(path: &Path, worktree_parent: &Path) -> bool {
    path.starts_with(worktree_parent)
}

pub(super) async fn candidate_is_busy(
    registry: &Arc<dyn RunningAgentRegistry>,
    candidate_path: &Path,
) -> bool {
    let candidate = candidate_path
        .canonicalize()
        .unwrap_or_else(|_| candidate_path.to_path_buf());
    registry
        .list_all()
        .await
        .into_iter()
        .filter_map(|(_, info)| info.worktree_path)
        .map(PathBuf::from)
        .map(|path| path.canonicalize().unwrap_or(path))
        .any(|path| path == candidate)
}
