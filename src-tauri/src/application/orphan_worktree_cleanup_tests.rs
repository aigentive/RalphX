use std::collections::HashSet;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

use super::orphan_worktree_cleanup::{
    cleanup_project_orphan_worktrees, detect_worktree_branch, is_ralphx_owned_branch,
    is_under_worktree_parent, resolve_target_ref_for_orphan, scan_canonical_directories,
    should_pause, OrphanCleanupStats,
};
use crate::domain::entities::Project;
use crate::domain::services::running_agent_registry::MemoryRunningAgentRegistry;
use crate::infrastructure::memory::MemoryAgentConversationWorkspaceRepository;

fn run_git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = dir.path();
    std::fs::create_dir_all(repo).expect("create repo path");
    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "test@example.com"]);
    run_git(repo, &["config", "user.name", "Test User"]);
    run_git(repo, &["checkout", "-b", "main"]);
    std::fs::write(repo.join("README.md"), "base\n").expect("write readme");
    run_git(repo, &["add", "."]);
    run_git(repo, &["commit", "-m", "initial"]);
    dir
}

#[test]
fn is_ralphx_owned_branch_recognizes_ralphx_prefix() {
    assert!(is_ralphx_owned_branch("ralphx/my-project/agent-abc12345"));
    assert!(is_ralphx_owned_branch("ralphx/project/plan-feature"));
}

#[test]
fn is_ralphx_owned_branch_rejects_non_ralphx() {
    assert!(!is_ralphx_owned_branch("feature/my-branch"));
    assert!(!is_ralphx_owned_branch("main"));
    assert!(!is_ralphx_owned_branch("origin/ralphx/project/agent-x"));
}

#[test]
fn orphan_cleanup_stats_default_is_zero() {
    let stats = OrphanCleanupStats::default();
    assert_eq!(stats.projects_seen, 0);
    assert_eq!(stats.contained_removals, 0);
    assert_eq!(stats.dirty_skips, 0);
    assert_eq!(stats.non_ralphx_skips, 0);
    assert_eq!(stats.branch_deletions, 0);
    assert_eq!(stats.projects_skipped_blocked, 0);
    assert_eq!(stats.worktrees_scanned, 0);
    assert_eq!(stats.directories_scanned, 0);
    assert_eq!(stats.db_matches, 0);
    assert_eq!(stats.db_missing_candidates, 0);
    assert_eq!(stats.unsafe_skips, 0);
}

#[test]
fn log_summary_does_not_panic() {
    let mut stats = OrphanCleanupStats::default();
    stats.projects_seen = 3;
    stats.contained_removals = 1;
    stats.dirty_skips = 2;
    let started_at = Instant::now();
    stats.log_summary(started_at, false);
    stats.log_summary(started_at, true);
}

#[test]
fn is_under_worktree_parent_matches_child_path() {
    let parent = Path::new("/home/user/ralphx-worktrees");
    let child = Path::new("/home/user/ralphx-worktrees/project-abc/agent-conversation-123");
    assert!(is_under_worktree_parent(child, parent));
}

#[test]
fn is_under_worktree_parent_rejects_sibling() {
    let parent = Path::new("/home/user/ralphx-worktrees");
    let sibling = Path::new("/home/user/other-dir/project-abc");
    assert!(!is_under_worktree_parent(sibling, parent));
}

#[test]
fn is_under_worktree_parent_rejects_parent_path() {
    let parent = Path::new("/home/user/ralphx-worktrees/project-abc");
    let above = Path::new("/home/user/ralphx-worktrees");
    assert!(!is_under_worktree_parent(above, parent));
}

#[test]
fn is_under_worktree_parent_matches_exact() {
    let parent = Path::new("/home/user/ralphx-worktrees");
    assert!(is_under_worktree_parent(parent, parent));
}

#[tokio::test]
async fn should_pause_returns_false_when_no_agents() {
    let registry = Arc::new(MemoryRunningAgentRegistry::new());
    assert!(!should_pause(&(registry as Arc<dyn crate::domain::services::RunningAgentRegistry>)).await);
}

