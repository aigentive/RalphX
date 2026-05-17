use super::super::*;
use std::process::Command;

#[tokio::test]
async fn test_resolve_project_default_branch_prefers_configured_branch() {
    let temp_dir = tempfile::tempdir().unwrap();
    let branch = GitService::resolve_project_default_branch(temp_dir.path(), Some(" develop "))
        .await;

    assert_eq!(branch, "develop");
}

#[tokio::test]
async fn test_resolve_project_default_branch_detects_git_default_without_config() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("tracked.txt"), "initial").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();

    let branch = GitService::resolve_project_default_branch(repo, None).await;

    assert_eq!(branch, "main");
}

#[tokio::test]
async fn test_list_branches_normalizes_remote_and_worktree_markers() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path().join("repo");
    let remote = temp_dir.path().join("remote.git");
    let linked_worktree = temp_dir.path().join("linked-worktree");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(&remote).unwrap();
    super::init_test_repo(&repo);
    Command::new("git")
        .args(["init", "--bare"])
        .current_dir(&remote)
        .output()
        .unwrap();
    std::fs::write(repo.join("tracked.txt"), "initial").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(&repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(&repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["branch", "feature/worktree"])
        .current_dir(&repo)
        .output()
        .unwrap();
    Command::new("git")
        .args([
            "worktree",
            "add",
            linked_worktree.to_str().unwrap(),
            "feature/worktree",
        ])
        .current_dir(&repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["remote", "add", "origin", remote.to_str().unwrap()])
        .current_dir(&repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["push", "origin", "main"])
        .current_dir(&repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["checkout", "-b", "release/0.8"])
        .current_dir(&repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("release.txt"), "release").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(&repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "release"])
        .current_dir(&repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["push", "origin", "release/0.8"])
        .current_dir(&repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(&repo)
        .output()
        .unwrap();
    Command::new("git")
        .args([
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ])
        .current_dir(&repo)
        .output()
        .unwrap();

    let branches = GitService::list_branches(&repo).await.unwrap();

    assert_eq!(branches.first().map(String::as_str), Some("main"));
    assert!(branches.contains(&"feature/worktree".to_string()));
    assert!(branches.contains(&"release/0.8".to_string()));
    assert!(!branches.iter().any(|branch| branch.contains("HEAD")));
    assert!(!branches
        .iter()
        .any(|branch| branch.starts_with('*') || branch.starts_with('+')));
}

#[tokio::test]
async fn test_list_local_branch_names_excludes_remote_refs() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path().join("repo");
    let remote = temp_dir.path().join("remote.git");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(&remote).unwrap();
    super::init_test_repo(&repo);
    Command::new("git")
        .args(["init", "--bare"])
        .current_dir(&remote)
        .output()
        .unwrap();
    std::fs::write(repo.join("tracked.txt"), "initial").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(&repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(&repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["branch", "feature/local"])
        .current_dir(&repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["remote", "add", "origin", remote.to_str().unwrap()])
        .current_dir(&repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["push", "origin", "main"])
        .current_dir(&repo)
        .output()
        .unwrap();

    let branches = GitService::list_local_branch_names(&repo).await.unwrap();

    assert!(branches.contains("main"));
    assert!(branches.contains("feature/local"));
    assert!(!branches.contains("origin/main"));
}

// =========================================================================
// clean_working_tree Tests
// =========================================================================

#[tokio::test]
async fn test_clean_working_tree_removes_untracked_files() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    // Initialize repo
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Create initial commit
    std::fs::write(repo.join("tracked.txt"), "initial").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Create untracked files and directories
    std::fs::write(repo.join("untracked.txt"), "untracked").unwrap();
    std::fs::create_dir(repo.join("untracked_dir")).unwrap();
    std::fs::write(repo.join("untracked_dir/file.txt"), "content").unwrap();

    // Verify untracked files exist
    assert!(repo.join("untracked.txt").exists());
    assert!(repo.join("untracked_dir").exists());

    // Clean working tree
    GitService::clean_working_tree(repo).await.unwrap();

    // Verify untracked files are removed
    assert!(!repo.join("untracked.txt").exists());
    assert!(!repo.join("untracked_dir").exists());

    // Verify tracked files are preserved
    assert!(repo.join("tracked.txt").exists());
}

