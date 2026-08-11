use std::collections::HashSet;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

use super::orphan_worktree_cleanup::{
    candidate_is_busy, cleanup_orphan_agent_worktrees_on_startup, cleanup_project_orphan_worktrees,
    detect_worktree_branch, detect_worktree_branch_and_head, is_ralphx_owned_branch,
    is_under_worktree_parent, resolve_target_ref_for_orphan, scan_canonical_directories,
    OrphanCleanupStats,
};
use crate::application::agent_conversation_workspace::resolve_agent_conversation_project_workspace_dir;
use crate::domain::entities::Project;
use crate::domain::services::running_agent_registry::MemoryRunningAgentRegistry;
use crate::domain::services::RunningAgentRegistry;
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryOrphanWorktreeCleanupMarkerRepository,
    MemoryProjectRepository,
};

fn marker_repo() -> Arc<dyn crate::domain::repositories::OrphanWorktreeCleanupMarkerRepository> {
    Arc::new(MemoryOrphanWorktreeCleanupMarkerRepository::new())
}

async fn register_running_worktree(registry: &MemoryRunningAgentRegistry, worktree_path: &Path) {
    registry
        .register(
            crate::domain::services::running_agent_registry::RunningAgentKey {
                context_type: "task".to_string(),
                context_id: "task-busy".to_string(),
            },
            0,
            "conversation-busy".to_string(),
            "run-busy".to_string(),
            Some(worktree_path.to_string_lossy().to_string()),
            None,
        )
        .await;
}

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