#[tokio::test]
async fn detect_worktree_branch_returns_branch_for_worktree() {
    let repo_dir = init_repo();
    let repo_path = repo_dir.path();

    run_git(
        repo_path,
        &["checkout", "-b", "ralphx/test/agent-detect"],
    );
    run_git(repo_path, &["checkout", "main"]);

    let worktree_dir = tempfile::tempdir().expect("worktree temp");
    let worktree_path = worktree_dir.path().join("detect-wt");
    run_git(
        repo_path,
        &[
            "worktree",
            "add",
            &worktree_path.to_string_lossy(),
            "ralphx/test/agent-detect",
        ],
    );

    let branch = detect_worktree_branch(&worktree_path).await;
    assert_eq!(branch.as_deref(), Some("ralphx/test/agent-detect"));
}

#[tokio::test]
async fn detect_worktree_branch_returns_none_for_nonexistent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let nonexistent = dir.path().join("no-such-dir");
    let branch = detect_worktree_branch(&nonexistent).await;
    assert!(branch.is_none());
}

#[tokio::test]
async fn detect_worktree_branch_returns_none_for_plain_dir() {
    let dir = tempfile::tempdir().expect("tempdir");
    let branch = detect_worktree_branch(dir.path()).await;
    assert!(branch.is_none());
}

#[tokio::test]
async fn resolve_target_ref_for_orphan_finds_main() {
    let repo_dir = init_repo();
    let target = resolve_target_ref_for_orphan(repo_dir.path()).await;
    assert_eq!(target, "main");
}

#[tokio::test]
async fn try_cleanup_skips_dirty_worktree() {
    let repo_dir = init_repo();
    let repo_path = repo_dir.path();

    run_git(repo_path, &["checkout", "-b", "ralphx/test-proj/agent-dirty"]);
    std::fs::write(repo_path.join("file.txt"), "work\n").expect("write");
    run_git(repo_path, &["add", "."]);
    run_git(repo_path, &["commit", "-m", "work"]);
    run_git(repo_path, &["checkout", "main"]);

    let worktree_dir = tempfile::tempdir().expect("worktree temp");
    let worktree_path = worktree_dir.path().join("dirty-wt");
    run_git(
        repo_path,
        &[
            "worktree",
            "add",
            &worktree_path.to_string_lossy(),
            "ralphx/test-proj/agent-dirty",
        ],
    );

    std::fs::write(worktree_path.join("uncommitted.txt"), "dirty\n").expect("dirty write");

    let local_branches = HashSet::from(["ralphx/test-proj/agent-dirty".to_string()]);
    let mut stats = OrphanCleanupStats::default();

    super::orphan_worktree_cleanup::try_cleanup_orphan_worktree(
        repo_path,
        &worktree_path,
        "ralphx/test-proj/agent-dirty",
        &local_branches,
        &mut stats,
    )
    .await;

    assert_eq!(stats.dirty_skips, 1);
    assert_eq!(stats.contained_removals, 0);
    assert!(worktree_path.exists());
}

#[tokio::test]
async fn try_cleanup_skips_non_contained_branch() {
    let repo_dir = init_repo();
    let repo_path = repo_dir.path();

    run_git(repo_path, &["checkout", "-b", "ralphx/test-proj/agent-ahead"]);
    std::fs::write(repo_path.join("ahead.txt"), "ahead\n").expect("write");
    run_git(repo_path, &["add", "."]);
    run_git(repo_path, &["commit", "-m", "ahead of main"]);
    run_git(repo_path, &["checkout", "main"]);

    let worktree_dir = tempfile::tempdir().expect("worktree temp");
    let worktree_path = worktree_dir.path().join("ahead-wt");
    run_git(
        repo_path,
        &[
            "worktree",
            "add",
            &worktree_path.to_string_lossy(),
            "ralphx/test-proj/agent-ahead",
        ],
    );

    let local_branches = HashSet::from(["ralphx/test-proj/agent-ahead".to_string()]);
    let mut stats = OrphanCleanupStats::default();

    super::orphan_worktree_cleanup::try_cleanup_orphan_worktree(
        repo_path,
        &worktree_path,
        "ralphx/test-proj/agent-ahead",
        &local_branches,
        &mut stats,
    )
    .await;

    assert_eq!(stats.unsafe_skips, 1);
    assert_eq!(stats.contained_removals, 0);
    assert!(worktree_path.exists());
}

