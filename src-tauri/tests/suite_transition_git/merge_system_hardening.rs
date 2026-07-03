//! Merge System Hardening Tests — Red-Green TDD
//!
//! Integration tests covering critical merge operations using real git repositories.
//! GAP tests document gaps/risks; Fix specs verify correct behavior.
//!
//! Reference plan: ~/.claude/plans/reflective-wiggling-crystal.md

use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use ralphx_lib::application::git_service::checkout_free::{
    try_merge_checkout_free, try_squash_merge_checkout_free, CheckoutFreeMergeResult,
};
use ralphx_lib::application::{GitService, MergeAttemptResult};
use ralphx_lib::domain::entities::{
    ArtifactId, IdeationSessionId, InternalStatus, PlanBranch, Project, ProjectId, Task,
    TaskCategory, TaskId,
};
use ralphx_lib::domain::repositories::{PlanBranchRepository, TaskRepository};
use ralphx_lib::domain::state_machine::resolve_merge_branches;
use ralphx_lib::domain::state_machine::transition_handler::complete_merge_internal;
use ralphx_lib::infrastructure::memory::{MemoryPlanBranchRepository, MemoryTaskRepository};

// ============================================================================
// Shared Helpers
// ============================================================================

/// Initialize a git repository with an initial commit on main
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

/// Create a branch with a file change and return to main
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

/// Create a 3-level branch hierarchy: main → plan/feature → task branch
fn create_plan_branch_hierarchy(repo: &Path) {
    // Create plan branch from main
    create_branch_with_change(repo, "plan/feature", "plan.txt", "Plan feature content\n");

    // Create task branch from plan/feature
    Command::new("git")
        .args(["checkout", "plan/feature"])
        .current_dir(repo)
        .output()
        .expect("git checkout plan/feature failed");

    create_branch_with_change(repo, "task-branch", "task.txt", "Task content\n");
}

/// Create a test project with a working directory pointing at a repo
fn create_test_project(name: &str, repo_path: &Path) -> Project {
    let mut project = Project::new(name.to_string(), repo_path.to_str().unwrap().to_string());
    project.base_branch = Some("main".to_string());
    project
}

/// Create a task in PendingMerge status with a task_branch
fn create_pending_merge_task(project: &Project, branch: &str) -> Task {
    let mut task = Task::new(project.id.clone(), "Test task".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some(branch.to_string());
    task
}

/// Setup a MemoryPlanBranchRepository with an active plan branch
async fn setup_plan_branch_repo(
    session_id: IdeationSessionId,
    project_id: ProjectId,
    branch_name: &str,
    merge_task_id: Option<TaskId>,
) -> Arc<dyn PlanBranchRepository> {
    let repo = MemoryPlanBranchRepository::new();
    let plan_branch = PlanBranch::new(
        ArtifactId::from_string("test-artifact".to_string()),
        session_id,
        project_id,
        branch_name.to_string(),
        "main".to_string(),
    );

    let mut plan_branch = plan_branch;
    if let Some(task_id) = merge_task_id {
        plan_branch.merge_task_id = Some(task_id);
    }

    repo.create(plan_branch).await.unwrap();

    Arc::new(repo)
}

/// Perform a real git merge (checkout + merge + commit) and return the merge commit SHA
fn merge_branch_via_checkout(repo: &Path, source: &str, target: &str) -> String {
    // Checkout target branch
    Command::new("git")
        .args(["checkout", target])
        .current_dir(repo)
        .output()
        .expect("git checkout failed");

    // Merge source into target
    Command::new("git")
        .args(["merge", source, "--no-edit"])
        .current_dir(repo)
        .output()
        .expect("git merge failed");

    // Get the merge commit SHA
    let sha_output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .expect("git rev-parse failed");

    String::from_utf8_lossy(&sha_output.stdout)
        .trim()
        .to_string()
}

// ============================================================================
// Group 1: Plan Branch Resolution (root cause) — 5 tests
// ============================================================================

#[tokio::test]
async fn gap_task_with_session_resolves_to_main_when_plan_repo_missing() {
    // GAP: Task with ideation_session_id falls back to (task_branch, "main")
    // when plan_branch_repo=None
    let project = create_test_project("test-project", Path::new("/tmp/test"));
    let mut task = create_pending_merge_task(&project, "ralphx/test/task-123");
    task.ideation_session_id = Some(IdeationSessionId::from_string("session-123".to_string()));

    let (source, target) = resolve_merge_branches(&task, &project, &None).await;

    assert_eq!(source, "ralphx/test/task-123");
    assert_eq!(target, "main");
}

#[tokio::test]
async fn gap_plan_merge_task_gets_wrong_source_without_plan_repo() {
    // GAP: plan_merge task uses task_branch as source (wrong), not plan_branch_name
    let project = create_test_project("test-project", Path::new("/tmp/test"));
    let mut task = create_pending_merge_task(&project, "ralphx/test/task-456");
    task.category = TaskCategory::PlanMerge;

    let (source, target) = resolve_merge_branches(&task, &project, &None).await;

    assert_eq!(source, "ralphx/test/task-456");
    assert_eq!(target, "main");
}

#[tokio::test]
async fn fix_task_with_session_resolves_to_plan_branch() {
    // FIX: With active plan branch in memory repo → resolves to (task_branch, "plan/feature")
    let project = create_test_project("test-project", Path::new("/tmp/test"));
    let session_id = IdeationSessionId::from_string("session-789".to_string());
    let mut task = create_pending_merge_task(&project, "ralphx/test/task-789");
    task.ideation_session_id = Some(session_id.clone());

    let plan_repo =
        setup_plan_branch_repo(session_id, project.id.clone(), "plan/feature", None).await;

    let (source, target) = resolve_merge_branches(&task, &project, &Some(plan_repo)).await;

    assert_eq!(source, "ralphx/test/task-789");
    assert_eq!(target, "plan/feature");
}

#[tokio::test]
async fn fix_plan_merge_task_resolves_plan_branch_to_main() {
    // FIX: merge_task_id matches → resolves to (plan_branch, "main")
    let project = create_test_project("test-project", Path::new("/tmp/test"));
    let session_id = IdeationSessionId::from_string("session-merge".to_string());
    let task_id = TaskId::new();
    let mut task = create_pending_merge_task(&project, "ralphx/test/task-merge");
    task.id = task_id.clone();
    task.category = TaskCategory::PlanMerge;
    task.ideation_session_id = Some(session_id.clone());

    let plan_repo = setup_plan_branch_repo(
        session_id,
        project.id.clone(),
        "plan/awesome-feature",
        Some(task_id),
    )
    .await;

    let (source, target) = resolve_merge_branches(&task, &project, &Some(plan_repo)).await;

    assert_eq!(source, "plan/awesome-feature");
    assert_eq!(target, "main");
}

#[tokio::test]
async fn fix_regular_task_resolves_to_base_branch() {
    // FIX: No session → (task_branch, "main") regardless of plan_branch_repo
    let project = create_test_project("test-project", Path::new("/tmp/test"));
    let task = create_pending_merge_task(&project, "ralphx/test/task-regular");

    let session_id = IdeationSessionId::from_string("session-unrelated".to_string());
    let plan_repo =
        setup_plan_branch_repo(session_id, project.id.clone(), "plan/other-feature", None).await;

    let (source, target) = resolve_merge_branches(&task, &project, &Some(plan_repo)).await;

    assert_eq!(source, "ralphx/test/task-regular");
    assert_eq!(target, "main");
}

// ============================================================================
// Group 2: Commit Verification Gate (the 68b731fe fix) — 4 tests
// ============================================================================

#[tokio::test]
async fn gap_complete_merge_rejects_commit_not_on_target() {
    // GAP: Commit on task branch but NOT on main → returns Err(Validation)
    let temp_dir = setup_test_repo();
    let repo = temp_dir.path();

    // Create task branch with a commit
    create_branch_with_change(repo, "task-branch", "task.txt", "Task content\n");

    // Get the commit SHA from task branch (NOT merged to main yet)
    Command::new("git")
        .args(["checkout", "task-branch"])
        .current_dir(repo)
        .output()
        .expect("git checkout failed");

    let sha_output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .expect("git rev-parse failed");
    let task_sha = String::from_utf8_lossy(&sha_output.stdout)
        .trim()
        .to_string();

    // Go back to main
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(repo)
        .output()
        .expect("git checkout failed");

    // Create project and task
    let project = create_test_project("test-project", repo);
    let mut task = create_pending_merge_task(&project, "task-branch");
    let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());

    // Try to complete merge with a SHA that's NOT on main
    let result = complete_merge_internal::<tauri::test::MockRuntime>(
        &mut task, &project, &task_sha, "", "main", &task_repo, None, None, None, None,
    )
    .await;

    // Should reject with Validation error
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, ralphx_lib::error::AppError::Validation(_)),
        "Expected Validation error, got: {:?}",
        err
    );
}

