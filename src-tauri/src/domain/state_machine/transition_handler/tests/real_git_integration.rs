// Real git repo integration tests for merge strategy dispatch
//
// These tests create actual git repositories in temp directories so that
// merge strategy dispatch is genuinely exercised (not blocked by a
// nonexistent repo path like the tests in test_quality_overhaul.rs).
//
// Key difference from existing tests:
// - `setup_pending_merge_with_real_repo()` wires the project to a real git dir
// - Merge code path reaches `pre_merge_cleanup()` AND strategy dispatch
// - Git log is checked post-merge to verify commits landed on `main`

use super::helpers::*;
use crate::domain::entities::{InternalStatus, MergeStrategy};
use crate::domain::state_machine::{State, TransitionHandler};

/// Verify a fast-forward merge (Merge strategy) succeeds end-to-end with a real git repo.
///
/// Setup: main has 1 commit, task branch has 1 additional commit (no divergence).
/// Expected: checkout-free merge succeeds, task transitions to Merged, git log on
/// main shows the task branch commit.
#[tokio::test]
async fn test_fast_forward_merge_success_with_real_repo() {
    let git_repo = setup_real_git_repo();
    let setup = setup_pending_merge_with_real_repo(
        "FF merge test",
        &git_repo.task_branch,
        &git_repo.path_string(),
        MergeStrategy::Merge,
    )
    .await;

    let task_id = setup.task_id.clone();
    let task_repo = Arc::clone(&setup.task_repo);
    let (mut machine, _task_repo, _task_id) = setup.into_machine();
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::Merged,
        "Task should be Merged after successful fast-forward merge, got {:?}. Metadata: {:?}",
        updated_task.internal_status,
        updated_task.metadata,
    );

    // Verify the feature commit is on main by checking git log
    let log_output = std::process::Command::new("git")
        .args(["log", "--oneline", "main"])
        .current_dir(git_repo.path())
        .output()
        .expect("git log");
    let log_str = String::from_utf8_lossy(&log_output.stdout);
    assert!(
        log_str.contains("add feature") || log_str.contains("feature"),
        "Git log on main should contain the task branch commit. Log:\n{}",
        log_str,
    );
}

/// Verify that merge code actually reaches strategy dispatch (not just the early-return guard).
///
/// With a real git repo, the merge path goes through:
///   1. pre_merge_cleanup (stop agents, clean worktrees)
///   2. strategy dispatch (checkout-free merge since main is checked out)
///   3. handle_merge_outcome → complete_merge_internal
///
/// We verify by checking that the merge commit SHA is on the main branch after
/// the merge, proving the strategy was actually dispatched and completed.
#[tokio::test]
async fn test_merge_reaches_strategy_dispatch_with_real_repo() {
    let git_repo = setup_real_git_repo();

    // Record main's HEAD SHA before merge
    let pre_merge_sha = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(git_repo.path())
        .output()
        .expect("git rev-parse");
    let pre_merge_sha = String::from_utf8_lossy(&pre_merge_sha.stdout)
        .trim()
        .to_string();

    let setup = setup_pending_merge_with_real_repo(
        "Strategy dispatch test",
        &git_repo.task_branch,
        &git_repo.path_string(),
        MergeStrategy::Merge,
    )
    .await;

    let task_id = setup.task_id.clone();
    let task_repo = Arc::clone(&setup.task_repo);
    let (mut machine, _task_repo, _task_id) = setup.into_machine();
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    // Verify task reached Merged
    let updated_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::Merged,
        "Task should be Merged, got {:?}. Metadata: {:?}",
        updated_task.internal_status,
        updated_task.metadata,
    );

    // Verify main's HEAD has advanced (merge commit or ff to task branch tip)
    let post_merge_sha = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(git_repo.path())
        .output()
        .expect("git rev-parse");
    let post_merge_sha = String::from_utf8_lossy(&post_merge_sha.stdout)
        .trim()
        .to_string();

    assert_ne!(
        pre_merge_sha, post_merge_sha,
        "main HEAD should advance after merge (strategy was dispatched)"
    );
}