#[tokio::test]
async fn test_clean_working_tree_resets_modified_tracked_files() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    // Initialize repo
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Create initial commit
    std::fs::write(repo.join("tracked.txt"), "initial").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Modify tracked file
    std::fs::write(repo.join("tracked.txt"), "modified").unwrap();

    // Verify file is modified
    let content = std::fs::read_to_string(repo.join("tracked.txt")).unwrap();
    assert_eq!(content, "modified");

    // Clean working tree
    GitService::clean_working_tree(repo).await.unwrap();

    // Verify file is reset to HEAD
    let content = std::fs::read_to_string(repo.join("tracked.txt")).unwrap();
    assert_eq!(content, "initial");
}

#[tokio::test]
async fn test_clean_working_tree_noop_when_clean() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    // Initialize repo
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Create initial commit
    std::fs::write(repo.join("tracked.txt"), "content").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Working tree is clean, should be no-op
    let result = GitService::clean_working_tree(repo).await;

    // Should succeed without error
    assert!(result.is_ok());

    // Verify file still exists and is unchanged
    let content = std::fs::read_to_string(repo.join("tracked.txt")).unwrap();
    assert_eq!(content, "content");
}

#[tokio::test]
#[cfg(unix)]
async fn test_clean_working_tree_handles_symlinks() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    // Create a separate temp directory for the target file
    // so it doesn't get deleted when the repo temp_dir is dropped
    let target_temp = tempfile::tempdir().unwrap();
    let target_file = target_temp.path().join("target_file.txt");
    std::fs::write(&target_file, "target content").unwrap();

    // Initialize repo
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Create initial commit
    std::fs::write(repo.join("tracked.txt"), "initial").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Create a symlink in the repo pointing to the target file
    let symlink_path = repo.join("symlink.txt");
    std::os::unix::fs::symlink(&target_file, &symlink_path).unwrap();

    // Verify symlink exists
    assert!(symlink_path.exists());

    // Clean working tree
    GitService::clean_working_tree(repo).await.unwrap();

    // Verify symlink is removed
    assert!(!symlink_path.exists());

    // Verify target file is NOT removed (symlink was the link, not the target)
    assert!(target_file.exists());
    let content = std::fs::read_to_string(&target_file).unwrap();
    assert_eq!(content, "target content");

    // Keep target_temp alive until the end of the test
    drop(target_temp);
}

// =========================================================================
// Feature Branch Operations Tests (Phase 85)
// =========================================================================

#[tokio::test]
async fn test_create_feature_branch_success() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    // Initialize repo with initial commit
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("file.txt"), "content").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Create feature branch
    let result = GitService::create_feature_branch(repo, "ralphx/my-app/plan-abc123", "main").await;
    assert!(
        result.is_ok(),
        "create_feature_branch should succeed: {:?}",
        result.err()
    );

    // Verify branch exists
    let output = Command::new("git")
        .args(["branch", "--list", "ralphx/my-app/plan-abc123"])
        .current_dir(repo)
        .output()
        .unwrap();
    let branches = String::from_utf8_lossy(&output.stdout);
    assert!(
        branches.contains("ralphx/my-app/plan-abc123"),
        "Feature branch should exist"
    );

    // Verify we didn't checkout the branch (still on main)
    let current = GitService::get_current_branch(repo).await.unwrap();
    assert_eq!(
        current, "main",
        "Should still be on main after creating feature branch"
    );
}

#[tokio::test]
async fn test_create_feature_branch_from_specific_source() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    // Initialize repo with initial commit on main
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("file.txt"), "initial").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Add another commit on main
    std::fs::write(repo.join("file2.txt"), "second").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "second"])
        .current_dir(repo)
        .output()
        .unwrap();

    let main_sha = GitService::get_head_sha(repo).await.unwrap();

    // Create feature branch from main
    let result = GitService::create_feature_branch(repo, "feature/plan-test", "main").await;
    assert!(result.is_ok());

    // Verify feature branch points to same commit as main
    let output = Command::new("git")
        .args(["rev-parse", "feature/plan-test"])
        .current_dir(repo)
        .output()
        .unwrap();
    let feature_sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert_eq!(
        feature_sha, main_sha,
        "Feature branch should point to main HEAD"
    );
}