#[tokio::test]
async fn fix_complete_merge_accepts_commit_on_target() {
    // FIX: After real merge, commit IS on main → Ok(()), task → Merged
    let temp_dir = setup_test_repo();
    let repo = temp_dir.path();

    // Create task branch with a commit
    create_branch_with_change(repo, "task-branch", "task.txt", "Task content\n");

    // Merge task-branch into main (real merge)
    let merge_sha = merge_branch_via_checkout(repo, "task-branch", "main");

    // Create project and task
    let project = create_test_project("test-project", repo);
    let mut task = create_pending_merge_task(&project, "task-branch");
    let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());

    // Complete merge with the SHA that IS on main
    let result = complete_merge_internal::<tauri::test::MockRuntime>(
        &mut task, &project, &merge_sha, "", "main", &task_repo, None, None, None, None,
    )
    .await;

    // Should succeed
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    assert_eq!(task.internal_status, InternalStatus::Merged);
    assert_eq!(task.merge_commit_sha.as_deref(), Some(merge_sha.as_str()));
}

#[tokio::test]
async fn fix_complete_merge_rejects_sha_on_wrong_branch() {
    // FIX: Commit merged to plan/feature but target="main" → Err
    let temp_dir = setup_test_repo();
    let repo = temp_dir.path();

    // Create 3-level hierarchy: main → plan/feature → task-branch
    create_plan_branch_hierarchy(repo);

    // Merge task-branch into plan/feature (NOT main)
    let merge_sha = merge_branch_via_checkout(repo, "task-branch", "plan/feature");

    // Create project and task
    let project = create_test_project("test-project", repo);
    let mut task = create_pending_merge_task(&project, "task-branch");
    let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());

    // Try to complete merge with target="main" (wrong - SHA is on plan/feature, not main)
    let result = complete_merge_internal::<tauri::test::MockRuntime>(
        &mut task, &project, &merge_sha, "", "main", &task_repo, None, None, None, None,
    )
    .await;

    // Should reject with Validation error
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, ralphx_lib::error::AppError::Validation(_)),
        "Expected Validation error, got: {:?}",
        err
    );
}

#[tokio::test]
async fn fix_complete_merge_accepts_plan_branch_target() {
    // FIX: Same commit, target="plan/feature" → Ok(())
    let temp_dir = setup_test_repo();
    let repo = temp_dir.path();

    // Create 3-level hierarchy: main → plan/feature → task-branch
    create_plan_branch_hierarchy(repo);

    // Merge task-branch into plan/feature
    let merge_sha = merge_branch_via_checkout(repo, "task-branch", "plan/feature");

    // Create project and task
    let project = create_test_project("test-project", repo);
    let mut task = create_pending_merge_task(&project, "task-branch");
    let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());

    // Complete merge with target="plan/feature" (correct)
    let result = complete_merge_internal::<tauri::test::MockRuntime>(
        &mut task,
        &project,
        &merge_sha,
        "",
        "plan/feature",
        &task_repo,
        None,
        None,
        None,
        None,
    )
    .await;

    // Should succeed
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    assert_eq!(task.internal_status, InternalStatus::Merged);
    assert_eq!(task.merge_commit_sha.as_deref(), Some(merge_sha.as_str()));
}

