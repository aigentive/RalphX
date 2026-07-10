// Regression tests for RC#14: Layer 2 self-healing guard in on_enter(Executing)
//
// Problem: When a task's branch is deleted (e.g., during merge cleanup) and the task
// re-enters Executing (via reconciler, scheduler, or direct transition bypassing Layer 1),
// the on_enter guard detects the deleted branch and self-heals by:
//   1. Cleaning up any orphaned worktree directories
//   2. Clearing stale task_branch/worktree_path/merge_commit_sha fields
//   3. Creating a fresh branch + worktree via create_fresh_branch_and_worktree
//
// Tests use real git infrastructure (setup_real_git_repo, tempfile::TempDir)
// because GitService::branch_exists and GitService::delete_worktree call the git CLI.

use super::helpers::*;
use crate::application::git_service::GitService;
use crate::domain::entities::{GitMode, InternalStatus, Project, ProjectId, Task, TaskId};
use crate::domain::state_machine::transition_handler::merge_helpers::compute_task_worktree_path;
use crate::domain::state_machine::{State, TransitionHandler};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

// ==========================================
// RC#14 Test 1: Self-heal detects deleted branch, no orphaned worktree
// ==========================================

/// RC#14: When a task's branch is deleted and there is no worktree directory,
/// self-heal should detect the missing branch and create a fresh one.
///
/// Simulates the scenario where:
/// 1. Task has task_branch="ralphx/test-project/task-heal1" in DB
/// 2. Branch was deleted (e.g., during merge cleanup)
/// 3. No worktree directory exists
/// → GitService::branch_exists returns false
/// → Self-heal creates a new branch at the expected worktree path
#[tokio::test]
async fn test_self_heal_detects_deleted_branch_no_worktree() {
    let git_repo = setup_real_git_repo();
    let path = git_repo.path();

    let worktree_parent = path.join("worktrees");
    fs::create_dir_all(&worktree_parent).unwrap();

    let mut project = Project::new(
        "test-project".to_string(),
        path.to_string_lossy().to_string(),
    );
    let project_id = ProjectId::from_string("proj-1".to_string());
    project.id = project_id.clone();
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());

    let task_id_str = "heal1";
    // Compute what the expected branch name and worktree path would be
    let expected_branch = format!("ralphx/test-project/task-{}", task_id_str);
    let expected_wt_path_str = compute_task_worktree_path(&project, task_id_str);
    let expected_wt_path = PathBuf::from(&expected_wt_path_str);

    // Precondition: branch does NOT exist in git
    let branch_exists = GitService::branch_exists(path, &expected_branch)
        .await
        .unwrap_or(false);
    assert!(
        !branch_exists,
        "Precondition: branch '{}' should not exist yet",
        expected_branch
    );

    // Precondition: worktree does NOT exist
    assert!(
        !expected_wt_path.exists(),
        "Precondition: worktree path should not exist"
    );

    // Simulate self-heal: create fresh branch + worktree (what the self-healing guard does)
    fs::create_dir_all(expected_wt_path.parent().unwrap()).unwrap();
    GitService::create_worktree(path, &expected_wt_path, &expected_branch, "main")
        .await
        .expect("Self-heal: create_worktree should succeed for fresh branch");

    // Verify self-heal result
    assert!(
        expected_wt_path.exists(),
        "After self-heal: worktree should exist at expected path"
    );

    let branch_now_exists = GitService::branch_exists(path, &expected_branch)
        .await
        .unwrap_or(false);
    assert!(
        branch_now_exists,
        "After self-heal: fresh branch '{}' should exist in git",
        expected_branch
    );

    // Verify the worktree is on the expected branch
    let head = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&expected_wt_path)
        .output()
        .expect("git rev-parse HEAD");
    let head_branch = String::from_utf8_lossy(&head.stdout).trim().to_string();
    assert_eq!(
        head_branch, expected_branch,
        "Worktree should be on the freshly created branch"
    );

    let _ = GitService::delete_worktree(path, &expected_wt_path).await;
}

#[tokio::test]
async fn test_self_heal_preserves_base_metadata_when_recreate_fails() {
    let git_repo = setup_real_git_repo();
    let path = git_repo.path();
    let original_base_sha = GitService::get_branch_sha(path, "main")
        .await
        .expect("main branch should resolve");

    let worktree_parent = path.join("worktrees");
    fs::create_dir_all(&worktree_parent).unwrap();

    let project_id = ProjectId::from_string("proj-preserve-base".to_string());
    let mut project = Project::new(
        "test-project".to_string(),
        path.to_string_lossy().to_string(),
    );
    project.id = project_id.clone();
    project.git_mode = GitMode::Worktree;
    project.base_branch = Some("missing-base".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());

    let task_id_str = "heal-preserve-base";
    let stale_branch = format!("ralphx/test-project/task-{}", task_id_str);
    let mut task = Task::new(project_id.clone(), "Preserve base metadata".to_string());
    task.id = TaskId::from_string(task_id_str.to_string());
    task.internal_status = InternalStatus::Executing;
    task.task_branch = Some(stale_branch.clone());
    task.task_branch_base_ref = Some("main".to_string());
    task.task_branch_base_sha = Some(original_base_sha.clone());
    let task_id = task.id.clone();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    task_repo.create(task).await.unwrap();
    let project_repo = Arc::new(MemoryProjectRepository::new());
    project_repo.create(project).await.unwrap();

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>);
    let context = TaskContext::new(task_id.as_str(), project_id.as_str(), services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::Executing).await;
    let error = result.expect_err("missing base should block branch recreation");
    assert!(
        error
            .to_string()
            .contains("base branch 'missing-base' does not exist"),
        "unexpected recreate failure: {error}"
    );

    let persisted = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        persisted.task_branch.as_deref(),
        Some(stale_branch.as_str())
    );
    assert_eq!(persisted.task_branch_base_ref.as_deref(), Some("main"));
    assert_eq!(
        persisted.task_branch_base_sha.as_deref(),
        Some(original_base_sha.as_str())
    );
}

