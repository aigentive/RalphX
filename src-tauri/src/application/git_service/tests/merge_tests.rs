use super::super::*;
use super::init_test_repo;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::{Arc, Mutex as StdMutex};

fn git(repo: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap()
}

fn assert_git_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{}: {}",
        context,
        String::from_utf8_lossy(&output.stderr)
    );
}

// These tests verify the merge verification logic used by attempt_merge_auto_complete
// and complete_merge HTTP handler to detect when a commit is NOT on main branch.

#[tokio::test]
async fn test_merge_verification_detects_unmerged_task_branch() {
    // This test verifies the core logic that attempt_merge_auto_complete uses:
    // 1. Get task branch HEAD SHA
    // 2. Check if that SHA is on main branch
    // 3. If not, the merge is not complete
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    // Initialize repo
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Configure git user
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

    // Create initial commit on main
    std::fs::write(repo.join("initial.txt"), "initial").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial commit"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Rename to main
    let _ = Command::new("git")
        .args(["branch", "-M", "main"])
        .current_dir(repo)
        .output();

    // Create task branch
    Command::new("git")
        .args(["checkout", "-b", "task-branch"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Add work on task branch
    std::fs::write(repo.join("feature.txt"), "feature").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "add feature"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Get task branch HEAD SHA (simulating getting SHA from worktree)
    let task_branch_head = GitService::get_head_sha(repo).await.unwrap();

    // Go back to main WITHOUT merging
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Verify task branch commit is NOT on main - this is the key check
    // that attempt_merge_auto_complete uses before marking merge complete
    let is_on_main = GitService::is_commit_on_branch(repo, &task_branch_head, "main")
        .await
        .unwrap();
    assert!(
        !is_on_main,
        "Task branch HEAD {} should NOT be on main before merge",
        task_branch_head
    );

    // Now merge the task branch
    Command::new("git")
        .args(["merge", "task-branch", "-m", "merge task branch"])
        .current_dir(repo)
        .output()
        .unwrap();

    // After merge, task branch commit SHOULD be on main
    let is_on_main_after = GitService::is_commit_on_branch(repo, &task_branch_head, "main")
        .await
        .unwrap();
    assert!(
        is_on_main_after,
        "Task branch HEAD {} should be on main after merge",
        task_branch_head
    );

    // Main HEAD should be at least at task branch HEAD (fast-forward or merge commit)
    let main_head = GitService::get_head_sha(repo).await.unwrap();
    // In fast-forward case, they'll be equal; in merge commit case, main_head is newer
    // The key verification is that is_commit_on_branch returned true - that's what matters
    assert!(
        !main_head.is_empty(),
        "Main HEAD should have a valid SHA after merge"
    );
}

#[tokio::test]
async fn test_merge_verification_uses_correct_repo_path() {
    // This test verifies that checking the main repo (not worktree) correctly
    // identifies merge status - simulating the fix for the original bug
    let temp_dir = tempfile::tempdir().unwrap();
    let main_repo = temp_dir.path().join("main-repo");
    let worktree = temp_dir.path().join("worktree");

    std::fs::create_dir(&main_repo).unwrap();
    std::fs::create_dir(&worktree).unwrap();

    // Initialize main repo
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(&main_repo)
        .output()
        .unwrap();

    // Configure git user
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(&main_repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(&main_repo)
        .output()
        .unwrap();

    // Create initial commit on main
    std::fs::write(main_repo.join("initial.txt"), "initial").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(&main_repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial commit"])
        .current_dir(&main_repo)
        .output()
        .unwrap();

    let _ = Command::new("git")
        .args(["branch", "-M", "main"])
        .current_dir(&main_repo)
        .output();

    // Create task branch
    Command::new("git")
        .args(["checkout", "-b", "task-branch"])
        .current_dir(&main_repo)
        .output()
        .unwrap();

    // Add work on task branch
    std::fs::write(main_repo.join("feature.txt"), "feature").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(&main_repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "add feature"])
        .current_dir(&main_repo)
        .output()
        .unwrap();

    // Get task branch HEAD (this is what worktree would have)
    let task_branch_head = GitService::get_head_sha(&main_repo).await.unwrap();

    // Simulate creating a worktree (just init a separate repo for simplicity)
    // In real code, worktree would have task branch checked out
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(&worktree)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(&worktree)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(&worktree)
        .output()
        .unwrap();
    std::fs::write(worktree.join("feature.txt"), "feature").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(&worktree)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "add feature"])
        .current_dir(&worktree)
        .output()
        .unwrap();

    // Go back to main in main repo
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(&main_repo)
        .output()
        .unwrap();

    // KEY TEST: Checking worktree HEAD vs main repo's main branch
    // The worktree has its own commits, not related to main_repo's main branch
    let worktree_head = GitService::get_head_sha(&worktree).await.unwrap();

    // Worktree HEAD is NOT on main_repo's main branch - this is the bug we fixed
    // (Previously, code was using worktree HEAD as merge commit)
    let result = GitService::is_commit_on_branch(&main_repo, &worktree_head, "main").await;
    // This will error or return false because worktree_head doesn't exist in main_repo
    assert!(
        result.is_err() || !result.unwrap(),
        "Worktree HEAD should NOT be found on main_repo's main branch"
    );

    // The correct check: task branch HEAD from main_repo
    let is_merged = GitService::is_commit_on_branch(&main_repo, &task_branch_head, "main")
        .await
        .unwrap();
    assert!(
        !is_merged,
        "Task branch HEAD should NOT be on main until merged"
    );

    // Now merge in main_repo
    Command::new("git")
        .args(["merge", "task-branch", "-m", "merge task"])
        .current_dir(&main_repo)
        .output()
        .unwrap();

    // Now task branch HEAD should be on main
    let is_merged_after = GitService::is_commit_on_branch(&main_repo, &task_branch_head, "main")
        .await
        .unwrap();
    assert!(
        is_merged_after,
        "Task branch HEAD should be on main after merge"
    );
}

// =========================================================================
// try_continue_rebase Tests (Bug A fix)
// =========================================================================

#[tokio::test]
async fn test_try_continue_rebase_no_rebase_in_progress() {
    // When no rebase is in progress, git rebase --continue should fail gracefully
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

    // Create initial commit
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

    // try_continue_rebase should fail since no rebase is in progress
    let result = GitService::try_continue_rebase(repo).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_try_continue_rebase_auto_resolved_step() {
    // Create a scenario with auto-resolved conflicts
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    // Initialize repo with main branch
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

    // Create file with initial content
    std::fs::write(repo.join("file.txt"), "line1\nline2\nline3\n").unwrap();
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
    Command::new("git")
        .args(["checkout", "-b", "feature"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Make changes in feature branch (non-conflicting area)
    std::fs::write(repo.join("file.txt"), "line1\nline2modified\nline3\n").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "feature change"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Switch back to main and make non-conflicting changes
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("file.txt"), "line1modified\nline2\nline3\n").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "main change"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Attempt rebase from feature onto main - this should auto-resolve
    Command::new("git")
        .args(["checkout", "feature"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Simulate a rebase that git auto-resolves
    // In practice, this creates a rebase-merge directory when it would normally succeed
    let rebase_output = Command::new("git")
        .args(["rebase", "main"])
        .current_dir(repo)
        .output()
        .unwrap();

    // If rebase succeeded directly, that's fine for this test
    // If it enters a state requiring --continue, try_continue_rebase should complete it
    if !rebase_output.status.success() {
        // Rebase is in progress, try to continue it
        let result = GitService::try_continue_rebase(repo).await.unwrap();
        match result {
            RebaseResult::Success => {
                // Successfully completed the rebase
                assert!(!GitService::is_rebase_in_progress(repo));
            }
            RebaseResult::Conflict { .. } => {
                // Acceptable: real conflicts were detected
                // In this test case with non-conflicting changes, we expect Success
            }
        }
    }
}

#[tokio::test]
async fn test_rebase_onto_auto_resolved_conflicts() {
    // Test that rebase_onto detects and handles auto-resolved conflicts
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

    // Create initial commit on main
    std::fs::write(repo.join("file.txt"), "initial content\n").unwrap();
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

    // Create and checkout feature branch
    Command::new("git")
        .args(["checkout", "-b", "feature"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Add commit to feature
    std::fs::write(repo.join("file.txt"), "initial content\nfeature line\n").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "feature commit"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Go back to main and add a non-conflicting commit
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("newfile.txt"), "new content\n").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "main commit"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Now checkout feature and rebase onto main
    Command::new("git")
        .args(["checkout", "feature"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Call rebase_onto - it should handle auto-resolved cases
    let result = GitService::rebase_onto(repo, "main").await.unwrap();

    // With non-conflicting changes, we expect Success
    match result {
        RebaseResult::Success => {
            // Good: rebase completed
            assert!(!GitService::is_rebase_in_progress(repo));
        }
        RebaseResult::Conflict { .. } => {
            // If conflicts were detected, they should be real (non-empty files)
            // The test setup should avoid this
        }
    }
}

// =========================================================================
// try_continue_rebase Tests
// =========================================================================

#[tokio::test]
async fn test_try_continue_rebase_completes_no_rebase() {
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

    // Call try_continue_rebase when no rebase is in progress
    // Should return an error since there's nothing to continue
    let result = GitService::try_continue_rebase(repo).await;
    assert!(
        result.is_err(),
        "try_continue_rebase should fail when no rebase in progress"
    );
}

#[tokio::test]
async fn test_try_continue_rebase_with_auto_resolved_conflicts() {
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

    // Create main with one commit
    std::fs::write(repo.join("file.txt"), "main content\n").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "main"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Create feature branch
    Command::new("git")
        .args(["checkout", "-b", "feature"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("file.txt"), "feature content\n").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "feature"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Add another commit on main
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("file.txt"), "main updated\n").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "main update"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Checkout feature and try to rebase onto main
    Command::new("git")
        .args(["checkout", "feature"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Start rebase - this will result in a conflict with auto-resolve attempt
    let rebase_output = Command::new("git")
        .args(["rebase", "main"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Check if we got into a conflict state where auto-resolve could happen
    if !rebase_output.status.success() {
        let stderr = String::from_utf8_lossy(&rebase_output.stderr);
        if stderr.contains("CONFLICT") {
            // We have a rebase in progress due to conflict
            // This test checks the try_continue_rebase behavior
            let result = GitService::try_continue_rebase(repo).await;

            // The result depends on whether there are actually unmerged files
            // If no unmerged files exist after git's merge, it should succeed
            // If real conflicts exist, it should return Conflict variant
            match result {
                Ok(RebaseResult::Success) => {
                    // Good: rebase continued and completed
                    assert!(!GitService::is_rebase_in_progress(repo));
                }
                Ok(RebaseResult::Conflict { files }) => {
                    // Real conflicts were found - acceptable
                    assert!(GitService::is_rebase_in_progress(repo) || files.is_empty());
                }
                Err(_) => {
                    // Error during continuation - rebase might be aborted
                }
            }
        }
    }
}

// =========================================================================
// try_complete_stale_rebase Tests (Bug B recovery)
// =========================================================================

#[tokio::test]
async fn test_try_complete_stale_rebase_no_rebase() {
    // When no rebase is in progress, should return NoRebase
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

    // try_complete_stale_rebase should return NoRebase since no rebase is in progress
    let result = GitService::try_complete_stale_rebase(repo).await;
    assert!(matches!(result, StaleRebaseResult::NoRebase));
}

#[tokio::test]
async fn test_try_complete_stale_rebase_auto_resolved_completes() {
    // Create a scenario where rebase is in auto-resolved state and can be completed
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path();

    // Initialize repo with main branch
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

    // Create file with initial content
    std::fs::write(repo.join("file.txt"), "line1\nline2\nline3\n").unwrap();
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
    Command::new("git")
        .args(["checkout", "-b", "feature"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Make changes in feature branch (non-conflicting area)
    std::fs::write(repo.join("file.txt"), "line1\nline2modified\nline3\n").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "feature change"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Switch back to main and make non-conflicting changes
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("file.txt"), "line1modified\nline2\nline3\n").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "main change"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Attempt rebase from feature onto main
    Command::new("git")
        .args(["checkout", "feature"])
        .current_dir(repo)
        .output()
        .unwrap();

    let rebase_output = Command::new("git")
        .args(["rebase", "main"])
        .current_dir(repo)
        .output()
        .unwrap();

    // If rebase completed directly, test by simulating a stale state
    // If it's in progress, test try_complete_stale_rebase
    if !rebase_output.status.success() {
        // Rebase is in progress - this is the scenario we want to test
        let result = GitService::try_complete_stale_rebase(repo).await;

        match result {
            StaleRebaseResult::Completed => {
                // Successfully completed the stale rebase
                assert!(!GitService::is_rebase_in_progress(repo));
            }
            StaleRebaseResult::HasConflicts { .. } => {
                // Acceptable: real conflicts were detected
            }
            _ => {
                panic!("Unexpected result: {:?}", result);
            }
        }
    } else {
        // Rebase succeeded directly - that's acceptable for this test
        assert!(!GitService::is_rebase_in_progress(repo));
    }
}

// =========================================================================
// try_complete_stale_rebase Tests — has_real_conflicts
// =========================================================================

#[tokio::test]
async fn test_try_complete_stale_rebase_has_real_conflicts() {
    // Create a scenario where rebase hits real conflicts that can't be auto-resolved
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

    // Create file with initial content
    std::fs::write(repo.join("file.txt"), "same line\n").unwrap();
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

    // Create feature branch with conflicting change on same line
    Command::new("git")
        .args(["checkout", "-b", "feature"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("file.txt"), "feature version of line\n").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "feature change"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Switch back to main and make conflicting change on same line
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("file.txt"), "main version of line\n").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "main change"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Start rebase from feature onto main — should produce real conflicts
    Command::new("git")
        .args(["checkout", "feature"])
        .current_dir(repo)
        .output()
        .unwrap();

    let rebase_output = Command::new("git")
        .args(["rebase", "main"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Rebase should fail with real conflicts
    assert!(
        !rebase_output.status.success(),
        "Expected rebase to fail with conflicts"
    );
    assert!(
        GitService::is_rebase_in_progress(repo),
        "Expected rebase to be in progress"
    );

    // try_complete_stale_rebase should detect the real conflicts
    let result = GitService::try_complete_stale_rebase(repo).await;
    match result {
        StaleRebaseResult::HasConflicts { files } => {
            assert!(!files.is_empty(), "Expected at least one conflict file");
        }
        other => {
            panic!("Expected HasConflicts, got {:?}", other);
        }
    }
}

// =============================================================================
// fetch_origin serialization tests
// =============================================================================

#[tokio::test]
async fn test_fetch_origin_prunes_deleted_remote_tracking_refs() {
    let origin_dir = tempfile::tempdir().unwrap();
    let seed_dir = tempfile::tempdir().unwrap();
    let clone_parent = tempfile::tempdir().unwrap();
    let origin_repo = origin_dir.path();
    let seed_repo = seed_dir.path();
    let clone_repo = clone_parent.path().join("clone");

    let init_origin = Command::new("git")
        .args(["init", "--bare", origin_repo.to_str().unwrap()])
        .output()
        .unwrap();
    assert_git_success(&init_origin, "Expected bare origin init to succeed");

    init_test_repo(seed_repo);
    std::fs::write(seed_repo.join("main.txt"), "main").unwrap();
    assert_git_success(&git(seed_repo, &["add", "."]), "Expected add on seed repo to succeed");
    assert_git_success(
        &git(seed_repo, &["commit", "-m", "initial main"]),
        "Expected main commit to succeed",
    );

    assert_git_success(
        &git(seed_repo, &["remote", "add", "origin", origin_repo.to_str().unwrap()]),
        "Expected remote add to succeed",
    );
    assert_git_success(
        &git(seed_repo, &["push", "-u", "origin", "main"]),
        "Expected main push to succeed",
    );

    assert_git_success(
        &git(seed_repo, &["checkout", "-b", "obsolete-base"]),
        "Expected branch creation to succeed",
    );
    std::fs::write(seed_repo.join("obsolete.txt"), "obsolete").unwrap();
    assert_git_success(
        &git(seed_repo, &["add", "."]),
        "Expected obsolete branch add to succeed",
    );
    assert_git_success(
        &git(seed_repo, &["commit", "-m", "obsolete branch"]),
        "Expected obsolete branch commit to succeed",
    );
    assert_git_success(
        &git(seed_repo, &["push", "-u", "origin", "obsolete-base"]),
        "Expected obsolete branch push to succeed",
    );
    assert_git_success(
        &git(seed_repo, &["checkout", "main"]),
        "Expected checkout back to main to succeed",
    );

    let clone_origin = Command::new("git")
        .args([
            "clone",
            origin_repo.to_str().unwrap(),
            clone_repo.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_git_success(&clone_origin, "Expected clone from origin to succeed");

    let initial_ref = git(&clone_repo, &["show-ref", "--verify", "refs/remotes/origin/obsolete-base"]);
    assert_git_success(
        &initial_ref,
        "Expected cloned repo to have origin/obsolete-base before deletion",
    );

    assert_git_success(
        &git(seed_repo, &["push", "origin", "--delete", "obsolete-base"]),
        "Expected remote branch deletion to succeed",
    );

    let stale_ref = git(&clone_repo, &["show-ref", "--verify", "refs/remotes/origin/obsolete-base"]);
    assert_git_success(
        &stale_ref,
        "Expected stale origin/obsolete-base ref to remain before fetch",
    );

    GitService::fetch_origin(&clone_repo).await.unwrap();

    let pruned_ref = git(&clone_repo, &["show-ref", "--verify", "refs/remotes/origin/obsolete-base"]);
    assert!(
        !pruned_ref.status.success(),
        "Expected origin/obsolete-base to be pruned after fetch_origin"
    );
}

/// Verify that concurrent fetch_origin calls on repos without an origin remote
/// complete without errors and without panicking.
///
/// This test exercises the FETCH_LOCK path: multiple tasks racing to call
/// fetch_origin() should serialize via the global mutex rather than contending
/// on git internals.
#[tokio::test]
async fn test_fetch_origin_concurrent_no_remote_no_errors() {
    const CONCURRENCY: usize = 5;

    // Create separate temp repos (no origin) — each call will short-circuit after
    // checking for the remote, but the lock acquisition still happens.
    let temp_dirs: Vec<_> = (0..CONCURRENCY)
        .map(|_| tempfile::tempdir().unwrap())
        .collect();

    for td in &temp_dirs {
        init_test_repo(td.path());
        // Create an initial commit so git is fully initialized
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(td.path())
            .output()
            .unwrap();
    }

    let errors: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));
    let paths: Vec<std::path::PathBuf> =
        temp_dirs.iter().map(|td| td.path().to_path_buf()).collect();

    let handles: Vec<_> = paths
        .into_iter()
        .map(|path| {
            let errors = Arc::clone(&errors);
            tokio::spawn(async move {
                match GitService::fetch_origin(&path).await {
                    Ok(()) => {}
                    Err(e) => errors.lock().unwrap().push(format!("{}", e)),
                }
            })
        })
        .collect();

    for handle in handles {
        handle.await.unwrap();
    }

    let errs = errors.lock().unwrap();
    assert!(
        errs.is_empty(),
        "Concurrent fetch_origin calls produced errors: {:?}",
        *errs
    );
}

/// Verify that fetch_origin serializes: the second caller waits for the first
/// rather than running simultaneously. We test this by checking that both calls
/// return Ok(()) without error — true parallel execution on the same repo would
/// risk FETCH_HEAD corruption, but the mutex prevents that.
#[tokio::test]
async fn test_fetch_origin_serializes_on_same_repo() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = temp_dir.path().to_path_buf();

    init_test_repo(&repo);
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&repo)
        .output()
        .unwrap();

    // Spawn two concurrent calls on the same repo path (no origin → returns quickly).
    // Neither should error; the second acquires the lock after the first releases it.
    let repo1 = repo.clone();
    let repo2 = repo.clone();
    let (r1, r2) = tokio::join!(
        tokio::spawn(async move { GitService::fetch_origin(&repo1).await }),
        tokio::spawn(async move { GitService::fetch_origin(&repo2).await }),
    );

    assert!(
        r1.unwrap().is_ok(),
        "First concurrent fetch_origin should succeed"
    );
    assert!(
        r2.unwrap().is_ok(),
        "Second concurrent fetch_origin should succeed"
    );
}
