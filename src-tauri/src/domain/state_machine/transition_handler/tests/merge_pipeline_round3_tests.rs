// Integration tests for merge pipeline round 3 fixes.
//
// Tests:
//   Bug A1: lsof +d (non-recursive) switch + skip-when-no-agents logic
//   Bug A2: os_thread_timeout correctness (fires independently of tokio timer driver)
//   Bug A3: spawn_blocking for SQLite (covered by existing sqlite_running_agent_registry_tests)
//   Bug B: merge watcher no longer uses consecutive_clean (covered by chat_service_merge_tests)
//   Bug C: has_prior_rebase_conflict() and dispatch strategy override
//   Bug D: force delete (-D) for feature branches after squash merge

use crate::domain::entities::Task;
use crate::domain::state_machine::transition_handler::cleanup_helpers::{
    os_thread_timeout, run_cleanup_step, CleanupStepResult,
};
use crate::domain::state_machine::transition_handler::merge_helpers::has_prior_rebase_conflict;
use std::time::Duration;

// ──────────────────────────────────────────────────────────────────────────────
// Bug A1: lsof +d (non-recursive) — unit tests for collect_pids functions
// ──────────────────────────────────────────────────────────────────────────────

/// lsof +d flag is used (non-recursive) in the sync pid collector.
/// We verify the function doesn't panic on a valid temp directory.
#[test]
fn test_collect_pids_in_worktree_runs_without_panic() {
    let dir = tempfile::tempdir().unwrap();
    // Just verifying the function completes — no processes have files open in a fresh tempdir.
    // kill_worktree_processes returns () — it should complete without panic.
    crate::domain::services::running_agent_registry::kill_worktree_processes(dir.path());
}

/// lsof +d flag is used (non-recursive) in the async pid collector.
/// We verify the async function completes with a short timeout on a valid temp directory.
#[tokio::test]
async fn test_collect_pids_in_worktree_async_completes() {
    let dir = tempfile::tempdir().unwrap();
    // With +d (non-recursive), this should complete quickly even on dirs with subdirectories.
    crate::domain::services::kill_worktree_processes_async(dir.path(), 5, false).await;
    // No panic, no hang — test passes.
}

// ──────────────────────────────────────────────────────────────────────────────
// Bug A2: os_thread_timeout — fires independently of tokio timer driver
// ──────────────────────────────────────────────────────────────────────────────

/// os_thread_timeout returns the future's result when it completes before the deadline.
#[tokio::test]
async fn test_os_thread_timeout_returns_value_on_success() {
    let result = os_thread_timeout(Duration::from_secs(5), async { 42 }).await;
    assert_eq!(result.unwrap(), 42);
}

/// os_thread_timeout returns OsTimeoutElapsed when the future exceeds the deadline.
#[tokio::test]
async fn test_os_thread_timeout_fires_on_slow_future() {
    let result = os_thread_timeout(
        Duration::from_millis(50),
        // This future will never complete — it sleeps for a very long time.
        tokio::time::sleep(Duration::from_secs(60)),
    )
    .await;
    assert!(result.is_err(), "Should return OsTimeoutElapsed");
}

/// os_thread_timeout fires even when the inner future never resolves.
/// This is the KEY property: it uses OS threads, not tokio timers.
/// Simulates a hung future (e.g., lsof in kernel D-state blocking the tokio thread).
#[tokio::test]
async fn test_os_thread_timeout_fires_on_pending_future() {
    // std::future::pending() never completes — simulates a genuinely hung operation.
    // The OS thread fires after 100ms real time, regardless of the future's state.
    let result =
        os_thread_timeout(Duration::from_millis(100), std::future::pending::<&str>()).await;

    assert!(
        result.is_err(),
        "os_thread_timeout must fire when the inner future never resolves"
    );
}

/// run_cleanup_step returns CleanupStepResult::Ok on success.
#[tokio::test]
async fn test_run_cleanup_step_success() {
    let result = run_cleanup_step("test_step", 5, "task-123", async { Ok::<(), String>(()) }).await;
    assert!(matches!(result, CleanupStepResult::Ok));
}

/// run_cleanup_step returns CleanupStepResult::Error on failure.
#[tokio::test]
async fn test_run_cleanup_step_error() {
    let result = run_cleanup_step("test_step", 5, "task-123", async {
        Err::<(), String>("something went wrong".to_string())
    })
    .await;
    assert!(matches!(result, CleanupStepResult::Error { .. }));
    if let CleanupStepResult::Error { message } = result {
        assert_eq!(message, "something went wrong");
    }
}

