// Tests for locked worktree handling: unlock + double-force removal
//
// ROOT CAUSE: `git worktree remove --force` does NOT override locks (only bypasses
// dirty-tree checks). Locked worktrees require either `git worktree unlock` first
// OR `git worktree remove -f -f` (double-force, git 2.17+).
//
// Empirical findings (verified locally):
// - `--force` on locked worktree: exit code 128 "cannot remove a locked working tree"
// - `-f -f` on locked worktree: exit code 0 (succeeds)
// - `git worktree unlock <deleted-path>`: exit code 0 (succeeds! enables prune cleanup)
// - `git worktree unlock <unlocked-path>`: exit code 128 "is not locked" (harmless, ignored)
// - After unlock on deleted path: `git worktree prune` cleans up stale metadata successfully

use crate::domain::state_machine::transition_handler::cleanup_helpers::{
    git_worktree_prune, remove_worktree_fast,
};

/// Helper: set up a real git repo with an empty initial commit.
async fn setup_repo(path: &std::path::Path) {
    tokio::process::Command::new("git")
        .args(["init"])
        .current_dir(path)
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(path)
        .output()
        .await
        .unwrap();
}

/// Helper: add a worktree and return true if it was added successfully.
async fn add_worktree(repo: &std::path::Path, wt: &std::path::Path, branch: &str) -> bool {
    let out = tokio::process::Command::new("git")
        .args(["worktree", "add", wt.to_str().unwrap(), "-b", branch])
        .current_dir(repo)
        .output()
        .await
        .unwrap();
    out.status.success()
}

/// Helper: lock a worktree.
async fn lock_worktree(repo: &std::path::Path, wt: &std::path::Path) {
    tokio::process::Command::new("git")
        .args(["worktree", "lock", wt.to_str().unwrap()])
        .current_dir(repo)
        .output()
        .await
        .unwrap();
}

/// Helper: check if a branch appears in `git worktree list` output.
async fn worktree_list_contains(repo: &std::path::Path, needle: &str) -> bool {
    let out = tokio::process::Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo)
        .output()
        .await
        .unwrap();
    String::from_utf8_lossy(&out.stdout).contains(needle)
}

// ==================
// Empirical verification: git worktree remove behavior on locked worktrees
// ==================

/// Verify that `git worktree remove --force` FAILS on a locked worktree.
/// This documents the root cause: single-force does NOT override locks.
#[tokio::test]
async fn test_single_force_fails_on_locked_worktree() {
    let temp = tempfile::TempDir::new().unwrap();
    let repo_path = temp.path().join("repo");
    let wt_path = temp.path().join("wt");

    tokio::fs::create_dir_all(&repo_path).await.unwrap();
    setup_repo(&repo_path).await;
    assert!(add_worktree(&repo_path, &wt_path, "test-branch").await);
    lock_worktree(&repo_path, &wt_path).await;

    // Single --force should FAIL on locked worktree
    let out = tokio::process::Command::new("git")
        .args(["worktree", "remove", "--force", wt_path.to_str().unwrap()])
        .current_dir(&repo_path)
        .output()
        .await
        .unwrap();

    assert!(
        !out.status.success(),
        "Single --force should fail on locked worktree (root cause RC1)"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("locked"),
        "Error should mention 'locked': {}",
        stderr
    );
    // Worktree still exists
    assert!(
        wt_path.exists(),
        "Locked worktree should still exist after failed remove"
    );
}

/// Verify that `git worktree remove -f -f` SUCCEEDS on a locked worktree.
/// This documents the fix: double-force atomically overrides locks.
#[tokio::test]
async fn test_double_force_succeeds_on_locked_worktree() {
    let temp = tempfile::TempDir::new().unwrap();
    let repo_path = temp.path().join("repo");
    let wt_path = temp.path().join("wt");

    tokio::fs::create_dir_all(&repo_path).await.unwrap();
    setup_repo(&repo_path).await;
    assert!(add_worktree(&repo_path, &wt_path, "test-branch").await);
    lock_worktree(&repo_path, &wt_path).await;

    // Double -f -f should SUCCEED on locked worktree
    let out = tokio::process::Command::new("git")
        .args(["worktree", "remove", "-f", "-f", wt_path.to_str().unwrap()])
        .current_dir(&repo_path)
        .output()
        .await
        .unwrap();

    assert!(
        out.status.success(),
        "Double -f -f should succeed on locked worktree. stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !wt_path.exists(),
        "Worktree directory should be removed after double-force"
    );
}

