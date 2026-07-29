use super::super::*;
use std::process::Command;

// =========================================================================
// checkout_existing_branch_worktree Tests
// =========================================================================

#[tokio::test]
async fn test_checkout_existing_branch_worktree_success() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    // Initialize repo with a commit
    Command::new("git")
        .args(["init"])
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
    std::fs::write(repo.join("test.txt"), "initial").unwrap();
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
    let _ = Command::new("git")
        .args(["branch", "-M", "main"])
        .current_dir(repo)
        .output();

    // Create a feature branch
    Command::new("git")
        .args(["branch", "feature-branch"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Create worktree checking out the existing branch
    let worktree_path = temp_dir.path().join("worktrees").join("merge-wt");
    let result =
        GitService::checkout_existing_branch_worktree(repo, &worktree_path, "feature-branch").await;
    assert!(result.is_ok(), "Should succeed: {:?}", result.err());

    // Verify worktree was created and is on the correct branch
    assert!(worktree_path.exists(), "Worktree directory should exist");
    let branch = GitService::get_current_branch(&worktree_path)
        .await
        .unwrap();
    assert_eq!(branch, "feature-branch");
}

#[tokio::test]
async fn test_checkout_existing_branch_worktree_creates_parent_dirs() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    Command::new("git")
        .args(["init"])
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
    std::fs::write(repo.join("test.txt"), "initial").unwrap();
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
    let _ = Command::new("git")
        .args(["branch", "-M", "main"])
        .current_dir(repo)
        .output();

    Command::new("git")
        .args(["branch", "feature"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Path with deeply nested non-existent parent dirs
    let worktree_path = temp_dir.path().join("deep").join("nested").join("merge-wt");
    let result =
        GitService::checkout_existing_branch_worktree(repo, &worktree_path, "feature").await;
    assert!(
        result.is_ok(),
        "Should create parent dirs: {:?}",
        result.err()
    );
    assert!(worktree_path.exists());
}

#[tokio::test]
async fn test_checkout_existing_branch_worktree_nonexistent_branch() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    Command::new("git")
        .args(["init"])
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
    std::fs::write(repo.join("test.txt"), "initial").unwrap();
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

    let worktree_path = temp_dir.path().join("merge-wt");
    let result =
        GitService::checkout_existing_branch_worktree(repo, &worktree_path, "nonexistent-branch")
            .await;
    assert!(result.is_err(), "Should fail for nonexistent branch");
}

// =========================================================================
// try_merge_in_worktree Tests
// =========================================================================

#[tokio::test]
async fn test_try_merge_in_worktree_fast_forward() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    // Setup: feature-branch as target, task-branch as source (fast-forward case)
    // Main repo stays on main; merge worktree checks out feature-branch
    Command::new("git")
        .args(["init"])
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
    std::fs::write(repo.join("test.txt"), "initial").unwrap();
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
    let _ = Command::new("git")
        .args(["branch", "-M", "main"])
        .current_dir(repo)
        .output();

    // Create feature branch (target) at current commit
    Command::new("git")
        .args(["branch", "feature-branch"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Create task branch with a new file (fast-forward from feature-branch)
    Command::new("git")
        .args(["checkout", "-b", "task-branch"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("new-file.txt"), "task work").unwrap();
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

    // Go back to main (user's working branch)
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(repo)
        .output()
        .unwrap();

    let merge_wt = temp_dir.path().join("merge-wt");
    let result =
        GitService::try_merge_in_worktree(repo, "task-branch", "feature-branch", &merge_wt).await;
    assert!(result.is_ok(), "Merge should succeed: {:?}", result.err());

    match result.unwrap() {
        MergeAttemptResult::Success { commit_sha } => {
            assert!(!commit_sha.is_empty(), "Should have commit SHA");
        }
        MergeAttemptResult::NeedsAgent { .. } => {
            panic!("Fast-forward merge should succeed, not need agent");
        }
        MergeAttemptResult::BranchNotFound { branch } => {
            panic!("Unexpected BranchNotFound: {}", branch);
        }
    }

    // Merge worktree should still exist (caller responsible for cleanup)
    assert!(
        merge_wt.exists(),
        "Merge worktree should still exist after success"
    );

    // Clean up worktree
    let _ = GitService::delete_worktree(repo, &merge_wt).await;
}

#[tokio::test]
async fn test_try_merge_in_worktree_conflict() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    // Setup: feature-branch and task-branch modify same file differently
    Command::new("git")
        .args(["init"])
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
    std::fs::write(repo.join("shared.txt"), "initial content").unwrap();
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
    let _ = Command::new("git")
        .args(["branch", "-M", "main"])
        .current_dir(repo)
        .output();

    // Create feature branch (target) and add divergent changes
    Command::new("git")
        .args(["checkout", "-b", "feature-branch"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("shared.txt"), "feature branch changes").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "feature changes"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Create task branch from main with conflicting changes
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["checkout", "-b", "task-branch"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("shared.txt"), "task branch changes").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "task changes"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Go back to main
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(repo)
        .output()
        .unwrap();

    let merge_wt = temp_dir.path().join("merge-wt");
    let result =
        GitService::try_merge_in_worktree(repo, "task-branch", "feature-branch", &merge_wt).await;
    assert!(
        result.is_ok(),
        "Should return Ok even on conflict: {:?}",
        result.err()
    );

    match result.unwrap() {
        MergeAttemptResult::NeedsAgent { conflict_files } => {
            assert!(!conflict_files.is_empty(), "Should report conflict files");
            // Merge worktree should still exist (for agent to resolve in)
            assert!(
                merge_wt.exists(),
                "Merge worktree should be kept for conflict resolution"
            );
            // MERGE_HEAD should exist (merge NOT aborted)
            assert!(
                GitService::is_merge_in_progress(&merge_wt),
                "Merge should still be in progress in worktree"
            );
        }
        MergeAttemptResult::Success { .. } => {
            panic!("Conflicting merge should need agent, not succeed");
        }
        MergeAttemptResult::BranchNotFound { branch } => {
            panic!("Unexpected BranchNotFound: {}", branch);
        }
    }

    // Clean up
    let _ = GitService::delete_worktree(repo, &merge_wt).await;
}

#[tokio::test]
async fn test_try_merge_in_worktree_does_not_touch_main_repo() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    // Setup repo with feature-branch as merge target
    Command::new("git")
        .args(["init"])
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
    std::fs::write(repo.join("test.txt"), "initial").unwrap();
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
    let _ = Command::new("git")
        .args(["branch", "-M", "main"])
        .current_dir(repo)
        .output();

    // Create feature branch (target)
    Command::new("git")
        .args(["branch", "feature-branch"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Create task branch
    Command::new("git")
        .args(["checkout", "-b", "task-branch"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("new.txt"), "task").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "task"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Go back to main — this is the branch the user is working on
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Record main repo state before merge
    let branch_before = GitService::get_current_branch(repo).await.unwrap();

    let merge_wt = temp_dir.path().join("merge-wt");
    let _ =
        GitService::try_merge_in_worktree(repo, "task-branch", "feature-branch", &merge_wt).await;

    // Main repo should still be on the same branch
    let branch_after = GitService::get_current_branch(repo).await.unwrap();
    assert_eq!(
        branch_before, branch_after,
        "Main repo branch should not change"
    );

    // Clean up
    let _ = GitService::delete_worktree(repo, &merge_wt).await;
}

// =========================================================================
// Worktree porcelain parsing tests
// =========================================================================

#[test]
fn test_parse_worktree_porcelain_normal() {
    let output = "\
worktree /home/user/project
HEAD abc1234567890abcdef1234567890abcdef123456
branch refs/heads/main

worktree /home/user/worktrees/feature
HEAD def4567890abcdef1234567890abcdef12345678
branch refs/heads/feature-branch

";
    let result = GitService::parse_worktree_porcelain(output);
    assert_eq!(result.len(), 2);

    assert_eq!(result[0].path, "/home/user/project");
    assert_eq!(
        result[0].head.as_deref(),
        Some("abc1234567890abcdef1234567890abcdef123456")
    );
    assert_eq!(result[0].branch.as_deref(), Some("main"));

    assert_eq!(result[1].path, "/home/user/worktrees/feature");
    assert_eq!(
        result[1].head.as_deref(),
        Some("def4567890abcdef1234567890abcdef12345678")
    );
    assert_eq!(result[1].branch.as_deref(), Some("feature-branch"));
}

#[test]
fn test_parse_worktree_porcelain_bare_repo() {
    let output = "\
worktree /home/user/project.git
bare

";
    let result = GitService::parse_worktree_porcelain(output);
    assert_eq!(result.len(), 1);

    assert_eq!(result[0].path, "/home/user/project.git");
    assert!(result[0].head.is_none());
    assert!(result[0].branch.is_none());
}

#[test]
fn test_parse_worktree_porcelain_detached_head() {
    let output = "\
worktree /home/user/project
HEAD abc1234567890abcdef1234567890abcdef123456
branch refs/heads/main

worktree /home/user/worktrees/detached
HEAD 9876543210abcdef1234567890abcdef12345678
detached

";
    let result = GitService::parse_worktree_porcelain(output);
    assert_eq!(result.len(), 2);

    assert_eq!(result[1].path, "/home/user/worktrees/detached");
    assert_eq!(
        result[1].head.as_deref(),
        Some("9876543210abcdef1234567890abcdef12345678")
    );
    assert!(result[1].branch.is_none());
}

#[test]
fn test_parse_worktree_porcelain_no_trailing_newline() {
    // Some git versions may not emit a trailing blank line
    let output = "\
worktree /home/user/project
HEAD abc1234567890abcdef1234567890abcdef123456
branch refs/heads/main";

    let result = GitService::parse_worktree_porcelain(output);
    assert_eq!(result.len(), 1);

    assert_eq!(result[0].path, "/home/user/project");
    assert_eq!(result[0].branch.as_deref(), Some("main"));
}

#[test]
fn test_parse_worktree_porcelain_empty_output() {
    let result = GitService::parse_worktree_porcelain("");
    assert!(result.is_empty());
}

#[test]
fn test_parse_worktree_porcelain_nested_branch_name() {
    let output = "\
worktree /home/user/worktrees/task
HEAD abc1234567890abcdef1234567890abcdef123456
branch refs/heads/ralphx/my-app/task-abc123

";
    let result = GitService::parse_worktree_porcelain(output);
    assert_eq!(result.len(), 1);

    // Nested branch names should be preserved after stripping refs/heads/
    assert_eq!(
        result[0].branch.as_deref(),
        Some("ralphx/my-app/task-abc123")
    );
}

#[test]
fn test_parse_worktree_porcelain_prunable() {
    let output = "\
worktree /home/user/project
HEAD abc1234567890abcdef1234567890abcdef123456
branch refs/heads/main

worktree /tmp/stale-wt
HEAD def4567890abcdef1234567890abcdef12345678
branch refs/heads/old-branch
prunable gitdir file points to non-existent location

";
    let result = GitService::parse_worktree_porcelain(output);
    assert_eq!(result.len(), 2);

    // Prunable flag is ignored (we just parse path/head/branch)
    assert_eq!(result[1].path, "/tmp/stale-wt");
    assert_eq!(result[1].branch.as_deref(), Some("old-branch"));
}

// =========================================================================
// Branch Recovery Tests (for re-execution scenarios)
// =========================================================================

#[tokio::test]
async fn test_worktree_recovery_existing_branch_checkout() {
    // Simulates re-entering Executing state where branch exists from previous attempt
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    // Initialize repo
    Command::new("git")
        .args(["init"])
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
    std::fs::write(repo.join("test.txt"), "initial").unwrap();
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
    let _ = Command::new("git")
        .args(["branch", "-M", "main"])
        .current_dir(repo)
        .output();

    // Simulate previous execution: create task branch with some work
    let task_branch = "ralphx/test-project/task-abc123";
    Command::new("git")
        .args(["checkout", "-b", task_branch])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("work.txt"), "previous attempt").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "previous work"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Go back to main (simulating user's working state)
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Verify branch exists
    let branch_exists = GitService::branch_exists(repo, task_branch).await.unwrap_or(false);
    assert!(
        branch_exists,
        "Task branch should exist from previous attempt"
    );

    // Re-execution: checkout existing branch into new worktree
    let worktree_path = temp_dir.path().join("worktrees").join("task-abc123");
    let result =
        GitService::checkout_existing_branch_worktree(repo, &worktree_path, task_branch).await;
    assert!(
        result.is_ok(),
        "Should successfully checkout existing branch: {:?}",
        result.err()
    );

    // Verify worktree was created and has the previous work
    assert!(worktree_path.exists(), "Worktree should be created");
    assert!(
        worktree_path.join("work.txt").exists(),
        "Previous work should be present"
    );
    let branch = GitService::get_current_branch(&worktree_path)
        .await
        .unwrap();
    assert_eq!(branch, task_branch, "Should be on the task branch");
}

#[tokio::test]
async fn test_worktree_creation_new_branch_when_not_exists() {
    // Verifies normal flow when branch doesn't exist yet
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    // Initialize repo
    Command::new("git")
        .args(["init"])
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
    std::fs::write(repo.join("test.txt"), "initial").unwrap();
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
    let _ = Command::new("git")
        .args(["branch", "-M", "main"])
        .current_dir(repo)
        .output();

    let new_branch = "ralphx/test-project/task-new";

    // Verify branch does NOT exist
    let branch_exists = GitService::branch_exists(repo, new_branch).await.unwrap_or(true);
    assert!(!branch_exists, "New task branch should not exist yet");

    // Create worktree with new branch
    let worktree_path = temp_dir.path().join("worktrees").join("task-new");
    let result = GitService::create_worktree(repo, &worktree_path, new_branch, "main").await;
    assert!(
        result.is_ok(),
        "Should successfully create worktree with new branch: {:?}",
        result.err()
    );

    // Verify worktree was created and is on new branch
    assert!(worktree_path.exists(), "Worktree should be created");
    let branch = GitService::get_current_branch(&worktree_path)
        .await
        .unwrap();
    assert_eq!(branch, new_branch, "Should be on the new task branch");

    // Verify branch now exists
    let branch_exists_after = GitService::branch_exists(repo, new_branch).await.unwrap_or(false);
    assert!(branch_exists_after, "Branch should exist after creation");
}

#[tokio::test]
async fn test_create_worktree_recovers_when_branch_exists() {
    // Verifies the race-condition fix: create_worktree with -b for an already-existing branch
    // now retries as a checkout (without -b) instead of returning an error.
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    // Initialize repo
    Command::new("git")
        .args(["init"])
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
    std::fs::write(repo.join("test.txt"), "initial").unwrap();
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
    let _ = Command::new("git")
        .args(["branch", "-M", "main"])
        .current_dir(repo)
        .output();

    // Create an existing branch
    let existing_branch = "ralphx/test-project/task-exists";
    Command::new("git")
        .args(["branch", existing_branch])
        .current_dir(repo)
        .output()
        .unwrap();

    // Attempt to create worktree with -b for existing branch — should now succeed via retry
    let worktree_path = temp_dir.path().join("worktrees").join("task-exists");
    let result = GitService::create_worktree(repo, &worktree_path, existing_branch, "main").await;

    assert!(
        result.is_ok(),
        "create_worktree should recover when branch already exists by retrying as checkout: {:?}",
        result.err()
    );
    // Verify the worktree was actually created
    assert!(
        worktree_path.exists(),
        "Worktree path should exist after successful recovery"
    );
}

// =========================================================================
// Locked stale entry retry guard tests
// =========================================================================

/// Helper: lock a worktree path so git metadata records it as locked.
fn lock_worktree_sync(repo: &std::path::Path, wt: &std::path::Path) {
    Command::new("git")
        .args(["worktree", "lock", wt.to_str().unwrap()])
        .current_dir(repo)
        .output()
        .unwrap();
}

/// create_worktree: stale locked entry at target path → retry guard unlocks + prunes → retry succeeds.
///
/// Scenario: a previous worktree at the same path was locked (e.g. by SIGKILL aftermath),
/// then the directory was deleted. The locked git metadata blocks `git worktree add`.
/// The retry guard should detect "locked" in stderr, run unlock+prune, and retry successfully.
#[tokio::test]
async fn test_create_worktree_retries_after_locked_stale_entry() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    init_git_repo(repo);

    let wt_path = temp_dir.path().join("worktrees").join("task-wt");

    // Step 1: Create a worktree at wt_path with a temporary branch
    Command::new("git")
        .args(["branch", "temp-branch"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args([
            "worktree",
            "add",
            wt_path.to_str().unwrap(),
            "temp-branch",
        ])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(wt_path.exists(), "Worktree should exist before locking");

    // Step 2: Lock the worktree — simulates SIGKILL leaving locked metadata
    lock_worktree_sync(repo, &wt_path);

    // Step 3: Delete the directory — leaves stale locked metadata in git
    std::fs::remove_dir_all(&wt_path).unwrap();
    assert!(!wt_path.exists(), "Directory should be gone");

    // Step 4: create_worktree should succeed via retry guard (unlock+prune+retry)
    let result = GitService::create_worktree(repo, &wt_path, "new-task-branch", "main").await;
    assert!(
        result.is_ok(),
        "create_worktree should succeed after unlock+prune of stale locked entry: {:?}",
        result.err()
    );

    // Worktree should now exist on the new branch
    assert!(wt_path.exists(), "Worktree should exist after retry");
    let branch = GitService::get_current_branch(&wt_path).await.unwrap();
    assert_eq!(branch, "new-task-branch", "Worktree should be on new branch");

    // Clean up
    let _ = GitService::delete_worktree(repo, &wt_path).await;
}

/// checkout_existing_branch_worktree: stale locked entry at target path → retry guard → success.
///
/// Same scenario as above but for the existing-branch checkout path (used by merge worktrees).
#[tokio::test]
async fn test_checkout_existing_branch_worktree_retries_after_locked_stale_entry() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    init_git_repo(repo);

    let wt_path = temp_dir.path().join("worktrees").join("merge-wt");

    // Create another branch for the eventual checkout
    Command::new("git")
        .args(["branch", "target-branch"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Step 1: Create a worktree at wt_path with a temporary branch
    Command::new("git")
        .args(["branch", "temp-branch"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args([
            "worktree",
            "add",
            wt_path.to_str().unwrap(),
            "temp-branch",
        ])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(wt_path.exists(), "Worktree should exist before locking");

    // Step 2: Lock the worktree
    lock_worktree_sync(repo, &wt_path);

    // Step 3: Delete the directory — leaves stale locked metadata
    std::fs::remove_dir_all(&wt_path).unwrap();
    assert!(!wt_path.exists(), "Directory should be gone");

    // Step 4: checkout_existing_branch_worktree should succeed via retry guard
    let result =
        GitService::checkout_existing_branch_worktree(repo, &wt_path, "target-branch").await;
    assert!(
        result.is_ok(),
        "checkout_existing_branch_worktree should succeed after unlock+prune of stale locked entry: {:?}",
        result.err()
    );

    assert!(wt_path.exists(), "Worktree should exist after retry");
    let branch = GitService::get_current_branch(&wt_path).await.unwrap();
    assert_eq!(branch, "target-branch", "Worktree should be on target branch");

    // Clean up
    let _ = GitService::delete_worktree(repo, &wt_path).await;
}

/// checkout_existing_branch_worktree: branch already checked out at another path → recovery.
///
/// Scenario: a stale worktree from a prior execution still has the task branch checked out.
/// The retry guard should detect "already checked out at" in stderr, delete the stale
/// worktree, prune git metadata, and retry successfully.
///
/// This exercises the Fix 2 recovery path for the "reference already exists" race condition
/// described in the worktree path-exists race bug.
#[tokio::test]
async fn test_checkout_existing_branch_worktree_recovers_from_already_checked_out_at() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    init_git_repo(repo);

    // Create the task branch that will be "stale-checked-out"
    Command::new("git")
        .args(["branch", "task/stale-branch"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Step 1: Check out the branch in a "stale" worktree (simulating prior execution)
    let stale_wt_path = temp_dir.path().join("worktrees").join("stale-task-wt");
    std::fs::create_dir_all(stale_wt_path.parent().unwrap()).unwrap();
    Command::new("git")
        .args([
            "worktree",
            "add",
            stale_wt_path.to_str().unwrap(),
            "task/stale-branch",
        ])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        stale_wt_path.exists(),
        "Stale worktree should exist before test"
    );

    // Step 2: Attempt to checkout the same branch into a DIFFERENT path.
    // Without the fix, this would fail with "already checked out at <stale_wt_path>".
    let target_wt_path = temp_dir.path().join("worktrees").join("new-task-wt");
    let result =
        GitService::checkout_existing_branch_worktree(repo, &target_wt_path, "task/stale-branch")
            .await;
    assert!(
        result.is_ok(),
        "checkout_existing_branch_worktree should recover from 'already checked out at': {:?}",
        result.err()
    );

    // Verify new worktree was created at the target path
    assert!(
        target_wt_path.exists(),
        "New worktree should exist at target path after recovery"
    );

    // Verify it's on the correct branch
    let branch = GitService::get_current_branch(&target_wt_path).await.unwrap();
    assert_eq!(
        branch, "task/stale-branch",
        "New worktree should be on the task branch"
    );

    // Verify the stale worktree was cleaned up (branch freed)
    assert!(
        !stale_wt_path.exists(),
        "Stale worktree should be deleted after recovery"
    );

    // Clean up
    let _ = GitService::delete_worktree(repo, &target_wt_path).await;
}

/// create_worktree with no locked entry succeeds on first attempt (no extra git calls).
/// This is already covered by test_worktree_creation_new_branch_when_not_exists but
/// is repeated here explicitly as a guard that the retry path is not triggered unnecessarily.
#[tokio::test]
async fn test_create_worktree_no_locked_entry_succeeds_first_attempt() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    init_git_repo(repo);

    let wt_path = temp_dir.path().join("worktrees").join("clean-wt");

    // No stale entry — should succeed on first attempt without any retry
    let result = GitService::create_worktree(repo, &wt_path, "clean-branch", "main").await;
    assert!(
        result.is_ok(),
        "create_worktree should succeed on first attempt when no locked entry: {:?}",
        result.err()
    );
    assert!(wt_path.exists(), "Worktree should exist");
    let branch = GitService::get_current_branch(&wt_path).await.unwrap();
    assert_eq!(branch, "clean-branch");

    let _ = GitService::delete_worktree(repo, &wt_path).await;
}

// =========================================================================
// remove_stale_index_lock Tests
// =========================================================================

/// Helper: initialise a bare-minimum git repo in `dir`.
fn init_git_repo(dir: &std::path::Path) {
    Command::new("git")
        .args(["init"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(dir)
        .output()
        .unwrap();
    std::fs::write(dir.join("file.txt"), "init").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(dir)
        .output()
        .unwrap();
    let _ = Command::new("git")
        .args(["branch", "-M", "main"])
        .current_dir(dir)
        .output();
}

#[test]
fn test_remove_stale_index_lock_no_lock_file() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    init_git_repo(repo);

    // No index.lock file exists — should return Ok(false)
    let result = GitService::remove_stale_index_lock(repo, 5);
    assert!(result.is_ok(), "Should not error: {:?}", result.err());
    assert!(!result.unwrap(), "No lock file => false");
}

#[test]
fn test_remove_stale_index_lock_fresh_lock_not_removed() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    init_git_repo(repo);

    // Create a fresh index.lock (age ~0s)
    let lock_path = repo.join(".git").join("index.lock");
    std::fs::write(&lock_path, "locked").unwrap();

    // With threshold of 60s, a fresh lock should NOT be removed
    let result = GitService::remove_stale_index_lock(repo, 60);
    assert!(result.is_ok(), "Should not error: {:?}", result.err());
    assert!(!result.unwrap(), "Fresh lock should not be removed");

    // Lock file should still exist
    assert!(lock_path.exists(), "Fresh lock should not be deleted");
}

#[test]
fn test_remove_stale_index_lock_stale_lock_removed() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    init_git_repo(repo);

    // Create an index.lock and backdate its mtime by 10 seconds using `touch -t`
    let lock_path = repo.join(".git").join("index.lock");
    std::fs::write(&lock_path, "locked").unwrap();

    // Use `touch` to set mtime to a time well in the past (10 seconds ago)
    // Date format for touch: [[CC]YY]MMDDhhmm[.SS]
    // We write a fixed date far in the past to ensure staleness
    let _ = Command::new("touch")
        .args(["-t", "202001010000.00", lock_path.to_str().unwrap()])
        .output();

    // With threshold of 5s, a lock from 2020 SHOULD be removed
    let result = GitService::remove_stale_index_lock(repo, 5);
    assert!(result.is_ok(), "Should not error: {:?}", result.err());
    assert!(result.unwrap(), "Stale lock should be removed");

    // Lock file should be gone
    assert!(!lock_path.exists(), "Stale lock should be deleted");
}

#[test]
fn test_remove_stale_index_lock_zero_threshold_removes_any_lock() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    init_git_repo(repo);

    let lock_path = repo.join(".git").join("index.lock");
    std::fs::write(&lock_path, "locked").unwrap();

    // Threshold of 0 means remove any lock, regardless of age
    let result = GitService::remove_stale_index_lock(repo, 0);
    assert!(result.is_ok(), "Should not error: {:?}", result.err());
    assert!(
        result.unwrap(),
        "Any lock with threshold=0 should be removed"
    );
    assert!(!lock_path.exists(), "Lock should be deleted");
}

// =========================================================================
// delete_worktree fallback regression tests
// =========================================================================

/// Regression: directory exists on disk but is NOT registered with git worktree.
/// `git worktree remove` fails (not a known worktree), so the rm-rf fallback
/// should kick in and remove the directory, returning Ok(()).
#[tokio::test]
async fn test_delete_worktree_succeeds_for_unregistered_directory() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    init_git_repo(repo);

    // Create a directory that looks like a worktree path but is NOT registered with git
    let fake_wt = temp_dir.path().join("worktrees").join("stale-task-wt");
    std::fs::create_dir_all(&fake_wt).unwrap();
    std::fs::write(fake_wt.join("dummy.txt"), "stale contents").unwrap();

    // Precondition: directory exists
    assert!(
        fake_wt.exists(),
        "Fake worktree dir should exist before deletion"
    );

    let result = GitService::delete_worktree(repo, &fake_wt).await;
    assert!(
        result.is_ok(),
        "delete_worktree should succeed via rm-rf fallback: {:?}",
        result.err()
    );

    // Directory must be gone after the call
    assert!(
        !fake_wt.exists(),
        "Stale worktree directory should be removed"
    );
}

/// Regression: directory doesn't exist at all.
/// `git worktree remove` fails (nothing to remove), directory check is false,
/// so the function should return Ok(()) without error.
#[tokio::test]
async fn test_delete_worktree_succeeds_when_dir_already_gone() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    init_git_repo(repo);

    // Path that was never created
    let nonexistent_wt = temp_dir.path().join("worktrees").join("already-gone-wt");
    assert!(
        !nonexistent_wt.exists(),
        "Path should not exist before the call"
    );

    let result = GitService::delete_worktree(repo, &nonexistent_wt).await;
    assert!(
        result.is_ok(),
        "delete_worktree should succeed when directory is already gone: {:?}",
        result.err()
    );
}

// =========================================================================
// cleanup_stale_worktree_artifacts Tests
// =========================================================================

/// Builds a minimal Project for testing cleanup_stale_worktree_artifacts.
/// Uses a fixed worktree_parent_directory so task_worktree_path is deterministic.
fn make_test_project(worktree_parent: &str) -> crate::domain::entities::project::Project {
    use crate::domain::entities::project::{GitMode, Project};
    use crate::domain::entities::ProjectId;
    use chrono::Utc;
    let now = Utc::now();
    Project {
        id: ProjectId::from_string("test-project-id".to_string()),
        name: "test-project".to_string(),
        working_directory: "/tmp/test".to_string(),
        git_mode: GitMode::Worktree,
        base_branch: Some("main".to_string()),
        worktree_parent_directory: Some(worktree_parent.to_string()),
        use_feature_branches: true,
        merge_validation_mode: crate::domain::entities::project::MergeValidationMode::default(),
        merge_strategy: crate::domain::entities::project::MergeStrategy::default(),
        detected_analysis: None,
        custom_analysis: None,
        analyzed_at: None,
        github_pr_enabled: true,
        created_at: now,
        updated_at: now,
        archived_at: None,
    }
}

/// cleanup_stale_worktree_artifacts: explicit worktree path provided, directory exists.
/// The directory should be deleted by the cleanup (it is not in use).
#[tokio::test]
async fn test_cleanup_with_explicit_path() {
    let repo_dir = tempfile::tempdir().unwrap();
    let worktree_dir = tempfile::tempdir().unwrap();
    init_git_repo(repo_dir.path());

    // Create a stale index.lock backdated to the past so it gets removed
    let lock_path = repo_dir.path().join(".git").join("index.lock");
    std::fs::write(&lock_path, "stale").unwrap();
    let _ = Command::new("touch")
        .args(["-t", "202001010000.00", lock_path.to_str().unwrap()])
        .output();

    // Verify the worktree dir exists before cleanup
    assert!(worktree_dir.path().exists(), "Worktree dir should exist before cleanup");

    let project = make_test_project("/tmp/irrelevant");
    let result = GitService::cleanup_stale_worktree_artifacts(
        repo_dir.path(),
        Some(worktree_dir.path()),
        &project,
        "task-test",
    )
    .await;

    assert!(result.is_ok(), "cleanup should succeed: {:?}", result.err());
    // Directory should be deleted (it was not in use)
    assert!(
        !worktree_dir.path().exists(),
        "Worktree directory should be deleted after cleanup"
    );
}

/// cleanup_stale_worktree_artifacts: None path → computes from project.
/// The computed path doesn't exist on disk → Ok(()) without error.
#[tokio::test]
async fn test_cleanup_with_none_path_uses_project() {
    let repo_dir = tempfile::tempdir().unwrap();
    let parent_dir = tempfile::tempdir().unwrap();
    init_git_repo(repo_dir.path());

    let project = make_test_project(parent_dir.path().to_str().unwrap());
    // The computed path will be something like <parent>/test-project/task-nonexistent-task-id
    // That path does not exist, so cleanup should succeed without deleting anything.
    let result = GitService::cleanup_stale_worktree_artifacts(
        repo_dir.path(),
        None,
        &project,
        "nonexistent-task-id",
    )
    .await;

    assert!(
        result.is_ok(),
        "cleanup with non-existent computed path should succeed: {:?}",
        result.err()
    );
}

/// cleanup_stale_worktree_artifacts: directory exists but WorktreePermit held → not deleted.
#[tokio::test]
async fn test_cleanup_skips_deletion_when_guard_active() {
    use crate::domain::services::worktree_guard::acquire_worktree_permit;

    let repo_dir = tempfile::tempdir().unwrap();
    let worktree_dir = tempfile::tempdir().unwrap();
    init_git_repo(repo_dir.path());

    // Acquire a permit for the worktree path — this marks it as in-use
    let _permit = acquire_worktree_permit(worktree_dir.path());

    let project = make_test_project("/tmp/irrelevant");
    let result = GitService::cleanup_stale_worktree_artifacts(
        repo_dir.path(),
        Some(worktree_dir.path()),
        &project,
        "task-guarded",
    )
    .await;

    assert!(result.is_ok(), "cleanup should succeed even with guard: {:?}", result.err());
    // Directory should still exist — deletion was skipped due to in-use guard
    assert!(
        worktree_dir.path().exists(),
        "Worktree directory should NOT be deleted when permit is held"
    );
}

/// cleanup_stale_worktree_artifacts: index.lock exists but is too young → not removed.
#[tokio::test]
async fn test_cleanup_index_lock_too_young_not_removed() {
    let repo_dir = tempfile::tempdir().unwrap();
    init_git_repo(repo_dir.path());

    // Create a fresh index.lock — mtime is "now", well within the stale threshold
    let lock_path = repo_dir.path().join(".git").join("index.lock");
    std::fs::write(&lock_path, "fresh lock").unwrap();

    // The worktree_dir is the explicit path but we do NOT want it deleted either.
    // However, since it's a real dir (not in use), cleanup WILL delete it.
    // We only care about verifying the lock file was not removed.
    // Use a subdirectory that doesn't exist to avoid the dir-deletion assertion complicating things.
    let nonexistent_wt = repo_dir.path().join("nonexistent-worktree");

    let project = make_test_project("/tmp/irrelevant");
    let result = GitService::cleanup_stale_worktree_artifacts(
        repo_dir.path(),
        Some(&nonexistent_wt),
        &project,
        "task-fresh-lock",
    )
    .await;

    assert!(result.is_ok(), "cleanup should succeed: {:?}", result.err());
    // Lock file should still exist — it was too young to be removed
    assert!(
        lock_path.exists(),
        "Fresh index.lock should NOT be removed (too young)"
    );
}

/// RC9 fix: delete_worktree runs `git worktree prune` even when the path doesn't exist.
///
/// Previously, prune only ran inside `if worktree.exists()`, so a stale git metadata
/// entry would remain if the directory was already deleted. This test creates a real
/// worktree, manually deletes the directory (simulating external deletion), then calls
/// delete_worktree and verifies the stale git metadata was cleaned up by prune.
#[tokio::test]
async fn test_delete_worktree_prunes_even_when_path_missing() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();
    init_git_repo(repo);

    // Create a feature branch and worktree
    Command::new("git")
        .args(["branch", "prune-test-branch"])
        .current_dir(repo)
        .output()
        .unwrap();

    let wt_path = temp_dir.path().join("worktrees").join("prune-test-wt");
    std::fs::create_dir_all(wt_path.parent().unwrap()).unwrap();

    let create_result =
        GitService::checkout_existing_branch_worktree(repo, &wt_path, "prune-test-branch").await;
    assert!(
        create_result.is_ok(),
        "Worktree creation should succeed: {:?}",
        create_result.err()
    );
    assert!(wt_path.exists(), "Worktree should exist after creation");

    // Verify the worktree shows up in git's list before deletion
    let worktrees_before = GitService::list_worktrees(repo).await.unwrap();
    let has_wt_before = worktrees_before
        .iter()
        .any(|w| w.branch.as_deref() == Some("prune-test-branch"));
    assert!(
        has_wt_before,
        "Worktree should be listed before deletion"
    );

    // Manually delete the directory (simulating external deletion, leaving stale git metadata)
    std::fs::remove_dir_all(&wt_path).unwrap();
    assert!(
        !wt_path.exists(),
        "Directory should be gone after manual deletion"
    );

    // delete_worktree should succeed and run git worktree prune (RC9 fix)
    let result = GitService::delete_worktree(repo, &wt_path).await;
    assert!(
        result.is_ok(),
        "delete_worktree should succeed even when path is already gone: {:?}",
        result.err()
    );

    // Verify the stale git metadata was cleaned up by prune
    let worktrees_after = GitService::list_worktrees(repo).await.unwrap();
    let has_wt_after = worktrees_after
        .iter()
        .any(|w| w.branch.as_deref() == Some("prune-test-branch"));
    assert!(
        !has_wt_after,
        "After delete_worktree, stale git metadata should be pruned \
         (RC9 fix: prune now runs outside if-path-exists block)"
    );
}

// =========================================================================
// branches_have_same_content guard in try_merge_in_worktree
// =========================================================================

/// Verify that `try_merge_in_worktree` early-returns when branches are identical
/// (same tree content), without creating the merge worktree or a new commit.
#[tokio::test]
async fn test_try_merge_in_worktree_skips_when_branches_identical() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    Command::new("git").args(["init"]).current_dir(repo).output().unwrap();
    Command::new("git").args(["config", "user.email", "test@example.com"]).current_dir(repo).output().unwrap();
    Command::new("git").args(["config", "user.name", "Test User"]).current_dir(repo).output().unwrap();

    std::fs::write(repo.join("file.txt"), "hello\n").unwrap();
    Command::new("git").args(["add", "."]).current_dir(repo).output().unwrap();
    Command::new("git").args(["commit", "-m", "initial"]).current_dir(repo).output().unwrap();
    let _ = Command::new("git").args(["branch", "-M", "main"]).current_dir(repo).output();

    // Create task branch pointing at the same commit — identical content
    Command::new("git").args(["branch", "task-branch"]).current_dir(repo).output().unwrap();

    let merge_wt = temp_dir.path().join("merge-wt");
    let result = GitService::try_merge_in_worktree(repo, "task-branch", "main", &merge_wt).await;

    assert!(result.is_ok(), "Should succeed: {:?}", result.err());
    assert!(
        matches!(result.unwrap(), MergeAttemptResult::Success { .. }),
        "Identical branches should return Success"
    );
    // Worktree must NOT be created — branches were identical, early return fires before Step 1
    assert!(
        !merge_wt.exists(),
        "Merge worktree should NOT be created when branches are already identical"
    );
}

// =========================================================================
// branches_have_same_content guard in try_rebase_and_merge_in_worktree
// =========================================================================

/// Verify that `try_rebase_and_merge_in_worktree` early-returns when branches are
/// identical, without creating either worktree or a new commit.
#[tokio::test]
async fn test_try_rebase_and_merge_in_worktree_skips_when_branches_identical() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    Command::new("git").args(["init"]).current_dir(repo).output().unwrap();
    Command::new("git").args(["config", "user.email", "test@example.com"]).current_dir(repo).output().unwrap();
    Command::new("git").args(["config", "user.name", "Test User"]).current_dir(repo).output().unwrap();

    std::fs::write(repo.join("file.txt"), "hello\n").unwrap();
    Command::new("git").args(["add", "."]).current_dir(repo).output().unwrap();
    Command::new("git").args(["commit", "-m", "initial"]).current_dir(repo).output().unwrap();
    // Second commit so base_commit_count > 1 (avoids early fallback to try_merge_in_worktree)
    std::fs::write(repo.join("file2.txt"), "world\n").unwrap();
    Command::new("git").args(["add", "."]).current_dir(repo).output().unwrap();
    Command::new("git").args(["commit", "-m", "second"]).current_dir(repo).output().unwrap();
    let _ = Command::new("git").args(["branch", "-M", "main"]).current_dir(repo).output();

    // Create task branch pointing at the same commit — identical content
    Command::new("git").args(["branch", "task-branch"]).current_dir(repo).output().unwrap();

    let rebase_wt = temp_dir.path().join("rebase-wt");
    let merge_wt = temp_dir.path().join("merge-wt");
    let result = GitService::try_rebase_and_merge_in_worktree(
        repo, "task-branch", "main", &rebase_wt, &merge_wt,
    )
    .await;

    assert!(result.is_ok(), "Should succeed: {:?}", result.err());
    assert!(
        matches!(result.unwrap(), MergeAttemptResult::Success { .. }),
        "Identical branches should return Success"
    );
    // Neither worktree should be created — early return fires before any worktree ops
    assert!(
        !rebase_wt.exists(),
        "Rebase worktree should NOT be created when branches are already identical"
    );
    assert!(
        !merge_wt.exists(),
        "Merge worktree should NOT be created when branches are already identical"
    );
}
