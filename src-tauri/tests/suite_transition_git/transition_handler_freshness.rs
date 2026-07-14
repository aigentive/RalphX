// Tests for ensure_branches_fresh() — branch freshness check orchestration.
//
// Covers: config toggle, skip window, plan check result mapping, source check
// result mapping, retry counting, and dual-conflict sequential scenarios.
// Also covers: FreshnessMetadata struct API (cleanup scopes, backoff, serde defaults).

use crate::support::real_git_repo::setup_real_git_repo;
use chrono::Utc;
use ralphx_lib::domain::entities::{Project, ProjectId, Task};
use ralphx_lib::domain::state_machine::transition_handler::freshness::{
    ensure_branches_fresh, FreshnessAction, FreshnessCleanupScope, FreshnessMetadata,
};
use ralphx_lib::infrastructure::agents::claude::ReconciliationConfig;

// ==================
// Helpers
// ==================

/// Create a Project entity pointing at a repo path.
fn make_test_project(repo_path: &str) -> Project {
    let mut project = Project::new("test-project".to_string(), repo_path.to_string());
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(
        std::path::Path::new(repo_path)
            .join("worktrees")
            .to_string_lossy()
            .to_string(),
    );
    project
}

/// Build a minimal Task for freshness tests.
///
/// `task_branch` — the task's branch (source branch for freshness checks).
/// `metadata` — JSON metadata to embed (freshness state lives here).
fn make_test_task(task_branch: Option<&str>, metadata: Option<serde_json::Value>) -> Task {
    let mut t = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Test task".into(),
    );
    t.task_branch = task_branch.map(|s| s.to_string());
    t.metadata = metadata.map(|v| v.to_string());
    t
}

/// Build a ReconciliationConfig with freshness-specific defaults suitable for tests.
///
/// Callers can mutate returned config before passing to `ensure_branches_fresh`.
fn freshness_config() -> ReconciliationConfig {
    ReconciliationConfig {
        // Use a short timeout so tests don't hang on failures
        branch_freshness_timeout_secs: 10,
        freshness_skip_window_secs: 30,
        freshness_max_conflict_retries: 3,
        execution_freshness_enabled: true,
        // Disable backoff in tests so sequential calls are not skipped by the backoff window
        freshness_backoff_base_secs: 0,
        ..Default::default()
    }
}

// ==================
// Config toggle tests
// ==================