/// Verify squash merge strategy works with a real git repo.
///
/// Squash merges condense all task branch commits into a single commit on main.
/// This tests the checkout-free squash path (since main is the checked-out branch).
#[tokio::test]
async fn test_squash_merge_success_with_real_repo() {
    let git_repo = setup_real_git_repo();

    let setup = setup_pending_merge_with_real_repo(
        "Squash merge test",
        &git_repo.task_branch,
        &git_repo.path_string(),
        MergeStrategy::Squash,
    )
    .await;

    let task_id = setup.task_id.clone();
    let task_repo = Arc::clone(&setup.task_repo);
    let (mut machine, _task_repo, _task_id) = setup.into_machine();
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::Merged,
        "Task should be Merged after squash merge, got {:?}. Metadata: {:?}",
        updated_task.internal_status,
        updated_task.metadata,
    );

    // Verify feature file exists in working tree after squash merge
    assert!(
        git_repo.path().join("feature.rs").exists(),
        "feature.rs should exist on main after squash merge"
    );
}

/// Verify merge with a nonexistent source branch transitions to MergeIncomplete
/// (not stuck in PendingMerge), even with a real git repo.
#[tokio::test]
async fn test_merge_missing_source_branch_with_real_repo() {
    let git_repo = setup_real_git_repo();

    let setup = setup_pending_merge_with_real_repo(
        "Missing branch test",
        "nonexistent/branch",
        &git_repo.path_string(),
        MergeStrategy::Merge,
    )
    .await;

    let task_id = setup.task_id.clone();
    let task_repo = Arc::clone(&setup.task_repo);
    let (mut machine, _task_repo, _task_id) = setup.into_machine();
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::MergeIncomplete,
        "Missing source branch should produce MergeIncomplete, got {:?}",
        updated_task.internal_status,
    );

    // Verify metadata contains branch_missing indicator
    let meta: serde_json::Value =
        serde_json::from_str(updated_task.metadata.as_deref().unwrap_or("{}")).unwrap();
    assert_eq!(
        meta.get("branch_missing"),
        Some(&serde_json::json!(true)),
        "Metadata should indicate branch_missing. Metadata: {:?}",
        updated_task.metadata,
    );
}

/// Verify the dedicated update workspace is isolated from an unrelated dirty checkout.
///
/// The source branch and target branch exist, but the source branch is checked out
/// in a dirty worktree. The old implementation reused that checkout and failed.
/// The dedicated workflow must update through its operation-owned worktree, merge
/// successfully, and leave the unrelated dirty checkout untouched.
#[tokio::test]
async fn test_isolated_source_update_ignores_unrelated_dirty_checkout() {
    let git_repo = setup_real_git_repo();
    let path = git_repo.path();

    std::fs::write(path.join("README.md"), "# test repo\nmain update\n").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "README.md"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "update readme on main"])
        .current_dir(path)
        .output();

    let source_wt_dir = tempfile::tempdir().unwrap();
    let source_wt = source_wt_dir.path().join("dirty-source-worktree");
    let add_wt = std::process::Command::new("git")
        .args([
            "worktree",
            "add",
            &source_wt.to_string_lossy(),
            &git_repo.task_branch,
        ])
        .current_dir(path)
        .output()
        .expect("git worktree add source branch");
    assert!(
        add_wt.status.success(),
        "source branch worktree should be created: {}",
        String::from_utf8_lossy(&add_wt.stderr)
    );
    std::fs::write(
        source_wt.join("README.md"),
        "# test repo\nlocal dirty source edit\n",
    )
    .unwrap();

    let setup = setup_pending_merge_with_real_repo(
        "Source update error test",
        &git_repo.task_branch,
        &git_repo.path_string(),
        MergeStrategy::Merge,
    )
    .await;

    let task_id = setup.task_id.clone();
    let task_repo = Arc::clone(&setup.task_repo);
    let (mut machine, _task_repo, _task_id) = setup.into_machine();
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::Merged,
        "Operation-owned source update should merge despite an unrelated dirty checkout. Got {:?}. Metadata: {:?}",
        updated_task.internal_status,
        updated_task.metadata,
    );
    assert_eq!(
        std::fs::read_to_string(source_wt.join("README.md")).unwrap(),
        "# test repo\nlocal dirty source edit\n",
        "Dedicated update must not modify the unrelated dirty checkout"
    );

    let _ = std::process::Command::new("git")
        .args([
            "worktree",
            "remove",
            "--force",
            &source_wt.to_string_lossy(),
        ])
        .current_dir(path)
        .output();
}

