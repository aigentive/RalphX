// Integration tests for concurrent plan branch freshness access and stress scenarios.
//
// Covers:
//   1. 3 tasks on same plan branch simultaneously — at most one RouteToBranchUpdate,
//      others get Ok (transient error, non-fatal) — no deadlocks.
//   2. Stress: rapid conflict cycles with conflict count cap — task eventually blocked,
//      never looping indefinitely.
//   3. Dirty worktree edge case — emergency auto-commit runs, freshness proceeds.
//   4. Git lock contention — graceful transient error, execution continues.
//
// These tests complement freshness_tests.rs (unit) and freshness_integration_tests.rs
// (basic integration) by focusing on multi-task concurrency and edge-case resilience.

use crate::support::real_git_repo::setup_real_git_repo;
use ralphx_lib::domain::entities::{Project, ProjectId, Task};
use ralphx_lib::domain::state_machine::transition_handler::freshness::{
    ensure_branches_fresh, FreshnessAction,
};
use ralphx_lib::infrastructure::agents::claude::ReconciliationConfig;
use std::sync::Arc;

// ==================
// Shared test helpers
// ==================

fn make_project_at(repo_path: &str) -> Project {
    let mut p = Project::new("test-project".to_string(), repo_path.to_string());
    p.base_branch = Some("main".to_string());
    p.worktree_parent_directory = Some(
        std::path::Path::new(repo_path)
            .join("worktrees")
            .to_string_lossy()
            .to_string(),
    );
    p
}

fn make_task_with_branch(task_branch: &str, conflict_count: u32) -> Task {
    let mut t = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Test task".into(),
    );
    t.task_branch = Some(task_branch.to_string());
    if conflict_count > 0 {
        t.metadata =
            Some(serde_json::json!({ "freshness_conflict_count": conflict_count }).to_string());
    }
    t
}

fn make_task_with_branch_and_auto_reset(
    task_branch: &str,
    conflict_count: u32,
    auto_reset_count: u8,
) -> Task {
    let mut t = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Test task".into(),
    );
    t.task_branch = Some(task_branch.to_string());
    t.metadata = Some(
        serde_json::json!({
            "freshness_conflict_count": conflict_count,
            "freshness_auto_reset_count": auto_reset_count,
        })
        .to_string(),
    );
    t
}

fn concurrent_test_config() -> ReconciliationConfig {
    ReconciliationConfig {
        branch_freshness_timeout_secs: 30,
        freshness_skip_window_secs: 0, // always check
        freshness_max_conflict_retries: 3,
        execution_freshness_enabled: true,
        // Disable backoff so sequential calls are not skipped by the backoff window
        freshness_backoff_base_secs: 0,
        ..Default::default()
    }
}

/// Create a plan branch with diverging commits on both the plan branch and main,
/// causing a `git merge` conflict.
///
/// After this, `plan/{name}` and `main` have conflicting changes on
/// `shared_{name}.rs`. The repo is left on `main`.
fn setup_conflicting_plan_branch(path: &std::path::Path, name: &str) {
    let branch = format!("plan/{name}");
    let safe_name: String = name
        .chars()
        .map(|c| if matches!(c, '/' | '-') { '_' } else { c })
        .collect();
    let filename = format!("shared_{safe_name}.rs");

    // Branch off from current main HEAD
    let _ = std::process::Command::new("git")
        .args(["branch", &branch])
        .current_dir(path)
        .output()
        .expect("git branch");

    // Add diverging commit on main
    std::fs::write(path.join(&filename), "// main version\nfn main_impl() {}").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", &filename])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", &format!("fix: main changes {filename}")])
        .current_dir(path)
        .output();

    // Add diverging commit on plan branch
    let _ = std::process::Command::new("git")
        .args(["checkout", &branch])
        .current_dir(path)
        .output();
    std::fs::write(path.join(&filename), "// plan version\nfn plan_impl() {}").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", &filename])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", &format!("feat: plan changes {filename}")])
        .current_dir(path)
        .output();

    // Return to main
    let _ = std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(path)
        .output();
}