// ============================================================================
// Group 3: "Already Merged" False Positive — 2 tests
// ============================================================================

#[tokio::test]
async fn gap_already_merged_check_passes_on_wrong_target() {
    // GAP: Task SHA accidentally on main → is_commit_on_branch returns true
    let temp_dir = setup_test_repo();
    let repo = temp_dir.path();

    // Create task branch with a commit
    create_branch_with_change(repo, "task-branch", "task.txt", "Task content\n");

    // Get the task branch SHA
    Command::new("git")
        .args(["checkout", "task-branch"])
        .current_dir(repo)
        .output()
        .expect("git checkout failed");

    let sha_output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .expect("git rev-parse failed");
    let task_sha = String::from_utf8_lossy(&sha_output.stdout)
        .trim()
        .to_string();

    // Merge task to main (accidental - should have gone to plan branch)
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(repo)
        .output()
        .expect("git checkout failed");

    Command::new("git")
        .args(["merge", "task-branch", "--no-edit"])
        .current_dir(repo)
        .output()
        .expect("git merge failed");

    // GAP: is_commit_on_branch returns true for the task SHA on main
    // This is the false positive - it checks if SHA is on main, but doesn't
    // verify it was SUPPOSED to go to main
    let is_on_main = GitService::is_commit_on_branch(repo, &task_sha, "main")
        .await
        .expect("is_commit_on_branch failed");

    assert!(
        is_on_main,
        "GAP: task SHA is on main (false positive - should have gone to plan branch)"
    );
}

#[tokio::test]
async fn fix_correct_target_distinguishes_plan_from_main() {
    // FIX: SHA on plan/feature, not on main → correct target returns true, wrong returns false
    let temp_dir = setup_test_repo();
    let repo = temp_dir.path();

    // Create 3-level hierarchy: main → plan/feature → task-branch
    create_plan_branch_hierarchy(repo);

    // Get the task branch SHA
    Command::new("git")
        .args(["checkout", "task-branch"])
        .current_dir(repo)
        .output()
        .expect("git checkout failed");

    let sha_output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .expect("git rev-parse failed");
    let task_sha = String::from_utf8_lossy(&sha_output.stdout)
        .trim()
        .to_string();

    // Merge task to plan/feature (correct)
    Command::new("git")
        .args(["checkout", "plan/feature"])
        .current_dir(repo)
        .output()
        .expect("git checkout failed");

    Command::new("git")
        .args(["merge", "task-branch", "--no-edit"])
        .current_dir(repo)
        .output()
        .expect("git merge failed");

    // FIX: Check correct target (plan/feature) - should return true
    let is_on_plan = GitService::is_commit_on_branch(repo, &task_sha, "plan/feature")
        .await
        .expect("is_commit_on_branch failed");
    assert!(
        is_on_plan,
        "FIX: task SHA should be on plan/feature (correct target)"
    );

    // FIX: Check wrong target (main) - should return false
    let is_on_main = GitService::is_commit_on_branch(repo, &task_sha, "main")
        .await
        .expect("is_commit_on_branch failed");
    assert!(
        !is_on_main,
        "FIX: task SHA should NOT be on main (wrong target)"
    );
}

// ============================================================================
// Group 4: Full Pipeline End-to-End — 4 tests
// ============================================================================

#[tokio::test]
async fn fix_full_pipeline_task_to_plan_branch() {
    // FIX: Task merges to plan/feature via checkout-free
    // Verify: commit on plan/feature, NOT on main, task → Merged
    let temp_dir = setup_test_repo();
    let repo = temp_dir.path();

    // Create plan branch hierarchy: main → plan/feature → task-branch
    create_plan_branch_hierarchy(repo);

    // Create project and task with session ID
    let project = create_test_project("test-project", repo);
    let session_id = IdeationSessionId::from_string("session-plan".to_string());
    let mut task = create_pending_merge_task(&project, "task-branch");
    task.ideation_session_id = Some(session_id.clone());

    // Setup plan branch repo
    let plan_repo =
        setup_plan_branch_repo(session_id, project.id.clone(), "plan/feature", None).await;

    // Resolve merge branches - should return (task-branch, plan/feature)
    let (source, target) = resolve_merge_branches(&task, &project, &Some(plan_repo.clone())).await;
    assert_eq!(source, "task-branch");
    assert_eq!(target, "plan/feature");

    // Perform checkout-free merge
    let merge_result = try_merge_checkout_free(repo, &source, &target)
        .await
        .expect("merge failed");
    let commit_sha = match merge_result {
        CheckoutFreeMergeResult::Success { commit_sha } => commit_sha,
        CheckoutFreeMergeResult::Conflict { .. } => panic!("Unexpected conflict"),
    };

    // Complete merge internal - should succeed
    let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());
    let result = complete_merge_internal::<tauri::test::MockRuntime>(
        &mut task,
        &project,
        &commit_sha,
        &source,
        &target,
        &task_repo,
        None,
        None,
        None,
        None,
    )
    .await;

    assert!(
        result.is_ok(),
        "complete_merge_internal failed: {:?}",
        result
    );
    assert_eq!(task.internal_status, InternalStatus::Merged);
    assert_eq!(task.merge_commit_sha.as_deref(), Some(commit_sha.as_str()));

    // Verify: commit is on plan/feature
    let is_on_plan = GitService::is_commit_on_branch(repo, &commit_sha, "plan/feature")
        .await
        .expect("is_commit_on_branch failed");
    assert!(is_on_plan, "Commit should be on plan/feature");

    // Verify: commit is NOT on main
    let is_on_main = GitService::is_commit_on_branch(repo, &commit_sha, "main")
        .await
        .expect("is_commit_on_branch failed");
    assert!(!is_on_main, "Commit should NOT be on main");
}