#[tokio::test]
async fn test_create_feature_branch_already_exists() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    // Initialize repo
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("file.txt"), "content").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Create branch first time
    GitService::create_feature_branch(repo, "feature/dup", "main")
        .await
        .unwrap();

    // Try to create again — should fail
    let result = GitService::create_feature_branch(repo, "feature/dup", "main").await;
    assert!(
        result.is_err(),
        "Creating duplicate feature branch should fail"
    );
}

#[tokio::test]
async fn test_create_feature_branch_invalid_source() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    // Initialize repo
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("file.txt"), "content").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Create from non-existent source branch
    let result = GitService::create_feature_branch(repo, "feature/bad", "nonexistent-branch").await;
    assert!(
        result.is_err(),
        "Creating from non-existent source should fail"
    );
}

#[tokio::test]
async fn test_delete_feature_branch_success() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    // Initialize repo
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("file.txt"), "content").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Create feature branch, then merge it back so -d works
    GitService::create_feature_branch(repo, "feature/to-delete", "main")
        .await
        .unwrap();

    // Delete it (safe delete — branch is fully merged since it's at same commit as main)
    let result = GitService::delete_feature_branch(repo, "feature/to-delete").await;
    assert!(
        result.is_ok(),
        "delete_feature_branch should succeed: {:?}",
        result.err()
    );

    // Verify branch no longer exists
    let output = Command::new("git")
        .args(["branch", "--list", "feature/to-delete"])
        .current_dir(repo)
        .output()
        .unwrap();
    let branches = String::from_utf8_lossy(&output.stdout);
    assert!(
        !branches.contains("feature/to-delete"),
        "Feature branch should be deleted"
    );
}

#[tokio::test]
async fn test_delete_feature_branch_nonexistent() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    // Initialize repo
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("file.txt"), "content").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Delete non-existent branch — should fail
    let result = GitService::delete_feature_branch(repo, "feature/nonexistent").await;
    assert!(result.is_err(), "Deleting non-existent branch should fail");
}

#[test]
fn test_is_branch_lock_error_already_used_by_worktree() {
    let error = AppError::GitOperation(
        "Failed to create worktree at '/tmp/merge-wt' for branch 'main': \
        fatal: 'main' is already used by worktree at '/home/user/project'"
            .to_string(),
    );
    assert!(GitService::is_branch_lock_error(&error));
}

#[test]
fn test_is_branch_lock_error_already_checked_out() {
    let error = AppError::GitOperation(
        "fatal: branch 'feature/foo' already checked out at '/tmp/worktree'".to_string(),
    );
    assert!(GitService::is_branch_lock_error(&error));
}

#[test]
fn test_is_branch_lock_error_is_already_checked_out_at() {
    let error = AppError::GitOperation(
        "fatal: 'main' is already checked out at '/home/user/ralphx'".to_string(),
    );
    assert!(GitService::is_branch_lock_error(&error));
}

#[test]
fn test_is_branch_lock_error_fatal_branch_checked_out() {
    let error =
        AppError::GitOperation("fatal: branch is checked out in another worktree".to_string());
    assert!(GitService::is_branch_lock_error(&error));
}

#[test]
fn test_is_branch_lock_error_case_insensitive() {
    let error =
        AppError::GitOperation("FATAL: 'main' IS ALREADY USED BY WORKTREE at '/path'".to_string());
    assert!(GitService::is_branch_lock_error(&error));
}

#[test]
fn test_is_branch_lock_error_merge_conflict_not_deferrable() {
    let error =
        AppError::GitOperation("CONFLICT (content): Merge conflict in src/main.rs".to_string());
    assert!(!GitService::is_branch_lock_error(&error));
}

#[test]
fn test_is_branch_lock_error_generic_git_error_not_deferrable() {
    let error = AppError::GitOperation("fatal: not a git repository".to_string());
    assert!(!GitService::is_branch_lock_error(&error));
}

#[test]
fn test_is_branch_lock_error_non_git_error_not_deferrable() {
    let error = AppError::Database("connection failed".to_string());
    assert!(!GitService::is_branch_lock_error(&error));
}

#[test]
fn test_is_branch_lock_error_false_positive_avoided() {
    // This error contains "fatal", "branch", and "checked out" but is NOT a branch lock error.
    // The old pattern would have incorrectly classified this as deferrable.
    let error = AppError::GitOperation(
        "fatal: could not read branch configuration, checked out files may be corrupt".to_string(),
    );
    assert!(!GitService::is_branch_lock_error(&error));
}

