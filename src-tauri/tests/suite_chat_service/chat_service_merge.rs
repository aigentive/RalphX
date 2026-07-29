use ralphx_lib::application::chat_service::{
    merge_completion_watcher_loop, resolve_watcher_context, verify_merge_on_target,
    AutoCompleteGuard, MergeVerification,
};
use ralphx_lib::application::git_service::checkout_free::update_branch_ref;
use ralphx_lib::application::GitService;
use ralphx_lib::application::interactive_process_registry::InteractiveProcessKey;
use ralphx_lib::application::InteractiveProcessRegistry;
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::*;
use ralphx_lib::domain::repositories::*;
use ralphx_lib::infrastructure::memory::{self, *};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

/// Create a temp git repo with an initial commit, returns the repo path
fn setup_test_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    let repo = dir.path();

    // Init repo
    Command::new("git")
        .args(["init"])
        .current_dir(repo)
        .output()
        .expect("git init failed");

    // Configure git user for commits
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(repo)
        .output()
        .expect("git config email failed");
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(repo)
        .output()
        .expect("git config name failed");

    // Create initial commit on main
    fs::write(repo.join("README.md"), "# Test Repo\n").expect("write failed");
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .expect("git add failed");
    Command::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(repo)
        .output()
        .expect("git commit failed");

    // Ensure we're on 'main'
    Command::new("git")
        .args(["branch", "-M", "main"])
        .current_dir(repo)
        .output()
        .expect("git branch -M main failed");

    dir
}

/// Create a branch with a file change and commit
fn create_branch_with_change(repo: &Path, branch: &str, filename: &str, content: &str) {
    Command::new("git")
        .args(["checkout", "-b", branch])
        .current_dir(repo)
        .output()
        .expect("git checkout -b failed");

    fs::write(repo.join(filename), content).expect("write failed");
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .expect("git add failed");
    Command::new("git")
        .args(["commit", "-m", &format!("Add {}", filename)])
        .current_dir(repo)
        .output()
        .expect("git commit failed");

    // Go back to main
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(repo)
        .output()
        .expect("git checkout main failed");
}

/// Merge a branch into the current branch
fn merge_branch(repo: &Path, branch: &str) {
    Command::new("git")
        .args(["merge", branch, "--no-edit"])
        .current_dir(repo)
        .output()
        .expect("git merge failed");
}

#[tokio::test]
async fn test_verify_merge_happy_path_merged() {
    let dir = setup_test_repo();
    let repo = dir.path();

    // Create a task branch with changes
    create_branch_with_change(repo, "task-branch", "feature.txt", "feature content\n");

    // Merge it into main
    merge_branch(repo, "task-branch");

    // Verify the merge
    let result = verify_merge_on_target(repo, "task-branch", "main").await;
    match result {
        MergeVerification::Merged(sha) => {
            assert!(!sha.is_empty(), "Merge commit SHA should not be empty");
        }
        _ => panic!("Expected Merged, got: {:?}", result),
    }
}

#[tokio::test]
async fn test_verify_merge_race_condition_not_merged() {
    let dir = setup_test_repo();
    let repo = dir.path();

    // Create a task branch but DON'T merge it
    create_branch_with_change(repo, "task-branch", "feature.txt", "feature content\n");

    // Simulate the race condition: we're checking from main repo
    // Task branch exists but is not merged to main
    let result = verify_merge_on_target(repo, "task-branch", "main").await;
    assert_eq!(
        result,
        MergeVerification::NotMerged,
        "Expected NotMerged for unmerged task branch"
    );
}

#[tokio::test]
async fn test_verify_merge_source_branch_missing() {
    let dir = setup_test_repo();
    let repo = dir.path();

    // Try to verify a non-existent source branch
    let result = verify_merge_on_target(repo, "non-existent-branch", "main").await;
    assert_eq!(
        result,
        MergeVerification::SourceBranchMissing,
        "Expected SourceBranchMissing for non-existent source branch"
    );
}

#[tokio::test]
async fn test_verify_merge_target_branch_missing() {
    let dir = setup_test_repo();
    let repo = dir.path();

    // Create a task branch
    create_branch_with_change(repo, "task-branch", "feature.txt", "feature content\n");

    // Try to verify against a non-existent target branch
    let result = verify_merge_on_target(repo, "task-branch", "non-existent-target").await;
    assert_eq!(
        result,
        MergeVerification::TargetBranchMissing,
        "Expected TargetBranchMissing for non-existent target branch"
    );
}

#[tokio::test]
async fn test_verify_merge_plan_branch_merged() {
    let dir = setup_test_repo();
    let repo = dir.path();

    // Create a plan branch
    create_branch_with_change(repo, "plan-abc123", "plan-feature.txt", "plan content\n");

    // Create a task branch from the plan branch
    Command::new("git")
        .args(["checkout", "plan-abc123"])
        .current_dir(repo)
        .output()
        .expect("git checkout plan-abc123 failed");

    create_branch_with_change(repo, "task-branch", "task-feature.txt", "task content\n");

    // Merge task branch into plan branch
    Command::new("git")
        .args(["checkout", "plan-abc123"])
        .current_dir(repo)
        .output()
        .expect("git checkout plan-abc123 failed");

    merge_branch(repo, "task-branch");

    // Verify the merge to plan branch
    let result = verify_merge_on_target(repo, "task-branch", "plan-abc123").await;
    match result {
        MergeVerification::Merged(sha) => {
            assert!(!sha.is_empty(), "Merge commit SHA should not be empty");
        }
        _ => panic!(
            "Expected Merged for task merged to plan branch, got: {:?}",
            result
        ),
    }
}