#[tokio::test]
async fn fix_full_pipeline_task_to_main_direct() {
    // FIX: Regular task merges to main via rebase+merge
    // Verify: commit on main, task → Merged
    let temp_dir = setup_test_repo();
    let repo = temp_dir.path();

    // Create task branch
    create_branch_with_change(repo, "task-branch", "task.txt", "Task content\n");

    // Create project and task (no session ID = regular task)
    let project = create_test_project("test-project", repo);
    let mut task = create_pending_merge_task(&project, "task-branch");

    // Resolve merge branches - should return (task-branch, main)
    let (source, target) = resolve_merge_branches(&task, &project, &None).await;
    assert_eq!(source, "task-branch");
    assert_eq!(target, "main");

    // Detach HEAD so 'main' is available for worktree checkout
    // (git worktree add refuses branches already checked out in any worktree)
    Command::new("git")
        .args(["checkout", "--detach", "HEAD"])
        .current_dir(repo)
        .output()
        .expect("git checkout --detach failed");

    // Perform rebase+merge (using worktree-based method)
    let rebase_wt = tempfile::tempdir().expect("Failed to create rebase worktree dir");
    let merge_wt = tempfile::tempdir().expect("Failed to create merge worktree dir");
    let merge_result = GitService::try_rebase_and_merge_in_worktree(
        repo,
        &source,
        &target,
        rebase_wt.path(),
        merge_wt.path(),
    )
    .await
    .expect("rebase+merge failed");
    let commit_sha = match merge_result {
        MergeAttemptResult::Success { commit_sha } => commit_sha,
        _ => panic!("Expected Success, got: {:?}", merge_result),
    };

    // Complete merge internal - should succeed
    let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());
    let result = complete_merge_internal::<tauri::test::MockRuntime>(
        &mut task,
        &project,
        &commit_sha,
        &source,
        &target,
        &task_repo,
        None,
        None,
        None,
        None,
    )
    .await;

    assert!(
        result.is_ok(),
        "complete_merge_internal failed: {:?}",
        result
    );
    assert_eq!(task.internal_status, InternalStatus::Merged);
    assert_eq!(task.merge_commit_sha.as_deref(), Some(commit_sha.as_str()));

    // Verify: commit is on main
    let is_on_main = GitService::is_commit_on_branch(repo, &commit_sha, "main")
        .await
        .expect("is_commit_on_branch failed");
    assert!(is_on_main, "Commit should be on main");
}

#[tokio::test]
async fn fix_full_pipeline_plan_merge_to_main() {
    // FIX: Plan merge task: plan/feature → main via checkout-free
    // Verify both branches updated
    let temp_dir = setup_test_repo();
    let repo = temp_dir.path();

    // Create plan branch with a commit
    create_branch_with_change(repo, "plan/feature", "feature.txt", "Feature content\n");

    // Create project and plan merge task
    let project = create_test_project("test-project", repo);
    let session_id = IdeationSessionId::from_string("session-plan-merge".to_string());
    let task_id = TaskId::new();
    let mut task = create_pending_merge_task(&project, "plan/feature");
    task.id = task_id.clone();
    task.category = TaskCategory::PlanMerge;
    task.ideation_session_id = Some(session_id.clone());

    // Setup plan branch repo with merge task ID
    let plan_repo = setup_plan_branch_repo(
        session_id,
        project.id.clone(),
        "plan/feature",
        Some(task_id),
    )
    .await;

    // Resolve merge branches - should return (plan/feature, main)
    let (source, target) = resolve_merge_branches(&task, &project, &Some(plan_repo.clone())).await;
    assert_eq!(source, "plan/feature");
    assert_eq!(target, "main");

    // Perform checkout-free merge
    let merge_result = try_merge_checkout_free(repo, &source, &target)
        .await
        .expect("merge failed");
    let commit_sha = match merge_result {
        CheckoutFreeMergeResult::Success { commit_sha } => commit_sha,
        CheckoutFreeMergeResult::Conflict { .. } => panic!("Unexpected conflict"),
    };

    // Complete merge internal - should succeed
    let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());
    let result = complete_merge_internal::<tauri::test::MockRuntime>(
        &mut task,
        &project,
        &commit_sha,
        &source,
        &target,
        &task_repo,
        None,
        None,
        None,
        None,
    )
    .await;

    assert!(
        result.is_ok(),
        "complete_merge_internal failed: {:?}",
        result
    );
    assert_eq!(task.internal_status, InternalStatus::Merged);

    // Verify: both branches are updated (plan/feature content is now on main)
    let is_on_main = GitService::is_commit_on_branch(repo, &commit_sha, "main")
        .await
        .expect("is_commit_on_branch failed");
    assert!(is_on_main, "Merge commit should be on main");
}