/// Verify that a conflict discovered while refreshing a stale source branch
/// routes to the dedicated branch-update workflow before merge dispatch.
///
/// Setup: main and task branch both modify the same file (creating a conflict).
#[tokio::test]
async fn test_stale_source_conflict_transitions_to_task_branch_update() {
    let git_repo = setup_real_git_repo();

    // Create a conflicting commit on main (modify feature.rs on main too)
    std::fs::write(git_repo.path().join("feature.rs"), "// conflict on main").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(git_repo.path())
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "conflicting change on main"])
        .current_dir(git_repo.path())
        .output();

    let setup = setup_pending_merge_with_real_repo(
        "Conflict test",
        &git_repo.task_branch,
        &git_repo.path_string(),
        MergeStrategy::Merge,
    )
    .await;

    let task_id = setup.task_id.clone();
    let task_repo = Arc::clone(&setup.task_repo);
    let (mut machine, _task_repo, _task_id) = setup.into_machine();
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::UpdatingTaskBranch,
        "Stale source conflict should transition to UpdatingTaskBranch, got {:?}. Metadata: {:?}",
        updated_task.internal_status,
        updated_task.metadata,
    );
}

/// Verify rebase-squash strategy (default) works end-to-end with a real git repo.
///
/// RebaseSquash is the project default — verifying it works ensures the most
/// common production path is covered.
#[tokio::test]
async fn test_rebase_squash_merge_success_with_real_repo() {
    let git_repo = setup_real_git_repo();

    let setup = setup_pending_merge_with_real_repo(
        "RebaseSquash merge test",
        &git_repo.task_branch,
        &git_repo.path_string(),
        MergeStrategy::RebaseSquash,
    )
    .await;

    let task_id = setup.task_id.clone();
    let task_repo = Arc::clone(&setup.task_repo);
    let (mut machine, _task_repo, _task_id) = setup.into_machine();
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::Merged,
        "Task should be Merged after rebase-squash, got {:?}. Metadata: {:?}",
        updated_task.internal_status,
        updated_task.metadata,
    );

    // Feature file should exist after squash
    assert!(
        git_repo.path().join("feature.rs").exists(),
        "feature.rs should exist on main after rebase-squash merge"
    );
}

/// Verify merge completes in bounded time even with a real git repo.
///
/// This is the real-repo equivalent of test_pending_merge_with_repos_completes_in_bounded_time
/// from test_quality_overhaul.rs. With a real git repo, the full merge path
/// (cleanup + strategy dispatch + outcome handling) runs.
#[tokio::test]
async fn test_real_repo_merge_completes_in_bounded_time() {
    let git_repo = setup_real_git_repo();

    let setup = setup_pending_merge_with_real_repo(
        "Bounded time real repo test",
        &git_repo.task_branch,
        &git_repo.path_string(),
        MergeStrategy::Merge,
    )
    .await;

    let (mut machine, task_repo, task_id) = setup.into_machine();
    let handler = TransitionHandler::new(&mut machine);

    let start = std::time::Instant::now();
    let _ = handler.on_enter(&State::PendingMerge).await;
    let elapsed = start.elapsed();

    // Full merge path (cleanup + strategy + outcome) should complete quickly
    assert!(
        elapsed.as_secs() < 30,
        "Real repo merge should complete in bounded time, took {}s",
        elapsed.as_secs()
    );

    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Merged,
        "Task should be Merged after bounded-time test, got {:?}",
        updated.internal_status,
    );
}

/// Verify that a zero-unique identical source branch is blocked instead of treated
/// as a successful no-op merge.
#[tokio::test]
async fn test_zero_unique_identical_rebase_squash_routes_merge_incomplete() {
    use crate::application::GitService;

    let git_repo = setup_real_git_repo();
    let repo = git_repo.path();

    // Fast-forward main to match task branch → branches now identical
    let _ = std::process::Command::new("git")
        .args(["merge", &git_repo.task_branch, "--ff-only"])
        .current_dir(repo)
        .output();

    // Verify branches are identical (precondition for the bug)
    let same_content = GitService::branches_have_same_content(repo, &git_repo.task_branch, "main")
        .await
        .unwrap();
    assert!(
        same_content,
        "Precondition: branches should be identical after fast-forward"
    );
    let unique_commits =
        GitService::count_commits_not_on_branch(repo, &git_repo.task_branch, "main")
            .await
            .unwrap();
    assert_eq!(
        unique_commits, 0,
        "Precondition: source branch has no unique commits"
    );

    // Run the merge via TransitionHandler
    let setup = setup_pending_merge_with_real_repo(
        "Zero-unique identical branch test",
        &git_repo.task_branch,
        &git_repo.path_string(),
        MergeStrategy::RebaseSquash,
    )
    .await;

    let task_id = setup.task_id.clone();
    let task_repo = Arc::clone(&setup.task_repo);
    let (mut machine, _task_repo, _task_id) = setup.into_machine();
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::MergeIncomplete,
        "Zero-unique identical branches must not be marked Merged, got {:?}. Metadata: {:?}",
        updated.internal_status,
        updated.metadata,
    );
}