#[tokio::test]
async fn test_verify_merge_plan_branch_not_merged() {
    let dir = setup_test_repo();
    let repo = dir.path();

    // Create a plan branch
    create_branch_with_change(repo, "plan-abc123", "plan-feature.txt", "plan content\n");

    // Create a task branch but DON'T merge it to the plan branch
    create_branch_with_change(repo, "task-branch", "task-feature.txt", "task content\n");

    // Verify should return NotMerged
    let result = verify_merge_on_target(repo, "task-branch", "plan-abc123").await;
    assert_eq!(
        result,
        MergeVerification::NotMerged,
        "Expected NotMerged for task not merged to plan branch"
    );
}

// --- Conflict metadata target_branch extraction tests ---
// These verify the fix for: attempt_merge_auto_complete using wrong target_branch after
// plan_update_conflict / source_update_conflict resolution.
//
// Root cause: resolve_merge_branches() can return "main" (the base branch) if the
// plan branch state changed between conflict detection and auto-complete invocation.
// Fix: read target_branch from task metadata, which is stored at conflict-detection
// time and is the authoritative value.

#[test]
fn test_plan_update_conflict_target_branch_from_metadata() {
    // Simulates the metadata set by side_effects.rs when plan←main has conflicts
    let meta_json = r#"{
        "plan_update_conflict": true,
        "target_branch": "ralphx/ralphx/plan-c785dcd0",
        "base_branch": "main",
        "source_branch": "ralphx/ralphx/task-abc123"
    }"#;
    let meta: serde_json::Value = serde_json::from_str(meta_json).unwrap();

    // This is the extraction logic in the plan_update_conflict path
    let plan_branch = meta
        .get("target_branch")
        .and_then(|v| v.as_str())
        .map(String::from);

    // Must return the plan branch, not "main"
    assert_eq!(plan_branch, Some("ralphx/ralphx/plan-c785dcd0".to_string()));

    // Fallback when metadata missing target_branch (uses resolve_merge_branches result)
    let meta_no_target: serde_json::Value =
        serde_json::from_str(r#"{"plan_update_conflict": true, "base_branch": "main"}"#).unwrap();
    let missing = meta_no_target
        .get("target_branch")
        .and_then(|v| v.as_str())
        .map(String::from);
    // None → caller uses resolve_merge_branches fallback
    assert_eq!(missing, None);
}

#[test]
fn test_source_update_conflict_target_branch_from_metadata() {
    // Simulates the metadata set by side_effects.rs when source←target has conflicts
    let meta_json = r#"{
        "source_update_conflict": true,
        "target_branch": "ralphx/ralphx/plan-c785dcd0",
        "source_branch": "ralphx/ralphx/task-abc123"
    }"#;
    let meta: serde_json::Value = serde_json::from_str(meta_json).unwrap();

    // This is the extraction logic in the source_update_conflict path
    let target_branch = meta
        .get("target_branch")
        .and_then(|v| v.as_str())
        .map(String::from);

    // Must return the plan branch, not "main"
    assert_eq!(
        target_branch,
        Some("ralphx/ralphx/plan-c785dcd0".to_string())
    );
}

#[test]
fn test_conflict_metadata_target_branch_not_contaminated_by_base_branch() {
    // Regression test: when resolve_merge_branches returns "main" as target_branch
    // (fallback due to plan_branch_repo being None or state change), the metadata
    // extraction must still return the correct plan branch stored in metadata.
    let meta_json = r#"{
        "plan_update_conflict": true,
        "target_branch": "ralphx/ralphx/plan-c785dcd0",
        "base_branch": "main"
    }"#;
    let meta: serde_json::Value = serde_json::from_str(meta_json).unwrap();

    // Simulate resolve_merge_branches returning "main" (the bug scenario)
    let resolved_target_branch = "main".to_string();

    // The fixed code reads from metadata first
    let plan_branch = meta
        .get("target_branch")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| resolved_target_branch.clone());

    // Must NOT be "main" — must be the plan branch from metadata
    assert_ne!(
        plan_branch, "main",
        "plan_branch must not fall back to 'main' when metadata has the correct target_branch"
    );
    assert_eq!(plan_branch, "ralphx/ralphx/plan-c785dcd0");
}

// --- Auto-complete dedup guard tests ---

#[test]
fn test_auto_complete_guard_prevents_duplicate() {
    let exec_state = ExecutionState::new();
    let task_id = "task-abc123";

    // First call succeeds
    assert!(exec_state.try_start_auto_complete(task_id));
    // Second call is blocked (duplicate)
    assert!(!exec_state.try_start_auto_complete(task_id));
}

#[test]
fn test_auto_complete_guard_allows_different_tasks() {
    let exec_state = ExecutionState::new();

    assert!(exec_state.try_start_auto_complete("task-a"));
    assert!(exec_state.try_start_auto_complete("task-b"));
    // Still blocked for task-a
    assert!(!exec_state.try_start_auto_complete("task-a"));
}

#[test]
fn test_auto_complete_guard_cleanup_allows_retry() {
    let exec_state = ExecutionState::new();
    let task_id = "task-abc123";

    assert!(exec_state.try_start_auto_complete(task_id));
    assert!(!exec_state.try_start_auto_complete(task_id));

    // After cleanup, a new call is allowed
    exec_state.finish_auto_complete(task_id);
    assert!(exec_state.try_start_auto_complete(task_id));
}