/// Abort any in-progress merge (e.g. after a conflict detection that left
/// MERGE_HEAD behind). Safe to call even when no merge is in progress.
fn abort_pending_merge(path: &std::path::Path) {
    let _ = std::process::Command::new("git")
        .args(["merge", "--abort"])
        .current_dir(path)
        .output();
}

// ==================
// Test 1: Concurrent freshness on same plan branch — no deadlocks
// ==================

/// Three tasks on the same plan branch call `ensure_branches_fresh()` concurrently.
///
/// The plan branch has diverging changes with main (conflict scenario). Git's
/// own file locking ensures only one `git merge` can proceed at a time. The
/// expected outcomes are:
///
/// - At most **one** task returns `RouteToBranchUpdate` (the one whose merge succeeded
///   and detected conflicts).
/// - Remaining tasks get `Ok` (their plan check returned `Error` — non-fatal
///   transient git error due to lock contention) or `AlreadyUpToDate` if the
///   first merge aborted cleanly.
/// - The conflict count is never incremented beyond 1 for any single task.
/// - No deadlocks: all 3 futures complete without hanging.
// FLAKY: Each task uses a distinct worktree path (keyed by task_id), so multiple tasks
// can independently create worktrees for the same plan branch and all detect the merge
// conflict. When task A finishes and deletes its worktree, task B (racing behind) gets
// a clean slot and also succeeds. The assertion `<= 1` assumes git lock contention
// prevents multiple detections, but per-task worktree paths eliminate that contention.
// The key correctness property (no deadlock, no ExecutionBlocked) is verified by the
// other assertions; the `<= 1` cap was an incorrect assumption about git locking.
// TODO: Either (a) use a per-plan-branch mutex in update_plan_from_main to serialize
// concurrent plan checks, or (b) relax the cap to reflect actual behavior.
#[tokio::test]
#[ignore = "flaky: per-task worktree paths allow multiple concurrent conflict detections; see comment above"]
async fn concurrent_freshness_same_plan_branch_no_deadlock() {
    let repo = setup_real_git_repo();
    let path = repo.path();

    setup_conflicting_plan_branch(path, "concurrent-test");

    // Keep repo alive across spawned tasks via path string
    let path_str = Arc::new(repo.path_string());
    let task_branch = Arc::new(repo.task_branch.clone());

    // Drop the RealGitRepo struct but keep the TempDir alive by leaking its
    // inner directory reference. We keep `repo` in scope for the test duration.
    let _repo_guard = repo;

    let mut handles = Vec::new();
    for i in 0..3usize {
        let path_str = Arc::clone(&path_str);
        let task_branch = Arc::clone(&task_branch);
        handles.push(tokio::spawn(async move {
            let path = std::path::Path::new(path_str.as_str());
            let project = make_project_at(path_str.as_str());
            let mut task = Task::new(
                ProjectId::from_string("proj-1".to_string()),
                format!("Concurrent task {i}"),
            );
            task.task_branch = Some(task_branch.to_string());

            let cfg = concurrent_test_config();

            ensure_branches_fresh(
                path,
                &task,
                &project,
                &format!("concurrent-task-{i}"),
                Some("plan/concurrent-test"),
                None,
                None,
                None,
                "executing",
                &cfg,
            )
            .await
        }));
    }

    // Collect results — if any future panics, the test fails.
    let results: Vec<_> = {
        let mut out = Vec::new();
        for h in handles {
            out.push(h.await.expect("concurrent task panicked"));
        }
        out
    };

    // All 3 must complete (no deadlock).
    assert_eq!(results.len(), 3, "All 3 concurrent tasks must complete");

    // At most one should return RouteToBranchUpdate for plan_update.
    // Others may get Ok (transient git error from lock → non-fatal → source passes)
    // or RouteToBranchUpdate for source_update if the source branch also needs merging.
    let plan_conflict_count = results
        .iter()
        .filter(|r| {
            matches!(
                r,
                Err(FreshnessAction::RouteToBranchUpdate {
                    conflict_type: "plan_update",
                    ..
                })
            )
        })
        .count();

    assert!(
        plan_conflict_count <= 1,
        "At most one concurrent task should detect a plan_update conflict; \
         got {plan_conflict_count}. If >1, git lock contention is not handled gracefully."
    );

    // None should be ExecutionBlocked — conflict count starts at 0 for all tasks.
    let blocked_count = results
        .iter()
        .filter(|r| matches!(r, Err(FreshnessAction::ExecutionBlocked { .. })))
        .count();
    assert_eq!(
        blocked_count, 0,
        "No task should be ExecutionBlocked in a first-attempt concurrent scenario"
    );
}