#[tokio::test]
async fn try_cleanup_removes_contained_worktree_and_branch() {
    let repo_dir = init_repo();
    let repo_path = repo_dir.path();

    run_git(
        repo_path,
        &["checkout", "-b", "ralphx/test-proj/agent-merged"],
    );
    run_git(repo_path, &["checkout", "main"]);

    let worktree_dir = tempfile::tempdir().expect("worktree temp");
    let worktree_path = worktree_dir.path().join("merged-wt");
    run_git(
        repo_path,
        &[
            "worktree",
            "add",
            &worktree_path.to_string_lossy(),
            "ralphx/test-proj/agent-merged",
        ],
    );

    let local_branches = HashSet::from(["ralphx/test-proj/agent-merged".to_string()]);
    let mut stats = OrphanCleanupStats::default();

    super::orphan_worktree_cleanup::try_cleanup_orphan_worktree(
        repo_path,
        &worktree_path,
        "ralphx/test-proj/agent-merged",
        &local_branches,
        &mut stats,
    )
    .await;

    assert_eq!(stats.contained_removals, 1);
    assert_eq!(stats.branch_deletions, 1);
    assert!(!worktree_path.exists());

    let branch_check = Command::new("git")
        .args(["rev-parse", "--verify", "ralphx/test-proj/agent-merged"])
        .current_dir(repo_path)
        .output()
        .expect("git check");
    assert!(!branch_check.status.success());
}

#[tokio::test]
async fn try_cleanup_skips_nonexistent_worktree() {
    let repo_dir = init_repo();
    let repo_path = repo_dir.path();
    let nonexistent = repo_dir.path().join("no-such-worktree");

    let local_branches = HashSet::from(["ralphx/test/agent-gone".to_string()]);
    let mut stats = OrphanCleanupStats::default();

    super::orphan_worktree_cleanup::try_cleanup_orphan_worktree(
        repo_path,
        &nonexistent,
        "ralphx/test/agent-gone",
        &local_branches,
        &mut stats,
    )
    .await;

    assert_eq!(stats.contained_removals, 0);
    assert_eq!(stats.dirty_skips, 0);
    assert_eq!(stats.unsafe_skips, 0);
}