/// Verify that `git worktree unlock <deleted-path>` SUCCEEDS (exit 0).
/// This enables prune to clean up stale locked metadata entries.
#[tokio::test]
async fn test_unlock_on_deleted_path_succeeds() {
    let temp = tempfile::TempDir::new().unwrap();
    let repo_path = temp.path().join("repo");
    let wt_path = temp.path().join("wt");

    tokio::fs::create_dir_all(&repo_path).await.unwrap();
    setup_repo(&repo_path).await;
    assert!(add_worktree(&repo_path, &wt_path, "test-branch").await);
    lock_worktree(&repo_path, &wt_path).await;

    // Delete the directory (simulates SIGKILL aftermath — directory gone, metadata locked)
    tokio::fs::remove_dir_all(&wt_path).await.unwrap();
    assert!(!wt_path.exists(), "Directory should be gone");

    // Unlock on the deleted path should SUCCEED (exit 0)
    let out = tokio::process::Command::new("git")
        .args(["worktree", "unlock", wt_path.to_str().unwrap()])
        .current_dir(&repo_path)
        .output()
        .await
        .unwrap();

    assert!(
        out.status.success(),
        "git worktree unlock on deleted path should succeed (exit 0). stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Verify that `git worktree unlock <unlocked-path>` returns non-zero ("not locked").
/// This documents why we use `let _ =` to ignore errors from the unlock call.
#[tokio::test]
async fn test_unlock_on_already_unlocked_worktree_fails_harmlessly() {
    let temp = tempfile::TempDir::new().unwrap();
    let repo_path = temp.path().join("repo");
    let wt_path = temp.path().join("wt");

    tokio::fs::create_dir_all(&repo_path).await.unwrap();
    setup_repo(&repo_path).await;
    assert!(add_worktree(&repo_path, &wt_path, "test-branch").await);
    // Do NOT lock — worktree is unlocked

    let out = tokio::process::Command::new("git")
        .args(["worktree", "unlock", wt_path.to_str().unwrap()])
        .current_dir(&repo_path)
        .output()
        .await
        .unwrap();

    // Should fail with "is not locked" — this is why errors are ignored via `let _ =`
    assert!(
        !out.status.success(),
        "Unlock on already-unlocked worktree should fail"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("not locked"),
        "Error should say 'not locked': {}",
        stderr
    );
    // The worktree is still intact (unlock failure is harmless)
    assert!(wt_path.exists(), "Worktree should still exist");
}

/// Verify prune cleans stale locked metadata after unlock on deleted path.
/// This is the full unlock→delete→prune cycle for stale locked entries.
#[tokio::test]
async fn test_prune_cleans_stale_locked_metadata_after_unlock() {
    let temp = tempfile::TempDir::new().unwrap();
    let repo_path = temp.path().join("repo");
    let wt_path = temp.path().join("wt");

    tokio::fs::create_dir_all(&repo_path).await.unwrap();
    setup_repo(&repo_path).await;
    assert!(add_worktree(&repo_path, &wt_path, "test-branch").await);
    lock_worktree(&repo_path, &wt_path).await;

    // Verify git knows about the locked worktree
    assert!(
        worktree_list_contains(&repo_path, "test-branch").await,
        "git should list locked worktree before cleanup"
    );

    // Delete the directory (simulate SIGKILL — directory gone, metadata still locked)
    tokio::fs::remove_dir_all(&wt_path).await.unwrap();

    // Without unlock: prune would skip the locked entry
    // With unlock first: prune should clean it up
    let unlock_out = tokio::process::Command::new("git")
        .args(["worktree", "unlock", wt_path.to_str().unwrap()])
        .current_dir(&repo_path)
        .output()
        .await
        .unwrap();
    assert!(
        unlock_out.status.success(),
        "unlock on deleted path should succeed"
    );

    git_worktree_prune(&repo_path).await;

    // After unlock + prune, stale metadata should be gone
    assert!(
        !worktree_list_contains(&repo_path, "test-branch").await,
        "Stale locked metadata should be cleaned up after unlock + prune"
    );
}

// ==================
// remove_worktree_fast: locked worktree scenarios
// ==================

/// remove_worktree_fast handles a locked worktree via unlock + double-force.
#[tokio::test]
async fn test_remove_worktree_fast_locked_worktree_succeeds() {
    let temp = tempfile::TempDir::new().unwrap();
    let repo_path = temp.path().join("repo");
    let wt_path = temp.path().join("wt");

    tokio::fs::create_dir_all(&repo_path).await.unwrap();
    setup_repo(&repo_path).await;
    assert!(add_worktree(&repo_path, &wt_path, "test-branch").await);
    lock_worktree(&repo_path, &wt_path).await;

    assert!(
        wt_path.exists(),
        "Locked worktree should exist before removal"
    );

    let result = remove_worktree_fast(&wt_path, &repo_path).await;
    assert!(
        result.is_ok(),
        "remove_worktree_fast should succeed on locked worktree: {:?}",
        result.err()
    );
    assert!(
        !wt_path.exists(),
        "Locked worktree directory should be removed"
    );

    // Git metadata should be cleaned up
    assert!(
        !worktree_list_contains(&repo_path, "test-branch").await,
        "Git metadata should be cleaned up after remove_worktree_fast on locked worktree"
    );
}

/// remove_worktree_fast on an already-unlocked worktree: unlock fails silently, remove succeeds.
#[tokio::test]
async fn test_remove_worktree_fast_unlocked_worktree_succeeds() {
    let temp = tempfile::TempDir::new().unwrap();
    let repo_path = temp.path().join("repo");
    let wt_path = temp.path().join("wt");

    tokio::fs::create_dir_all(&repo_path).await.unwrap();
    setup_repo(&repo_path).await;
    assert!(add_worktree(&repo_path, &wt_path, "test-branch").await);
    // Do NOT lock — test that unlock-error is silently ignored

    let result = remove_worktree_fast(&wt_path, &repo_path).await;
    assert!(
        result.is_ok(),
        "remove_worktree_fast should succeed even when unlock returns 'not locked': {:?}",
        result.err()
    );
    assert!(!wt_path.exists(), "Unlocked worktree should be removed");
    assert!(
        !worktree_list_contains(&repo_path, "test-branch").await,
        "Git metadata should be cleaned up"
    );
}