fn project_with_worktree_parent(name: &str, repo_path: &Path, worktree_parent: &Path) -> Project {
    let mut project = Project::new(name.to_string(), repo_path.to_string_lossy().to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    project
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
    assert_eq!(stats.unique_candidates, 0);
    assert_eq!(stats.duplicate_candidate_skips, 0);
    assert_eq!(stats.unsafe_skips, 0);
    assert_eq!(stats.marker_skips, 0);
    assert_eq!(stats.dirty_markers_written, 0);
    assert_eq!(stats.unsafe_markers_written, 0);
    assert_eq!(stats.marker_read_failures, 0);
    assert_eq!(stats.marker_write_failures, 0);
}

#[test]
fn log_summary_does_not_panic() {
    let stats = OrphanCleanupStats {
        projects_seen: 3,
        contained_removals: 1,
        dirty_skips: 2,
        ..Default::default()
    };
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
async fn candidate_is_not_busy_when_no_agents() {
    let registry = Arc::new(MemoryRunningAgentRegistry::new());
    assert!(
        !candidate_is_busy(
            &(registry as Arc<dyn crate::domain::services::RunningAgentRegistry>),
            Path::new("/tmp/ralphx-candidate"),
        )
        .await
    );
}

#[tokio::test]
async fn detect_worktree_branch_returns_branch_for_worktree() {
    let repo_dir = init_repo();
    let repo_path = repo_dir.path();

    run_git(repo_path, &["checkout", "-b", "ralphx/test/agent-detect"]);
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

    let (branch, head_sha) = detect_worktree_branch_and_head(&worktree_path)
        .await
        .expect("branch and head should be detected");
    let expected_head = {
        let output = Command::new("git")
            .args(["rev-parse", "ralphx/test/agent-detect"])
            .current_dir(repo_path)
            .output()
            .expect("git rev-parse branch");
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };
    assert_eq!(branch, "ralphx/test/agent-detect");
    assert_eq!(head_sha.as_deref(), Some(expected_head.as_str()));
    assert_ne!(
        head_sha.as_deref(),
        Some("ralphx/test/agent-detect"),
        "marker head identity must be the commit SHA, not the branch name"
    );
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

    run_git(
        repo_path,
        &["checkout", "-b", "ralphx/test-proj/agent-dirty"],
    );
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
    let project = Project::new(
        "dirty-project".to_string(),
        repo_path.to_string_lossy().to_string(),
    );

    super::orphan_worktree_cleanup::try_cleanup_orphan_worktree(
        &project,
        repo_path,
        &worktree_path,
        "ralphx/test-proj/agent-dirty",
        None,
        "main",
        &local_branches,
        &marker_repo(),
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

    run_git(
        repo_path,
        &["checkout", "-b", "ralphx/test-proj/agent-ahead"],
    );
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
    let project = Project::new(
        "ahead-project".to_string(),
        repo_path.to_string_lossy().to_string(),
    );

    super::orphan_worktree_cleanup::try_cleanup_orphan_worktree(
        &project,
        repo_path,
        &worktree_path,
        "ralphx/test-proj/agent-ahead",
        None,
        "main",
        &local_branches,
        &marker_repo(),
        &mut stats,
    )
    .await;

    assert_eq!(stats.unsafe_skips, 1);
    assert_eq!(stats.contained_removals, 0);
    assert!(worktree_path.exists());
}

#[tokio::test]
async fn try_cleanup_skips_recent_unsafe_marker() {
    let repo_dir = init_repo();
    let repo_path = repo_dir.path();

    run_git(
        repo_path,
        &["checkout", "-b", "ralphx/test-proj/agent-marked-unsafe"],
    );
    std::fs::write(repo_path.join("unsafe.txt"), "ahead\n").expect("write");
    run_git(repo_path, &["add", "."]);
    run_git(repo_path, &["commit", "-m", "ahead of main"]);
    run_git(repo_path, &["checkout", "main"]);

    let worktree_dir = tempfile::tempdir().expect("worktree temp");
    let worktree_path = worktree_dir.path().join("marked-unsafe-wt");
    run_git(
        repo_path,
        &[
            "worktree",
            "add",
            &worktree_path.to_string_lossy(),
            "ralphx/test-proj/agent-marked-unsafe",
        ],
    );

    let head_sha = {
        let output = Command::new("git")
            .args(["rev-parse", "ralphx/test-proj/agent-marked-unsafe"])
            .current_dir(repo_path)
            .output()
            .expect("git rev-parse");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };
    let project = Project::new(
        "marked-unsafe-project".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    let marker_repo = marker_repo();
    marker_repo
        .mark(crate::domain::repositories::OrphanWorktreeCleanupMarker {
            key: crate::domain::repositories::OrphanWorktreeCleanupMarkerKey {
                project_id: project.id.clone(),
                worktree_path: worktree_path.to_string_lossy().to_string(),
                branch_name: "ralphx/test-proj/agent-marked-unsafe".to_string(),
                cleanup_status: "unsafe".to_string(),
                head_sha: Some(head_sha.clone()),
                target_ref: Some("main".to_string()),
            },
            checked_at: chrono::Utc::now(),
        })
        .await
        .unwrap();

    let local_branches = HashSet::from(["ralphx/test-proj/agent-marked-unsafe".to_string()]);
    let mut stats = OrphanCleanupStats::default();

    super::orphan_worktree_cleanup::try_cleanup_orphan_worktree(
        &project,
        repo_path,
        &worktree_path,
        "ralphx/test-proj/agent-marked-unsafe",
        Some(&head_sha),
        "main",
        &local_branches,
        &marker_repo,
        &mut stats,
    )
    .await;

    assert_eq!(stats.marker_skips, 1);
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
    let project = Project::new(
        "merged-project".to_string(),
        repo_path.to_string_lossy().to_string(),
    );

    super::orphan_worktree_cleanup::try_cleanup_orphan_worktree(
        &project,
        repo_path,
        &worktree_path,
        "ralphx/test-proj/agent-merged",
        None,
        "main",
        &local_branches,
        &marker_repo(),
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
    let project = Project::new(
        "gone-project".to_string(),
        repo_path.to_string_lossy().to_string(),
    );

    super::orphan_worktree_cleanup::try_cleanup_orphan_worktree(
        &project,
        repo_path,
        &nonexistent,
        "ralphx/test/agent-gone",
        None,
        "main",
        &local_branches,
        &marker_repo(),
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

    run_git(repo_path, &["checkout", "-b", "ralphx/test/agent-orphan"]);
    run_git(repo_path, &["checkout", "main"]);

    run_git(repo_path, &["checkout", "-b", "ralphx/test/agent-known"]);
    run_git(repo_path, &["checkout", "main"]);

    let worktree_base = tempfile::tempdir().expect("worktree base");
    let project = project_with_worktree_parent("test-project", repo_path, worktree_base.path());
    let project_dir =
        resolve_agent_conversation_project_workspace_dir(&project).expect("project dir");
    std::fs::create_dir_all(&project_dir).expect("create project dir");

    let orphan_path = project_dir.join("agent-conversation-orphan-wt");
    run_git(
        repo_path,
        &[
            "worktree",
            "add",
            &orphan_path.to_string_lossy(),
            "ralphx/test/agent-orphan",
        ],
    );

    let known_path = project_dir.join("agent-conversation-known-wt");
    run_git(
        repo_path,
        &[
            "worktree",
            "add",
            &known_path.to_string_lossy(),
            "ralphx/test/agent-known",
        ],
    );

    let workspace_repo: Arc<dyn crate::domain::repositories::AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let registry: Arc<dyn crate::domain::services::RunningAgentRegistry> =
        Arc::new(MemoryRunningAgentRegistry::new());

    let known_path_str = known_path.to_string_lossy().to_string();
    {
        use crate::domain::entities::{
            AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatConversationId,
            IdeationAnalysisBaseRefKind,
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
    let marker_repo = marker_repo();

    cleanup_project_orphan_worktrees(
        &project,
        &workspace_repo,
        &marker_repo,
        &registry,
        &mut stats,
    )
    .await;

    assert!(
        stats.contained_removals >= 1,
        "at least the orphan contained worktree should be removed"
    );
    assert!(
        !orphan_path.exists(),
        "orphan worktree directory should be removed"
    );
}

#[tokio::test]
async fn cleanup_project_orphan_worktrees_dedupes_registered_and_canonical_candidates() {
    let repo_dir = init_repo();
    let repo_path = repo_dir.path();

    run_git(repo_path, &["checkout", "-b", "ralphx/test/agent-dedupe"]);
    run_git(repo_path, &["checkout", "main"]);

    let worktree_base = tempfile::tempdir().expect("worktree base");
    let project = project_with_worktree_parent("test-dedupe", repo_path, worktree_base.path());
    let project_dir =
        resolve_agent_conversation_project_workspace_dir(&project).expect("project dir");
    std::fs::create_dir_all(&project_dir).expect("create project dir");

    let orphan_path = project_dir.join("agent-conversation-dedupe");
    run_git(
        repo_path,
        &[
            "worktree",
            "add",
            &orphan_path.to_string_lossy(),
            "ralphx/test/agent-dedupe",
        ],
    );
    std::fs::write(orphan_path.join("README.md"), "dirty\n").expect("dirty tracked file");

    let workspace_repo: Arc<dyn crate::domain::repositories::AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let registry: Arc<dyn crate::domain::services::RunningAgentRegistry> =
        Arc::new(MemoryRunningAgentRegistry::new());
    let marker_repo = marker_repo();
    let mut stats = OrphanCleanupStats::default();

    cleanup_project_orphan_worktrees(
        &project,
        &workspace_repo,
        &marker_repo,
        &registry,
        &mut stats,
    )
    .await;

    assert_eq!(
        stats.unique_candidates, 1,
        "the same orphan path should be a single cleanup candidate"
    );
    assert_eq!(
        stats.duplicate_candidate_skips, 1,
        "canonical scan should skip the already-processed worktree-list path"
    );
    assert_eq!(stats.dirty_skips, 1);
    assert_eq!(stats.dirty_markers_written, 1);
    assert!(orphan_path.exists(), "dirty worktree must not be removed");
}

#[tokio::test]
async fn cleanup_project_orphan_worktrees_ignores_task_worktrees_under_shared_parent() {
    let repo_dir = init_repo();
    let repo_path = repo_dir.path();

    run_git(repo_path, &["checkout", "-b", "ralphx/test/task-ignored"]);
    run_git(repo_path, &["checkout", "main"]);

    let worktree_parent = tempfile::tempdir().expect("worktree parent");
    let task_parent = worktree_parent.path().join("ralphx");
    std::fs::create_dir_all(&task_parent).expect("create task parent");
    let task_worktree = task_parent.join("task-ignored");
    run_git(
        repo_path,
        &[
            "worktree",
            "add",
            &task_worktree.to_string_lossy(),
            "ralphx/test/task-ignored",
        ],
    );

    let project = project_with_worktree_parent("test-project", repo_path, worktree_parent.path());
    let workspace_repo: Arc<dyn crate::domain::repositories::AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let registry: Arc<dyn crate::domain::services::RunningAgentRegistry> =
        Arc::new(MemoryRunningAgentRegistry::new());
    let mut stats = OrphanCleanupStats::default();
    let marker_repo = marker_repo();

    cleanup_project_orphan_worktrees(
        &project,
        &workspace_repo,
        &marker_repo,
        &registry,
        &mut stats,
    )
    .await;

    assert_eq!(
        stats.worktrees_scanned, 0,
        "absent agent-conversation project dir should skip registered worktree Git scans"
    );
    assert_eq!(
        stats.db_missing_candidates, 0,
        "task worktrees should not become agent-conversation orphan candidates"
    );
    assert_eq!(stats.contained_removals, 0);
    assert_eq!(stats.dirty_skips, 0);
    assert_eq!(stats.unsafe_skips, 0);
    assert!(
        task_worktree.exists(),
        "task worktree should not be touched by agent-conversation orphan cleanup"
    );

    let branch_check = Command::new("git")
        .args(["rev-parse", "--verify", "ralphx/test/task-ignored"])
        .current_dir(repo_path)
        .output()
        .expect("git branch check");
    assert!(branch_check.status.success());
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
    let project = project_with_worktree_parent("test-scan", repo_path, worktree_parent.path());
    let project_dir =
        resolve_agent_conversation_project_workspace_dir(&project).expect("project dir");
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
        "main",
        &local_branches,
        &registry,
        &marker_repo(),
        &mut stats,
    )
    .await;

    assert!(
        stats.directories_scanned >= 1,
        "should scan agent-conversation dirs"
    );
    assert_eq!(
        stats.contained_removals, 1,
        "contained orphan should be removed"
    );
    assert!(
        !conv_path.exists(),
        "orphan conversation dir should be removed"
    );
}

#[tokio::test]
async fn scan_canonical_directories_skips_non_matching_dirs() {
    let worktree_parent = tempfile::tempdir().expect("worktree parent");

    let random_dir = worktree_parent.path().join("some-random-dir");
    std::fs::create_dir_all(&random_dir).expect("create random dir");

    let non_project = worktree_parent.path().join("not-a-project");
    std::fs::create_dir_all(&non_project).expect("create non-project dir");

    let repo_dir = init_repo();
    let project =
        project_with_worktree_parent("test-skip", repo_dir.path(), worktree_parent.path());

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
        "main",
        &local_branches,
        &registry,
        &marker_repo(),
        &mut stats,
    )
    .await;

    assert_eq!(
        stats.directories_scanned, 0,
        "non-matching dirs should not be scanned"
    );
}

#[tokio::test]
async fn scan_canonical_directories_skips_known_paths() {
    let repo_dir = init_repo();
    let repo_path = repo_dir.path();

    run_git(
        repo_path,
        &["checkout", "-b", "ralphx/test/agent-known-scan"],
    );
    run_git(repo_path, &["checkout", "main"]);

    let worktree_parent = tempfile::tempdir().expect("worktree parent");
    let project =
        project_with_worktree_parent("test-known-scan", repo_path, worktree_parent.path());
    let project_dir =
        resolve_agent_conversation_project_workspace_dir(&project).expect("project dir");
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

    let known_path = conv_path
        .canonicalize()
        .unwrap_or_else(|_| conv_path.clone())
        .to_string_lossy()
        .to_string();
    let known_paths: HashSet<String> = HashSet::from([known_path]);
    let local_branches = HashSet::from(["ralphx/test/agent-known-scan".to_string()]);
    let registry: Arc<dyn crate::domain::services::RunningAgentRegistry> =
        Arc::new(MemoryRunningAgentRegistry::new());
    let mut stats = OrphanCleanupStats::default();

    scan_canonical_directories(
        &project,
        repo_path,
        worktree_parent.path(),
        &known_paths,
        "main",
        &local_branches,
        &registry,
        &marker_repo(),
        &mut stats,
    )
    .await;

    assert!(stats.db_matches >= 1, "known path should be matched");
    assert_eq!(
        stats.contained_removals, 0,
        "known path should not be removed"
    );
    assert!(conv_path.exists(), "known worktree should still exist");
}

#[tokio::test]
async fn scan_canonical_directories_scopes_to_current_project_dir() {
    let repo_a = init_repo();
    let repo_b = init_repo();
    let worktree_parent = tempfile::tempdir().expect("worktree parent");

    run_git(
        repo_a.path(),
        &["checkout", "-b", "ralphx/test/agent-project-a"],
    );
    run_git(repo_a.path(), &["checkout", "main"]);

    let project_a =
        project_with_worktree_parent("test-project-a", repo_a.path(), worktree_parent.path());
    let project_b =
        project_with_worktree_parent("test-project-b", repo_b.path(), worktree_parent.path());
    let project_a_dir =
        resolve_agent_conversation_project_workspace_dir(&project_a).expect("project A dir");
    std::fs::create_dir_all(&project_a_dir).expect("create project A dir");

    let conv_path = project_a_dir.join("agent-conversation-project-a");
    run_git(
        repo_a.path(),
        &[
            "worktree",
            "add",
            &conv_path.to_string_lossy(),
            "ralphx/test/agent-project-a",
        ],
    );

    let known_paths: HashSet<String> = HashSet::new();
    let local_branches: HashSet<String> = HashSet::new();
    let registry: Arc<dyn RunningAgentRegistry> = Arc::new(MemoryRunningAgentRegistry::new());
    let mut stats = OrphanCleanupStats::default();

    scan_canonical_directories(
        &project_b,
        repo_b.path(),
        worktree_parent.path(),
        &known_paths,
        "main",
        &local_branches,
        &registry,
        &marker_repo(),
        &mut stats,
    )
    .await;

    assert_eq!(
        stats.directories_scanned, 0,
        "project B cleanup must not scan project A's canonical worktree dir"
    );
    assert!(
        conv_path.exists(),
        "project A worktree should be untouched by project B cleanup"
    );
}

#[tokio::test]
async fn startup_cleanup_skips_blocked_projects() {
    let repo_dir = init_repo();
    let repo_path = repo_dir.path();

    let mut project = Project::new(
        "blocked-project".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(repo_path.to_string_lossy().to_string());

    let project_repo = Arc::new(MemoryProjectRepository::with_projects(
        vec![project.clone()],
    ));
    let workspace_repo: Arc<dyn crate::domain::repositories::AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let registry: Arc<dyn RunningAgentRegistry> = Arc::new(MemoryRunningAgentRegistry::new());

    let blocked = Arc::new(HashSet::from([project.id.clone()]));

    cleanup_orphan_agent_worktrees_on_startup(
        project_repo,
        workspace_repo,
        marker_repo(),
        blocked,
        registry,
    )
    .await;
}

#[tokio::test]
async fn startup_cleanup_with_empty_projects() {
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let workspace_repo: Arc<dyn crate::domain::repositories::AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let registry: Arc<dyn RunningAgentRegistry> = Arc::new(MemoryRunningAgentRegistry::new());
    let blocked = Arc::new(HashSet::new());

    cleanup_orphan_agent_worktrees_on_startup(
        project_repo,
        workspace_repo,
        marker_repo(),
        blocked,
        registry,
    )
    .await;
}

#[tokio::test]
async fn orphan_cleanup_pass_runs_once_through_background_lane() {
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let workspace_repo: Arc<dyn crate::domain::repositories::AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let registry: Arc<dyn RunningAgentRegistry> = Arc::new(MemoryRunningAgentRegistry::new());
    let blocked = Arc::new(HashSet::new());

    super::orphan_worktree_cleanup::run_orphan_agent_worktree_cleanup_pass(
        project_repo,
        workspace_repo,
        marker_repo(),
        blocked,
        registry,
    )
    .await;
}

#[tokio::test]
async fn candidate_busy_check_ignores_unrelated_agent_and_matches_exact_worktree() {
    let registry = Arc::new(MemoryRunningAgentRegistry::new());
    let exact_path = "/tmp/ralphx-candidate-busy";
    register_running_worktree(&registry, Path::new(exact_path)).await;
    let reg: Arc<dyn RunningAgentRegistry> = registry;
    assert!(!candidate_is_busy(&reg, Path::new("/tmp/ralphx-unrelated")).await);
    assert!(candidate_is_busy(&reg, Path::new(exact_path)).await);
}

#[tokio::test]
async fn cleanup_project_skips_busy_registered_and_canonical_worktree_candidate() {
    let repo_dir = init_repo();
    let repo_path = repo_dir.path();

    run_git(repo_path, &["checkout", "-b", "ralphx/test/agent-busy"]);
    run_git(repo_path, &["checkout", "main"]);

    let worktree_base = tempfile::tempdir().expect("worktree base");
    let project = project_with_worktree_parent("test-busy", repo_path, worktree_base.path());
    let project_dir =
        resolve_agent_conversation_project_workspace_dir(&project).expect("project dir");
    std::fs::create_dir_all(&project_dir).expect("create project dir");
    let busy_path = project_dir.join("agent-conversation-busy-wt");
    run_git(
        repo_path,
        &[
            "worktree",
            "add",
            &busy_path.to_string_lossy(),
            "ralphx/test/agent-busy",
        ],
    );

    let workspace_repo: Arc<dyn crate::domain::repositories::AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let concrete_registry = Arc::new(MemoryRunningAgentRegistry::new());
    register_running_worktree(&concrete_registry, &busy_path).await;
    let registry: Arc<dyn RunningAgentRegistry> = concrete_registry;
    let mut stats = OrphanCleanupStats::default();

    cleanup_project_orphan_worktrees(
        &project,
        &workspace_repo,
        &marker_repo(),
        &registry,
        &mut stats,
    )
    .await;

    assert!(busy_path.exists(), "busy worktree must not be removed");
    assert_eq!(stats.contained_removals, 0);
    assert_eq!(
        stats.db_missing_candidates, 0,
        "busy paths are skipped before becoming cleanup candidates"
    );
    assert!(
        stats.worktrees_scanned >= 1,
        "registered worktree list should be inspected"
    );
    assert!(
        stats.directories_scanned >= 1,
        "canonical directory scan should see the same busy path"
    );
}

#[tokio::test]
async fn cleanup_project_fails_closed_when_known_workspace_lookup_fails() {
    let repo_dir = init_repo();
    let repo_path = repo_dir.path();
    let branch = "ralphx/test/agent-repo-failure";
    run_git(repo_path, &["checkout", "-b", branch]);
    run_git(repo_path, &["checkout", "main"]);

    let worktree_base = tempfile::tempdir().expect("worktree base");
    let project =
        project_with_worktree_parent("test-repo-failure", repo_path, worktree_base.path());
    let project_dir =
        resolve_agent_conversation_project_workspace_dir(&project).expect("project dir");
    std::fs::create_dir_all(&project_dir).expect("create project dir");
    let worktree_path = project_dir.join("agent-conversation-repo-failure");
    run_git(
        repo_path,
        &["worktree", "add", &worktree_path.to_string_lossy(), branch],
    );

    let concrete_workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    concrete_workspace_repo.fail_next_worktree_path_list("workspace lookup unavailable");
    let workspace_repo: Arc<dyn crate::domain::repositories::AgentConversationWorkspaceRepository> =
        concrete_workspace_repo;
    let registry: Arc<dyn RunningAgentRegistry> = Arc::new(MemoryRunningAgentRegistry::new());
    let mut stats = OrphanCleanupStats::default();

    cleanup_project_orphan_worktrees(
        &project,
        &workspace_repo,
        &marker_repo(),
        &registry,
        &mut stats,
    )
    .await;

    assert!(
        worktree_path.exists(),
        "repository uncertainty must preserve registered worktrees"
    );
    assert_eq!(stats.contained_removals, 0);
    assert_eq!(stats.db_missing_candidates, 0);
}

#[tokio::test]
async fn try_cleanup_removes_worktree_but_skips_branch_deletion_when_not_local() {
    let repo_dir = init_repo();
    let repo_path = repo_dir.path();

    run_git(
        repo_path,
        &["checkout", "-b", "ralphx/test-proj/agent-remote-only"],
    );
    run_git(repo_path, &["checkout", "main"]);

    let worktree_dir = tempfile::tempdir().expect("worktree temp");
    let worktree_path = worktree_dir.path().join("remote-only-wt");
    run_git(
        repo_path,
        &[
            "worktree",
            "add",
            &worktree_path.to_string_lossy(),
            "ralphx/test-proj/agent-remote-only",
        ],
    );

    let local_branches = HashSet::new();
    let mut stats = OrphanCleanupStats::default();
    let project = Project::new(
        "remote-only".to_string(),
        repo_path.to_string_lossy().to_string(),
    );

    super::orphan_worktree_cleanup::try_cleanup_orphan_worktree(
        &project,
        repo_path,
        &worktree_path,
        "ralphx/test-proj/agent-remote-only",
        None,
        "main",
        &local_branches,
        &marker_repo(),
        &mut stats,
    )
    .await;

    assert_eq!(stats.contained_removals, 1);
    assert_eq!(
        stats.branch_deletions, 0,
        "branch should not be deleted when not in local_branches"
    );
    assert!(!worktree_path.exists());
}

#[tokio::test]
async fn scan_canonical_directories_skips_non_ralphx_branches() {
    let repo_dir = init_repo();
    let repo_path = repo_dir.path();

    run_git(repo_path, &["checkout", "-b", "feature/not-ralphx"]);
    run_git(repo_path, &["checkout", "main"]);

    let worktree_parent = tempfile::tempdir().expect("worktree parent");
    let project =
        project_with_worktree_parent("test-non-ralphx", repo_path, worktree_parent.path());
    let project_dir =
        resolve_agent_conversation_project_workspace_dir(&project).expect("project dir");
    std::fs::create_dir_all(&project_dir).expect("create project dir");

    let conv_path = project_dir.join("agent-conversation-nonralphx");
    run_git(
        repo_path,
        &[
            "worktree",
            "add",
            &conv_path.to_string_lossy(),
            "feature/not-ralphx",
        ],
    );

    let known_paths: HashSet<String> = HashSet::new();
    let local_branches = HashSet::from(["feature/not-ralphx".to_string()]);
    let registry: Arc<dyn RunningAgentRegistry> = Arc::new(MemoryRunningAgentRegistry::new());
    let mut stats = OrphanCleanupStats::default();

    scan_canonical_directories(
        &project,
        repo_path,
        worktree_parent.path(),
        &known_paths,
        "main",
        &local_branches,
        &registry,
        &marker_repo(),
        &mut stats,
    )
    .await;

    assert!(
        stats.directories_scanned >= 1,
        "should scan the conversation dir"
    );
    assert_eq!(
        stats.non_ralphx_skips, 1,
        "non-ralphx branch should be skipped"
    );
    assert_eq!(
        stats.contained_removals, 0,
        "should not remove non-ralphx worktree"
    );
    assert!(conv_path.exists(), "non-ralphx worktree should still exist");
}

#[tokio::test]
async fn resolve_target_ref_for_orphan_falls_back_to_master() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo_path = dir.path();
    std::fs::create_dir_all(repo_path).expect("create repo path");
    run_git(repo_path, &["init"]);
    run_git(repo_path, &["config", "user.email", "test@example.com"]);
    run_git(repo_path, &["config", "user.name", "Test User"]);
    run_git(repo_path, &["checkout", "-b", "master"]);
    std::fs::write(repo_path.join("README.md"), "base\n").expect("write readme");
    run_git(repo_path, &["add", "."]);
    run_git(repo_path, &["commit", "-m", "initial"]);

    let target = resolve_target_ref_for_orphan(repo_path).await;
    assert_eq!(target, "master");
}

#[tokio::test]
async fn startup_cleanup_processes_multiple_projects() {
    let repo_dir1 = init_repo();
    let repo_dir2 = init_repo();

    let mut project1 = Project::new(
        "multi-project-1".to_string(),
        repo_dir1.path().to_string_lossy().to_string(),
    );
    project1.worktree_parent_directory = Some(repo_dir1.path().to_string_lossy().to_string());

    let mut project2 = Project::new(
        "multi-project-2".to_string(),
        repo_dir2.path().to_string_lossy().to_string(),
    );
    project2.worktree_parent_directory = Some(repo_dir2.path().to_string_lossy().to_string());

    let blocked = Arc::new(HashSet::from([project2.id.clone()]));

    let project_repo = Arc::new(MemoryProjectRepository::with_projects(vec![
        project1, project2,
    ]));
    let workspace_repo: Arc<dyn crate::domain::repositories::AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let registry: Arc<dyn RunningAgentRegistry> = Arc::new(MemoryRunningAgentRegistry::new());

    cleanup_orphan_agent_worktrees_on_startup(
        project_repo,
        workspace_repo,
        marker_repo(),
        blocked,
        registry,
    )
    .await;
}

#[tokio::test]
async fn scan_canonical_skips_file_entries_not_dirs() {
    let worktree_parent = tempfile::tempdir().expect("worktree parent");
    let repo_dir = init_repo();
    let project =
        project_with_worktree_parent("test-file-entry", repo_dir.path(), worktree_parent.path());
    let project_dir =
        resolve_agent_conversation_project_workspace_dir(&project).expect("project dir");
    std::fs::create_dir_all(&project_dir).expect("create project dir");

    std::fs::write(
        project_dir.join("agent-conversation-not-a-dir"),
        "just a file",
    )
    .expect("write file");

    let known_paths: HashSet<String> = HashSet::new();
    let local_branches: HashSet<String> = HashSet::new();
    let registry: Arc<dyn RunningAgentRegistry> = Arc::new(MemoryRunningAgentRegistry::new());
    let mut stats = OrphanCleanupStats::default();

    scan_canonical_directories(
        &project,
        repo_dir.path(),
        worktree_parent.path(),
        &known_paths,
        "main",
        &local_branches,
        &registry,
        &marker_repo(),
        &mut stats,
    )
    .await;

    assert_eq!(
        stats.directories_scanned, 0,
        "file entries should not be scanned as dirs"
    );
}

#[tokio::test]
async fn scan_canonical_skips_non_conversation_dirs_under_project() {
    let worktree_parent = tempfile::tempdir().expect("worktree parent");
    let repo_dir = init_repo();
    let project =
        project_with_worktree_parent("test-mixed", repo_dir.path(), worktree_parent.path());
    let project_dir =
        resolve_agent_conversation_project_workspace_dir(&project).expect("project dir");
    std::fs::create_dir_all(&project_dir).expect("create project dir");

    std::fs::create_dir_all(project_dir.join("not-a-conversation")).expect("create random dir");
    std::fs::create_dir_all(project_dir.join("some-other-thing")).expect("create random dir");

    let known_paths: HashSet<String> = HashSet::new();
    let local_branches: HashSet<String> = HashSet::new();
    let registry: Arc<dyn RunningAgentRegistry> = Arc::new(MemoryRunningAgentRegistry::new());
    let mut stats = OrphanCleanupStats::default();

    scan_canonical_directories(
        &project,
        repo_dir.path(),
        worktree_parent.path(),
        &known_paths,
        "main",
        &local_branches,
        &registry,
        &marker_repo(),
        &mut stats,
    )
    .await;

    assert_eq!(
        stats.directories_scanned, 0,
        "non-conversation dirs should be skipped"
    );
}

#[tokio::test]
async fn cleanup_project_handles_bad_working_directory() {
    let project = Project::new(
        "bad-dir".to_string(),
        "/nonexistent/path/to/repo".to_string(),
    );

    let workspace_repo: Arc<dyn crate::domain::repositories::AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let registry: Arc<dyn RunningAgentRegistry> = Arc::new(MemoryRunningAgentRegistry::new());
    let mut stats = OrphanCleanupStats::default();

    cleanup_project_orphan_worktrees(
        &project,
        &workspace_repo,
        &marker_repo(),
        &registry,
        &mut stats,
    )
    .await;

    assert_eq!(stats.worktrees_scanned, 0);
    assert_eq!(stats.contained_removals, 0);
}

#[tokio::test]
async fn scan_canonical_skips_conversation_dirs_without_git() {
    let worktree_parent = tempfile::tempdir().expect("worktree parent");
    let repo_dir = init_repo();
    let project =
        project_with_worktree_parent("test-nogit", repo_dir.path(), worktree_parent.path());
    let project_dir =
        resolve_agent_conversation_project_workspace_dir(&project).expect("project dir");
    std::fs::create_dir_all(&project_dir).expect("create project dir");

    let conv_dir = project_dir.join("agent-conversation-nogit123");
    std::fs::create_dir_all(&conv_dir).expect("create conversation dir");
    std::fs::write(conv_dir.join("some-file.txt"), "not a git worktree").expect("write file");

    let known_paths: HashSet<String> = HashSet::new();
    let local_branches: HashSet<String> = HashSet::new();
    let registry: Arc<dyn RunningAgentRegistry> = Arc::new(MemoryRunningAgentRegistry::new());
    let mut stats = OrphanCleanupStats::default();

    scan_canonical_directories(
        &project,
        repo_dir.path(),
        worktree_parent.path(),
        &known_paths,
        "main",
        &local_branches,
        &registry,
        &marker_repo(),
        &mut stats,
    )
    .await;

    assert!(stats.directories_scanned >= 1, "should scan the dir");
    assert_eq!(
        stats.contained_removals, 0,
        "non-git dirs should not be cleaned"
    );
    assert_eq!(
        stats.db_missing_candidates, 0,
        "no branch detected means no cleanup candidate"
    );
}

#[tokio::test]
async fn cleanup_project_handles_worktree_with_no_branch() {
    let repo_dir = init_repo();
    let repo_path = repo_dir.path();

    let worktree_parent = tempfile::tempdir().expect("worktree temp");
    let project = project_with_worktree_parent("test-detached", repo_path, worktree_parent.path());
    let project_dir =
        resolve_agent_conversation_project_workspace_dir(&project).expect("project dir");
    std::fs::create_dir_all(&project_dir).expect("create project dir");
    let worktree_path = project_dir.join("agent-conversation-detached-wt");

    let head_sha = {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo_path)
            .output()
            .expect("git rev-parse");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };

    run_git(
        repo_path,
        &[
            "worktree",
            "add",
            "--detach",
            &worktree_path.to_string_lossy(),
            &head_sha,
        ],
    );

    let workspace_repo: Arc<dyn crate::domain::repositories::AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let registry: Arc<dyn RunningAgentRegistry> = Arc::new(MemoryRunningAgentRegistry::new());
    let mut stats = OrphanCleanupStats::default();

    cleanup_project_orphan_worktrees(
        &project,
        &workspace_repo,
        &marker_repo(),
        &registry,
        &mut stats,
    )
    .await;

    assert!(
        stats.worktrees_scanned >= 1,
        "should scan worktrees including detached"
    );
    assert_eq!(
        stats.contained_removals, 0,
        "detached worktree has no branch, should be skipped"
    );
}

#[tokio::test]
async fn cleanup_project_skips_non_ralphx_worktrees() {
    let repo_dir = init_repo();
    let repo_path = repo_dir.path();

    run_git(repo_path, &["checkout", "-b", "feature/user-branch"]);
    run_git(repo_path, &["checkout", "main"]);

    let worktree_dir = tempfile::tempdir().expect("worktree temp");
    let worktree_parent_canonical =
        std::fs::canonicalize(worktree_dir.path()).expect("canonicalize");
    let project =
        project_with_worktree_parent("test-non-ralphx-wt", repo_path, &worktree_parent_canonical);
    let project_dir =
        resolve_agent_conversation_project_workspace_dir(&project).expect("project dir");
    std::fs::create_dir_all(&project_dir).expect("create project dir");
    let worktree_path = project_dir.join("agent-conversation-user-wt");
    run_git(
        repo_path,
        &[
            "worktree",
            "add",
            &worktree_path.to_string_lossy(),
            "feature/user-branch",
        ],
    );

    let workspace_repo: Arc<dyn crate::domain::repositories::AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let registry: Arc<dyn RunningAgentRegistry> = Arc::new(MemoryRunningAgentRegistry::new());
    let mut stats = OrphanCleanupStats::default();

    cleanup_project_orphan_worktrees(
        &project,
        &workspace_repo,
        &marker_repo(),
        &registry,
        &mut stats,
    )
    .await;

    assert!(
        stats.non_ralphx_skips >= 1,
        "non-ralphx branch should be skipped"
    );
    assert_eq!(stats.contained_removals, 0);
    assert!(
        worktree_path.exists(),
        "non-ralphx worktree should not be removed"
    );
}

#[tokio::test]
async fn cleanup_project_skips_worktree_outside_parent() {
    let repo_dir = init_repo();
    let repo_path = repo_dir.path();

    run_git(repo_path, &["checkout", "-b", "ralphx/test/agent-outside"]);
    run_git(repo_path, &["checkout", "main"]);

    let worktree_dir = tempfile::tempdir().expect("worktree temp");
    let worktree_path = worktree_dir.path().join("outside-wt");
    run_git(
        repo_path,
        &[
            "worktree",
            "add",
            &worktree_path.to_string_lossy(),
            "ralphx/test/agent-outside",
        ],
    );

    let different_parent = tempfile::tempdir().expect("different parent");
    let project =
        project_with_worktree_parent("test-outside-parent", repo_path, different_parent.path());
    let project_dir =
        resolve_agent_conversation_project_workspace_dir(&project).expect("project dir");
    std::fs::create_dir_all(&project_dir).expect("create project dir");

    let workspace_repo: Arc<dyn crate::domain::repositories::AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let registry: Arc<dyn RunningAgentRegistry> = Arc::new(MemoryRunningAgentRegistry::new());
    let mut stats = OrphanCleanupStats::default();

    cleanup_project_orphan_worktrees(
        &project,
        &workspace_repo,
        &marker_repo(),
        &registry,
        &mut stats,
    )
    .await;

    assert!(
        stats.non_ralphx_skips >= 1,
        "worktree outside parent should be skipped (counted as non_ralphx)"
    );
    assert_eq!(stats.contained_removals, 0);
    assert!(worktree_path.exists());
}

#[tokio::test]
async fn cleanup_project_matches_known_worktree_paths() {
    let repo_dir = init_repo();
    let repo_path = repo_dir.path();

    run_git(repo_path, &["checkout", "-b", "ralphx/test/agent-dbknown"]);
    run_git(repo_path, &["checkout", "main"]);

    let worktree_dir = tempfile::tempdir().expect("worktree temp");
    let worktree_parent_canonical =
        std::fs::canonicalize(worktree_dir.path()).expect("canonicalize");
    let project =
        project_with_worktree_parent("test-db-known", repo_path, &worktree_parent_canonical);
    let project_dir =
        resolve_agent_conversation_project_workspace_dir(&project).expect("project dir");
    std::fs::create_dir_all(&project_dir).expect("create project dir");
    let worktree_path = project_dir.join("agent-conversation-dbknown-wt");
    run_git(
        repo_path,
        &[
            "worktree",
            "add",
            &worktree_path.to_string_lossy(),
            "ralphx/test/agent-dbknown",
        ],
    );

    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    {
        use crate::domain::entities::{
            AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatConversationId,
            IdeationAnalysisBaseRefKind,
        };
        let workspace = AgentConversationWorkspace::new(
            ChatConversationId::new(),
            project.id.clone(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            None,
            None,
            "ralphx/test/agent-dbknown".to_string(),
            worktree_path.to_string_lossy().to_string(),
        );
        let ws_repo: &dyn crate::domain::repositories::AgentConversationWorkspaceRepository =
            workspace_repo.as_ref();
        ws_repo.create_or_update(workspace).await.unwrap();
    }

    let ws_repo: Arc<dyn crate::domain::repositories::AgentConversationWorkspaceRepository> =
        workspace_repo;
    let registry: Arc<dyn RunningAgentRegistry> = Arc::new(MemoryRunningAgentRegistry::new());
    let mut stats = OrphanCleanupStats::default();

    cleanup_project_orphan_worktrees(&project, &ws_repo, &marker_repo(), &registry, &mut stats)
        .await;

    assert!(
        stats.db_matches >= 1,
        "known workspace path should be matched"
    );
    assert_eq!(
        stats.contained_removals, 0,
        "known worktree should not be removed"
    );
    assert!(worktree_path.exists());
}

#[tokio::test]
async fn scan_canonical_nonexistent_parent_is_noop() {
    let repo_dir = init_repo();
    let nonexistent = Path::new("/tmp/ralphx-test-nonexistent-worktree-parent-12345");
    let project =
        project_with_worktree_parent("test-nonexistent-parent", repo_dir.path(), nonexistent);

    let known_paths: HashSet<String> = HashSet::new();
    let local_branches: HashSet<String> = HashSet::new();
    let registry: Arc<dyn RunningAgentRegistry> = Arc::new(MemoryRunningAgentRegistry::new());
    let mut stats = OrphanCleanupStats::default();

    scan_canonical_directories(
        &project,
        repo_dir.path(),
        nonexistent,
        &known_paths,
        "main",
        &local_branches,
        &registry,
        &marker_repo(),
        &mut stats,
    )
    .await;

    assert_eq!(stats.directories_scanned, 0);
    assert_eq!(stats.contained_removals, 0);
}