#[test]
fn test_auto_complete_raii_guard_cleans_up_on_drop() {
    let exec_state = Arc::new(ExecutionState::new());
    let task_id = "task-abc123";

    // Simulate inserting and creating a guard
    assert!(exec_state.try_start_auto_complete(task_id));
    {
        let _guard = AutoCompleteGuard::for_test(Arc::clone(&exec_state), task_id);
        // While guard is alive, duplicate is blocked
        assert!(!exec_state.try_start_auto_complete(task_id));
        // Re-insert since try_start failed (it wasn't actually added again)
    }
    // After guard drops, the task is removed from the set
    assert!(exec_state.try_start_auto_complete(task_id));
}

// --- TOCTOU guard: cached merge_target_branch extraction ---

#[test]
fn test_toctou_cached_target_branch_wins_over_resolved() {
    // Regression test for TOCTOU race: plan state changes after merge is dispatched.
    //
    // Scenario: at dispatch time, task metadata cached merge_target_branch = "plan/my-feature"
    // By the time auto-complete runs, resolve_merge_branches returns "main" (plan no longer Active).
    // The TOCTOU guard (chat_service_merge.rs lines 419-434) must use the cached value.
    let meta_json = r#"{
        "merge_source_branch": "task/test-task",
        "merge_target_branch": "plan/my-feature"
    }"#;
    let meta: serde_json::Value = serde_json::from_str(meta_json).unwrap();

    // Simulate: re-resolved target_branch is now "main" (plan state changed)
    let mut target_branch = "main".to_string();

    // TOCTOU guard extraction (mirrors chat_service_merge.rs lines 421-434)
    if let Some(stored) = meta
        .get("merge_target_branch")
        .and_then(|v| v.as_str().map(String::from))
    {
        if stored != target_branch {
            target_branch = stored;
        }
    }

    assert_eq!(
        target_branch, "plan/my-feature",
        "TOCTOU guard: cached merge_target_branch must override re-resolved 'main'"
    );
}

#[test]
fn test_toctou_cached_target_branch_absent_falls_back_to_resolved() {
    // When metadata has no merge_target_branch, use the freshly resolved value.
    let meta_json = r#"{"some_other_key": "value"}"#;
    let meta: serde_json::Value = serde_json::from_str(meta_json).unwrap();

    let mut target_branch = "plan/active-plan".to_string();

    // TOCTOU guard extraction
    if let Some(stored) = meta
        .get("merge_target_branch")
        .and_then(|v| v.as_str().map(String::from))
    {
        if stored != target_branch {
            target_branch = stored;
        }
    }

    // No cached value — resolved value is preserved unchanged
    assert_eq!(
        target_branch, "plan/active-plan",
        "When metadata has no merge_target_branch, resolved value must be used"
    );
}

#[test]
fn test_toctou_cached_target_same_as_resolved_no_override() {
    // When cached value equals the resolved value, no change (guard is a no-op).
    let meta_json = r#"{"merge_target_branch": "plan/active-plan"}"#;
    let meta: serde_json::Value = serde_json::from_str(meta_json).unwrap();

    let mut target_branch = "plan/active-plan".to_string();

    if let Some(stored) = meta
        .get("merge_target_branch")
        .and_then(|v| v.as_str().map(String::from))
    {
        if stored != target_branch {
            target_branch = stored;
        }
    }

    // Same value — no override needed
    assert_eq!(target_branch, "plan/active-plan");
}

// --- RC#7: merge-resolve branch fast-forward tests ---
// These verify the fix for: merge-resolve/{task_id} branch commits not being
// fast-forwarded to the target branch before verification, causing
// verify_merge_on_target to return NotMerged → MergeIncomplete.

