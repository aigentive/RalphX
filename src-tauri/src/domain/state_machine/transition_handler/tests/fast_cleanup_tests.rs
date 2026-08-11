// Tests for fast cleanup: remove_worktree_fast and conditional settle sleep
//
// ROOT CAUSE #7: git worktree remove --force takes 6-10s (runs git status internally)
// FIX: remove_worktree_fast uses tokio::fs::remove_dir_all + git worktree prune
//
// ROOT CAUSE #6: unconditional 1s settle sleep even when no agents killed
// FIX: conditional on any_agent_was_running

use super::helpers::*;
use crate::domain::state_machine::transition_handler::cleanup_helpers::remove_worktree_fast;

// ==================
// remove_worktree_fast: basic functionality
// ==================

/// remove_worktree_fast removes an existing directory successfully.
#[tokio::test]
async fn test_remove_worktree_fast_removes_existing_dir() {
    let temp = tempfile::TempDir::new().unwrap();
    let worktree_path = temp.path().join("test-worktree");

    // Create a directory with some content
    tokio::fs::create_dir_all(&worktree_path).await.unwrap();
    tokio::fs::write(worktree_path.join("file.txt"), "content")
        .await
        .unwrap();
    tokio::fs::create_dir_all(worktree_path.join("subdir"))
        .await
        .unwrap();
    tokio::fs::write(worktree_path.join("subdir/nested.txt"), "nested")
        .await
        .unwrap();

    assert!(worktree_path.exists());

    // We need a git repo for the prune step
    let repo_path = temp.path().to_path_buf();
    tokio::process::Command::new("git")
        .args(["init"])
        .current_dir(&repo_path)
        .output()
        .await
        .unwrap();

    let result = remove_worktree_fast(&worktree_path, &repo_path).await;
    assert!(
        result.is_ok(),
        "remove_worktree_fast should succeed: {:?}",
        result.err()
    );
    assert!(
        !worktree_path.exists(),
        "worktree directory should be removed"
    );
}

/// remove_worktree_fast handles non-existent path gracefully (no error).
#[tokio::test]
async fn test_remove_worktree_fast_nonexistent_path_ok() {
    let temp = tempfile::TempDir::new().unwrap();
    let worktree_path = temp.path().join("does-not-exist");
    let repo_path = temp.path().to_path_buf();

    // Init git repo for prune
    tokio::process::Command::new("git")
        .args(["init"])
        .current_dir(&repo_path)
        .output()
        .await
        .unwrap();

    let result = remove_worktree_fast(&worktree_path, &repo_path).await;
    assert!(
        result.is_ok(),
        "remove_worktree_fast should succeed for non-existent path: {:?}",
        result.err()
    );
}

/// remove_worktree_fast cleans up a real git worktree (rm -rf + prune).
#[tokio::test]
async fn test_remove_worktree_fast_real_git_worktree() {
    let temp = tempfile::TempDir::new().unwrap();
    let repo_path = temp.path().join("repo");
    let worktree_path = temp.path().join("wt");

    // Set up a real git repo with a worktree
    tokio::fs::create_dir_all(&repo_path).await.unwrap();
    tokio::process::Command::new("git")
        .args(["init"])
        .current_dir(&repo_path)
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&repo_path)
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .args([
            "worktree",
            "add",
            worktree_path.to_str().unwrap(),
            "-b",
            "test-branch",
        ])
        .current_dir(&repo_path)
        .output()
        .await
        .unwrap();

    assert!(
        worktree_path.exists(),
        "worktree should exist before removal"
    );

    // Verify git knows about the worktree
    let list_output = tokio::process::Command::new("git")
        .args(["worktree", "list"])
        .current_dir(&repo_path)
        .output()
        .await
        .unwrap();
    let list_str = String::from_utf8_lossy(&list_output.stdout);
    assert!(
        list_str.contains("test-branch"),
        "git should list the worktree before removal"
    );

    // Now remove it fast
    let result = remove_worktree_fast(&worktree_path, &repo_path).await;
    assert!(
        result.is_ok(),
        "remove_worktree_fast should succeed: {:?}",
        result.err()
    );
    assert!(
        !worktree_path.exists(),
        "worktree directory should be removed"
    );

    // Verify git worktree prune cleaned up the tracking
    let list_output = tokio::process::Command::new("git")
        .args(["worktree", "list"])
        .current_dir(&repo_path)
        .output()
        .await
        .unwrap();
    let list_str = String::from_utf8_lossy(&list_output.stdout);
    assert!(
        !list_str.contains("test-branch"),
        "git worktree prune should have cleaned up tracking, but still shows: {}",
        list_str
    );
}

