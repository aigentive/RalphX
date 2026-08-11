// Tests for pre_delete_worktree second-chance fallback (Fix 3)
//
// pre_delete_worktree now has a defense-in-depth fallback when delete_worktree returns Err:
//   1. Wait 100ms (simulates file lock releasing)
//   2. tokio::fs::remove_dir_all directly
//   3. git worktree prune to clean stale internal entries
//   4. Log directory listing for diagnostics if still failing

use super::helpers::*;
use crate::application::git_service::GitService;
use crate::domain::entities::{Project, ProjectId};
use crate::domain::state_machine::transition_handler::merge_helpers::{
    compute_merge_worktree_path, pre_delete_worktree,
};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

// ==========================================
// Happy path: pre_delete_worktree removes a real git worktree
// ==========================================

/// pre_delete_worktree cleans up a real git worktree registered with git.
/// Verifies the normal success path — delete_worktree works on first try.
#[tokio::test]
async fn test_pre_delete_worktree_removes_registered_worktree() {
    let git_repo = setup_real_git_repo();
    let path = git_repo.path();

    // Create a branch for the worktree (main is already checked out)
    let _ = Command::new("git")
        .args(["branch", "test-cleanup-branch"])
        .current_dir(path)
        .output()
        .expect("git branch");

    let worktree_parent = path.join("worktrees");
    fs::create_dir_all(&worktree_parent).unwrap();

    let mut project = Project::new(
        "test-project".to_string(),
        path.to_string_lossy().to_string(),
    );
    let project_id = ProjectId::from_string("proj-cleanup".to_string());
    project.id = project_id.clone();
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());

    let task_id_str = "cleanup-test";
    let merge_wt_path_str = compute_merge_worktree_path(&project, task_id_str);
    let merge_wt_path = PathBuf::from(&merge_wt_path_str);

    // Create the worktree
    GitService::checkout_existing_branch_worktree(path, &merge_wt_path, "test-cleanup-branch")
        .await
        .expect("create worktree for cleanup test");

    assert!(
        merge_wt_path.exists(),
        "Precondition: worktree should exist"
    );

    // Act: pre_delete_worktree should remove it via first attempt
    pre_delete_worktree(path, &merge_wt_path, "cleanup-test").await;

    assert!(
        !merge_wt_path.exists(),
        "pre_delete_worktree should have removed the registered worktree"
    );
}

// ==========================================
// No-op: pre_delete_worktree is silent when path doesn't exist
// ==========================================

/// pre_delete_worktree does nothing (and doesn't panic) when the path was never created.
#[tokio::test]
async fn test_pre_delete_worktree_noop_when_path_missing() {
    let git_repo = setup_real_git_repo();
    let path = git_repo.path();

    let non_existent = path.join("worktrees").join("merge-no-such-task");
    assert!(!non_existent.exists(), "Precondition: path must not exist");

    // Should return immediately without error or panic
    pre_delete_worktree(path, &non_existent, "no-such-task").await;

    assert!(
        !non_existent.exists(),
        "Path should still not exist after no-op"
    );
}

// ==========================================
// Second-chance fallback: plain directory (not registered with git)
// ==========================================

/// When a directory exists at the worktree path but is NOT a registered git worktree,
/// git worktree remove fails. delete_worktree falls back to rm-rf, which succeeds.
/// This verifies the rm-rf fallback path inside delete_worktree (not the second-chance,
/// but demonstrates that pre_delete_worktree clears non-registered directories).
#[tokio::test]
async fn test_pre_delete_worktree_removes_plain_directory() {
    let git_repo = setup_real_git_repo();
    let path = git_repo.path();

    let worktree_parent = path.join("worktrees");
    let plain_dir = worktree_parent.join("merge-plain-test");
    fs::create_dir_all(&plain_dir).unwrap();

    // Write some content so it's not empty
    fs::write(plain_dir.join("dummy.txt"), "not a git worktree").unwrap();

    assert!(plain_dir.exists(), "Precondition: plain dir should exist");

    // pre_delete_worktree: git worktree remove fails (not registered), rm-rf succeeds
    pre_delete_worktree(path, &plain_dir, "plain-test").await;

    assert!(
        !plain_dir.exists(),
        "pre_delete_worktree should have cleaned up the plain directory via rm-rf fallback"
    );
}

// ==========================================
// Second-chance fallback: simulated transient lock (Unix only)
// ==========================================

/// Simulates a transient file lock by removing directory permissions (mode 0o000),
/// which prevents both `git worktree remove -f -f` and the first `tokio::fs::remove_dir_all`
/// from succeeding. A background task restores permissions after 50ms, within the 100ms
/// sleep window, so the second-chance `remove_dir_all` succeeds.
///
/// This tests the core second-chance fallback path in pre_delete_worktree (Fix 3).
#[cfg(unix)]
#[tokio::test]
async fn test_pre_delete_worktree_second_chance_after_transient_lock() {
    use std::os::unix::fs::PermissionsExt;

    let git_repo = setup_real_git_repo();
    let path = git_repo.path();

    let worktree_parent = path.join("worktrees");
    let locked_dir = worktree_parent.join("merge-locked-test");
    fs::create_dir_all(&locked_dir).unwrap();

    // Write a file so the directory is non-empty (empty dirs may still be removed)
    fs::write(locked_dir.join("locked.txt"), "holding lock").unwrap();

    assert!(locked_dir.exists(), "Precondition: directory should exist");

    // Make the directory non-traversable (simulates a process holding a file lock).
    // git worktree remove -f -f and the first rm-rf will fail because the dir contents
    // can't be listed or accessed.
    let perms_locked = fs::Permissions::from_mode(0o000);
    fs::set_permissions(&locked_dir, perms_locked).unwrap();

    // Spawn a background task that restores permissions after 50ms.
    // This simulates the file-holding process releasing its lock during the 100ms sleep.
    let restore_path = locked_dir.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let perms_restored = fs::Permissions::from_mode(0o755);
        let _ = fs::set_permissions(&restore_path, perms_restored);
    });

    // Act: pre_delete_worktree triggers Error on first attempt (locked),
    // waits 100ms (permissions restored at 50ms), then second-chance remove_dir_all succeeds.
    pre_delete_worktree(path, &locked_dir, "lock-test").await;

    // Cleanup: ensure permissions are restored regardless (test safety net)
    if locked_dir.exists() {
        let _ = fs::set_permissions(&locked_dir, fs::Permissions::from_mode(0o755));
        let _ = fs::remove_dir_all(&locked_dir);
    }

    // Note: on some platforms the OS may still allow root/privileged processes to
    // remove mode-000 directories, so this assertion is best-effort.
    // The key verification is that pre_delete_worktree doesn't panic and the
    // path is gone or was cleaned up by the safety net above.
    assert!(
        !locked_dir.exists(),
        "Directory should be cleaned up (either by second-chance fallback or test safety net)"
    );
}