/// Verify that identical branches with zero unique source commits do not become
/// ghost-successful with the Merge strategy.
#[tokio::test]
async fn test_zero_unique_identical_merge_strategy_routes_merge_incomplete() {
    use crate::application::GitService;

    let git_repo = setup_real_git_repo();
    let repo = git_repo.path();

    // Fast-forward main to match task branch → branches now identical
    let _ = std::process::Command::new("git")
        .args(["merge", &git_repo.task_branch, "--ff-only"])
        .current_dir(repo)
        .output();

    let same_content = GitService::branches_have_same_content(repo, &git_repo.task_branch, "main")
        .await
        .unwrap();
    assert!(
        same_content,
        "Precondition: branches should be identical after fast-forward"
    );
    let unique_commits =
        GitService::count_commits_not_on_branch(repo, &git_repo.task_branch, "main")
            .await
            .unwrap();
    assert_eq!(
        unique_commits, 0,
        "Precondition: source branch has no unique commits"
    );

    let main_sha_before = GitService::get_branch_sha(repo, "main").await.unwrap();

    let setup = setup_pending_merge_with_real_repo(
        "Identical branches Merge strategy test",
        &git_repo.task_branch,
        &git_repo.path_string(),
        MergeStrategy::Merge,
    )
    .await;

    let task_id = setup.task_id.clone();
    let task_repo = Arc::clone(&setup.task_repo);
    let (mut machine, _task_repo, _task_id) = setup.into_machine();
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::MergeIncomplete,
        "Task should stop at MergeIncomplete with zero-unique identical branches, got {:?}",
        updated.internal_status,
    );

    // Verify no new commit was created on main
    let main_sha_after = GitService::get_branch_sha(repo, "main").await.unwrap();
    assert_eq!(
        main_sha_before, main_sha_after,
        "No new commit should be created on main when branches are already identical"
    );
}

/// Verify that identical branches with zero unique source commits do not become
/// ghost-successful with the Rebase strategy.
#[tokio::test]
async fn test_zero_unique_identical_rebase_strategy_routes_merge_incomplete() {
    use crate::application::GitService;

    let git_repo = setup_real_git_repo();
    let repo = git_repo.path();

    // Fast-forward main to match task branch → branches now identical
    let _ = std::process::Command::new("git")
        .args(["merge", &git_repo.task_branch, "--ff-only"])
        .current_dir(repo)
        .output();

    let same_content = GitService::branches_have_same_content(repo, &git_repo.task_branch, "main")
        .await
        .unwrap();
    assert!(
        same_content,
        "Precondition: branches should be identical after fast-forward"
    );
    let unique_commits =
        GitService::count_commits_not_on_branch(repo, &git_repo.task_branch, "main")
            .await
            .unwrap();
    assert_eq!(
        unique_commits, 0,
        "Precondition: source branch has no unique commits"
    );

    let main_sha_before = GitService::get_branch_sha(repo, "main").await.unwrap();

    let setup = setup_pending_merge_with_real_repo(
        "Identical branches Rebase strategy test",
        &git_repo.task_branch,
        &git_repo.path_string(),
        MergeStrategy::Rebase,
    )
    .await;

    let task_id = setup.task_id.clone();
    let task_repo = Arc::clone(&setup.task_repo);
    let (mut machine, _task_repo, _task_id) = setup.into_machine();
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::MergeIncomplete,
        "Task should stop at MergeIncomplete with zero-unique identical branches, got {:?}",
        updated.internal_status,
    );

    // Verify no new commit was created on main
    let main_sha_after = GitService::get_branch_sha(repo, "main").await.unwrap();
    assert_eq!(
        main_sha_before, main_sha_after,
        "No new commit should be created on main when branches are already identical"
    );
}