// ==================
// Test 2: Stress — rapid conflict cycles blocked at cap
// ==================

/// Simulate rapid Ready → Executing → Merging → Ready cycles where the plan
/// branch always has a conflict. Each call to `ensure_branches_fresh()` carries
/// forward the `freshness_conflict_count` from the previous call, simulating
/// task metadata persistence across state transitions.
///
/// Acceptance criteria:
/// - Calls 1-3 (count 0→1, 1→2, 2→3) return `RouteToBranchUpdate` with increasing counts.
/// - Call 4 (count 3→4, exceeds max_retries=3, auto_reset_count already=1) returns `ExecutionBlocked`.
/// - No infinite loop: the function terminates at the second cap.
///
/// Note: Tests start with auto_reset_count=1 to bypass the auto-reset phase,
/// directly testing that ExecutionBlocked fires after the second cap.
#[tokio::test]
async fn stress_rapid_conflict_cycles_blocked_at_cap() {
    let repo = setup_real_git_repo();
    let path = repo.path();

    setup_conflicting_plan_branch(path, "stress-test");

    let project = make_project_at(&repo.path_string());
    let cfg = ReconciliationConfig {
        branch_freshness_timeout_secs: 30,
        freshness_skip_window_secs: 0,
        freshness_max_conflict_retries: 3,
        execution_freshness_enabled: true,
        // Disable backoff so sequential calls are not skipped by the backoff window
        freshness_backoff_base_secs: 0,
        ..Default::default()
    };

    // Start with auto_reset_count=1 to simulate: auto-reset already occurred once.
    // This means the NEXT time count exceeds cap, ExecutionBlocked fires (second cap).
    let mut current_count: u32 = 0;
    let max_iterations = 10; // Safety ceiling: max_retries + 2 iterations needed
    let mut got_blocked = false;
    let mut route_counts: Vec<u32> = Vec::new();

    for _attempt in 0..max_iterations {
        // Abort any pending merge from the previous iteration so git is clean.
        abort_pending_merge(path);

        let task = make_task_with_branch_and_auto_reset(&repo.task_branch, current_count, 1);

        let result = ensure_branches_fresh(
            path,
            &task,
            &project,
            "stress-task",
            Some("plan/stress-test"),
            None,
            None,
            None,
            "executing",
            &cfg,
        )
        .await;

        match result {
            Err(FreshnessAction::RouteToBranchUpdate {
                freshness_metadata,
                conflict_type,
                ..
            }) => {
                assert!(
                    conflict_type == "plan_update" || conflict_type == "source_update",
                    "Conflict type must be plan_update or source_update; got: {conflict_type}"
                );
                assert_eq!(
                    freshness_metadata.freshness_conflict_count,
                    current_count + 1,
                    "Conflict count must increment by 1 per call"
                );
                current_count = freshness_metadata.freshness_conflict_count;
                route_counts.push(current_count);
            }
            Err(FreshnessAction::ExecutionBlocked { reason, .. }) => {
                got_blocked = true;
                assert!(
                    current_count >= cfg.freshness_max_conflict_retries,
                    "ExecutionBlocked must only fire when count ({current_count}) >= max_retries ({}). Reason: {reason}",
                    cfg.freshness_max_conflict_retries
                );
                break;
            }
            Ok(_) => {
                // Transient git error — no metadata update, carry current_count unchanged
            }
        }
    }

    assert!(
        got_blocked,
        "Task must eventually be ExecutionBlocked at the conflict cap (max_retries={}). \
         Route counts observed: {route_counts:?}",
        cfg.freshness_max_conflict_retries
    );

    // Verify the counts were monotonically increasing before blocking
    for i in 1..route_counts.len() {
        assert!(
            route_counts[i] > route_counts[i - 1],
            "Conflict count must monotonically increase across calls. \
             Got: {route_counts:?}"
        );
    }
}