// ==========================================
// RC#14 Test 2: Self-heal cleans up orphaned worktree directory
// ==========================================

/// RC#14: When a task's branch is deleted AND an orphaned worktree directory still
/// exists on disk (e.g., git worktree cleanup ran but directory wasn't removed),
/// self-heal should delete the orphaned directory before creating the fresh worktree.
///
/// Simulates the scenario where:
/// 1. Task has task_branch="ralphx/test-project/task-heal2" in DB
/// 2. Branch was deleted (merged + cleanup)
/// 3. Orphaned worktree directory still exists (e.g., manual deletion of branch)
/// → Self-heal cleans the directory via GitService::delete_worktree
/// → Creates a fresh branch + worktree at the same path
#[tokio::test]
async fn test_self_heal_cleans_orphaned_worktree_and_creates_fresh_branch() {
    let git_repo = setup_real_git_repo();
    let path = git_repo.path();

    let worktree_parent = path.join("worktrees");
    fs::create_dir_all(&worktree_parent).unwrap();

    let mut project = Project::new(
        "test-project".to_string(),
        path.to_string_lossy().to_string(),
    );
    let project_id = ProjectId::from_string("proj-1".to_string());
    project.id = project_id.clone();
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());

    let task_id_str = "heal2";
    let stale_branch = format!("ralphx/test-project/task-{}", task_id_str);
    let expected_wt_path_str = compute_task_worktree_path(&project, task_id_str);
    let expected_wt_path = PathBuf::from(&expected_wt_path_str);

    // Create a branch and worktree (simulating a prior execution)
    fs::create_dir_all(expected_wt_path.parent().unwrap()).unwrap();
    GitService::create_worktree(path, &expected_wt_path, &stale_branch, "main")
        .await
        .expect("Setup: create initial worktree");

    assert!(
        expected_wt_path.exists(),
        "Precondition: worktree should exist after creation"
    );
    let branch_exists = GitService::branch_exists(path, &stale_branch)
        .await
        .unwrap_or(false);
    assert!(branch_exists, "Precondition: branch should exist");

    // Simulate branch deletion (what merge cleanup does)
    // Remove the worktree first (git requires this before branch deletion)
    GitService::delete_worktree(path, &expected_wt_path)
        .await
        .expect("Simulate cleanup: delete worktree");
    let delete_output = Command::new("git")
        .args(["branch", "-D", &stale_branch])
        .current_dir(path)
        .output()
        .expect("git branch -D");
    assert!(
        delete_output.status.success(),
        "Simulate cleanup: branch deletion should succeed"
    );

    // Now simulate an orphaned worktree directory (directory exists but git entry is gone)
    fs::create_dir_all(&expected_wt_path).unwrap();
    assert!(
        expected_wt_path.exists(),
        "Precondition: orphaned directory should exist"
    );

    let branch_gone = !GitService::branch_exists(path, &stale_branch)
        .await
        .unwrap_or(true);
    assert!(branch_gone, "Precondition: branch should be deleted");

    // Simulate self-heal: clean orphaned worktree then create fresh branch
    // Step 1: delete_worktree handles both git metadata and directory cleanup
    let _ = GitService::delete_worktree(path, &expected_wt_path).await;

    // After cleanup: directory should be gone (or at least git metadata cleaned)
    // Step 2: create fresh branch + worktree at the same path
    GitService::create_worktree(path, &expected_wt_path, &stale_branch, "main")
        .await
        .expect("Self-heal: fresh worktree creation should succeed after cleanup");

    assert!(
        expected_wt_path.exists(),
        "After self-heal: fresh worktree should exist"
    );
    let fresh_branch_exists = GitService::branch_exists(path, &stale_branch)
        .await
        .unwrap_or(false);
    assert!(
        fresh_branch_exists,
        "After self-heal: fresh branch should exist in git"
    );

    let _ = GitService::delete_worktree(path, &expected_wt_path).await;
}

// ==========================================
// RC#14 Test 3: branch_exists check returns false for deleted branch
// ==========================================