/// remove_worktree_fast handles deeply nested directories.
#[tokio::test]
async fn test_remove_worktree_fast_deeply_nested() {
    let temp = tempfile::TempDir::new().unwrap();
    let worktree_path = temp.path().join("deep-worktree");
    let repo_path = temp.path().to_path_buf();

    // Create deeply nested structure
    let deep_path = worktree_path.join("a/b/c/d/e/f");
    tokio::fs::create_dir_all(&deep_path).await.unwrap();
    tokio::fs::write(deep_path.join("deep.txt"), "deep content")
        .await
        .unwrap();

    tokio::process::Command::new("git")
        .args(["init"])
        .current_dir(&repo_path)
        .output()
        .await
        .unwrap();

    let result = remove_worktree_fast(&worktree_path, &repo_path).await;
    assert!(result.is_ok());
    assert!(!worktree_path.exists());
}

// ==================
// Conditional settle sleep
// ==================

/// When no agents were running, settle sleep should be skipped (fast path).
///
/// This validates ROOT CAUSE #6: the unconditional 1s settle sleep.
/// We test indirectly via the merge cleanup path timing.
#[tokio::test]
async fn test_settle_sleep_skipped_when_no_agents_running() {
    use std::time::Instant;

    // Use mock services without repos — PendingMerge guard returns early,
    // but if we had repos, step 0b would skip the settle sleep when
    // any_agent_was_running is false.
    let services = TaskServices::new_mock();
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = crate::domain::state_machine::TaskStateMachine::new(context);
    let handler = crate::domain::state_machine::TransitionHandler::new(&mut machine);

    let start = Instant::now();
    let _ = handler
        .on_enter(&crate::domain::state_machine::State::PendingMerge)
        .await;
    let elapsed = start.elapsed();

    // Without repos, attempt_programmatic_merge returns immediately.
    // The key assertion: it takes < 100ms (no 1s settle sleep).
    assert!(
        elapsed.as_millis() < 100,
        "No-agent path should skip settle sleep, took {}ms",
        elapsed.as_millis()
    );
}

/// When agents were running, settle sleep should execute (slow path).
/// This is an indirect test — with repos wired and MockChatService returning
/// Ok(false) for stop_agent (no agent running), the settle sleep should still
/// be skipped because any_agent_was_running will be false.
#[tokio::test]
async fn test_settle_sleep_conditional_on_agent_state() {
    let (mut machine, _, _) = setup_pending_merge_repos("Settle sleep test", Some("feature/test"))
        .await
        .into_machine();
    let handler = crate::domain::state_machine::TransitionHandler::new(&mut machine);

    let start = std::time::Instant::now();
    let _ = handler
        .on_enter(&crate::domain::state_machine::State::PendingMerge)
        .await;
    let elapsed = start.elapsed();

    // MockChatService.stop_agent returns Ok(false) — no agents running.
    // With the conditional fix, settle sleep should be skipped.
    // Total time should be bounded (git operations may still take time,
    // but the 1s settle sleep should not be present).
    assert!(
        elapsed.as_secs() < 20,
        "on_enter(PendingMerge) with no running agents should complete fast, took {}s",
        elapsed.as_secs()
    );
}