#[tokio::test]
async fn fix_full_pipeline_squash_merge_to_plan_branch() {
    // FIX: Squash merge task → plan/feature
    // Verify single parent, on plan/feature, not on main
    let temp_dir = setup_test_repo();
    let repo = temp_dir.path();

    // Create plan branch hierarchy: main → plan/feature → task-branch
    create_plan_branch_hierarchy(repo);

    // Create project and task with session ID
    let project = create_test_project("test-project", repo);
    let session_id = IdeationSessionId::from_string("session-squash".to_string());
    let mut task = create_pending_merge_task(&project, "task-branch");
    task.ideation_session_id = Some(session_id.clone());

    // Setup plan branch repo
    let plan_repo =
        setup_plan_branch_repo(session_id, project.id.clone(), "plan/feature", None).await;

    // Resolve merge branches - should return (task-branch, plan/feature)
    let (source, target) = resolve_merge_branches(&task, &project, &Some(plan_repo.clone())).await;
    assert_eq!(source, "task-branch");
    assert_eq!(target, "plan/feature");

    // Perform checkout-free squash merge
    let commit_message = "Squash merge: Add task.txt";
    let merge_result = try_squash_merge_checkout_free(repo, &source, &target, commit_message)
        .await
        .expect("merge failed");
    let commit_sha = match merge_result {
        CheckoutFreeMergeResult::Success { commit_sha } => commit_sha,
        CheckoutFreeMergeResult::Conflict { .. } => panic!("Unexpected conflict"),
    };

    // Complete merge internal - should succeed
    let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());
    let result = complete_merge_internal::<tauri::test::MockRuntime>(
        &mut task,
        &project,
        &commit_sha,
        &source,
        &target,
        &task_repo,
        None,
        None,
        None,
        None,
    )
    .await;

    assert!(
        result.is_ok(),
        "complete_merge_internal failed: {:?}",
        result
    );
    assert_eq!(task.internal_status, InternalStatus::Merged);

    // Verify: commit is on plan/feature
    let is_on_plan = GitService::is_commit_on_branch(repo, &commit_sha, "plan/feature")
        .await
        .expect("is_commit_on_branch failed");
    assert!(is_on_plan, "Commit should be on plan/feature");

    // Verify: commit is NOT on main
    let is_on_main = GitService::is_commit_on_branch(repo, &commit_sha, "main")
        .await
        .expect("is_commit_on_branch failed");
    assert!(!is_on_main, "Commit should NOT be on main");

    // Verify: squash commit has single parent (not a merge commit)
    let parent_output = Command::new("git")
        .args(["rev-list", "--parents", "-n", "1", &commit_sha])
        .current_dir(repo)
        .output()
        .expect("git rev-list failed");
    let parent_line = String::from_utf8_lossy(&parent_output.stdout);
    let parts: Vec<&str> = parent_line.split_whitespace().collect();
    // Format: <commit_sha> <parent1> [<parent2>...]
    // Squash commits should have exactly 1 parent (2 parts total: commit + 1 parent)
    assert_eq!(
        parts.len(),
        2,
        "Squash commit should have exactly 1 parent, got {} parts: {:?}",
        parts.len(),
        parts
    );
}

// ============================================================================
// Group 5: Recovery Paths — 2 tests
// ============================================================================

#[tokio::test]
async fn fix_deleted_source_branch_recovery() {
    // FIX: Source deleted after merge → find_commit_by_message_grep finds commits on target
    let temp_dir = setup_test_repo();
    let repo = temp_dir.path();

    // Create task branch with a commit
    create_branch_with_change(repo, "task-branch", "task.txt", "Task content\n");

    // Merge task-branch into main
    let _merge_sha = merge_branch_via_checkout(repo, "task-branch", "main");

    // Delete the source branch (simulating cleanup after merge)
    Command::new("git")
        .args(["branch", "-D", "task-branch"])
        .current_dir(repo)
        .output()
        .expect("git branch -D failed");

    // Verify branch is deleted
    let branch_exists = Command::new("git")
        .args(["rev-parse", "--verify", "task-branch"])
        .current_dir(repo)
        .output()
        .expect("git rev-parse failed")
        .status
        .success();
    assert!(!branch_exists, "Branch should be deleted");

    // Recovery: find commits by message grep on target branch
    let found_sha = GitService::find_commit_by_message_grep(repo, "Add task.txt", "main")
        .await
        .expect("find_commit_by_message_grep failed");

    assert!(found_sha.is_some(), "Should find commit by message");
    let found_sha = found_sha.unwrap();

    // Verify: the merge commit (or one of its parents) contains the task.txt commit
    let is_on_main = GitService::is_commit_on_branch(repo, &found_sha, "main")
        .await
        .expect("is_commit_on_branch failed");
    assert!(is_on_main, "Found commit should be on main");
}

#[tokio::test]
async fn fix_checkout_free_merge_while_main_checked_out() {
    // FIX: Merge to plan/feature while main is checked out → working tree unchanged, main unchanged
    let temp_dir = setup_test_repo();
    let repo = temp_dir.path();

    // Create plan branch hierarchy: main → plan/feature → task-branch
    create_plan_branch_hierarchy(repo);

    // Ensure we're on main and get initial state
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(repo)
        .output()
        .expect("git checkout main failed");

    let main_sha_before = GitService::get_branch_sha(repo, "main")
        .await
        .expect("get main SHA failed");
    let plan_sha_before = GitService::get_branch_sha(repo, "plan/feature")
        .await
        .expect("get plan SHA failed");

    // Verify working tree state (should only have README.md from initial commit)
    assert!(repo.join("README.md").exists(), "README.md should exist");
    assert!(
        !repo.join("task.txt").exists(),
        "task.txt should NOT exist in working tree (on main)"
    );

    // Perform checkout-free merge: task-branch → plan/feature (while main is checked out)
    let merge_result = try_merge_checkout_free(repo, "task-branch", "plan/feature")
        .await
        .expect("checkout-free merge failed");
    let commit_sha = match merge_result {
        CheckoutFreeMergeResult::Success { commit_sha } => commit_sha,
        CheckoutFreeMergeResult::Conflict { .. } => panic!("Unexpected conflict"),
    };

    // Verify: main branch unchanged
    let main_sha_after = GitService::get_branch_sha(repo, "main")
        .await
        .expect("get main SHA failed");
    assert_eq!(
        main_sha_before, main_sha_after,
        "Main branch should be unchanged"
    );

    // Verify: plan/feature branch advanced
    let plan_sha_after = GitService::get_branch_sha(repo, "plan/feature")
        .await
        .expect("get plan SHA failed");
    assert_ne!(
        plan_sha_before, plan_sha_after,
        "Plan branch should be advanced"
    );
    assert_eq!(
        plan_sha_after, commit_sha,
        "Plan branch should point to merge commit"
    );

    // Verify: working tree unchanged (still on main, task.txt NOT present)
    assert!(
        !repo.join("task.txt").exists(),
        "task.txt should NOT exist in working tree (still on main)"
    );

    // Verify: current branch is still main
    let current_branch = GitService::get_current_branch(repo)
        .await
        .expect("get current branch failed");
    assert_eq!(current_branch, "main", "Should still be on main branch");

    // Verify: commit is on plan/feature
    let is_on_plan = GitService::is_commit_on_branch(repo, &commit_sha, "plan/feature")
        .await
        .expect("is_commit_on_branch failed");
    assert!(is_on_plan, "Commit should be on plan/feature");

    // Verify: commit is NOT on main
    let is_on_main = GitService::is_commit_on_branch(repo, &commit_sha, "main")
        .await
        .expect("is_commit_on_branch failed");
    assert!(!is_on_main, "Commit should NOT be on main");
}