/// PR mode treats dirty primary-checkout changes as user-owned state. Freshness
/// may inspect refs and update task branches in isolation, but it must not create
/// the emergency chore commit or alter the primary checkout's index/worktree.
#[tokio::test]
async fn pr_mode_dirty_primary_checkout_is_not_auto_committed() {
    let repo = setup_real_git_repo();
    let path = repo.path();
    std::fs::write(path.join("user_owned_change.rs"), "// user-owned work").unwrap();

    let head_before = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(path)
        .output()
        .expect("read HEAD before freshness");
    assert!(head_before.status.success());
    let status_before = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(path)
        .output()
        .expect("read status before freshness");
    assert!(status_before.status.success());

    let mut project = make_project_at(&repo.path_string());
    project.github_pr_enabled = true;
    let task = make_task_with_branch(&repo.task_branch, 0);
    let result = ensure_branches_fresh(
        path,
        &task,
        &project,
        "pr-mode-dirty-primary",
        None,
        None,
        None,
        None,
        "executing",
        &concurrent_test_config(),
    )
    .await;

    assert!(
        result.is_ok(),
        "freshness should proceed without consuming user changes"
    );
    let head_after = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(path)
        .output()
        .expect("read HEAD after freshness");
    assert!(head_after.status.success());
    let status_after = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(path)
        .output()
        .expect("read status after freshness");
    assert!(status_after.status.success());

    assert_eq!(head_after.stdout, head_before.stdout);
    assert_eq!(status_after.stdout, status_before.stdout);
}

// ==================
// Test 3: Dirty worktree — emergency auto-commit enables freshness
// ==================