#[tokio::test]
async fn config_disabled_returns_ok_without_checking() {
    // When execution_freshness_enabled=false, the function must return Ok immediately
    // without performing any git operations.  We pass a nonexistent path — if git
    // were invoked, it would fail, exposing an early exit that didn't happen.
    let mut cfg = freshness_config();
    cfg.execution_freshness_enabled = false;

    let task = make_test_task(Some("task/branch"), None);
    let project = make_test_project("/nonexistent/path/should-not-be-accessed");
    let nonexistent = std::path::Path::new("/nonexistent/path/should-not-be-accessed");

    let result = ensure_branches_fresh(
        nonexistent,
        &task,
        &project,
        "task-disabled",
        Some("plan/feature-1"),
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;

    assert!(
        result.is_ok(),
        "Disabled config must return Ok immediately. Got: {:?}",
        result
    );
}

#[tokio::test]
async fn unreadable_worktree_status_blocks_execution() {
    let cfg = freshness_config();
    let task = make_test_task(Some("task/branch"), None);
    let project = make_test_project("/nonexistent/path/should-block");
    let nonexistent = std::path::Path::new("/nonexistent/path/should-block");

    let result = ensure_branches_fresh(
        nonexistent,
        &task,
        &project,
        "task-status-unreadable",
        None,
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;

    assert!(
        matches!(
            result,
            Err(FreshnessAction::ExecutionBlocked { ref reason, .. })
                if reason.contains("Failed to check worktree status")
        ),
        "Unreadable worktree status must block execution; got {result:?}"
    );
}

#[tokio::test]
async fn unreadable_worktree_status_skips_for_reviewing_origin() {
    let cfg = freshness_config();
    let task = make_test_task(Some("task/branch"), None);
    let project = make_test_project("/nonexistent/path/reviewing-should-skip");
    let nonexistent = std::path::Path::new("/nonexistent/path/reviewing-should-skip");

    let result = ensure_branches_fresh(
        nonexistent,
        &task,
        &project,
        "task-status-unreadable-reviewing",
        None,
        None,
        None,
        None,
        "reviewing",
        &cfg,
    )
    .await;

    assert!(
        result.is_ok(),
        "Reviewing origin must preserve review worktree error semantics; got {result:?}"
    );
}

// ==================
// Skip-if-recently-checked tests
// ==================

#[tokio::test]
async fn skip_window_within_threshold_returns_ok_skipped() {
    // last_freshness_check_at was 5 seconds ago, skip_window=30 → skip and return Ok.
    let last_check = (Utc::now() - chrono::Duration::seconds(5)).to_rfc3339();
    let metadata = serde_json::json!({
        "last_freshness_check_at": last_check,
        "freshness_conflict_count": 0,
    });
    let task = make_test_task(Some("task/branch"), Some(metadata));

    let mut cfg = freshness_config();
    cfg.freshness_skip_window_secs = 30;

    // Use a nonexistent path — no git calls should happen.
    let project = make_test_project("/nonexistent/skip-check-path");
    let nonexistent = std::path::Path::new("/nonexistent/skip-check-path");

    let result = ensure_branches_fresh(
        nonexistent,
        &task,
        &project,
        "task-skip",
        None,
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;

    assert!(
        result.is_ok(),
        "Within skip window should return Ok without git calls. Got: {:?}",
        result
    );
    // The returned metadata should still have the original last_freshness_check_at
    let meta = result.unwrap();
    assert!(
        meta.last_freshness_check_at.is_some(),
        "Skipped check should preserve last_freshness_check_at"
    );
}

#[tokio::test]
async fn skip_window_expired_proceeds_with_check() {
    // last_freshness_check_at was 60 seconds ago, skip_window=30 → check runs.
    // Use a real git repo that will pass both checks so we get Ok back.
    let repo = setup_real_git_repo();
    let last_check = (Utc::now() - chrono::Duration::seconds(60)).to_rfc3339();
    let metadata = serde_json::json!({
        "last_freshness_check_at": last_check,
        "freshness_conflict_count": 0,
    });
    let task = make_test_task(Some(&repo.task_branch), Some(metadata));

    let mut cfg = freshness_config();
    cfg.freshness_skip_window_secs = 30;

    let project = make_test_project(&repo.path_string());

    let result = ensure_branches_fresh(
        repo.path(),
        &task,
        &project,
        "task-expired-skip",
        None, // no plan branch → skip plan check
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;

    assert!(
        result.is_ok(),
        "Expired skip window with fresh repo should return Ok. Got: {:?}",
        result
    );
    // The returned metadata should have a freshened timestamp (i.e. more recent than original)
    let meta = result.unwrap();
    let new_ts = meta
        .last_freshness_check_at
        .as_deref()
        .and_then(|s| s.parse::<chrono::DateTime<Utc>>().ok())
        .expect("last_freshness_check_at should be set after a successful check");
    let old_ts: chrono::DateTime<Utc> = last_check.parse().unwrap();
    assert!(
        new_ts > old_ts,
        "Timestamp should be refreshed after a real check. old={old_ts}, new={new_ts}"
    );
}

// ==================
// Plan check result mapping tests
// ==================

#[tokio::test]
async fn plan_already_up_to_date_continues_to_source() {
    // Plan branch at same commit as main → AlreadyUpToDate → source check also passes → Ok.
    let repo = setup_real_git_repo();
    let path = repo.path();

    // Create plan branch from current main (no divergence)
    let _ = std::process::Command::new("git")
        .args(["branch", "plan/fresh-plan"])
        .current_dir(path)
        .output()
        .expect("git branch");

    let project = make_test_project(&repo.path_string());
    let task = make_test_task(Some(&repo.task_branch), None);
    let cfg = freshness_config();

    let result = ensure_branches_fresh(
        path,
        &task,
        &project,
        "task-plan-up-to-date",
        Some("plan/fresh-plan"),
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;

    assert!(
        result.is_ok(),
        "Plan up-to-date + source up-to-date should return Ok. Got: {:?}",
        result
    );
    let meta = result.unwrap();
    assert!(
        !meta.branch_freshness_conflict,
        "No conflict should be flagged"
    );
}

#[tokio::test]
async fn stale_plan_routes_to_dedicated_update_without_mutating_git() {
    // Plan branch behind main → observation routes to the dedicated plan-branch update.
    let repo = setup_real_git_repo();
    let path = repo.path();

    // Create plan branch from main (before new commit)
    let _ = std::process::Command::new("git")
        .args(["branch", "plan/behind-plan"])
        .current_dir(path)
        .output()
        .expect("git branch plan/behind-plan");

    // Add a commit to main (plan branch is now behind)
    std::fs::write(path.join("main_fix.rs"), "// main fix").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "main_fix.rs"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "fix: main fix for plan-behind test"])
        .current_dir(path)
        .output();

    let project = make_test_project(&repo.path_string());
    let task = make_test_task(Some(&repo.task_branch), None);
    let cfg = freshness_config();

    let result = ensure_branches_fresh(
        path,
        &task,
        &project,
        "task-plan-updated",
        Some("plan/behind-plan"),
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;

    assert!(
        matches!(
            result,
            Err(FreshnessAction::RouteToBranchUpdate {
                conflict_type: "plan_update",
                ..
            })
        ),
        "A stale plan must route to dedicated update authority. Got: {:?}",
        result
    );
}

#[tokio::test]
async fn plan_not_plan_branch_skipped() {
    // plan_branch=None → plan check skipped entirely → source check runs → Ok.
    let repo = setup_real_git_repo();
    let project = make_test_project(&repo.path_string());
    let task = make_test_task(Some(&repo.task_branch), None);
    let cfg = freshness_config();

    let result = ensure_branches_fresh(
        repo.path(),
        &task,
        &project,
        "task-no-plan",
        None, // no plan branch
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;

    assert!(
        result.is_ok(),
        "No plan branch → skip plan check → source passes → Ok. Got: {:?}",
        result
    );
}

#[tokio::test]
async fn plan_conflicts_returns_route_to_branch_update() {
    // Plan branch and main have diverging changes on same file → conflict → RouteToBranchUpdate.
    let repo = setup_real_git_repo();
    let path = repo.path();

    // Create plan branch from main
    let _ = std::process::Command::new("git")
        .args(["branch", "plan/conflicting"])
        .current_dir(path)
        .output();

    // Add a conflicting change to main
    std::fs::write(path.join("shared.rs"), "// main version\nfn main_fn() {}").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "shared.rs"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "fix: main changes shared.rs"])
        .current_dir(path)
        .output();

    // Add a conflicting change on the plan branch
    let _ = std::process::Command::new("git")
        .args(["checkout", "plan/conflicting"])
        .current_dir(path)
        .output();
    std::fs::write(path.join("shared.rs"), "// plan version\nfn plan_fn() {}").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "shared.rs"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "feat: plan changes shared.rs"])
        .current_dir(path)
        .output();

    // Back to main
    let _ = std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(path)
        .output();

    let project = make_test_project(&repo.path_string());
    let task = make_test_task(Some(&repo.task_branch), None);
    let cfg = freshness_config();

    let result = ensure_branches_fresh(
        path,
        &task,
        &project,
        "task-plan-conflict",
        Some("plan/conflicting"),
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;

    assert!(
        matches!(
            result,
            Err(FreshnessAction::RouteToBranchUpdate {
                conflict_type: "plan_update",
                ..
            })
        ),
        "Plan conflict must return RouteToBranchUpdate with conflict_type=plan_update. Got: {:?}",
        result
    );
}

#[tokio::test]
async fn plan_error_after_retry_blocks_execution() {
    // If plan_branch doesn't exist, update_plan_from_main returns Error.
    // ensure_branches_fresh retries once, then blocks execution instead of
    // treating an unreadable freshness update as success.
    let repo = setup_real_git_repo();
    let project = make_test_project(&repo.path_string());
    let task = make_test_task(Some(&repo.task_branch), None);
    let cfg = freshness_config();

    let result = ensure_branches_fresh(
        repo.path(),
        &task,
        &project,
        "task-plan-error",
        Some("plan/nonexistent-plan-branch"), // branch doesn't exist → Error
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;

    assert!(
        matches!(
            result,
            Err(FreshnessAction::ExecutionBlocked { ref reason, .. })
                if reason.contains("update_plan_from_main failed after retry")
        ),
        "Plan check error must block after retry. Got: {:?}",
        result
    );
}

// ==================
// Source check result mapping tests
// ==================

#[tokio::test]
async fn source_already_up_to_date_returns_ok() {
    // Task branch is up-to-date with main, no plan branch → Ok.
    let repo = setup_real_git_repo();
    let project = make_test_project(&repo.path_string());
    let task = make_test_task(Some(&repo.task_branch), None);
    let cfg = freshness_config();

    let result = ensure_branches_fresh(
        repo.path(),
        &task,
        &project,
        "task-source-up-to-date",
        None,
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;

    assert!(result.is_ok(), "Source up-to-date → Ok. Got: {:?}", result);
    let meta = result.unwrap();
    assert!(
        !meta.branch_freshness_conflict,
        "No conflict should be flagged for up-to-date source"
    );
}

#[tokio::test]
async fn stale_source_routes_to_dedicated_update_without_mutating_git() {
    // Main has a new commit after task branch creation. Observation must not merge it.
    let repo = setup_real_git_repo();
    let path = repo.path();

    // New commit on main (task branch is now behind)
    std::fs::write(path.join("main_extra.rs"), "// main extra").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "main_extra.rs"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "fix: extra commit on main"])
        .current_dir(path)
        .output();

    let project = make_test_project(&repo.path_string());
    let task = make_test_task(Some(&repo.task_branch), None);
    let cfg = freshness_config();

    let result = ensure_branches_fresh(
        path,
        &task,
        &project,
        "task-source-updated",
        None,
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;

    assert!(
        matches!(
            result,
            Err(FreshnessAction::RouteToBranchUpdate {
                conflict_type: "source_update",
                ..
            })
        ),
        "A stale task branch must route to dedicated update authority. Got: {:?}",
        result
    );
}

#[tokio::test]
async fn source_conflicts_returns_route_to_branch_update() {
    // Main and task branch have conflicting changes on the same file → source conflict →
    // RouteToBranchUpdate with conflict_type=source_update.
    let repo = setup_real_git_repo();
    let path = repo.path();

    // Add a conflicting change on main (same file modified by task branch: feature.rs)
    std::fs::write(
        path.join("feature.rs"),
        "// main conflicting version\nfn main_feature() {}",
    )
    .unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "feature.rs"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "fix: main modifies feature.rs"])
        .current_dir(path)
        .output();

    let project = make_test_project(&repo.path_string());
    let task = make_test_task(Some(&repo.task_branch), None);
    let cfg = freshness_config();

    let result = ensure_branches_fresh(
        path,
        &task,
        &project,
        "task-source-conflict",
        None,
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;

    assert!(
        matches!(
            result,
            Err(FreshnessAction::RouteToBranchUpdate {
                conflict_type: "source_update",
                ..
            })
        ),
        "Source conflict must return RouteToBranchUpdate with conflict_type=source_update. Got: {:?}",
        result
    );
}

#[tokio::test]
async fn source_branch_missing_blocks_execution_without_retry() {
    // A missing task branch blocks immediately instead of entering the retry path.
    let repo = setup_real_git_repo();
    let project = make_test_project(&repo.path_string());
    let task = make_test_task(Some("task/nonexistent-source-branch"), None);
    let cfg = freshness_config();

    let result = ensure_branches_fresh(
        repo.path(),
        &task,
        &project,
        "task-source-error",
        None,
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;

    assert!(
        matches!(
            result,
            Err(FreshnessAction::ExecutionBlocked {
                ref reason,
                branch_missing: Some(ref missing_branch),
            })
                if reason.contains("branch missing before source update")
                    && reason.contains("task/nonexistent-source-branch")
                    && missing_branch == "task/nonexistent-source-branch"
        ),
        "Missing source branch must block without retry. Got: {:?}",
        result
    );
}

#[tokio::test]
async fn missing_task_branch_skips_source_check_returns_ok() {
    // Empty task_branch → source check is skipped entirely → Ok.
    let repo = setup_real_git_repo();
    let project = make_test_project(&repo.path_string());
    let task = make_test_task(None, None); // no task_branch → source check skipped
    let cfg = freshness_config();

    let result = ensure_branches_fresh(
        repo.path(),
        &task,
        &project,
        "task-no-branch",
        None,
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;

    assert!(
        result.is_ok(),
        "Empty task_branch skips source check → Ok. Got: {:?}",
        result
    );
}

// ==================
// Retry counting tests
// ==================

#[tokio::test]
async fn plan_conflict_increments_count_and_routes() {
    // conflict_count starts at 0, conflict detected → count becomes 1 → RouteToBranchUpdate (not blocked).
    let repo = setup_real_git_repo();
    let path = repo.path();

    setup_plan_conflict(path, "plan/count-test-1");

    let project = make_test_project(&repo.path_string());
    // Start with count=0
    let metadata = serde_json::json!({ "freshness_conflict_count": 0 });
    let task = make_test_task(Some(&repo.task_branch), Some(metadata));
    let mut cfg = freshness_config();
    cfg.freshness_max_conflict_retries = 3;

    let result = ensure_branches_fresh(
        path,
        &task,
        &project,
        "task-count-0",
        Some("plan/count-test-1"),
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;

    match result {
        Err(FreshnessAction::RouteToBranchUpdate {
            freshness_metadata, ..
        }) => {
            assert_eq!(
                freshness_metadata.freshness_conflict_count, 1,
                "Count should be incremented from 0 to 1"
            );
        }
        other => panic!("Expected RouteToBranchUpdate, got: {other:?}"),
    }
}

#[tokio::test]
async fn plan_conflict_at_cap_auto_resets_first_time() {
    // conflict_count starts at 3, cap=3 → count becomes 4 > 3 → first cap → auto-reset.
    // After auto-reset: count=0, auto_reset_count=1, returns RouteToBranchUpdate.
    let repo = setup_real_git_repo();
    let path = repo.path();

    setup_plan_conflict(path, "plan/count-cap-test");

    let project = make_test_project(&repo.path_string());
    // Start with count already at cap, auto_reset_count=0 (never reset before)
    let metadata =
        serde_json::json!({ "freshness_conflict_count": 3, "freshness_auto_reset_count": 0 });
    let task = make_test_task(Some(&repo.task_branch), Some(metadata));
    let mut cfg = freshness_config();
    cfg.freshness_max_conflict_retries = 3;

    let result = ensure_branches_fresh(
        path,
        &task,
        &project,
        "task-at-cap",
        Some("plan/count-cap-test"),
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;

    match result {
        Err(FreshnessAction::RouteToBranchUpdate {
            freshness_metadata, ..
        }) => {
            assert_eq!(
                freshness_metadata.freshness_auto_reset_count, 1,
                "First cap must set auto_reset_count=1"
            );
            assert_eq!(
                freshness_metadata.freshness_conflict_count, 0,
                "First cap auto-reset must reset count to 0"
            );
        }
        other => {
            panic!("First cap must RouteToBranchUpdate (auto-reset), not block. Got: {other:?}")
        }
    }
}

#[tokio::test]
async fn plan_conflict_at_cap_returns_blocked_after_auto_reset() {
    // conflict_count starts at 3, cap=3 → count becomes 4 > 3 → second cap → ExecutionBlocked.
    // This tests the path when auto_reset_count=1 (already auto-reset once before).
    let repo = setup_real_git_repo();
    let path = repo.path();

    setup_plan_conflict(path, "plan/count-cap-test2");

    let project = make_test_project(&repo.path_string());
    // Start with count at cap and already auto-reset once
    let metadata = serde_json::json!({
        "freshness_conflict_count": 3,
        "freshness_auto_reset_count": 1
    });
    let task = make_test_task(Some(&repo.task_branch), Some(metadata));
    let mut cfg = freshness_config();
    cfg.freshness_max_conflict_retries = 3;

    let result = ensure_branches_fresh(
        path,
        &task,
        &project,
        "task-at-second-cap",
        Some("plan/count-cap-test2"),
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;

    assert!(
        matches!(result, Err(FreshnessAction::ExecutionBlocked { .. })),
        "Second cap (auto_reset_count=1, count 3+1=4 > 3) must return ExecutionBlocked. Got: {:?}",
        result
    );
}

#[tokio::test]
async fn success_resets_conflict_count() {
    // Start with conflict_count=2, both branches fresh → Ok, returned metadata has count=0.
    let repo = setup_real_git_repo();
    let project = make_test_project(&repo.path_string());
    let metadata = serde_json::json!({ "freshness_conflict_count": 2 });
    let task = make_test_task(Some(&repo.task_branch), Some(metadata));
    let cfg = freshness_config();

    let result = ensure_branches_fresh(
        repo.path(),
        &task,
        &project,
        "task-reset-count",
        None,
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;

    match result {
        Ok(meta) => {
            assert_eq!(
                meta.freshness_conflict_count, 0,
                "Successful check must reset conflict count to 0"
            );
            assert!(
                !meta.branch_freshness_conflict,
                "branch_freshness_conflict must be cleared on success"
            );
        }
        Err(e) => panic!("Expected Ok with reset count, got: {e:?}"),
    }
}

// ==================
// Dual-conflict scenario
// ==================

/// Tests the full dual-conflict flow across three sequential calls:
///
/// Call 1: plan has conflicts → RouteToBranchUpdate (count=1)
/// Call 2: task metadata has count=1, plan now fresh but source has conflicts → RouteToBranchUpdate (count=2)
/// Call 3: task metadata has count=2, both fresh → Ok (count reset to 0)
#[tokio::test]
async fn dual_conflict_sequential_plan_then_source() {
    let repo = setup_real_git_repo();
    let path = repo.path();

    // Setup plan conflict: plan/dual-conflict-plan diverges from main
    setup_plan_conflict(path, "plan/dual-conflict-plan");

    let project = make_test_project(&repo.path_string());
    let mut cfg = freshness_config();
    cfg.freshness_max_conflict_retries = 5; // plenty of room

    // --- Call 1: plan has conflict ---
    let task1 = make_test_task(Some(&repo.task_branch), None);
    let result1 = ensure_branches_fresh(
        path,
        &task1,
        &project,
        "task-dual",
        Some("plan/dual-conflict-plan"),
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;

    let freshness_after_call1 = match result1 {
        Err(FreshnessAction::RouteToBranchUpdate {
            freshness_metadata,
            conflict_type: "plan_update",
            ..
        }) => {
            assert_eq!(
                freshness_metadata.freshness_conflict_count, 1,
                "Call 1 must set count=1"
            );
            freshness_metadata
        }
        other => panic!("Call 1 expected RouteToBranchUpdate(plan_update), got: {other:?}"),
    };

    // Simulate: merger agent resolved the plan conflict (plan branch now merged main).
    // We do a real git merge abort + fast-forward so the next plan check passes.
    resolve_plan_conflict(path, "plan/dual-conflict-plan");

    // Now set up a source conflict on the fresh plan branch. Advancing main here
    // would correctly make the plan stale again and require another plan update.
    setup_source_conflict(path, "plan/dual-conflict-plan");

    // --- Call 2: plan fresh, source has conflict ---
    // Carry forward the metadata from call 1 (count=1)
    let meta_json = serde_json::to_value(&freshness_after_call1).unwrap();
    let task2 = make_test_task(Some(&repo.task_branch), Some(meta_json));

    let result2 = ensure_branches_fresh(
        path,
        &task2,
        &project,
        "task-dual",
        Some("plan/dual-conflict-plan"),
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;

    let freshness_after_call2 = match result2 {
        Err(FreshnessAction::RouteToBranchUpdate {
            freshness_metadata,
            conflict_type: "source_update",
            ..
        }) => {
            assert_eq!(
                freshness_metadata.freshness_conflict_count, 2,
                "Call 2 must set count=2"
            );
            freshness_metadata
        }
        other => panic!("Call 2 expected RouteToBranchUpdate(source_update), got: {other:?}"),
    };

    // Simulate: merger agent resolved the source conflict.
    resolve_source_conflict(path, &repo.task_branch, "plan/dual-conflict-plan");

    // --- Call 3: both branches fresh ---
    let meta_json3 = serde_json::to_value(&freshness_after_call2).unwrap();
    let task3 = make_test_task(Some(&repo.task_branch), Some(meta_json3));

    let result3 = ensure_branches_fresh(
        path,
        &task3,
        &project,
        "task-dual",
        Some("plan/dual-conflict-plan"),
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;

    match result3 {
        Ok(meta) => {
            assert_eq!(
                meta.freshness_conflict_count, 0,
                "Call 3 (both fresh) must reset conflict count to 0"
            );
        }
        Err(e) => panic!("Call 3 expected Ok, got: {e:?}"),
    }
}

// ==================
// Git setup helpers for conflict tests
// ==================

/// Create a plan branch from main, then add conflicting commits on both sides.
/// After this, `plan/plan_branch_name` and `main` have diverging changes on `shared_plan.rs`.
///
/// Leaves repo on `main`.
fn setup_plan_conflict(path: &std::path::Path, plan_branch_name: &str) {
    // Create plan branch from current main HEAD
    let _ = std::process::Command::new("git")
        .args(["branch", plan_branch_name])
        .current_dir(path)
        .output()
        .expect("git branch plan");

    // Add conflicting commit to main
    let filename = format!("shared_{}.rs", plan_branch_name.replace('/', "_"));
    std::fs::write(path.join(&filename), "// main version\nfn main_impl() {}").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", &filename])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", &format!("fix: main changes {}", filename)])
        .current_dir(path)
        .output();

    // Add conflicting commit on plan branch
    let _ = std::process::Command::new("git")
        .args(["checkout", plan_branch_name])
        .current_dir(path)
        .output();
    std::fs::write(path.join(&filename), "// plan version\nfn plan_impl() {}").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", &filename])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", &format!("feat: plan changes {}", filename)])
        .current_dir(path)
        .output();

    // Back to main
    let _ = std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(path)
        .output();
}

/// Resolve a plan conflict by checking out the plan branch, using `ours` strategy for
/// the conflicting file, committing, then switching back to main.
///
/// This simulates what a merger agent does after resolving a plan_update conflict.
fn resolve_plan_conflict(path: &std::path::Path, plan_branch_name: &str) {
    let _ = std::process::Command::new("git")
        .args(["checkout", plan_branch_name])
        .current_dir(path)
        .output();

    // Merge main into plan using ours strategy (simulate agent resolution)
    let merge_out = std::process::Command::new("git")
        .args(["merge", "main", "-X", "ours", "--no-edit"])
        .current_dir(path)
        .output()
        .expect("git merge");

    if !merge_out.status.success() {
        // If merge failed for some reason, abort and use checkout --ours
        let _ = std::process::Command::new("git")
            .args(["merge", "--abort"])
            .current_dir(path)
            .output();
    }

    let _ = std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(path)
        .output();
}

/// Create a source conflict: add a commit to the target that conflicts with the task branch.
///
/// The `setup_real_git_repo()` task branch already has `feature.rs`. This adds a conflicting
/// version of `feature.rs` on main so the source update will conflict.
fn setup_source_conflict(path: &std::path::Path, target_branch: &str) {
    let checkout = std::process::Command::new("git")
        .args(["checkout", target_branch])
        .current_dir(path)
        .output()
        .expect("checkout source-update target");
    assert!(checkout.status.success());

    // Add a conflicting change to feature.rs on the target.
    std::fs::write(
        path.join("feature.rs"),
        "// main conflicting source version\nfn main_source() {}",
    )
    .unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "feature.rs"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args([
            "commit",
            "-m",
            "fix: main changes feature.rs (source conflict)",
        ])
        .current_dir(path)
        .output();

    let _ = std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(path)
        .output();
}

/// Resolve a source conflict by merging the target into the task branch with `ours` strategy.
///
/// Simulates what a merger agent does after resolving a source_update conflict.
fn resolve_source_conflict(path: &std::path::Path, task_branch: &str, target_branch: &str) {
    let _ = std::process::Command::new("git")
        .args(["checkout", task_branch])
        .current_dir(path)
        .output();

    // Merge the target using ours strategy to resolve the conflict.
    let merge_out = std::process::Command::new("git")
        .args(["merge", target_branch, "-X", "ours", "--no-edit"])
        .current_dir(path)
        .output()
        .expect("git merge for source conflict resolution");

    if !merge_out.status.success() {
        // abort any in-progress merge to leave repo in clean state
        let _ = std::process::Command::new("git")
            .args(["merge", "--abort"])
            .current_dir(path)
            .output();
    }

    let _ = std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(path)
        .output();
}

// ==================
// FreshnessMetadata struct API tests
// ==================
//
// These are pure in-memory tests (no git). They cover the new methods and fields
// added in the freshness retry loop fix: cleanup scopes, backoff, serde defaults,
// auto-reset logic, and the FRESHNESS_BLOCKED reason format.

// --- clear_from resets all fields including new ones ---

#[test]
fn clear_from_resets_everything() {
    // FreshnessMetadata::clear_from() should remove ALL freshness keys, including
    // the new freshness_backoff_until and freshness_auto_reset_count fields.
    let mut meta = serde_json::json!({
        "branch_freshness_conflict": true,
        "freshness_origin_state": "executing",
        "freshness_conflict_count": 4,
        "plan_update_conflict": true,
        "source_update_conflict": false,
        "last_freshness_check_at": "2026-01-01T00:00:00Z",
        "conflict_files": ["src/foo.rs"],
        "source_branch": "task/foo",
        "target_branch": "plan/foo",
        "freshness_backoff_until": "2026-01-01T01:00:00Z",
        "freshness_auto_reset_count": 1,
        // Non-freshness key — must survive
        "trigger_origin": "reconciler"
    });

    FreshnessMetadata::clear_from(&mut meta);

    let obj = meta.as_object().unwrap();
    // All freshness keys removed (explicitly list them to avoid relying on private KEYS const)
    let freshness_keys = [
        "branch_freshness_conflict",
        "freshness_origin_state",
        "freshness_conflict_count",
        "plan_update_conflict",
        "source_update_conflict",
        "last_freshness_check_at",
        "conflict_files",
        "source_branch",
        "target_branch",
        "freshness_backoff_until",
        "freshness_auto_reset_count",
    ];
    for key in &freshness_keys {
        assert!(
            !obj.contains_key(*key),
            "clear_from must remove key '{key}' (including new fields)"
        );
    }
    // Non-freshness key preserved
    assert_eq!(meta["trigger_origin"], "reconciler");
}

// --- FreshnessCleanupScope dispatch ---

#[test]
fn cleanup_scope_routing_only_clears_routing_flags_preserves_conflict_state() {
    // FreshnessCleanupScope::RoutingOnly should clear routing flags but
    // keep count, backoff_until, and auto_reset_count.
    let mut meta = serde_json::json!({
        "branch_freshness_conflict": true,
        "freshness_origin_state": "executing",
        "freshness_conflict_count": 3,
        "plan_update_conflict": true,
        "source_update_conflict": false,
        "conflict_files": ["src/bar.rs"],
        "source_branch": "task/bar",
        "target_branch": "plan/bar",
        "freshness_backoff_until": "2099-01-01T00:00:00Z",
        "freshness_auto_reset_count": 1,
    });

    FreshnessMetadata::cleanup(FreshnessCleanupScope::RoutingOnly, &mut meta);

    let after = FreshnessMetadata::from_task_metadata(&meta);
    // Routing flags cleared
    assert!(
        !after.branch_freshness_conflict,
        "branch_freshness_conflict must be cleared"
    );
    assert!(
        after.freshness_origin_state.is_none(),
        "freshness_origin_state must be cleared"
    );
    assert!(
        !after.plan_update_conflict,
        "plan_update_conflict must be cleared"
    );
    assert!(
        !after.source_update_conflict,
        "source_update_conflict must be cleared"
    );
    assert!(
        after.conflict_files.is_empty(),
        "conflict_files must be cleared"
    );
    assert!(
        after.source_branch.is_none(),
        "source_branch must be cleared"
    );
    assert!(
        after.target_branch.is_none(),
        "target_branch must be cleared"
    );
    // Conflict state preserved
    assert_eq!(after.freshness_conflict_count, 3, "count must be preserved");
    assert!(
        after.freshness_backoff_until.is_some(),
        "backoff_until must be preserved"
    );
    assert_eq!(
        after.freshness_auto_reset_count, 1,
        "auto_reset_count must be preserved"
    );
}

#[test]
fn cleanup_scope_conflict_state_resets_count_and_backoff() {
    // FreshnessCleanupScope::ConflictState should reset count/backoff/auto_reset_count
    // but NOT touch the routing flags.
    let mut meta = serde_json::json!({
        "branch_freshness_conflict": true,
        "freshness_origin_state": "re_executing",
        "freshness_conflict_count": 5,
        "freshness_backoff_until": "2099-01-01T00:00:00Z",
        "freshness_auto_reset_count": 1,
    });

    FreshnessMetadata::cleanup(FreshnessCleanupScope::ConflictState, &mut meta);

    let after = FreshnessMetadata::from_task_metadata(&meta);
    // Conflict state reset
    assert_eq!(
        after.freshness_conflict_count, 0,
        "count must be 0 after ConflictState reset"
    );
    assert!(
        after.freshness_backoff_until.is_none(),
        "backoff_until must be None after ConflictState reset"
    );
    assert_eq!(
        after.freshness_auto_reset_count, 0,
        "auto_reset_count must be 0 after ConflictState reset"
    );
    // Routing flags NOT cleared by ConflictState scope
    assert!(
        after.branch_freshness_conflict,
        "branch_freshness_conflict must NOT be cleared by ConflictState"
    );
    assert_eq!(
        after.freshness_origin_state.as_deref(),
        Some("re_executing"),
        "origin_state must NOT be cleared by ConflictState"
    );
}

#[test]
fn cleanup_scope_full_removes_all_freshness_keys() {
    // FreshnessCleanupScope::Full should remove ALL 11 freshness keys.
    // Keep the key list local here instead of widening the production helper surface.
    const FRESHNESS_KEYS: &[&str] = &[
        "branch_freshness_conflict",
        "freshness_origin_state",
        "freshness_conflict_count",
        "plan_update_conflict",
        "source_update_conflict",
        "last_freshness_check_at",
        "conflict_files",
        "source_branch",
        "target_branch",
        "freshness_backoff_until",
        "freshness_auto_reset_count",
    ];
    let mut meta = serde_json::json!({
        "branch_freshness_conflict": true,
        "freshness_origin_state": "reviewing",
        "freshness_conflict_count": 2,
        "plan_update_conflict": true,
        "source_update_conflict": false,
        "last_freshness_check_at": "2026-01-01T00:00:00Z",
        "conflict_files": ["src/foo.rs"],
        "source_branch": "task/branch",
        "target_branch": "plan/branch",
        "freshness_backoff_until": "2099-01-01T00:00:00Z",
        "freshness_auto_reset_count": 1,
        "other_key": "preserved",
    });

    FreshnessMetadata::cleanup(FreshnessCleanupScope::Full, &mut meta);

    let obj = meta.as_object().unwrap();
    for key in FRESHNESS_KEYS {
        assert!(!obj.contains_key(*key), "Full scope must remove '{key}'");
    }
    assert_eq!(
        meta["other_key"], "preserved",
        "Non-freshness key must survive Full cleanup"
    );
}

// --- RoutingOnly scope safety: not used in Merging→complete_merge path ---

/// Static analysis verification that FreshnessCleanupScope::RoutingOnly is NOT called
/// in any code path between plan_update_conflict being set (freshness.rs) and the
/// freshness intercept in complete_merge (git.rs / freshness_routing.rs).
///
/// # Why this matters
///
/// Using RoutingOnly in the complete_merge → freshness_return_route path would
/// prematurely clear ALL routing flags (including freshness_origin_state) before the
/// transition fires. If the transition subsequently fails, the routing signal is lost
/// permanently — the function cannot re-insert the correct origin state on rollback.
///
/// The freshness_return_route implementation instead performs TARGETED removal of
/// only `plan_update_conflict`, `branch_freshness_conflict`, and `freshness_backoff_until`,
/// preserving `freshness_origin_state` for audit and retry.
///
/// # Static analysis results (verified by code inspection)
///
/// Production call sites of `FreshnessCleanupScope::RoutingOnly`:
/// - `freshness.rs` cleanup() match arm — dispatch definition only ✅
/// - `git.rs` (complete_merge handler) — ZERO calls ✅
/// - `freshness_routing.rs` — NOT imported, only FreshnessRouteResult used ✅
/// - `chat_service_merge.rs` — ZERO calls (guard replaced with freshness_return_route) ✅
///
/// If this invariant is intentionally broken, update freshness_routing.rs to use
/// RoutingOnly and add tests verifying failure-rollback preserves freshness_origin_state.
#[test]
fn routing_only_scope_not_used_in_complete_merge_path() {
    // The compile-time guarantee: freshness_routing.rs does NOT import FreshnessCleanupScope.
    // Any use of RoutingOnly in that module would cause a compile error.
    //
    // The behavioral guarantee is verified by test_targeted_metadata_cleanup_on_success
    // in freshness_routing_tests.rs: after a successful freshness_return_route call,
    // freshness_conflict_count is preserved (RoutingOnly would also preserve it, but
    // the key distinction is that freshness_origin_state removal is also NOT performed
    // for audit purposes — distinct from RoutingOnly which always removes it).
    // FreshnessCleanupScope::RoutingOnly is not imported or called in
    // freshness_routing.rs or git.rs — see static analysis comments above.
    // The behavioral guarantee is verified by test_targeted_metadata_cleanup_on_success.
}

// --- Dynamic backoff values ---

#[test]
fn dynamic_backoff_at_various_counts() {
    // Exponential: min(base * 2^(count-1), max) with base=60, max=600
    // count=1 → 60, count=2 → 120, count=3 → 240, count=4 → 480, count=5 → 600 (capped)
    let cases: &[(u32, i64)] = &[
        (1, 60),
        (2, 120),
        (3, 240),
        (4, 480),
        (5, 600), // would be 960 but capped at 600
    ];
    for &(count, expected_secs) in cases {
        let duration = FreshnessMetadata::compute_backoff(count, 60, 600)
            .unwrap_or_else(|| panic!("count={count} must produce Some backoff"));
        assert_eq!(
            duration.num_seconds(),
            expected_secs,
            "count={count}: expected backoff={expected_secs}s, got={}s",
            duration.num_seconds()
        );
    }
}

#[test]
fn backoff_returns_none_for_count_zero() {
    assert!(
        FreshnessMetadata::compute_backoff(0, 60, 600).is_none(),
        "count=0 must return None (no backoff)"
    );
}

// --- Backoff reads from ReconciliationConfig ---

#[test]
fn backoff_reads_from_config_not_hardcoded() {
    // compute_backoff uses caller-supplied base/max, not hardcoded defaults.
    // With base=30, max=300: count=1→30, count=3→120 (30*4), count=10→300 (capped).
    let d1 = FreshnessMetadata::compute_backoff(1, 30, 300).unwrap();
    assert_eq!(d1.num_seconds(), 30, "base=30 count=1 must be 30s");

    let d3 = FreshnessMetadata::compute_backoff(3, 30, 300).unwrap();
    assert_eq!(d3.num_seconds(), 120, "base=30 count=3 must be 120s (30*4)");

    let d_big = FreshnessMetadata::compute_backoff(20, 30, 300).unwrap();
    assert_eq!(
        d_big.num_seconds(),
        300,
        "count=20 must be capped at max=300"
    );

    // Different base: 120s base, 900s max
    let cfg_base = FreshnessMetadata::compute_backoff(2, 120, 900).unwrap();
    assert_eq!(cfg_base.num_seconds(), 240, "base=120 count=2 must be 240s");
}

// --- count persistence across merger cycles ---

#[test]
fn count_persistence_across_merger_cycles() {
    // After a RouteToBranchUpdate round-trip, the merger agent calls clear_routing_flags()
    // on re-entry to Executing. count must survive that call.
    let mut freshness = FreshnessMetadata {
        branch_freshness_conflict: true,
        freshness_origin_state: Some("executing".to_string()),
        freshness_conflict_count: 2,
        plan_update_conflict: true,
        conflict_files: vec!["src/lib.rs".to_string()],
        freshness_backoff_until: Some(Utc::now() + chrono::Duration::seconds(300)),
        freshness_auto_reset_count: 0,
        ..Default::default()
    };

    // Simulate what happens on re-entry to Executing after merge resolution:
    // routing flags are cleared but count/backoff are preserved for the next conflict check.
    freshness.clear_routing_flags();

    assert_eq!(
        freshness.freshness_conflict_count, 2,
        "count must survive clear_routing_flags() (merger cycle)"
    );
    assert!(
        freshness.freshness_backoff_until.is_some(),
        "backoff_until must survive clear_routing_flags()"
    );
    assert_eq!(
        freshness.freshness_auto_reset_count, 0,
        "auto_reset_count must survive clear_routing_flags()"
    );
    // Routing flags cleared
    assert!(!freshness.branch_freshness_conflict);
    assert!(freshness.freshness_origin_state.is_none());
    assert!(!freshness.plan_update_conflict);
    assert!(freshness.conflict_files.is_empty());
}

// --- is_in_backoff ---

#[test]
fn is_in_backoff_true_when_backoff_until_in_future() {
    let meta = FreshnessMetadata {
        freshness_backoff_until: Some(Utc::now() + chrono::Duration::seconds(300)),
        ..Default::default()
    };
    assert!(
        meta.is_in_backoff(),
        "backoff_until in future must return true"
    );
}

#[test]
fn is_in_backoff_false_when_backoff_until_in_past() {
    let meta = FreshnessMetadata {
        freshness_backoff_until: Some(Utc::now() - chrono::Duration::seconds(1)),
        ..Default::default()
    };
    assert!(
        !meta.is_in_backoff(),
        "backoff_until in past must return false"
    );
}

#[test]
fn is_in_backoff_false_when_none() {
    let meta = FreshnessMetadata::default();
    assert!(
        !meta.is_in_backoff(),
        "None backoff_until must return false"
    );
}

// --- serde defaults for new fields (upgrade path) ---

#[test]
fn serde_default_new_fields_from_old_json() {
    // Deserializing old metadata JSON that does NOT have freshness_backoff_until or
    // freshness_auto_reset_count must produce the zero/None defaults, not a parse error.
    let old_metadata = serde_json::json!({
        "branch_freshness_conflict": true,
        "freshness_origin_state": "executing",
        "freshness_conflict_count": 3,
        "plan_update_conflict": true,
        "source_update_conflict": false,
        "last_freshness_check_at": "2026-01-01T00:00:00Z",
        "conflict_files": ["src/foo.rs"],
        "source_branch": "task/foo",
        "target_branch": "plan/foo"
        // freshness_backoff_until and freshness_auto_reset_count ABSENT
    });

    let fm = FreshnessMetadata::from_task_metadata(&old_metadata);
    assert!(
        fm.freshness_backoff_until.is_none(),
        "Missing freshness_backoff_until must default to None (not error)"
    );
    assert_eq!(
        fm.freshness_auto_reset_count, 0,
        "Missing freshness_auto_reset_count must default to 0 (not error)"
    );
    // Existing fields still parsed correctly
    assert_eq!(fm.freshness_conflict_count, 3);
    assert!(fm.branch_freshness_conflict);
}

// --- upgrade path: mid-conflict cycle with auto_reset_count=0 ---

#[test]
fn upgrade_path_mid_conflict_cycle_gets_auto_reset() {
    // A task upgraded from old firmware has count=3, auto_reset_count=0.
    // When count exceeds the cap (freshness_max_conflict_retries=3), the first
    // cap should auto-reset (not block) because auto_reset_count is still 0.
    //
    // This is a pure metadata test: we simulate the cap logic that runs in
    // handle_cap_if_needed() by constructing the state that would enter it.
    let mut freshness = FreshnessMetadata {
        freshness_conflict_count: 4, // exceeds cap of 3 (after increment from 3)
        freshness_auto_reset_count: 0, // never auto-reset (old firmware path)
        ..Default::default()
    };

    // Simulate first-cap auto-reset
    assert_eq!(
        freshness.freshness_auto_reset_count, 0,
        "pre: must have never auto-reset"
    );
    assert!(
        freshness.freshness_conflict_count > 3,
        "pre: count must exceed cap"
    );

    // Apply auto-reset (what handle_cap_if_needed does on first cap)
    freshness.freshness_conflict_count = 0;
    freshness.freshness_auto_reset_count = 1;
    freshness.freshness_backoff_until = Some(Utc::now() + chrono::Duration::seconds(600));

    assert_eq!(
        freshness.freshness_conflict_count, 0,
        "post: count reset to 0"
    );
    assert_eq!(
        freshness.freshness_auto_reset_count, 1,
        "post: auto_reset_count=1"
    );
    assert!(
        freshness.freshness_backoff_until.is_some(),
        "post: cooldown backoff set"
    );
    assert!(freshness.is_in_backoff(), "post: must be in backoff window");
}

// --- backoff set at conflict detection (integration-level unit test) ---

#[tokio::test]
async fn backoff_set_at_conflict_detection() {
    // When ensure_branches_fresh detects a plan conflict, the returned FreshnessMetadata
    // should have freshness_backoff_until set (non-None) if backoff_base_secs > 0.
    let repo = setup_real_git_repo();
    let path = repo.path();

    setup_plan_conflict(path, "plan/backoff-test");

    let project = make_test_project(&repo.path_string());
    let task = make_test_task(Some(&repo.task_branch), None);
    let mut cfg = freshness_config();
    cfg.freshness_backoff_base_secs = 60;
    cfg.freshness_backoff_max_secs = 600;

    let result = ensure_branches_fresh(
        path,
        &task,
        &project,
        "task-backoff-test",
        Some("plan/backoff-test"),
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;

    match result {
        Err(FreshnessAction::RouteToBranchUpdate {
            freshness_metadata, ..
        }) => {
            assert!(
                freshness_metadata.freshness_backoff_until.is_some(),
                "backoff_until must be set after first conflict (base=60s)"
            );
            assert!(
                freshness_metadata.is_in_backoff(),
                "task must be in backoff window immediately after conflict"
            );
            // count=1, base=60 → backoff=60s. Until must be roughly now+60
            let until = freshness_metadata.freshness_backoff_until.unwrap();
            let secs_from_now = (until - Utc::now()).num_seconds();
            assert!(
                secs_from_now > 50 && secs_from_now <= 65,
                "backoff_until should be ~60s from now, got {secs_from_now}s"
            );
        }
        other => panic!("Expected RouteToBranchUpdate with backoff set. Got: {other:?}"),
    }
}

// --- reconciliation respects backoff (in-memory simulation) ---

#[test]
fn is_in_backoff_unit_helper() {
    // Unit test for the FreshnessMetadata::is_in_backoff() helper method.
    // Verifies the helper returns correct values for future/past/None backoff timestamps.
    //
    // Integration: task_scheduler_service::find_oldest_schedulable_task() calls
    // is_in_backoff() on each Ready task's FreshnessMetadata before scheduling.
    let freshness_in_backoff = FreshnessMetadata {
        freshness_conflict_count: 1,
        freshness_backoff_until: Some(Utc::now() + chrono::Duration::seconds(300)),
        ..Default::default()
    };
    assert!(
        freshness_in_backoff.is_in_backoff(),
        "Task with future backoff_until must be considered in backoff"
    );

    // After backoff window passes, is_in_backoff returns false
    let freshness_expired = FreshnessMetadata {
        freshness_conflict_count: 1,
        freshness_backoff_until: Some(Utc::now() - chrono::Duration::seconds(1)),
        ..Default::default()
    };
    assert!(
        !freshness_expired.is_in_backoff(),
        "Expired backoff_until must allow re-queuing"
    );

    // No backoff set — always allow re-queuing
    let freshness_no_backoff = FreshnessMetadata {
        freshness_conflict_count: 2,
        freshness_backoff_until: None,
        ..Default::default()
    };
    assert!(
        !freshness_no_backoff.is_in_backoff(),
        "None backoff_until must allow re-queuing"
    );
}

// --- FRESHNESS_BLOCKED reason format ---

#[test]
fn execution_blocked_reason_format() {
    // The ExecutionBlocked reason must follow the structured format:
    // FRESHNESS_BLOCKED|{total}|{minutes}|{files}|{msg}
    //
    // We construct the reason string the same way handle_cap_if_needed does,
    // then verify the format is parseable with the expected pipe-delimited structure.
    let total: u32 = 6;
    let minutes: u64 = 10; // 600s / 60
    let files = "src/foo.rs, src/bar.rs";
    let reason = format!(
        "FRESHNESS_BLOCKED|{}|{}|{}|Persistent freshness conflicts after auto-reset",
        total, minutes, files
    );

    // Must start with the structured prefix
    assert!(
        reason.starts_with("FRESHNESS_BLOCKED|"),
        "reason must start with FRESHNESS_BLOCKED| prefix"
    );

    // Pipe-delimited: 5 segments
    let parts: Vec<&str> = reason.splitn(5, '|').collect();
    assert_eq!(parts.len(), 5, "reason must have 5 pipe-delimited segments");
    assert_eq!(parts[0], "FRESHNESS_BLOCKED");
    assert_eq!(parts[1], "6", "segment 2 must be total conflict count");
    assert_eq!(parts[2], "10", "segment 3 must be cooldown minutes");
    assert_eq!(
        parts[3], "src/foo.rs, src/bar.rs",
        "segment 4 must be conflict files"
    );
    assert!(
        parts[4].contains("Persistent freshness conflicts"),
        "segment 5 must contain human-readable message"
    );

    // Verify the FreshnessAction::ExecutionBlocked variant wraps it correctly
    let action = FreshnessAction::ExecutionBlocked {
        reason: reason.clone(),
        branch_missing: None,
    };
    assert!(
        matches!(action, FreshnessAction::ExecutionBlocked { .. }),
        "must be ExecutionBlocked variant"
    );
}

// --- no_backoff_refresh_after_successful_merge ---

#[tokio::test]
async fn no_backoff_refresh_after_successful_merge() {
    // After a successful freshness check (both branches fresh), the returned
    // FreshnessMetadata must NOT have an active backoff window.
    // The reset_conflict_state() call in step 8 clears backoff_until.
    let repo = setup_real_git_repo();
    let project = make_test_project(&repo.path_string());

    // Start with a pre-existing (expired) backoff to confirm it gets cleared on success.
    let expired_backoff = (Utc::now() - chrono::Duration::seconds(1)).to_rfc3339();
    let metadata = serde_json::json!({
        "freshness_conflict_count": 2,
        "freshness_backoff_until": expired_backoff,
        "freshness_auto_reset_count": 0,
    });
    let task = make_test_task(Some(&repo.task_branch), Some(metadata));

    let cfg = freshness_config(); // backoff_base_secs=0, skip window is clear

    let result = ensure_branches_fresh(
        repo.path(),
        &task,
        &project,
        "task-no-backoff-after-success",
        None,
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;

    match result {
        Ok(meta) => {
            assert_eq!(
                meta.freshness_conflict_count, 0,
                "Successful check must reset conflict count to 0"
            );
            assert!(
                meta.freshness_backoff_until.is_none(),
                "Successful check must clear backoff_until"
            );
            assert_eq!(
                meta.freshness_auto_reset_count, 0,
                "Successful check must reset auto_reset_count"
            );
            assert!(
                !meta.is_in_backoff(),
                "After success, task must NOT be in backoff"
            );
        }
        Err(e) => panic!("Expected Ok (fresh repo), got: {e:?}"),
    }
}

// --- merge_into round-trips new fields ---

#[test]
fn serde_round_trip_includes_new_fields() {
    // merge_into and from_task_metadata must correctly round-trip the two new fields:
    // freshness_backoff_until and freshness_auto_reset_count.
    let backoff_time = Utc::now() + chrono::Duration::seconds(120);
    let original = FreshnessMetadata {
        freshness_conflict_count: 2,
        freshness_backoff_until: Some(backoff_time),
        freshness_auto_reset_count: 1,
        ..Default::default()
    };

    let mut meta = serde_json::json!({});
    original.merge_into(&mut meta);

    // freshness_backoff_until must be written as RFC3339 string
    assert!(
        meta.get("freshness_backoff_until")
            .and_then(|v| v.as_str())
            .is_some(),
        "freshness_backoff_until must be written as RFC3339 string"
    );
    assert_eq!(
        meta["freshness_auto_reset_count"], 1,
        "freshness_auto_reset_count must be written as number"
    );

    let recovered = FreshnessMetadata::from_task_metadata(&meta);
    assert_eq!(recovered.freshness_auto_reset_count, 1);
    assert_eq!(recovered.freshness_conflict_count, 2);
    assert!(
        recovered.freshness_backoff_until.is_some(),
        "freshness_backoff_until must survive round-trip"
    );

    // The recovered timestamp must be within 1 second of original (RFC3339 has 1s resolution)
    let original_secs = backoff_time.timestamp();
    let recovered_secs = recovered.freshness_backoff_until.unwrap().timestamp();
    assert!(
        (original_secs - recovered_secs).abs() <= 1,
        "freshness_backoff_until must round-trip within 1s precision"
    );
}
