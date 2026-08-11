// Real git integration tests for ensure_branches_fresh() entry point.
//
// These tests verify end-to-end behaviour using actual git repositories
// (tempfile::TempDir + git CLI) to confirm that the freshness check
// orchestration function behaves correctly for the most important scenarios:
//
//   1. Fresh task branch passes silently (timestamp updated, no conflict)
//   2. Stale + conflicting task branch routes to a dedicated branch update
//   3. Config disabled skips all git operations
//   4. Fresh branch updates last_freshness_check_at timestamp
//   5. Skip window prevents recheck when called twice within the window
//
// The companion unit tests in freshness_tests.rs cover config toggle,
// skip window, plan/source result mapping, retry counting, and dual-conflict
// sequential scenarios using mocked and real repos. These integration tests
// focus on verifiable end-to-end git interaction patterns.

use crate::support::real_git_repo::{setup_real_git_repo, RealGitRepo};
use ralphx_lib::domain::entities::{Project, ProjectId, Task};
use ralphx_lib::domain::state_machine::transition_handler::freshness::{
    ensure_branches_fresh, FreshnessAction, FreshnessMetadata,
};
use ralphx_lib::infrastructure::agents::claude::ReconciliationConfig;

// ==================
// Local helpers
// ==================

fn make_project(repo_path: &str) -> Project {
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

fn make_task_with_branch_and_meta(task_branch: &str, metadata: Option<serde_json::Value>) -> Task {
    let mut t = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Test task".into(),
    );
    t.task_branch = Some(task_branch.to_string());
    t.metadata = metadata.map(|v| v.to_string());
    t
}

fn integration_test_config() -> ReconciliationConfig {
    ReconciliationConfig {
        branch_freshness_timeout_secs: 30,
        freshness_skip_window_secs: 0, // never skip by default
        freshness_max_conflict_retries: 3,
        execution_freshness_enabled: true,
        ..Default::default()
    }
}

/// Build a git repo where main and task branch have diverging changes on the
/// same file (`shared.rs`), causing a merge conflict.
fn setup_conflicting_git_repo() -> RealGitRepo {
    let dir = tempfile::TempDir::new().expect("create temp dir");
    let path = dir.path();

    // git init
    let _ = std::process::Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(path)
        .output()
        .expect("git init");
    let _ = std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(path)
        .output();

    // Initial commit with a shared file
    std::fs::write(path.join("shared.rs"), "// line A").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "initial commit"])
        .current_dir(path)
        .output();

    // Task branch: modify shared.rs
    let task_branch = "task/conflict-test".to_string();
    let _ = std::process::Command::new("git")
        .args(["checkout", "-b", &task_branch])
        .current_dir(path)
        .output();
    std::fs::write(path.join("shared.rs"), "// line B from task branch").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "task: change shared.rs"])
        .current_dir(path)
        .output();

    // Back to main: modify shared.rs differently → conflict
    let _ = std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(path)
        .output();
    std::fs::write(path.join("shared.rs"), "// line C from main").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "main: change shared.rs"])
        .current_dir(path)
        .output();

    RealGitRepo { dir, task_branch }
}

// ==================
// Integration test 1: Fresh task branch passes silently
// ==================