#[tokio::test]
async fn test_branch_exists_returns_true_for_existing_branch() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Create an initial commit so main/master exists
    std::fs::write(repo.join("file.txt"), "hello").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Get the actual default branch name
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    let branch_name = String::from_utf8_lossy(&output.stdout).trim().to_string();

    assert!(
        GitService::branch_exists(repo, &branch_name)
            .await
            .unwrap_or(false)
    );
}

#[tokio::test]
async fn test_branch_exists_returns_false_for_nonexistent_branch() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo)
        .output()
        .unwrap();
    // Create an initial commit so the repo is valid
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("file.txt"), "hello").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();

    assert!(
        !GitService::branch_exists(repo, "nonexistent-branch")
            .await
            .unwrap_or(true)
    );
}

// =========================================================================
// is_ancestor Tests
// =========================================================================

/// Helper: initialize a repo, make an initial commit, return HEAD sha
fn setup_repo_with_commit(repo: &std::path::Path) -> String {
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("file.txt"), "initial").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[tokio::test]
async fn test_is_ancestor_returns_true_when_commit_is_ancestor() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    let first_sha = setup_repo_with_commit(repo);

    // Add a second commit on the same branch
    std::fs::write(repo.join("file2.txt"), "second").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "second"])
        .current_dir(repo)
        .output()
        .unwrap();
    let second_sha = {
        let out = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    // first_sha is an ancestor of second_sha
    let result = GitService::is_ancestor(repo, &first_sha, &second_sha)
        .await
        .unwrap_or(false);
    assert!(result, "first commit should be ancestor of second commit");
}

#[tokio::test]
async fn test_is_ancestor_returns_false_when_commit_is_not_ancestor() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    let first_sha = setup_repo_with_commit(repo);

    // Add a second commit
    std::fs::write(repo.join("file2.txt"), "second").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "second"])
        .current_dir(repo)
        .output()
        .unwrap();
    let second_sha = {
        let out = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    // second_sha is NOT an ancestor of first_sha (reverse order)
    let result = GitService::is_ancestor(repo, &second_sha, &first_sha)
        .await
        .unwrap_or(true); // unwrap_or(true) so a false-positive Err would fail the assertion
    assert!(
        !result,
        "second commit should NOT be ancestor of first commit"
    );
}

#[tokio::test]
async fn test_is_ancestor_returns_false_for_invalid_ref() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    setup_repo_with_commit(repo);

    let head_sha = {
        let out = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    // Invalid ref should return false (conservative failure mode)
    let result = GitService::is_ancestor(repo, "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef", &head_sha)
        .await
        .unwrap_or(false);
    assert!(
        !result,
        "invalid ref should not be considered an ancestor"
    );
}

// =========================================================================
// is_branch_merged_or_content_equivalent Tests
// =========================================================================

/// Helper: create a minimal git repo with one initial commit on main/master.
fn setup_squash_test_repo(repo: &std::path::Path) {
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(repo)
        .output()
        .unwrap();
    // initial commit on default branch (main/master)
    std::fs::write(repo.join("base.txt"), "base content").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap();
}

