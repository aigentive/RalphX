use std::path::Path;
use std::process::Command;

use super::orphan_worktree_cleanup::{is_ralphx_owned_branch, OrphanCleanupStats};

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
}

#[tokio::test]
async fn try_cleanup_skips_dirty_worktree() {
    use std::collections::HashSet;

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
    use std::collections::HashSet;

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
    use std::collections::HashSet;

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