/// A task branch that is already up-to-date with main must pass the freshness
/// check and return Ok with last_freshness_check_at set and no conflict flag.
#[tokio::test]
async fn test_fresh_task_branch_passes_silently() {
    let repo = setup_real_git_repo();
    let project = make_project(&repo.path_string());
    let task = make_task_with_branch_and_meta(&repo.task_branch, None);
    let cfg = integration_test_config();

    let result = ensure_branches_fresh(
        repo.path(),
        &task,
        &project,
        "integration-fresh-task",
        None, // no plan branch — skip plan check
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;

    assert!(
        result.is_ok(),
        "Fresh task branch must return Ok, got: {:?}",
        result
    );
    let meta = result.unwrap();
    assert!(
        meta.last_freshness_check_at.is_some(),
        "last_freshness_check_at must be set after a successful check"
    );
    assert!(
        !meta.branch_freshness_conflict,
        "branch_freshness_conflict must be false for a fresh task branch"
    );
    assert_eq!(
        meta.freshness_conflict_count, 0,
        "conflict count must be 0 for a fresh task branch"
    );
}

// ==================
// Integration test 2: Stale + conflicting task branch routes to a dedicated branch update
// ==================

/// When the task branch and main have diverging changes on the same file,
/// ensure_branches_fresh must return Err(RouteToBranchUpdate) with the correct
/// conflict_type and updated freshness metadata.
#[tokio::test]
async fn test_stale_task_branch_routes_to_branch_update() {
    let repo = setup_conflicting_git_repo();
    let project = make_project(&repo.path_string());
    let task = make_task_with_branch_and_meta(&repo.task_branch, None);
    let cfg = integration_test_config();

    let result = ensure_branches_fresh(
        repo.path(),
        &task,
        &project,
        "integration-conflict-task",
        None, // no plan branch → source check uses main as target
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;

    match result {
        Err(FreshnessAction::RouteToBranchUpdate {
            conflict_type,
            freshness_metadata,
            ..
        }) => {
            assert_eq!(
                conflict_type, "source_update",
                "Conflict from task←main must be conflict_type=source_update"
            );
            assert!(
                freshness_metadata.branch_freshness_conflict,
                "branch_freshness_conflict must be true after routing to Merging"
            );
            assert!(
                freshness_metadata.source_update_conflict,
                "source_update_conflict must be true"
            );
            assert!(
                !freshness_metadata.plan_update_conflict,
                "plan_update_conflict must be false"
            );
            assert_eq!(
                freshness_metadata.freshness_conflict_count, 1,
                "conflict count must be incremented to 1"
            );
            assert_eq!(
                freshness_metadata.freshness_origin_state.as_deref(),
                Some("executing"),
                "origin state must be recorded"
            );
        }
        other => panic!(
            "Expected Err(RouteToBranchUpdate(source_update)), got: {:?}",
            other
        ),
    }
}

// ==================
// Integration test 3: Freshness disabled skips all git operations
// ==================

/// When execution_freshness_enabled=false, ensure_branches_fresh must return
/// Ok immediately without touching git. We pass a nonexistent path — any git
/// invocation would fail, exposing an early-exit that didn't happen.
#[tokio::test]
async fn test_freshness_disabled_skips_check() {
    let mut cfg = integration_test_config();
    cfg.execution_freshness_enabled = false;

    let task = make_task_with_branch_and_meta("task/any-branch", None);
    let project = make_project("/nonexistent/path-should-not-be-accessed");
    let nonexistent = std::path::Path::new("/nonexistent/path-should-not-be-accessed");

    let result = ensure_branches_fresh(
        nonexistent,
        &task,
        &project,
        "integration-disabled",
        Some("plan/any-plan"),
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;

    assert!(
        result.is_ok(),
        "Disabled freshness must return Ok immediately, got: {:?}",
        result
    );
    let meta = result.unwrap();
    assert_eq!(
        meta,
        FreshnessMetadata::default(),
        "Disabled freshness must return default (empty) metadata"
    );
}

// ==================
// Integration test 4: Fresh branch updates last_freshness_check_at timestamp
// ==================

/// Two consecutive calls to ensure_branches_fresh with skip_window_secs=0 must
/// both succeed and both set last_freshness_check_at. The second call's timestamp
/// must be >= the first call's timestamp (monotonically non-decreasing).
#[tokio::test]
async fn test_fresh_branch_updates_last_check_timestamp() {
    let repo = setup_real_git_repo();
    let project = make_project(&repo.path_string());
    let cfg = integration_test_config(); // skip_window_secs=0 → always check

    // First call
    let task1 = make_task_with_branch_and_meta(&repo.task_branch, None);
    let result1 = ensure_branches_fresh(
        repo.path(),
        &task1,
        &project,
        "integration-timestamp-1",
        None,
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;
    assert!(
        result1.is_ok(),
        "First call must succeed. Got: {:?}",
        result1
    );
    let meta1 = result1.unwrap();
    let ts1 = meta1
        .last_freshness_check_at
        .as_deref()
        .and_then(|s| s.parse::<chrono::DateTime<chrono::Utc>>().ok())
        .expect("First call must set last_freshness_check_at");

    // Second call (skip_window=0 → no skip)
    let task2 = make_task_with_branch_and_meta(
        &repo.task_branch,
        Some(serde_json::to_value(&meta1).unwrap()),
    );
    let result2 = ensure_branches_fresh(
        repo.path(),
        &task2,
        &project,
        "integration-timestamp-2",
        None,
        None,
        None,
        None,
        "executing",
        &cfg,
    )
    .await;
    assert!(
        result2.is_ok(),
        "Second call must succeed. Got: {:?}",
        result2
    );
    let meta2 = result2.unwrap();
    let ts2 = meta2
        .last_freshness_check_at
        .as_deref()
        .and_then(|s| s.parse::<chrono::DateTime<chrono::Utc>>().ok())
        .expect("Second call must set last_freshness_check_at");

    assert!(
        ts2 >= ts1,
        "Second check timestamp must be >= first. ts1={ts1}, ts2={ts2}"
    );
}

// ==================
// Integration test 5: Skip window prevents recheck within window
// ==================

/// When a freshness check was performed recently (last_freshness_check_at is
/// within skip_window_secs), a second call must return Ok immediately without
/// running git operations. The returned metadata must preserve the ORIGINAL
/// last_freshness_check_at timestamp from the first call.
#[tokio::test]
async fn test_skip_window_prevents_recheck() {
    let repo = setup_real_git_repo();
    let project = make_project(&repo.path_string());

    // First call with skip_window=0 to force a check and record timestamp
    let mut cfg_no_skip = integration_test_config();
    cfg_no_skip.freshness_skip_window_secs = 0;

    let task1 = make_task_with_branch_and_meta(&repo.task_branch, None);
    let result1 = ensure_branches_fresh(
        repo.path(),
        &task1,
        &project,
        "integration-skip-window-1",
        None,
        None,
        None,
        None,
        "executing",
        &cfg_no_skip,
    )
    .await;
    assert!(
        result1.is_ok(),
        "First call must succeed. Got: {:?}",
        result1
    );
    let meta1 = result1.unwrap();
    let original_ts = meta1
        .last_freshness_check_at
        .clone()
        .expect("First call must set last_freshness_check_at");

    // Second call with a large skip window (1 hour) → should be skipped
    let mut cfg_long_skip = integration_test_config();
    cfg_long_skip.freshness_skip_window_secs = 3600;

    // Pass task with the freshness metadata from first call (has recent timestamp)
    let task2 = make_task_with_branch_and_meta(
        &repo.task_branch,
        Some(serde_json::to_value(&meta1).unwrap()),
    );
    let result2 = ensure_branches_fresh(
        // Use a nonexistent path — if git were called, it would fail
        std::path::Path::new("/nonexistent/skip-window-guard"),
        &task2,
        &project,
        "integration-skip-window-2",
        None,
        None,
        None,
        None,
        "executing",
        &cfg_long_skip,
    )
    .await;

    assert!(
        result2.is_ok(),
        "Second call within skip window must return Ok (skipped). Got: {:?}",
        result2
    );
    let meta2 = result2.unwrap();
    assert_eq!(
        meta2.last_freshness_check_at.as_deref(),
        Some(original_ts.as_str()),
        "Skip window must preserve the original last_freshness_check_at (not update it)"
    );
}