// ============================================================================
// Group 6: Sequential Merges to Plan Branch — 2 tests
// ============================================================================

#[tokio::test]
async fn fix_two_tasks_merge_sequentially_to_plan_branch() {
    // FIX: Task-A then Task-B → plan/feature via checkout-free
    // Both files present, main unchanged
    let temp_dir = setup_test_repo();
    let repo = temp_dir.path();

    // Create plan branch from main
    create_branch_with_change(repo, "plan/feature", "plan.txt", "Plan content\n");

    // Create task-A branch from plan/feature
    Command::new("git")
        .args(["checkout", "plan/feature"])
        .current_dir(repo)
        .output()
        .expect("git checkout plan/feature failed");

    create_branch_with_change(repo, "task-a", "task-a.txt", "Task A content\n");

    // Create task-B branch from plan/feature (independent of task-A)
    Command::new("git")
        .args(["checkout", "plan/feature"])
        .current_dir(repo)
        .output()
        .expect("git checkout plan/feature failed");

    create_branch_with_change(repo, "task-b", "task-b.txt", "Task B content\n");

    // Go back to main
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(repo)
        .output()
        .expect("git checkout main failed");

    let main_sha_before = GitService::get_branch_sha(repo, "main")
        .await
        .expect("get main SHA failed");

    // Merge task-A → plan/feature
    let merge_a_result = try_merge_checkout_free(repo, "task-a", "plan/feature")
        .await
        .expect("merge task-a failed");
    let commit_a_sha = match merge_a_result {
        CheckoutFreeMergeResult::Success { commit_sha } => commit_sha,
        CheckoutFreeMergeResult::Conflict { .. } => panic!("Unexpected conflict in task-a"),
    };

    // Merge task-B → plan/feature
    let merge_b_result = try_merge_checkout_free(repo, "task-b", "plan/feature")
        .await
        .expect("merge task-b failed");
    let commit_b_sha = match merge_b_result {
        CheckoutFreeMergeResult::Success { commit_sha } => commit_sha,
        CheckoutFreeMergeResult::Conflict { .. } => panic!("Unexpected conflict in task-b"),
    };

    // Verify: main unchanged
    let main_sha_after = GitService::get_branch_sha(repo, "main")
        .await
        .expect("get main SHA failed");
    assert_eq!(main_sha_before, main_sha_after, "Main should be unchanged");

    // Verify: both commits are on plan/feature
    let is_a_on_plan = GitService::is_commit_on_branch(repo, &commit_a_sha, "plan/feature")
        .await
        .expect("is_commit_on_branch failed");
    assert!(is_a_on_plan, "Task-A commit should be on plan/feature");

    let is_b_on_plan = GitService::is_commit_on_branch(repo, &commit_b_sha, "plan/feature")
        .await
        .expect("is_commit_on_branch failed");
    assert!(is_b_on_plan, "Task-B commit should be on plan/feature");

    // Verify: neither commit is on main
    let is_a_on_main = GitService::is_commit_on_branch(repo, &commit_a_sha, "main")
        .await
        .expect("is_commit_on_branch failed");
    assert!(!is_a_on_main, "Task-A commit should NOT be on main");

    let is_b_on_main = GitService::is_commit_on_branch(repo, &commit_b_sha, "main")
        .await
        .expect("is_commit_on_branch failed");
    assert!(!is_b_on_main, "Task-B commit should NOT be on main");

    // Verify: both files exist when we checkout plan/feature
    Command::new("git")
        .args(["checkout", "plan/feature"])
        .current_dir(repo)
        .output()
        .expect("git checkout plan/feature failed");

    assert!(
        repo.join("task-a.txt").exists(),
        "task-a.txt should exist on plan/feature"
    );
    assert!(
        repo.join("task-b.txt").exists(),
        "task-b.txt should exist on plan/feature"
    );
}