/// run_cleanup_step returns CleanupStepResult::TimedOut when the operation exceeds its deadline.
#[tokio::test]
async fn test_run_cleanup_step_timeout() {
    let result = run_cleanup_step(
        "slow_step",
        1, // 1 second timeout
        "task-456",
        async {
            // Sleep longer than the timeout — this uses OS-thread timeout internally
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok::<(), String>(())
        },
    )
    .await;
    assert!(
        matches!(result, CleanupStepResult::TimedOut { .. }),
        "Expected TimedOut, got: {:?}",
        result
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Bug C: has_prior_rebase_conflict() — strategy override for RebaseSquash
// ──────────────────────────────────────────────────────────────────────────────

/// has_prior_rebase_conflict returns true when metadata contains conflict_type: "rebase".
#[test]
fn test_has_prior_rebase_conflict_true() {
    let project_id = crate::domain::entities::ProjectId::from_string("p1".to_string());
    let mut task = Task::new(project_id, "Test".to_string());
    task.metadata = Some(
        serde_json::json!({
            "conflict_type": "rebase",
            "conflict_files": ["src/main.rs"]
        })
        .to_string(),
    );
    assert!(has_prior_rebase_conflict(&task));
}

/// has_prior_rebase_conflict returns false when metadata has no conflict_type.
#[test]
fn test_has_prior_rebase_conflict_false_no_conflict_type() {
    let project_id = crate::domain::entities::ProjectId::from_string("p1".to_string());
    let mut task = Task::new(project_id, "Test".to_string());
    task.metadata = Some(serde_json::json!({"some_key": "value"}).to_string());
    assert!(!has_prior_rebase_conflict(&task));
}

/// has_prior_rebase_conflict returns false when conflict_type is not "rebase".
#[test]
fn test_has_prior_rebase_conflict_false_different_type() {
    let project_id = crate::domain::entities::ProjectId::from_string("p1".to_string());
    let mut task = Task::new(project_id, "Test".to_string());
    task.metadata = Some(serde_json::json!({"conflict_type": "merge"}).to_string());
    assert!(!has_prior_rebase_conflict(&task));
}

/// has_prior_rebase_conflict returns false when task has no metadata.
#[test]
fn test_has_prior_rebase_conflict_false_no_metadata() {
    let project_id = crate::domain::entities::ProjectId::from_string("p1".to_string());
    let task = Task::new(project_id, "Test".to_string());
    assert!(!has_prior_rebase_conflict(&task));
}

// ──────────────────────────────────────────────────────────────────────────────
// Bug D: force delete (-D) for feature branches after squash merge
// ──────────────────────────────────────────────────────────────────────────────

/// delete_feature_branch uses force delete (-D), which should succeed even when
/// the branch was squash-merged (not a merge ancestor of the target).
#[tokio::test]
async fn test_delete_feature_branch_force_deletes_squash_merged_branch() {
    use crate::application::GitService;

    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    // Init repo
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(repo)
        .output()
        .expect("git init");
    std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("README.md"), "# test\n").unwrap();
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "Initial"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["branch", "-M", "main"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Create a feature branch with a commit
    std::process::Command::new("git")
        .args(["checkout", "-b", "feature/test"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("feature.txt"), "feature\n").unwrap();
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "Add feature"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Switch back to main and squash-merge the feature
    std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["merge", "--squash", "feature/test"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "Squash merge feature/test"])
        .current_dir(repo)
        .output()
        .unwrap();

    // Verify that `git branch -d` (safe delete) would FAIL for a squash-merged branch
    let safe_delete = std::process::Command::new("git")
        .args(["branch", "-d", "feature/test"])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        !safe_delete.status.success(),
        "git branch -d should FAIL for squash-merged branches (not fully merged ancestor)"
    );

    // Verify that delete_feature_branch (force -D) succeeds
    let result = GitService::delete_feature_branch(repo, "feature/test").await;
    assert!(
        result.is_ok(),
        "delete_feature_branch should succeed with -D flag: {:?}",
        result.err()
    );

    // Verify branch is actually gone
    assert!(
        !GitService::branch_exists(repo, "feature/test")
            .await
            .unwrap_or(true),
        "Branch should be deleted after delete_feature_branch"
    );
}