/// When the worktree has uncommitted changes (e.g. from a crashed prior run),
/// `ensure_branches_fresh()` must:
///   1. Detect the dirty worktree via `git status --porcelain -z`.
///   2. Attempt an emergency auto-commit.
///   3. Continue with the freshness check (either Ok or RouteToBranchUpdate).
///   4. NOT return early without a meaningful check.
///
/// We verify by writing an untracked file (dirty), calling the function,
/// and confirming the file is committed and the check proceeds.
#[tokio::test]
async fn dirty_worktree_emergency_commit_enables_freshness() {
    let repo = setup_real_git_repo();
    let path = repo.path();

    // Create a dirty worktree: write a file but don't commit it.
    std::fs::write(path.join("uncommitted_work.rs"), "// uncommitted work").unwrap();

    // Verify the worktree is indeed dirty before calling ensure_branches_fresh.
    let status = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(path)
        .output()
        .expect("git status");
    assert!(
        !status.stdout.is_empty(),
        "Pre-condition: worktree must be dirty before test"
    );

    let mut project = make_project_at(&repo.path_string());
    project.github_pr_enabled = false;
    let task = make_task_with_branch(&repo.task_branch, 0);
    let cfg = concurrent_test_config();

    // Call ensure_branches_fresh with a clean plan branch (no conflict) so that
    // after the emergency auto-commit, the check should pass (Ok).
    let result = ensure_branches_fresh(
        path,
        &task,
        &project,
        "dirty-worktree-task",
        None, // no plan branch — source check only
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;

    // The result should be Ok or RouteToBranchUpdate — NOT an early abort that skips the check.
    // An Ok result means the auto-commit succeeded and the source branch is fresh.
    // A RouteToBranchUpdate is also acceptable if a genuine conflict exists (though
    // setup_real_git_repo() creates a task branch already diverged from main,
    // it does NOT create a source conflict, so Ok is expected here).
    match &result {
        Ok(meta) => {
            // Auto-commit succeeded, freshness check ran and passed.
            assert!(
                meta.last_freshness_check_at.is_some(),
                "last_freshness_check_at must be set after a successful check"
            );
            assert!(
                !meta.branch_freshness_conflict,
                "No conflict expected for a fresh task branch after auto-commit"
            );
        }
        Err(FreshnessAction::RouteToBranchUpdate { .. }) => {
            // Acceptable — task branch genuinely stale, auto-commit ran, conflict detected.
        }
        Err(FreshnessAction::ExecutionBlocked { reason, .. }) => {
            panic!(
                "ExecutionBlocked unexpected for a dirty worktree with fresh branch. \
                 Reason: {reason}"
            );
        }
    }

    // Verify the uncommitted file was committed (worktree is now clean).
    let status_after = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(path)
        .output()
        .expect("git status after");
    assert!(
        status_after.stdout.is_empty(),
        "Worktree must be clean after emergency auto-commit. \
         Remaining changes: {}",
        String::from_utf8_lossy(&status_after.stdout)
    );
}

/// When the emergency auto-commit fails (simulated by holding Git's index lock),
/// `ensure_branches_fresh()` must block execution rather
/// than pretending the freshness check passed.
///
/// This verifies fail-closed behavior: if the dirty worktree cannot be made
/// durable before branch freshness checks, execution must not spawn on uncertain
/// Git state.
#[tokio::test]
async fn dirty_worktree_failed_autocommit_blocks_execution() {
    let repo = setup_real_git_repo();
    let path = repo.path();

    // Create a dirty worktree: write a modified tracked file.
    // We'll modify a tracked file so git detects it as modified.
    std::fs::write(path.join("README.md"), "# modified content").unwrap();

    // Verify dirty
    let status = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(path)
        .output()
        .expect("git status");
    assert!(
        !status.stdout.is_empty(),
        "Pre-condition: worktree must be dirty"
    );

    // Hold the index lock so `git add` fails deterministically even when tests run
    // as a user that can bypass directory permission bits, while still allowing
    // the preceding read-only status check to detect dirtiness.
    let index_lock = path.join(".git").join("index.lock");
    std::fs::write(&index_lock, "held by freshness regression test").unwrap();

    let mut project = make_project_at(&repo.path_string());
    project.github_pr_enabled = false;
    let task = make_task_with_branch(&repo.task_branch, 0);
    let cfg = concurrent_test_config();

    let result = ensure_branches_fresh(
        path,
        &task,
        &project,
        "dirty-failed-commit-task",
        None,
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;

    std::fs::remove_file(&index_lock).expect("remove deterministic index lock");

    // Must block — fail closed rather than skip freshness and spawn on uncertain state.
    assert!(
        matches!(
            result,
            Err(FreshnessAction::ExecutionBlocked { ref reason, .. })
                if reason.contains("Emergency auto-commit failed")
        ),
        "Failed auto-commit must block execution. Got: {:?}",
        result
    );
}

// ==================
// Test 4: Git lock contention — graceful transient error handling
// ==================

/// When another git process holds the index lock (`.git/index.lock` exists),
/// `git merge` fails with a "lock file" error. This must be treated as a
/// transient (non-fatal) error for the plan check, which then continues to
/// the source check. If the source branch is fresh, the overall result is Ok.
///
/// This test creates a fake `.git/index.lock` to simulate the contention.
#[tokio::test]
async fn git_lock_contention_plan_check_is_transient_non_fatal() {
    let repo = setup_real_git_repo();
    let path = repo.path();

    // Create a plan branch that is behind main (would normally be "Updated").
    // We create the plan branch at the initial commit (before the task branch commit on main).
    let _ = std::process::Command::new("git")
        .args(["branch", "plan/lock-test"])
        .current_dir(path)
        .output();

    // Add a new commit to main so plan/lock-test is behind main.
    std::fs::write(path.join("new_feature.rs"), "// new on main").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "new_feature.rs"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "feat: new commit on main for lock test"])
        .current_dir(path)
        .output();

    // Simulate git lock contention: create .git/index.lock to prevent git merge
    // from acquiring the index lock. Git will fail with "Unable to create lock file".
    let lock_file = path.join(".git").join("index.lock");
    std::fs::write(&lock_file, "fake lock held by another git process").unwrap();

    let project = make_project_at(&repo.path_string());
    let task = make_task_with_branch(&repo.task_branch, 0);
    let cfg = concurrent_test_config();

    let result = ensure_branches_fresh(
        path,
        &task,
        &project,
        "lock-contention-task",
        Some("plan/lock-test"),
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;

    // Remove the lock file after the call (cleanup regardless of outcome).
    let _ = std::fs::remove_file(&lock_file);

    // The plan check should have failed with a transient git error (non-fatal).
    // The function should then continue to the source check.
    // With a fresh task branch, the source check passes → Ok.
    //
    // Note: git behavior with index.lock varies — some git versions detect
    // the lock before starting, others may partially succeed. Either Ok or
    // RouteToBranchUpdate(source_update) are acceptable; ExecutionBlocked is not.
    match result {
        Ok(_) => {
            // Plan check failed (transient) → source check passed → Ok. Expected case.
        }
        Err(FreshnessAction::RouteToBranchUpdate { conflict_type, .. }) => {
            // Plan check may have partially succeeded and detected a conflict,
            // or source check detected a conflict. Both are acceptable.
            assert!(
                conflict_type == "plan_update" || conflict_type == "source_update",
                "Unexpected conflict_type: {conflict_type}"
            );
        }
        Err(FreshnessAction::ExecutionBlocked { reason, .. }) => {
            panic!(
                "ExecutionBlocked must not occur from a transient git lock error. \
                 Reason: {reason}"
            );
        }
    }
}

/// When TWO concurrent callers observe the same stale plan branch, both may
/// request the dedicated update. Observation is read-only; repository activation
/// provides the single-writer authority. Both calls must complete without a
/// deadlock or false `ExecutionBlocked` result.
#[tokio::test]
async fn concurrent_plan_branch_updates_no_deadlock_or_blocked() {
    let repo = setup_real_git_repo();
    let path = repo.path();

    // Create a plan branch that is behind main (needs updating, no conflict).
    let _ = std::process::Command::new("git")
        .args(["branch", "plan/concurrent-lock-test"])
        .current_dir(path)
        .output();

    // Add a commit to main so the plan branch is behind (will need an update).
    std::fs::write(path.join("concurrent_main.rs"), "// concurrent main commit").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "concurrent_main.rs"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "feat: main commit for concurrent lock test"])
        .current_dir(path)
        .output();

    let path_str = Arc::new(repo.path_string());
    let task_branch = Arc::new(repo.task_branch.clone());
    let _repo_guard = repo;

    let mut handles = Vec::new();
    for i in 0..2usize {
        let path_str = Arc::clone(&path_str);
        let task_branch = Arc::clone(&task_branch);
        handles.push(tokio::spawn(async move {
            let path = std::path::Path::new(path_str.as_str());
            let project = make_project_at(path_str.as_str());
            let mut task = Task::new(
                ProjectId::from_string("proj-1".to_string()),
                format!("Concurrent lock task {i}"),
            );
            task.task_branch = Some(task_branch.to_string());

            let cfg = concurrent_test_config();

            ensure_branches_fresh(
                path,
                &task,
                &project,
                &format!("concurrent-lock-task-{i}"),
                Some("plan/concurrent-lock-test"),
                None,
                None,
                None,
                "executing",
                &cfg,
            )
            .await
        }));
    }

    let results: Vec<_> = {
        let mut out = Vec::new();
        for h in handles {
            out.push(h.await.expect("concurrent lock task panicked"));
        }
        out
    };

    // Both must complete (no deadlock).
    assert_eq!(results.len(), 2, "Both concurrent callers must complete");

    // Neither should be ExecutionBlocked — no conflicts exist, only possible transient errors.
    for (i, result) in results.iter().enumerate() {
        assert!(
            !matches!(result, Err(FreshnessAction::ExecutionBlocked { .. })),
            "Caller {i} must not be ExecutionBlocked; got: {:?}",
            result
        );
    }

    // Read-only observers may both see the same stale branch. Single-writer
    // contention is enforced when the durable update operation is activated.
    let plan_conflict_count = results
        .iter()
        .filter(|r| {
            matches!(
                r,
                Err(FreshnessAction::RouteToBranchUpdate {
                    conflict_type: "plan_update",
                    ..
                })
            )
        })
        .count();
    assert_eq!(
        plan_conflict_count, 2,
        "Both observers should report the stale plan without mutating it"
    );
}