#[tokio::test]
async fn fix_plan_branch_then_to_main() {
    // FIX: Tasks → plan/feature, then plan/feature → main
    // Full lifecycle. All commits reachable from main
    let temp_dir = setup_test_repo();
    let repo = temp_dir.path();

    // Create plan branch from main
    create_branch_with_change(repo, "plan/feature", "plan.txt", "Plan content\n");

    // Create task branch from plan/feature
    Command::new("git")
        .args(["checkout", "plan/feature"])
        .current_dir(repo)
        .output()
        .expect("git checkout plan/feature failed");

    create_branch_with_change(repo, "task-branch", "task.txt", "Task content\n");

    // Go back to main
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(repo)
        .output()
        .expect("git checkout main failed");

    // Step 1: Merge task → plan/feature
    let merge_task_result = try_merge_checkout_free(repo, "task-branch", "plan/feature")
        .await
        .expect("merge task failed");
    let task_commit_sha = match merge_task_result {
        CheckoutFreeMergeResult::Success { commit_sha } => commit_sha,
        CheckoutFreeMergeResult::Conflict { .. } => panic!("Unexpected conflict"),
    };

    // Verify: task commit on plan/feature, NOT on main
    let is_task_on_plan = GitService::is_commit_on_branch(repo, &task_commit_sha, "plan/feature")
        .await
        .expect("is_commit_on_branch failed");
    assert!(is_task_on_plan, "Task commit should be on plan/feature");

    let is_task_on_main = GitService::is_commit_on_branch(repo, &task_commit_sha, "main")
        .await
        .expect("is_commit_on_branch failed");
    assert!(!is_task_on_main, "Task commit should NOT be on main yet");

    // Step 2: Merge plan/feature → main
    let merge_plan_result = try_merge_checkout_free(repo, "plan/feature", "main")
        .await
        .expect("merge plan failed");
    let plan_commit_sha = match merge_plan_result {
        CheckoutFreeMergeResult::Success { commit_sha } => commit_sha,
        CheckoutFreeMergeResult::Conflict { .. } => panic!("Unexpected conflict"),
    };

    // Verify: plan merge commit is on main
    let is_plan_on_main = GitService::is_commit_on_branch(repo, &plan_commit_sha, "main")
        .await
        .expect("is_commit_on_branch failed");
    assert!(is_plan_on_main, "Plan merge commit should be on main");

    // Verify: task commit is NOW reachable from main (because plan/feature was merged)
    let is_task_on_main_now = GitService::is_commit_on_branch(repo, &task_commit_sha, "main")
        .await
        .expect("is_commit_on_branch failed");
    assert!(
        is_task_on_main_now,
        "Task commit should NOW be reachable from main after plan merge"
    );

    // Sync working tree to main (checkout-free merge doesn't update working tree)
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(repo)
        .output()
        .expect("git checkout main failed");

    // Hard reset to sync working tree with main branch
    GitService::hard_reset_to_head(repo)
        .await
        .expect("hard reset failed");

    // Verify: all files exist on main
    assert!(
        repo.join("plan.txt").exists(),
        "plan.txt should exist on main"
    );
    assert!(
        repo.join("task.txt").exists(),
        "task.txt should exist on main"
    );
}

// ============================================================================
// Group 7: Merge Verification Edge Cases — 3 tests
// ============================================================================

#[tokio::test]
async fn gap_non_merge_commit_with_merge_message() {
    // GAP: Regular (non-merge) commit with "Merge" in message but only 1 parent
    // This was the actual bug scenario (commit ad643ec5)
    let temp_dir = setup_test_repo();
    let repo = temp_dir.path();

    // Create task branch with a commit
    create_branch_with_change(repo, "task-branch", "task.txt", "Task content\n");

    // Get the task branch SHA
    Command::new("git")
        .args(["checkout", "task-branch"])
        .current_dir(repo)
        .output()
        .expect("git checkout failed");

    let sha_output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .expect("git rev-parse failed");
    let task_sha = String::from_utf8_lossy(&sha_output.stdout)
        .trim()
        .to_string();

    // Create a non-merge commit on main with a "Merge" message
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(repo)
        .output()
        .expect("git checkout failed");

    fs::write(repo.join("fake-merge.txt"), "Not a real merge\n").expect("write failed");
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .expect("git add failed");

    Command::new("git")
        .args(["commit", "-m", "Merge plan into main"])
        .current_dir(repo)
        .output()
        .expect("git commit failed");

    let fake_merge_sha = String::from_utf8_lossy(
        &Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo)
            .output()
            .expect("git rev-parse failed")
            .stdout,
    )
    .trim()
    .to_string();

    // Verify: this commit has a "Merge" message but only 1 parent
    let parent_output = Command::new("git")
        .args(["rev-list", "--parents", "-n", "1", &fake_merge_sha])
        .current_dir(repo)
        .output()
        .expect("git rev-list failed");
    let parent_line = String::from_utf8_lossy(&parent_output.stdout);
    let parts: Vec<&str> = parent_line.split_whitespace().collect();

    assert_eq!(
        parts.len(),
        2,
        "Fake merge commit should have exactly 1 parent, got {} parts: {:?}",
        parts.len(),
        parts
    );

    // GAP: is_commit_on_branch correctly identifies it's NOT a merge
    // (it's on the branch, but it's not a merge commit from task-branch)
    let is_fake_on_main = GitService::is_commit_on_branch(repo, &fake_merge_sha, "main")
        .await
        .expect("is_commit_on_branch failed");
    assert!(is_fake_on_main, "Fake merge should be on main");

    // The task SHA should NOT be on main (task-branch was never merged)
    let is_task_on_main = GitService::is_commit_on_branch(repo, &task_sha, "main")
        .await
        .expect("is_commit_on_branch failed");
    assert!(
        !is_task_on_main,
        "GAP: task SHA should NOT be on main (task-branch was never actually merged)"
    );
}