/// Get the current default branch name (main or master depending on git config)
fn default_branch(repo: &std::path::Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Squash merge recipe:
///   plan-branch ← git merge --squash task-branch → creates new commit
///   result: task-branch NOT ancestor of plan-branch, but content matches
#[tokio::test]
async fn test_is_branch_merged_or_content_equivalent_squash_merge_returns_content_match() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    setup_squash_test_repo(repo);
    let base = default_branch(repo);

    // Create plan-branch from base
    Command::new("git")
        .args(["checkout", "-b", "plan-branch"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Create task-branch with a feature commit
    Command::new("git")
        .args(["checkout", "-b", "task-branch"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("feature.txt"), "feature content").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "feat: add feature"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Squash merge task-branch into plan-branch
    Command::new("git")
        .args(["checkout", "plan-branch"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["merge", "--squash", "task-branch"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "squash merge"])
        .current_dir(repo)
        .output()
        .unwrap();

    // task-branch is NOT an ancestor of plan-branch (squash creates new commit)
    let is_anc = GitService::is_ancestor(repo, "task-branch", "plan-branch")
        .await
        .unwrap_or(true);
    assert!(!is_anc, "squash merge should break ancestor relationship");

    // But content matches → safe to delete
    let (safe, reason) =
        GitService::is_branch_merged_or_content_equivalent(repo, "task-branch", "plan-branch")
            .await;
    assert!(safe, "squash merge: should be safe to delete");
    assert_eq!(reason, "content_match");

    drop(base); // suppress unused warning
}

/// Normal (non-squash) merge: task-branch IS an ancestor → returns "ancestor"
#[tokio::test]
async fn test_is_branch_merged_or_content_equivalent_normal_merge_returns_ancestor() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    setup_squash_test_repo(repo);

    // Create plan-branch and task-branch from base
    Command::new("git")
        .args(["checkout", "-b", "plan-branch"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["checkout", "-b", "task-branch"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("feature.txt"), "feature content").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "feat: add feature"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Regular merge (preserves ancestor relationship)
    Command::new("git")
        .args(["checkout", "plan-branch"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["merge", "--no-ff", "task-branch", "-m", "merge task"])
        .current_dir(repo)
        .output()
        .unwrap();

    let (safe, reason) =
        GitService::is_branch_merged_or_content_equivalent(repo, "task-branch", "plan-branch")
            .await;
    assert!(safe, "normal merge: should be safe to delete");
    assert_eq!(reason, "ancestor");
}

/// Diverged branches: neither ancestor nor content match → returns "content_differs"
#[tokio::test]
async fn test_is_branch_merged_or_content_equivalent_diverged_returns_content_differs() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    setup_squash_test_repo(repo);

    // Create plan-branch with its own diverging commit
    Command::new("git")
        .args(["checkout", "-b", "plan-branch"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("plan.txt"), "plan-only content").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "plan work"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Go back to default branch and create task-branch with different content
    let base = default_branch(repo);
    Command::new("git")
        .args(["checkout", &base])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["checkout", "-b", "task-branch"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("task.txt"), "task-only content").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "task work"])
        .current_dir(repo)
        .output()
        .unwrap();

    // task-branch and plan-branch have diverged — neither ancestor nor content match
    let (safe, reason) =
        GitService::is_branch_merged_or_content_equivalent(repo, "task-branch", "plan-branch")
            .await;
    assert!(!safe, "diverged branches: should NOT be safe to delete");
    assert_eq!(reason, "content_differs");
}

/// Deleted task-branch: both git ops fail → unwrap_or(false) → returns (false, "content_differs")
#[tokio::test]
async fn test_is_branch_merged_or_content_equivalent_deleted_branch_returns_content_differs() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    setup_squash_test_repo(repo);

    // Create plan-branch (stays)
    Command::new("git")
        .args(["branch", "plan-branch"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Call with a branch that doesn't exist — git ops return errors which collapse to false
    let (safe, reason) =
        GitService::is_branch_merged_or_content_equivalent(repo, "nonexistent-task-branch", "plan-branch")
            .await;
    // Expected: both ancestor check and content check fail → (false, "content_differs")
    assert!(!safe, "deleted/nonexistent branch: should NOT be safe to delete");
    assert_eq!(reason, "content_differs");
}

#[tokio::test]
async fn test_branch_exists_uses_refs_heads_prefix() {
    // Verifies that branch_exists checks local branches (refs/heads/) specifically,
    // not arbitrary refs.
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    setup_repo_with_commit(repo);

    // Get the actual default branch name
    let out = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    let branch_name = String::from_utf8_lossy(&out.stdout).trim().to_string();

    // Create another branch
    Command::new("git")
        .args(["branch", "feature/test-branch"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Existing branch should return true
    let exists = GitService::branch_exists(repo, "feature/test-branch")
        .await
        .unwrap_or(false);
    assert!(exists, "feature/test-branch should exist");

    // Default branch should exist
    let default_exists = GitService::branch_exists(repo, &branch_name)
        .await
        .unwrap_or(false);
    assert!(default_exists, "default branch should exist");

    // Non-existent branch should return false
    let missing = GitService::branch_exists(repo, "no-such-branch")
        .await
        .unwrap_or(true); // unwrap_or(true) so Err would fail the assertion
    assert!(!missing, "no-such-branch should not exist");
}