#[tokio::test]
async fn cleanup_project_orphan_worktrees_removes_orphan_and_skips_known() {
    let repo_dir = init_repo();
    let repo_path = repo_dir.path();

    run_git(
        repo_path,
        &["checkout", "-b", "ralphx/test/agent-orphan"],
    );
    run_git(repo_path, &["checkout", "main"]);

    run_git(
        repo_path,
        &["checkout", "-b", "ralphx/test/agent-known"],
    );
    run_git(repo_path, &["checkout", "main"]);

    let worktree_base = tempfile::tempdir().expect("worktree base");
    let worktree_base_canonical = std::fs::canonicalize(worktree_base.path()).expect("canonicalize");

    let orphan_path = worktree_base_canonical.join("orphan-wt");
    run_git(
        repo_path,
        &[
            "worktree",
            "add",
            &orphan_path.to_string_lossy(),
            "ralphx/test/agent-orphan",
        ],
    );

    let known_path = worktree_base_canonical.join("known-wt");
    run_git(
        repo_path,
        &[
            "worktree",
            "add",
            &known_path.to_string_lossy(),
            "ralphx/test/agent-known",
        ],
    );

    let mut project = Project::new(
        "test-project".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory =
        Some(worktree_base_canonical.to_string_lossy().to_string());

    let workspace_repo: Arc<dyn crate::domain::repositories::AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let registry: Arc<dyn crate::domain::services::RunningAgentRegistry> =
        Arc::new(MemoryRunningAgentRegistry::new());

    let known_path_str = known_path.to_string_lossy().to_string();
    {
        use crate::domain::entities::{
            AgentConversationWorkspace, AgentConversationWorkspaceMode,
            ChatConversationId, IdeationAnalysisBaseRefKind,
        };
        let workspace = AgentConversationWorkspace::new(
            ChatConversationId::new(),
            project.id.clone(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            None,
            None,
            "ralphx/test/agent-known".to_string(),
            known_path_str.clone(),
        );
        workspace_repo.create_or_update(workspace).await.unwrap();
    }

    let mut stats = OrphanCleanupStats::default();

    cleanup_project_orphan_worktrees(&project, &workspace_repo, &registry, &mut stats).await;

    assert!(stats.contained_removals >= 1, "at least the orphan contained worktree should be removed");
    assert!(!orphan_path.exists(), "orphan worktree directory should be removed");
}

#[tokio::test]
async fn scan_canonical_directories_finds_orphan_conversation_dirs() {
    let repo_dir = init_repo();
    let repo_path = repo_dir.path();

    run_git(
        repo_path,
        &["checkout", "-b", "ralphx/test/agent-scan-orphan"],
    );
    run_git(repo_path, &["checkout", "main"]);

    let worktree_parent = tempfile::tempdir().expect("worktree parent");
    let project_dir = worktree_parent.path().join("project-abc123");
    std::fs::create_dir_all(&project_dir).expect("create project dir");

    let conv_path = project_dir.join("agent-conversation-def456");
    run_git(
        repo_path,
        &[
            "worktree",
            "add",
            &conv_path.to_string_lossy(),
            "ralphx/test/agent-scan-orphan",
        ],
    );

    let mut project = Project::new(
        "test-scan".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory =
        Some(worktree_parent.path().to_string_lossy().to_string());

    let known_paths: HashSet<String> = HashSet::new();
    let local_branches = HashSet::from(["ralphx/test/agent-scan-orphan".to_string()]);
    let registry: Arc<dyn crate::domain::services::RunningAgentRegistry> =
        Arc::new(MemoryRunningAgentRegistry::new());
    let mut stats = OrphanCleanupStats::default();

    scan_canonical_directories(
        &project,
        repo_path,
        worktree_parent.path(),
        &known_paths,
        &local_branches,
        &registry,
        &mut stats,
    )
    .await;

    assert!(stats.directories_scanned >= 1, "should scan agent-conversation dirs");
    assert_eq!(stats.contained_removals, 1, "contained orphan should be removed");
    assert!(!conv_path.exists(), "orphan conversation dir should be removed");
}

#[tokio::test]
async fn scan_canonical_directories_skips_non_matching_dirs() {
    let worktree_parent = tempfile::tempdir().expect("worktree parent");

    let random_dir = worktree_parent.path().join("some-random-dir");
    std::fs::create_dir_all(&random_dir).expect("create random dir");

    let non_project = worktree_parent.path().join("not-a-project");
    std::fs::create_dir_all(&non_project).expect("create non-project dir");

    let repo_dir = init_repo();
    let project = Project::new(
        "test-skip".to_string(),
        repo_dir.path().to_string_lossy().to_string(),
    );

    let known_paths: HashSet<String> = HashSet::new();
    let local_branches: HashSet<String> = HashSet::new();
    let registry: Arc<dyn crate::domain::services::RunningAgentRegistry> =
        Arc::new(MemoryRunningAgentRegistry::new());
    let mut stats = OrphanCleanupStats::default();

    scan_canonical_directories(
        &project,
        repo_dir.path(),
        worktree_parent.path(),
        &known_paths,
        &local_branches,
        &registry,
        &mut stats,
    )
    .await;

    assert_eq!(stats.directories_scanned, 0, "non-matching dirs should not be scanned");
}

#[tokio::test]
async fn scan_canonical_directories_skips_known_paths() {
    let repo_dir = init_repo();
    let repo_path = repo_dir.path();

    run_git(repo_path, &["checkout", "-b", "ralphx/test/agent-known-scan"]);
    run_git(repo_path, &["checkout", "main"]);

    let worktree_parent = tempfile::tempdir().expect("worktree parent");
    let project_dir = worktree_parent.path().join("project-known123");
    std::fs::create_dir_all(&project_dir).expect("create project dir");

    let conv_path = project_dir.join("agent-conversation-known789");
    run_git(
        repo_path,
        &[
            "worktree",
            "add",
            &conv_path.to_string_lossy(),
            "ralphx/test/agent-known-scan",
        ],
    );

    let project = Project::new(
        "test-known-scan".to_string(),
        repo_path.to_string_lossy().to_string(),
    );

    let conv_path_str = conv_path.to_string_lossy().to_string();
    let known_paths: HashSet<String> = HashSet::from([conv_path_str]);
    let local_branches = HashSet::from(["ralphx/test/agent-known-scan".to_string()]);
    let registry: Arc<dyn crate::domain::services::RunningAgentRegistry> =
        Arc::new(MemoryRunningAgentRegistry::new());
    let mut stats = OrphanCleanupStats::default();

    scan_canonical_directories(
        &project,
        repo_path,
        worktree_parent.path(),
        &known_paths,
        &local_branches,
        &registry,
        &mut stats,
    )
    .await;

    assert!(stats.db_matches >= 1, "known path should be matched");
    assert_eq!(stats.contained_removals, 0, "known path should not be removed");
    assert!(conv_path.exists(), "known worktree should still exist");
}