#[tokio::test]
async fn fix_concurrent_merge_attempts_sequential() {
    // FIX: Two tasks trying to merge to the same plan branch
    // Tests sequential merges (since true concurrency is hard in tests)
    let temp_dir = setup_test_repo();
    let repo = temp_dir.path();

    // Create plan branch from main
    create_branch_with_change(repo, "plan/feature", "plan.txt", "Plan content\n");

    // Create first task branch from plan/feature
    Command::new("git")
        .args(["checkout", "plan/feature"])
        .current_dir(repo)
        .output()
        .expect("git checkout failed");

    create_branch_with_change(repo, "task-1", "task1.txt", "Task 1 content\n");

    // Create second task branch from plan/feature (independent)
    Command::new("git")
        .args(["checkout", "plan/feature"])
        .current_dir(repo)
        .output()
        .expect("git checkout failed");

    create_branch_with_change(repo, "task-2", "task2.txt", "Task 2 content\n");

    // Go back to main
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(repo)
        .output()
        .expect("git checkout failed");

    // Create project and setup plan branch repo
    let project = create_test_project("test-project", repo);
    let session_id = IdeationSessionId::from_string("session-concurrent".to_string());
    let _plan_repo =
        setup_plan_branch_repo(session_id.clone(), project.id.clone(), "plan/feature", None).await;

    // Merge first task
    let merge_1_result = try_merge_checkout_free(repo, "task-1", "plan/feature")
        .await
        .expect("merge task-1 failed");
    let commit_1_sha = match merge_1_result {
        CheckoutFreeMergeResult::Success { commit_sha } => commit_sha,
        CheckoutFreeMergeResult::Conflict { .. } => panic!("Unexpected conflict in task-1"),
    };

    // Complete merge for task-1
    let mut task_1 = create_pending_merge_task(&project, "task-1");
    task_1.ideation_session_id = Some(session_id.clone());
    let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());

    let result_1 = complete_merge_internal::<tauri::test::MockRuntime>(
        &mut task_1,
        &project,
        &commit_1_sha,
        "task-1",
        "plan/feature",
        &task_repo,
        None,
        None,
        None,
        None,
    )
    .await;

    assert!(
        result_1.is_ok(),
        "Task-1 merge should succeed: {:?}",
        result_1
    );
    assert_eq!(task_1.internal_status, InternalStatus::Merged);

    // Merge second task (after first task already merged)
    let merge_2_result = try_merge_checkout_free(repo, "task-2", "plan/feature")
        .await
        .expect("merge task-2 failed");
    let commit_2_sha = match merge_2_result {
        CheckoutFreeMergeResult::Success { commit_sha } => commit_sha,
        CheckoutFreeMergeResult::Conflict { .. } => panic!("Unexpected conflict in task-2"),
    };

    // Complete merge for task-2
    let mut task_2 = create_pending_merge_task(&project, "task-2");
    task_2.ideation_session_id = Some(session_id);

    let result_2 = complete_merge_internal::<tauri::test::MockRuntime>(
        &mut task_2,
        &project,
        &commit_2_sha,
        "task-2",
        "plan/feature",
        &task_repo,
        None,
        None,
        None,
        None,
    )
    .await;

    assert!(
        result_2.is_ok(),
        "Task-2 merge should succeed: {:?}",
        result_2
    );
    assert_eq!(task_2.internal_status, InternalStatus::Merged);

    // Verify: both commits are on plan/feature
    let is_1_on_plan = GitService::is_commit_on_branch(repo, &commit_1_sha, "plan/feature")
        .await
        .expect("is_commit_on_branch failed");
    assert!(is_1_on_plan, "Task-1 commit should be on plan/feature");

    let is_2_on_plan = GitService::is_commit_on_branch(repo, &commit_2_sha, "plan/feature")
        .await
        .expect("is_commit_on_branch failed");
    assert!(is_2_on_plan, "Task-2 commit should be on plan/feature");

    // Verify: both commits are NOT on main
    let is_1_on_main = GitService::is_commit_on_branch(repo, &commit_1_sha, "main")
        .await
        .expect("is_commit_on_branch failed");
    assert!(!is_1_on_main, "Task-1 commit should NOT be on main");

    let is_2_on_main = GitService::is_commit_on_branch(repo, &commit_2_sha, "main")
        .await
        .expect("is_commit_on_branch failed");
    assert!(!is_2_on_main, "Task-2 commit should NOT be on main");
}

#[tokio::test]
async fn gap_plan_merge_verification_failure() {
    // GAP: Plan branch → main merge attempt with wrong SHA
    // SHA is on plan branch but target is "main" (plan hasn't been merged yet)
    let temp_dir = setup_test_repo();
    let repo = temp_dir.path();

    // Create plan branch with commits
    create_branch_with_change(repo, "plan/feature", "plan.txt", "Plan content\n");

    // Get the plan branch SHA
    Command::new("git")
        .args(["checkout", "plan/feature"])
        .current_dir(repo)
        .output()
        .expect("git checkout failed");

    let plan_sha = String::from_utf8_lossy(
        &Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo)
            .output()
            .expect("git rev-parse failed")
            .stdout,
    )
    .trim()
    .to_string();

    // Go back to main
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(repo)
        .output()
        .expect("git checkout failed");

    // Create project and plan merge task
    let project = create_test_project("test-project", repo);
    let session_id = IdeationSessionId::from_string("session-plan-gap".to_string());
    let task_id = TaskId::new();
    let mut task = create_pending_merge_task(&project, "plan/feature");
    task.id = task_id.clone();
    task.category = TaskCategory::PlanMerge;
    task.ideation_session_id = Some(session_id.clone());

    let _plan_repo = setup_plan_branch_repo(
        session_id,
        project.id.clone(),
        "plan/feature",
        Some(task_id),
    )
    .await;

    // Verify plan SHA is on plan/feature but NOT on main
    let is_on_plan = GitService::is_commit_on_branch(repo, &plan_sha, "plan/feature")
        .await
        .expect("is_commit_on_branch failed");
    assert!(is_on_plan, "Plan SHA should be on plan/feature");

    let is_on_main = GitService::is_commit_on_branch(repo, &plan_sha, "main")
        .await
        .expect("is_commit_on_branch failed");
    assert!(!is_on_main, "Plan SHA should NOT be on main yet");

    // Attempt complete_merge_internal with a SHA on plan branch but target="main"
    // (without actually merging plan to main first)
    let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());
    let result = complete_merge_internal::<tauri::test::MockRuntime>(
        &mut task, &project, &plan_sha, "", "main", &task_repo, None, None, None, None,
    )
    .await;

    // Should reject with Validation error
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, ralphx_lib::error::AppError::Validation(_)),
        "Expected Validation error, got: {:?}",
        err
    );
}
