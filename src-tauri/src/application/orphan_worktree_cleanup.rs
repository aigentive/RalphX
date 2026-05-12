use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use crate::application::agent_conversation_workspace::expand_worktree_parent_public;
use crate::application::git_service::GitService;
use crate::domain::entities::{Project, ProjectId};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, ProjectRepository,
};
use crate::domain::services::RunningAgentRegistry;

const RALPHX_BRANCH_PREFIX: &str = "ralphx/";
const AGENT_CONVERSATION_DIR_PREFIX: &str = "agent-conversation-";
const PROJECT_DIR_PREFIX: &str = "project-";

#[derive(Debug, Default)]
pub(crate) struct OrphanCleanupStats {
    pub projects_seen: usize,
    pub projects_skipped_blocked: usize,
    pub worktrees_scanned: usize,
    pub directories_scanned: usize,
    pub db_matches: usize,
    pub db_missing_candidates: usize,
    pub contained_removals: usize,
    pub dirty_skips: usize,
    pub unsafe_skips: usize,
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
            contained_removals = self.contained_removals,
            dirty_skips = self.dirty_skips,
            unsafe_skips = self.unsafe_skips,
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
    if GitService::ref_exists(repo_path, "origin/main").await.unwrap_or(false) {
        return "origin/main".to_string();
    }
    if GitService::ref_exists(repo_path, "origin/master").await.unwrap_or(false) {
        return "origin/master".to_string();
    }
    if GitService::ref_exists(repo_path, "main").await.unwrap_or(false) {
        return "main".to_string();
    }
    "master".to_string()
}

pub(crate) async fn cleanup_orphan_agent_worktrees_on_startup(
    project_repo: Arc<dyn ProjectRepository>,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
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

        if should_pause(&running_agent_registry).await {
            stats.log_summary(started_at, true);
            return;
        }

        if blocked_git_project_ids.contains(&project.id) {
            stats.projects_skipped_blocked += 1;
            continue;
        }

        cleanup_project_orphan_worktrees(
            &project,
            &workspace_repo,
            &running_agent_registry,
            &mut stats,
        )
        .await;
    }

    stats.log_summary(started_at, false);
}

pub(super) async fn cleanup_project_orphan_worktrees(
    project: &Project,
    workspace_repo: &Arc<dyn AgentConversationWorkspaceRepository>,
    running_agent_registry: &Arc<dyn RunningAgentRegistry>,
    stats: &mut OrphanCleanupStats,
) {
    let repo_path = Path::new(&project.working_directory);

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

    let worktree_parent = match expand_worktree_parent_public(
        project.worktree_parent_or_default(),
    ) {
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

    let local_branches = GitService::list_local_branch_names(repo_path)
        .await
        .unwrap_or_default();

    let known_workspace_paths = match workspace_repo
        .list_worktree_paths_by_project_id(&project.id)
        .await
    {
        Ok(paths) => paths,
        Err(error) => {
            tracing::debug!(
                project_id = project.id.as_str(),
                error = %error,
                "Orphan cleanup: failed to list workspace paths from DB"
            );
            HashSet::new()
        }
    };

    for worktree in &worktrees {
        stats.worktrees_scanned += 1;

        if should_pause(running_agent_registry).await {
            return;
        }

        let Some(branch) = worktree.branch.as_deref() else {
            continue;
        };

        if !is_ralphx_owned_branch(branch) {
            stats.non_ralphx_skips += 1;
            continue;
        }

        let worktree_path = PathBuf::from(&worktree.path);
        if !is_under_worktree_parent(&worktree_path, &worktree_parent) {
            stats.non_ralphx_skips += 1;
            continue;
        }

        if known_workspace_paths.contains(&worktree.path) {
            stats.db_matches += 1;
            continue;
        }
        stats.db_missing_candidates += 1;

        try_cleanup_orphan_worktree(
            repo_path,
            &worktree_path,
            branch,
            &local_branches,
            stats,
        )
        .await;
    }

    scan_canonical_directories(
        project,
        repo_path,
        &worktree_parent,
        &known_workspace_paths,
        &local_branches,
        running_agent_registry,
        stats,
    )
    .await;
}

pub(super) async fn scan_canonical_directories(
    project: &Project,
    repo_path: &Path,
    worktree_parent: &Path,
    known_workspace_paths: &HashSet<String>,
    local_branches: &HashSet<String>,
    running_agent_registry: &Arc<dyn RunningAgentRegistry>,
    stats: &mut OrphanCleanupStats,
) {
    let project_dirs = match std::fs::read_dir(worktree_parent) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in project_dirs.flatten() {
        let dir_name = entry.file_name();
        let dir_name_str = dir_name.to_string_lossy();
        if !dir_name_str.starts_with(PROJECT_DIR_PREFIX) {
            continue;
        }

        let project_dir = entry.path();
        if !project_dir.is_dir() {
            continue;
        }

        let conversation_dirs = match std::fs::read_dir(&project_dir) {
            Ok(entries) => entries,
            Err(_) => continue,
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

            if should_pause(running_agent_registry).await {
                return;
            }

            let conv_path_str = conv_path.to_string_lossy().to_string();
            if known_workspace_paths.contains(&conv_path_str) {
                stats.db_matches += 1;
                continue;
            }

            let branch = match detect_worktree_branch(&conv_path).await {
                Some(b) => b,
                None => continue,
            };

            if !is_ralphx_owned_branch(&branch) {
                stats.non_ralphx_skips += 1;
                continue;
            }

            stats.db_missing_candidates += 1;

            try_cleanup_orphan_worktree(
                repo_path,
                &conv_path,
                &branch,
                local_branches,
                stats,
            )
            .await;
        }
    }

    let _ = project;
}

pub(super) async fn try_cleanup_orphan_worktree(
    repo_path: &Path,
    worktree_path: &Path,
    branch: &str,
    local_branches: &HashSet<String>,
    stats: &mut OrphanCleanupStats,
) {
    if !worktree_path.exists() {
        return;
    }

    if GitService::has_uncommitted_changes(worktree_path)
        .await
        .unwrap_or(true)
    {
        stats.dirty_skips += 1;
        tracing::debug!(
            path = %worktree_path.display(),
            branch,
            "Orphan cleanup: skipped dirty worktree"
        );
        return;
    }

    let target_ref = resolve_target_ref_for_orphan(repo_path).await;
    let (is_contained, reason) =
        GitService::is_branch_merged_or_content_equivalent(repo_path, branch, &target_ref)
            .await;

    if !is_contained {
        stats.unsafe_skips += 1;
        tracing::debug!(
            path = %worktree_path.display(),
            branch,
            target_ref,
            reason,
            "Orphan cleanup: skipped non-contained branch"
        );
        return;
    }

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
            tracing::info!(
                branch,
                "Orphan cleanup: deleted contained orphan branch"
            );
        }
    }
}

pub(super) async fn detect_worktree_branch(worktree_path: &Path) -> Option<String> {
    let head_file = worktree_path.join(".git");
    if !head_file.exists() {
        return None;
    }

    let output = crate::application::git_service::git_cmd::run(
        &["rev-parse", "--abbrev-ref", "HEAD"],
        worktree_path,
    )
    .await
    .ok()?;

    if !output.status.success() {
        return None;
    }

    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() || branch == "HEAD" {
        None
    } else {
        Some(branch)
    }
}

pub(super) fn is_under_worktree_parent(path: &Path, worktree_parent: &Path) -> bool {
    path.starts_with(worktree_parent)
}

pub(super) async fn should_pause(registry: &Arc<dyn RunningAgentRegistry>) -> bool {
    !registry.list_all().await.is_empty()
}