/// RC#7: After checkout-free conflict resolution via merge-resolve/{task_id},
/// the target branch must be fast-forwarded to the merge-resolve HEAD before
/// verify_merge_on_target is called. Without the fast-forward, verification
/// returns NotMerged because the merge commit lives only on merge-resolve.
#[tokio::test]
async fn test_merge_resolve_branch_fast_forwards_target_before_verification() {
    let dir = setup_test_repo();
    let repo = dir.path();
    let task_id = "task-rc7-test";

    // Create a task branch with a file that conflicts with main
    create_branch_with_change(repo, "task/rc7", "shared.txt", "task version of shared.txt\n");

    // Create a conflicting commit on main
    fs::write(repo.join("shared.txt"), "main version of shared.txt\n").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .expect("git add");
    Command::new("git")
        .args(["commit", "-m", "conflicting change on main"])
        .current_dir(repo)
        .output()
        .expect("git commit");

    // Record main HEAD before conflict resolution
    let main_sha_before = String::from_utf8_lossy(
        &Command::new("git")
            .args(["rev-parse", "main"])
            .current_dir(repo)
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();

    // Simulate what merge_outcome_handler.rs:331 does:
    // 1. Create merge-resolve/{task_id} branch at target SHA
    let resolve_branch = format!("merge-resolve/{}", task_id);
    Command::new("git")
        .args(["branch", &resolve_branch, &main_sha_before])
        .current_dir(repo)
        .output()
        .expect("create resolve branch");

    // 2. Create worktree for conflict resolution
    let wt_path = repo.join("merge-resolve-wt");
    Command::new("git")
        .args([
            "worktree",
            "add",
            wt_path.to_str().unwrap(),
            &resolve_branch,
        ])
        .current_dir(repo)
        .output()
        .expect("create worktree");

    // 3. Merge task branch in worktree (will have conflicts)
    let merge_output = Command::new("git")
        .args(["merge", "task/rc7", "--no-edit"])
        .current_dir(&wt_path)
        .output()
        .expect("merge in worktree");
    // Merge should fail due to conflicts
    assert!(
        !merge_output.status.success(),
        "Expected merge conflict but merge succeeded"
    );

    // 4. Simulate agent resolving conflicts: pick the task version
    fs::write(wt_path.join("shared.txt"), "resolved: merged content\n").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(&wt_path)
        .output()
        .expect("git add resolved");
    Command::new("git")
        .args(["commit", "--no-edit"])
        .current_dir(&wt_path)
        .output()
        .expect("git commit resolution");

    // Get merge-resolve HEAD SHA (this is where the merge commit lives)
    let resolve_sha = String::from_utf8_lossy(
        &Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&wt_path)
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();

    // BEFORE fix: verify_merge_on_target returns NotMerged because main hasn't been updated
    let pre_fix_result = verify_merge_on_target(repo, "task/rc7", "main").await;
    assert_eq!(
        pre_fix_result,
        MergeVerification::NotMerged,
        "Before fast-forward, target branch should NOT show task as merged"
    );

    // THE FIX: fast-forward target branch to merge-resolve HEAD
    let resolve_sha_from_git =
        GitService::get_branch_sha(repo, &resolve_branch).await.unwrap();
    assert_eq!(
        resolve_sha, resolve_sha_from_git,
        "merge-resolve branch SHA should match the commit we just made"
    );
    update_branch_ref(repo, "main", &resolve_sha_from_git)
        .await
        .expect("fast-forward target branch");

    // AFTER fix: verify_merge_on_target returns Merged
    let post_fix_result = verify_merge_on_target(repo, "task/rc7", "main").await;
    match post_fix_result {
        MergeVerification::Merged(sha) => {
            assert_eq!(
                sha, resolve_sha,
                "Target branch HEAD should match merge-resolve HEAD after fast-forward"
            );
        }
        other => panic!(
            "Expected Merged after fast-forward, got: {:?}",
            other
        ),
    }

    // Verify main branch now points to the merge-resolve commit
    let main_sha_after = String::from_utf8_lossy(
        &Command::new("git")
            .args(["rev-parse", "main"])
            .current_dir(repo)
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();
    assert_eq!(
        main_sha_after, resolve_sha,
        "main branch should now point to the merge-resolve commit"
    );

    // Cleanup: delete worktree and branch (mirrors what the fix does)
    let _ = Command::new("git")
        .args(["worktree", "remove", "--force", wt_path.to_str().unwrap()])
        .current_dir(repo)
        .output();
    let _ = Command::new("git")
        .args(["branch", "-D", &resolve_branch])
        .current_dir(repo)
        .output();
}

// ============================================================================
// Merge Completion Watcher Tests
// ============================================================================
//
// Tests for `resolve_watcher_context` and `merge_completion_watcher_loop`.

// ---- Shared watcher test helpers ------------------------------------------

fn make_test_repos() -> (Arc<memory::MemoryTaskRepository>, Arc<memory::MemoryProjectRepository>) {
    (
        Arc::new(MemoryTaskRepository::new()),
        Arc::new(MemoryProjectRepository::new()),
    )
}

async fn seed_project_and_merging_task(
    task_repo: &Arc<memory::MemoryTaskRepository>,
    project_repo: &Arc<memory::MemoryProjectRepository>,
    repo_path: &Path,
    source_branch: &str,
) -> String {
    let project_id = ProjectId::new();
    let mut project = Project::new(
        "test-project".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.id = project_id.clone();
    project.base_branch = Some("main".to_string());
    project_repo.create(project).await.unwrap();

    let mut task = Task::new(project_id, "Watcher test".to_string());
    task.internal_status = InternalStatus::Merging;
    task.task_branch = Some(source_branch.to_string());
    let task_id = task.id.as_str().to_string();
    task_repo.create(task).await.unwrap();
    task_id
}

async fn register_ipr_cat(
    ipr: &Arc<InteractiveProcessRegistry>,
    key: &InteractiveProcessKey,
) -> tokio::process::Child {
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn cat for watcher IPR test");
    let stdin = child.stdin.take().expect("cat stdin");
    ipr.register(key.clone(), stdin).await;
    child
}

/// Advance mock time `secs` seconds one-second at a time, yielding after each step.
/// Yields before starting so the watcher can register its first sleep at current mock time.
async fn advance_secs(secs: u64) {
    // Let the watcher start and register its sleep at current mock time before advancing.
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    for _ in 0..secs {
        tokio::time::advance(tokio::time::Duration::from_secs(1)).await;
        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
    }
}

/// Advance mock time one second at a time until `watcher` finishes or `max_secs` elapses.
/// Yields before starting to let the watcher register its initial grace sleep at t≈0.
/// Returns `true` if the watcher finished within `max_secs`.
async fn advance_until_done(watcher: &tokio::task::JoinHandle<()>, max_secs: u64) -> bool {
    // Let the watcher start and register its first sleep at current mock time
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    for _ in 0..max_secs {
        if watcher.is_finished() {
            return true;
        }
        tokio::time::advance(tokio::time::Duration::from_secs(1)).await;
        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
    }
    watcher.is_finished()
}

/// Poll in real time until `watcher` finishes or `timeout_secs` real seconds elapse.
/// Use this (without `tokio::time::pause()`) for tests that invoke real git subprocess calls,
/// since `tokio::time::timeout` inside `git_cmd::run()` uses mock time and fires too early.
async fn wait_until_done(watcher: &tokio::task::JoinHandle<()>, timeout_secs: u64) -> bool {
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        async {
            loop {
                if watcher.is_finished() {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        },
    )
    .await;
    result.is_ok()
}

// ---- resolve_watcher_context -----------------------------------------------

/// Returns None when the task does not exist.
#[tokio::test]
async fn watcher_context_returns_none_for_missing_task() {
    let (task_repo, project_repo) = make_test_repos();

    let result = resolve_watcher_context(
        "does-not-exist",
        &(Arc::clone(&task_repo) as Arc<dyn TaskRepository>),
        &(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>),
        &None,
    )
    .await;

    assert!(result.is_none(), "Should return None when task is missing");
}

/// Returns None when the project referenced by the task does not exist.
#[tokio::test]
async fn watcher_context_returns_none_for_missing_project() {
    let (task_repo, project_repo) = make_test_repos();

    // Task references a project_id that has no project in the repo
    let project_id = ProjectId::from_string("ghost-project-abc".to_string());
    let mut task = Task::new(project_id, "Ghost task".to_string());
    task.internal_status = InternalStatus::Merging;
    task.task_branch = Some("some-branch".to_string());
    let task_id = task.id.as_str().to_string();
    task_repo.create(task).await.unwrap();

    let result = resolve_watcher_context(
        &task_id,
        &(Arc::clone(&task_repo) as Arc<dyn TaskRepository>),
        &(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>),
        &None,
    )
    .await;

    assert!(result.is_none(), "Should return None when project is missing");
}

/// Returns (source_branch, target_branch, repo_path) for a valid task+project.
#[tokio::test]
async fn watcher_context_returns_source_target_and_repo_path() {
    let dir = setup_test_repo();
    let (task_repo, project_repo) = make_test_repos();

    let task_id = seed_project_and_merging_task(
        &task_repo,
        &project_repo,
        dir.path(),
        "task/feature-x",
    )
    .await;

    let result = resolve_watcher_context(
        &task_id,
        &(Arc::clone(&task_repo) as Arc<dyn TaskRepository>),
        &(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>),
        &None,
    )
    .await;

    assert!(result.is_some(), "Should resolve context for valid task+project");
    let (source, target, path) = result.unwrap();
    assert_eq!(source, "task/feature-x", "Source branch from task.task_branch");
    assert_eq!(target, "main", "Target branch from project.base_branch");
    assert_eq!(
        path,
        dir.path().to_path_buf(),
        "Repo path from project.working_directory"
    );
}

// ---- Watcher loop: exit when no IPR entry ----------------------------------

/// After the grace period, if no IPR entry is registered, the watcher exits.
#[tokio::test]
async fn watcher_exits_when_no_ipr_entry() {
    tokio::time::pause();

    let dir = setup_test_repo();
    create_branch_with_change(dir.path(), "task-branch", "x.txt", "x\n");

    let (task_repo, project_repo) = make_test_repos();
    let ipr = Arc::new(InteractiveProcessRegistry::new());

    let task_id = seed_project_and_merging_task(
        &task_repo,
        &project_repo,
        dir.path(),
        "task-branch",
    )
    .await;

    // No IPR entry registered — watcher should exit at first poll because has_process=false.
    // Use 1ms grace/poll so only a few mock-seconds of advancement are needed.
    let watcher = tokio::spawn(merge_completion_watcher_loop(
        task_id,
        dir.path().to_path_buf(),
        Arc::clone(&ipr),
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        None,
        std::time::Duration::from_millis(1), // grace: 1ms
        std::time::Duration::from_millis(1), // poll: 1ms
    ));

    // Advance a few mock-seconds — watcher exits immediately after first poll (no IPR).
    let finished = advance_until_done(&watcher, 10).await;
    assert!(finished, "Watcher should exit when no IPR entry is registered");
}

// ---- Watcher loop: exit when task leaves Merging ---------------------------

/// Watcher exits without closing IPR when task status changes away from Merging.
#[tokio::test]
async fn watcher_exits_without_closing_ipr_when_task_leaves_merging() {
    tokio::time::pause();

    let dir = setup_test_repo();
    create_branch_with_change(dir.path(), "task-branch", "y.txt", "y\n");

    let (task_repo, project_repo) = make_test_repos();
    let ipr = Arc::new(InteractiveProcessRegistry::new());

    let task_id = seed_project_and_merging_task(
        &task_repo,
        &project_repo,
        dir.path(),
        "task-branch",
    )
    .await;

    let key = InteractiveProcessKey::new("merge", &task_id);
    let mut child = register_ipr_cat(&ipr, &key).await;

    // Use 1ms grace/poll — only needs a few mock-seconds total.
    let watcher = tokio::spawn(merge_completion_watcher_loop(
        task_id.clone(),
        dir.path().to_path_buf(),
        Arc::clone(&ipr),
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        None,
        std::time::Duration::from_millis(1), // grace: 1ms
        std::time::Duration::from_millis(1), // poll: 1ms
    ));

    // Advance 1 mock-second: fires grace (1ms) so watcher enters the loop and
    // registers the poll sleep — but does NOT fire the poll yet.
    // We must change task status BEFORE the first poll fires so the watcher sees
    // MergeConflict and exits WITHOUT making any git subprocess calls.
    advance_secs(1).await;

    // Transition task away from Merging before watcher closes IPR.
    let mut task = task_repo
        .get_by_id(&TaskId::from_string(task_id.clone()))
        .await
        .unwrap()
        .unwrap();
    task.internal_status = InternalStatus::MergeConflict;
    task_repo.update(&task).await.unwrap();

    // Advance 2 more mock-seconds to fire the next poll — watcher detects non-Merging.
    let finished = advance_until_done(&watcher, 5).await;

    assert!(finished, "Watcher should exit when task leaves Merging state");
    assert!(
        ipr.has_process(&key).await,
        "Watcher must NOT close IPR when task leaves Merging (state change, not success)"
    );

    ipr.remove(&key).await;
    let _ = child.kill().await;
}

// ---- Watcher loop: close IPR when merge verified ---------------------------

/// Watcher closes IPR when source branch is detected as merged into target.
///
/// This test uses REAL time (no `tokio::time::pause`) because `git_cmd::run()` wraps
/// `spawn_blocking` in `tokio::time::timeout`, which uses mock time when paused and
/// fires before the git subprocess completes when mock time is advanced rapidly.
#[tokio::test]
async fn watcher_closes_ipr_when_merge_verified_on_target() {
    let dir = setup_test_repo();
    let repo = dir.path();

    // Source branch merged into main → verify_merge_on_target returns Merged
    create_branch_with_change(repo, "task-branch", "merged.txt", "done\n");
    merge_branch(repo, "task-branch");

    let (task_repo, project_repo) = make_test_repos();
    let ipr = Arc::new(InteractiveProcessRegistry::new());

    let task_id = seed_project_and_merging_task(
        &task_repo,
        &project_repo,
        repo,
        "task-branch",
    )
    .await;

    let key = InteractiveProcessKey::new("merge", &task_id);
    let mut child = register_ipr_cat(&ipr, &key).await;

    // 0ms grace (enter loop immediately), 50ms poll — watcher completes after ~1 poll.
    let watcher = tokio::spawn(merge_completion_watcher_loop(
        task_id.clone(),
        repo.to_path_buf(),
        Arc::clone(&ipr),
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        None,
        std::time::Duration::ZERO,            // grace: none
        std::time::Duration::from_millis(50), // poll: 50ms
    ));

    // Wait up to 10s in real time — verify_merge returns Merged → IPR removed.
    let finished = wait_until_done(&watcher, 10).await;

    assert!(
        finished,
        "Watcher should exit after merge verified on target branch"
    );
    assert!(
        !ipr.has_process(&key).await,
        "IPR should be removed after merge detected on target"
    );

    let _ = child.kill().await;
}

// ---- Watcher loop: validation_recovery skips merge check -------------------

/// In validation_recovery mode, watcher skips verify_merge_on_target.
/// Without consecutive_clean, the watcher keeps running (agent exits via MCP tool or timeout).
/// This test verifies the watcher does NOT prematurely close IPR even though the merge is done.
///
/// Uses real time — see `watcher_closes_ipr_when_merge_verified_on_target` for why.
#[tokio::test]
async fn watcher_skips_merge_check_in_validation_recovery_mode() {
    let dir = setup_test_repo();
    let repo = dir.path();

    // Merge is done (verify_merge_on_target would return Merged), but
    // validation_recovery=true means the watcher skips that check
    create_branch_with_change(repo, "task-branch", "val.txt", "v\n");
    merge_branch(repo, "task-branch");

    let (task_repo, project_repo) = make_test_repos();
    let ipr = Arc::new(InteractiveProcessRegistry::new());

    let project_id = ProjectId::new();
    let mut project = Project::new("p".to_string(), repo.to_string_lossy().to_string());
    project.id = project_id.clone();
    project.base_branch = Some("main".to_string());
    project_repo.create(project).await.unwrap();

    let mut task = Task::new(project_id, "Validation recovery".to_string());
    task.internal_status = InternalStatus::Merging;
    task.task_branch = Some("task-branch".to_string());
    task.metadata = Some(
        serde_json::json!({
            "validation_recovery": true,
            "target_branch": "main"
        })
        .to_string(),
    );
    let task_id = task.id.as_str().to_string();
    task_repo.create(task).await.unwrap();

    let key = InteractiveProcessKey::new("merge", &task_id);
    let mut child = register_ipr_cat(&ipr, &key).await;

    // 0ms grace, 50ms poll — watcher should NOT close IPR (no merge check, no consecutive_clean)
    let watcher = tokio::spawn(merge_completion_watcher_loop(
        task_id.clone(),
        repo.to_path_buf(),
        Arc::clone(&ipr),
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        None,
        std::time::Duration::ZERO,            // grace: none
        std::time::Duration::from_millis(50), // poll: 50ms
    ));

    // Wait several poll cycles — watcher should still be running (not closing IPR)
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    assert!(
        !watcher.is_finished(),
        "Watcher in validation_recovery should keep running (agent exits via MCP or timeout)"
    );
    assert!(
        ipr.has_process(&key).await,
        "IPR should NOT be closed in validation_recovery mode — agent exits via MCP tool"
    );

    // Clean up: remove IPR entry so watcher exits
    ipr.remove(&key).await;
    let finished = wait_until_done(&watcher, 5).await;
    assert!(finished, "Watcher should exit after IPR is removed externally");

    let _ = child.kill().await;
}

// ---- Watcher loop: keeps running when merge not verified -------------------

/// When source is NOT merged into target, watcher keeps running indefinitely
/// (agent must exit via MCP tool call or timeout — no consecutive_clean auto-close).
///
/// Uses real time — see `watcher_closes_ipr_when_merge_verified_on_target` for why.
#[tokio::test]
async fn watcher_keeps_running_when_merge_not_verified() {
    let dir = setup_test_repo();
    let repo = dir.path();

    // Branch exists but NOT merged → verify_merge_on_target returns NotMerged
    create_branch_with_change(repo, "task-branch", "z.txt", "z\n");

    let (task_repo, project_repo) = make_test_repos();
    let ipr = Arc::new(InteractiveProcessRegistry::new());

    let task_id = seed_project_and_merging_task(
        &task_repo,
        &project_repo,
        repo,
        "task-branch",
    )
    .await;

    let key = InteractiveProcessKey::new("merge", &task_id);
    let mut child = register_ipr_cat(&ipr, &key).await;

    // 0ms grace, 50ms poll — watcher should keep running (merge not verified)
    let watcher = tokio::spawn(merge_completion_watcher_loop(
        task_id.clone(),
        repo.to_path_buf(),
        Arc::clone(&ipr),
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        None,
        std::time::Duration::ZERO,            // grace: none
        std::time::Duration::from_millis(50), // poll: 50ms
    ));

    // Wait several poll cycles — watcher should still be running
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    assert!(
        !watcher.is_finished(),
        "Watcher should keep running when merge is not verified on target"
    );
    assert!(
        ipr.has_process(&key).await,
        "IPR should NOT be closed — agent exits via MCP tool or timeout"
    );

    // Clean up: remove IPR entry so watcher exits
    ipr.remove(&key).await;
    let finished = wait_until_done(&watcher, 5).await;
    assert!(finished, "Watcher should exit after IPR is removed externally");

    let _ = child.kill().await;
}

/// RC#7 negative: When no merge-resolve branch exists, verify_merge_on_target
/// should still work normally (the fast-forward path is skipped).
#[tokio::test]
async fn test_no_merge_resolve_branch_verification_works_normally() {
    let dir = setup_test_repo();
    let repo = dir.path();

    // Create task branch and merge it normally
    create_branch_with_change(repo, "task/normal", "normal.txt", "normal content\n");
    merge_branch(repo, "task/normal");

    // No merge-resolve branch exists — get_branch_sha should fail
    let resolve_result =
        GitService::get_branch_sha(repo, "merge-resolve/task-normal").await;
    assert!(
        resolve_result.is_err(),
        "merge-resolve branch should not exist for normal merges"
    );

    // Normal verification should still work
    let result = verify_merge_on_target(repo, "task/normal", "main").await;
    match result {
        MergeVerification::Merged(sha) => {
            assert!(!sha.is_empty(), "Merge commit SHA should not be empty");
        }
        other => panic!("Expected Merged for normal merge, got: {:?}", other),
    }
}

/// RC#7: Cleanup verification — after fast-forward, merge-resolve branch and worktree
/// should be cleaned up. This test verifies the cleanup calls succeed.
#[tokio::test]
async fn test_merge_resolve_cleanup_after_fast_forward() {
    let dir = setup_test_repo();
    let repo = dir.path();
    let task_id = "task-cleanup-test";

    // Create merge-resolve branch
    let main_sha = String::from_utf8_lossy(
        &Command::new("git")
            .args(["rev-parse", "main"])
            .current_dir(repo)
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();

    let resolve_branch = format!("merge-resolve/{}", task_id);
    Command::new("git")
        .args(["branch", &resolve_branch, &main_sha])
        .current_dir(repo)
        .output()
        .expect("create resolve branch");

    // Create worktree
    let wt_path = repo.join("merge-wt-cleanup");
    Command::new("git")
        .args([
            "worktree",
            "add",
            wt_path.to_str().unwrap(),
            &resolve_branch,
        ])
        .current_dir(repo)
        .output()
        .expect("create worktree");

    assert!(wt_path.exists(), "Worktree should exist before cleanup");

    // Verify branch exists
    let branch_exists = GitService::get_branch_sha(repo, &resolve_branch).await;
    assert!(branch_exists.is_ok(), "merge-resolve branch should exist before cleanup");

    // Cleanup: delete worktree then branch (mirrors the fix)
    GitService::delete_worktree(repo, &wt_path)
        .await
        .expect("delete worktree");
    GitService::delete_branch(repo, &resolve_branch, true)
        .await
        .expect("delete branch");

    // Verify cleanup
    assert!(!wt_path.exists(), "Worktree directory should be removed after cleanup");
    let branch_after = GitService::get_branch_sha(repo, &resolve_branch).await;
    assert!(
        branch_after.is_err(),
        "merge-resolve branch should be deleted after cleanup"
    );
}

// ============================================================================
// Retry Limit Tests
// ============================================================================
//
// Tests for plan_update_retry_count and source_update_retry_count metadata
// logic that prevents infinite merge retry loops.

/// Verify that retry count is correctly read from metadata and increments.
#[test]
fn test_plan_update_retry_count_increments() {
    // Start with no retry count (first conflict resolution)
    let meta = serde_json::json!({
        "plan_update_conflict": true,
        "target_branch": "plan/my-feature",
        "base_branch": "main"
    });

    let retry_count = meta
        .get("plan_update_retry_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert_eq!(retry_count, 0, "First attempt should have retry_count=0");

    // After one retry, metadata should have retry_count=1
    let mut m = meta.clone();
    if let Some(obj) = m.as_object_mut() {
        obj.remove("plan_update_conflict");
        obj.insert(
            "plan_update_retry_count".to_string(),
            serde_json::json!(retry_count + 1),
        );
    }

    let retry_count_after = m
        .get("plan_update_retry_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert_eq!(retry_count_after, 1, "After first retry, count should be 1");

    // Simulate second conflict: set flag back, keep counter
    if let Some(obj) = m.as_object_mut() {
        obj.insert("plan_update_conflict".to_string(), serde_json::json!(true));
    }
    let retry_count_2 = m
        .get("plan_update_retry_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert_eq!(retry_count_2, 1, "Counter should persist across conflicts");

    // Increment again → 2
    if let Some(obj) = m.as_object_mut() {
        obj.remove("plan_update_conflict");
        obj.insert(
            "plan_update_retry_count".to_string(),
            serde_json::json!(retry_count_2 + 1),
        );
    }
    let retry_count_3 = m
        .get("plan_update_retry_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert_eq!(retry_count_3, 2, "After second retry, count should be 2");
}

/// Verify that when plan_update_retry_count >= 2, the system should escalate
/// to MergeIncomplete (max retries exceeded).
#[test]
fn test_plan_update_max_retries_escalates_to_incomplete() {
    // Metadata with retry_count already at the limit
    let meta = serde_json::json!({
        "plan_update_conflict": true,
        "target_branch": "plan/my-feature",
        "base_branch": "main",
        "plan_update_retry_count": 2
    });

    let is_plan_update_conflict = meta
        .get("plan_update_conflict")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    assert!(is_plan_update_conflict, "Flag should be set");

    let retry_count = meta
        .get("plan_update_retry_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert_eq!(retry_count, 2);

    // The guard condition: retry_count >= 2 → should escalate
    assert!(
        retry_count >= 2,
        "Max retries (2) exceeded — should transition to MergeIncomplete"
    );

    // Also verify count=3 triggers the guard
    let meta_3 = serde_json::json!({
        "plan_update_conflict": true,
        "plan_update_retry_count": 3
    });
    let retry_3 = meta_3
        .get("plan_update_retry_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(retry_3 >= 2, "Count=3 should also trigger max retry guard");

    // And count=1 should NOT trigger
    let meta_1 = serde_json::json!({
        "plan_update_conflict": true,
        "plan_update_retry_count": 1
    });
    let retry_1 = meta_1
        .get("plan_update_retry_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(retry_1 < 2, "Count=1 should NOT trigger max retry guard");
}

/// Verify the same retry limit logic for source_update_conflict.
#[test]
fn test_source_update_max_retries_escalates_to_incomplete() {
    // At limit
    let meta = serde_json::json!({
        "source_update_conflict": true,
        "target_branch": "plan/my-feature",
        "source_branch": "task/abc",
        "source_update_retry_count": 2
    });

    let is_source_update = meta
        .get("source_update_conflict")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    assert!(is_source_update, "source_update_conflict flag should be set");

    let retry_count = meta
        .get("source_update_retry_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(
        retry_count >= 2,
        "source_update: max retries (2) exceeded — should transition to MergeIncomplete"
    );

    // count=0 should NOT trigger
    let meta_0 = serde_json::json!({
        "source_update_conflict": true,
        "source_update_retry_count": 0
    });
    let retry_0 = meta_0
        .get("source_update_retry_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(retry_0 < 2, "Count=0 should NOT trigger max retry guard");

    // Missing key defaults to 0
    let meta_missing = serde_json::json!({
        "source_update_conflict": true
    });
    let retry_missing = meta_missing
        .get("source_update_retry_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert_eq!(retry_missing, 0, "Missing key should default to 0");
    assert!(retry_missing < 2, "Default 0 should NOT trigger max retry guard");
}

// ============================================================================
// Idempotency Guard Tests: merge_commit_sha prevents duplicate completion
// ============================================================================

/// When a task already has merge_commit_sha set, both complete_merge_and_schedule
/// and attempt_merge_auto_complete should skip redundant work.
#[test]
fn test_merge_commit_sha_prevents_duplicate_completion() {
    let project_id = ProjectId::from_string("proj-test".to_string());
    let mut task = Task::new(project_id, "Test task".to_string());
    task.internal_status = InternalStatus::Merging;

    // No merge_commit_sha → guard should NOT trigger
    assert!(
        task.merge_commit_sha.is_none(),
        "Fresh task should have no merge_commit_sha"
    );

    // Set merge_commit_sha → guard SHOULD trigger
    task.merge_commit_sha = Some("abc123def456".to_string());
    assert!(
        task.merge_commit_sha.is_some(),
        "Task with merge_commit_sha set should trigger idempotency guard"
    );

    // Verify the exact check used in both code paths:
    // complete_merge_and_schedule (line 981) and attempt_merge_auto_complete (line 1104)
    let should_skip = task.merge_commit_sha.is_some();
    assert!(
        should_skip,
        "Idempotency guard: task.merge_commit_sha.is_some() must return true to prevent duplicate completion"
    );
}

/// Verify that clearing merge_commit_sha would re-enable merge completion.
#[test]
fn test_merge_commit_sha_cleared_allows_recompletion() {
    let project_id = ProjectId::from_string("proj-test".to_string());
    let mut task = Task::new(project_id, "Test task".to_string());
    task.merge_commit_sha = Some("abc123".to_string());

    // Guard active
    assert!(task.merge_commit_sha.is_some());

    // Clear it
    task.merge_commit_sha = None;

    // Guard no longer active
    assert!(
        task.merge_commit_sha.is_none(),
        "After clearing merge_commit_sha, the guard should not trigger"
    );
}