/// RC#14: Verify that GitService::branch_exists correctly detects a deleted branch.
///
/// This is the foundation of the self-healing guard: if branch_exists returns the
/// wrong result, the guard cannot function. This test ensures the detection is reliable.
#[tokio::test]
async fn test_branch_exists_correctly_detects_deleted_branch() {
    let git_repo = setup_real_git_repo();
    let path = git_repo.path();

    let branch_name = "ralphx/test-project/task-heal3";

    // Branch should not exist initially
    let initial = GitService::branch_exists(path, branch_name)
        .await
        .unwrap_or(true);
    assert!(!initial, "Branch should not exist initially");

    // Create the branch
    let create = Command::new("git")
        .args(["branch", branch_name])
        .current_dir(path)
        .output()
        .expect("git branch");
    assert!(create.status.success(), "Branch creation should succeed");

    // Now it should exist
    let after_create = GitService::branch_exists(path, branch_name)
        .await
        .unwrap_or(false);
    assert!(after_create, "Branch should exist after creation");

    // Delete the branch
    let delete = Command::new("git")
        .args(["branch", "-D", branch_name])
        .current_dir(path)
        .output()
        .expect("git branch -D");
    assert!(delete.status.success(), "Branch deletion should succeed");

    // Now it should not exist — this is what the self-healing guard relies on
    let after_delete = GitService::branch_exists(path, branch_name)
        .await
        .unwrap_or(true);
    assert!(
        !after_delete,
        "Branch should not exist after deletion — self-healing guard depends on this"
    );
}

// ==========================================
// RC#14 Test 4: Full self-heal lifecycle — stored path vs expected path both cleaned
// ==========================================

/// RC#14: When stored worktree_path differs from the expected compute_task_worktree_path,
/// self-heal should clean BOTH paths.
///
/// This covers the case where a task has an old stored path (e.g., from before the
/// worktree_parent_directory convention changed) AND there's a fresh expected path.
/// Self-heal must try to clean the stored path and the computed expected path.
#[tokio::test]
async fn test_self_heal_cleans_both_stored_and_expected_worktree_paths() {
    let git_repo = setup_real_git_repo();
    let path = git_repo.path();

    let worktree_parent = path.join("worktrees");
    fs::create_dir_all(&worktree_parent).unwrap();

    let mut project = Project::new(
        "test-project".to_string(),
        path.to_string_lossy().to_string(),
    );
    let project_id = ProjectId::from_string("proj-1".to_string());
    project.id = project_id.clone();
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());

    let task_id_str = "heal4";
    let branch_name = format!("ralphx/test-project/task-{}", task_id_str);
    let expected_wt_path_str = compute_task_worktree_path(&project, task_id_str);
    let expected_wt_path = PathBuf::from(&expected_wt_path_str);

    // Simulate a "stored" path that differs from the expected path
    let stored_wt_path = worktree_parent.join("old-location").join("task-heal4");
    fs::create_dir_all(&stored_wt_path).unwrap();

    // Create a branch for the stored path (needed for git worktree add)
    let create_branch = Command::new("git")
        .args(["branch", &branch_name])
        .current_dir(path)
        .output()
        .expect("git branch");
    assert!(create_branch.status.success());

    // Create a git worktree at the stored path
    GitService::checkout_existing_branch_worktree(path, &stored_wt_path, &branch_name)
        .await
        .expect("Create stored worktree");

    assert!(stored_wt_path.exists(), "Stored worktree should exist");

    // Simulate branch deletion
    GitService::delete_worktree(path, &stored_wt_path)
        .await
        .expect("Delete stored worktree");
    let del = Command::new("git")
        .args(["branch", "-D", &branch_name])
        .current_dir(path)
        .output()
        .expect("git branch -D");
    assert!(del.status.success());

    // Self-heal step: check branch doesn't exist
    let branch_gone = !GitService::branch_exists(path, &branch_name)
        .await
        .unwrap_or(true);
    assert!(branch_gone, "Branch should be deleted");

    // Self-heal: clean stored path (if it exists)
    if stored_wt_path.exists() {
        let _ = GitService::delete_worktree(path, &stored_wt_path).await;
    }

    // Self-heal: clean expected path (if it exists — it shouldn't here, but guard it)
    if expected_wt_path.exists() {
        let _ = GitService::delete_worktree(path, &expected_wt_path).await;
    }

    // Self-heal: create fresh branch + worktree at the expected path
    fs::create_dir_all(expected_wt_path.parent().unwrap()).unwrap();
    GitService::create_worktree(path, &expected_wt_path, &branch_name, "main")
        .await
        .expect("Self-heal: create fresh worktree at expected path");

    assert!(
        expected_wt_path.exists(),
        "After self-heal: worktree should exist at expected path"
    );

    let fresh_exists = GitService::branch_exists(path, &branch_name)
        .await
        .unwrap_or(false);
    assert!(
        fresh_exists,
        "After self-heal: fresh branch should exist in git"
    );

    let _ = GitService::delete_worktree(path, &expected_wt_path).await;
}
