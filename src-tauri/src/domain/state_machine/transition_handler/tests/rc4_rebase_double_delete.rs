// Regression tests for RC#4: rebase worktree double-delete ownership contract
//
// Bug: Two code paths deleted the rebase worktree on a successful RebaseSquash merge:
//   1. Inner: git_service/merge.rs::try_rebase_squash_merge_in_worktree() Step 5
//   2. Outer: merge_strategies.rs::rebase_squash_worktree_strategy() after Success
//
// This produced spurious WARN-level "git worktree remove: not a working tree" logs.
//
// Fix:
//   - Removed inner Step 5 delete — outer caller owns the rebase worktree lifecycle.
//   - pre_delete_worktree now guards with exists() to skip paths never created.
//
// Tests:
//   1. pre_delete_worktree is a no-op on non-existent paths (no WARN emitted).
//   2. Successful RebaseSquash merge leaves no stale rebase or merge worktrees.
//   3. [Scenario 3] git worktree registry is clean after merge — exactly ONE entry in `git worktree list`.

use super::helpers::*;
use crate::domain::entities::{InternalStatus, MergeStrategy, Project, ProjectId, Task};
use crate::domain::state_machine::transition_handler::merge_helpers::{
    compute_merge_worktree_path, compute_rebase_worktree_path, pre_delete_worktree,
};
use crate::domain::state_machine::{State, TaskStateMachine, TransitionHandler};
use std::path::PathBuf;
use std::process::Command;

fn path_absent(path: &std::path::Path) -> bool {
    !crate::utils::path_safety::checked_exists(path, "test worktree path").unwrap_or(false)
}

/// RC#4 guard 1: pre_delete_worktree is a no-op when the path does not exist.
///
/// Before the fix, calling pre_delete_worktree on a non-existent path would
/// attempt `git worktree remove --force <path>`, fail (path is not a working tree),
/// and emit a WARN-level log. After the fix, the exists() guard exits early silently.
#[tokio::test]
async fn test_pre_delete_worktree_nonexistent_path_is_noop() {
    let dir = tempfile::TempDir::new().expect("create temp dir");
    let nonexistent = dir.path().join("nonexistent-rebase-wt");

    // Precondition: path does not exist
    assert!(!nonexistent.exists(), "precondition: path should not exist");

    // Should complete without panicking or emitting any error
    pre_delete_worktree(dir.path(), &nonexistent, "test-rc4").await;

    // Path still doesn't exist — nothing was created
    assert!(
        path_absent(&nonexistent),
        "path should remain absent after no-op pre_delete"
    );
}

/// RC#4 guard 2: Successful RebaseSquash merge leaves no stale rebase or merge worktrees.
///
/// Before the fix:
///   - rebase worktree was deleted twice (inner + outer), producing WARN on the second delete.
///   - pre_delete_worktree on never-created paths also produced WARN.
///
/// After the fix, both worktree paths should be absent after the merge completes.
#[tokio::test]
async fn test_rebase_squash_leaves_no_stale_worktrees() {
    // --- 1. Set up a real git repo ---
    let dir = tempfile::TempDir::new().expect("create temp dir");
    let path = dir.path();

    let _ = Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(path)
        .output();
    let _ = Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(path)
        .output();
    let _ = Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(path)
        .output();

    // Initial commit on main
    std::fs::write(path.join("README.md"), "# test").unwrap();
    let _ = Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output();
    let _ = Command::new("git")
        .args(["commit", "-m", "initial commit"])
        .current_dir(path)
        .output();

    // Add a second commit on main so rebase has a real base (base_commit_count > 1 skips squash fallback)
    std::fs::write(path.join("base.rs"), "// base").unwrap();
    let _ = Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output();
    let _ = Command::new("git")
        .args(["commit", "-m", "base commit"])
        .current_dir(path)
        .output();

    // Create task branch with a feature commit
    let task_branch = "task/rc4-test";
    let _ = Command::new("git")
        .args(["checkout", "-b", task_branch])
        .current_dir(path)
        .output();
    std::fs::write(path.join("feature.rs"), "fn feature() {}").unwrap();
    let _ = Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output();
    let _ = Command::new("git")
        .args(["commit", "-m", "add feature"])
        .current_dir(path)
        .output();

    // Return to main, then create a "workspace" branch to be the current checked-out branch.
    // We need: current_branch != "main" (target) to force the worktree path, AND
    //          task_branch not checked out in the main repo (so git can create a rebase worktree for it).
    let _ = Command::new("git")
        .args(["checkout", "main"])
        .current_dir(path)
        .output();
    let _ = Command::new("git")
        .args(["checkout", "-b", "workspace"])
        .current_dir(path)
        .output();
    // Now: current="workspace" != target="main" → rebase_squash_worktree_strategy worktree path

    // --- 2. Set up project with temp worktree_parent_directory ---
    let worktree_parent = dir.path().join("wt");
    std::fs::create_dir_all(&worktree_parent).unwrap();

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut project = Project::new(
        "test-project".to_string(),
        path.to_string_lossy().to_string(),
    );
    project.id = project_id.clone();
    project.base_branch = Some("main".to_string());
    project.merge_strategy = MergeStrategy::RebaseSquash;
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());

    // --- 3. Compute expected worktree paths before the merge ---
    let mut task = Task::new(project_id.clone(), "RC4 worktree test".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some(task_branch.to_string());
    let task_id = task.id.clone();
    let task_id_str = task_id.as_str().to_string();

    let rebase_wt_path = PathBuf::from(compute_rebase_worktree_path(&project, &task_id_str));
    let merge_wt_path = PathBuf::from(compute_merge_worktree_path(&project, &task_id_str));

    // --- 4. Set up repos ---
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    task_repo.create(task).await.unwrap();
    project_repo.create(project).await.unwrap();

    // --- 5. Run the merge via TransitionHandler ---
    let services = with_default_test_branch_update_authority(
        TaskServices::new_mock(),
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
    )
    .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
    .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>);
    let context =
        crate::domain::state_machine::context::TaskContext::new(&task_id_str, "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    // --- 6. Verify merge succeeded ---
    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Merged,
        "Task should be Merged after rebase-squash, got {:?}. Metadata: {:?}",
        updated.internal_status,
        updated.metadata,
    );

    // --- 7. Verify no stale worktrees remain ---
    assert!(
        path_absent(&rebase_wt_path),
        "RC#4: rebase worktree should be cleaned up after merge (no double-delete WARN). Path: {}",
        rebase_wt_path.display()
    );
    assert!(
        path_absent(&merge_wt_path),
        "RC#4: merge worktree should be cleaned up after merge. Path: {}",
        merge_wt_path.display()
    );
}

/// RC#4 Scenario 3: Successful RebaseSquash merge leaves git worktree registry with exactly
/// one entry (the main worktree). No stale rebase-* or merge-* entries remain in `git worktree list`.
///
/// Before the fix: inner delete removed the rebase worktree directory but if `git worktree remove`
/// ran and then the outer caller also ran `git worktree remove`, the second call would emit WARN.
/// After the fix: outer caller is sole owner; git worktree list shows only the main worktree.
#[tokio::test]
async fn test_rebase_squash_git_worktree_list_shows_only_main_worktree() {
    // --- 1. Set up a real git repo (same pattern as above) ---
    let dir = tempfile::TempDir::new().expect("create temp dir");
    let path = dir.path();

    let _ = Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(path)
        .output();
    let _ = Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(path)
        .output();
    let _ = Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(path)
        .output();

    std::fs::write(path.join("README.md"), "# test").unwrap();
    let _ = Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output();
    let _ = Command::new("git")
        .args(["commit", "-m", "initial commit"])
        .current_dir(path)
        .output();

    std::fs::write(path.join("base.rs"), "// base").unwrap();
    let _ = Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output();
    let _ = Command::new("git")
        .args(["commit", "-m", "base commit"])
        .current_dir(path)
        .output();

    let task_branch = "task/rc4-registry-test";
    let _ = Command::new("git")
        .args(["checkout", "-b", task_branch])
        .current_dir(path)
        .output();
    std::fs::write(path.join("feature2.rs"), "fn feature2() {}").unwrap();
    let _ = Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output();
    let _ = Command::new("git")
        .args(["commit", "-m", "add feature2"])
        .current_dir(path)
        .output();
    let _ = Command::new("git")
        .args(["checkout", "main"])
        .current_dir(path)
        .output();
    let _ = Command::new("git")
        .args(["checkout", "-b", "workspace"])
        .current_dir(path)
        .output();

    // --- 2. Set up project ---
    let worktree_parent = dir.path().join("wt2");
    std::fs::create_dir_all(&worktree_parent).unwrap();

    let project_id = ProjectId::from_string("proj-2".to_string());
    let mut project = Project::new(
        "registry-test".to_string(),
        path.to_string_lossy().to_string(),
    );
    project.id = project_id.clone();
    project.base_branch = Some("main".to_string());
    project.merge_strategy = MergeStrategy::RebaseSquash;
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());

    let mut task = Task::new(project_id.clone(), "RC4 registry test".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some(task_branch.to_string());
    let task_id = task.id.clone();
    let task_id_str = task_id.as_str().to_string();

    let rebase_wt_path = PathBuf::from(compute_rebase_worktree_path(&project, &task_id_str));
    let merge_wt_path = PathBuf::from(compute_merge_worktree_path(&project, &task_id_str));

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    task_repo.create(task).await.unwrap();
    project_repo.create(project).await.unwrap();

    // --- 3. Run the merge ---
    let services = with_default_test_branch_update_authority(
        TaskServices::new_mock(),
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
    )
    .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
    .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>);
    let context =
        crate::domain::state_machine::context::TaskContext::new(&task_id_str, "proj-2", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);
    let _ = handler.on_enter(&State::PendingMerge).await;

    // --- 4. Verify merge succeeded ---
    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Merged,
        "RC#4 Scenario 3: merge must succeed. Got {:?}. Metadata: {:?}",
        updated.internal_status,
        updated.metadata,
    );

    // --- 5. Verify worktree paths are absent from disk ---
    assert!(
        path_absent(&rebase_wt_path),
        "RC#4 Scenario 3: rebase worktree must not exist on disk after merge. Path: {}",
        rebase_wt_path.display()
    );
    assert!(
        path_absent(&merge_wt_path),
        "RC#4 Scenario 3: merge worktree must not exist on disk after merge. Path: {}",
        merge_wt_path.display()
    );

    // --- 6. Verify git worktree registry: EXACTLY ONE entry (the main worktree) ---
    // `git worktree list --porcelain` emits one "worktree <path>" line per registered worktree.
    // Before the RC#4 fix, a stale registry entry could remain even after the directory was deleted,
    // because `git worktree prune` was skipped when the outer delete attempted a non-existent path.
    let worktree_list = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(path)
        .output()
        .expect("git worktree list --porcelain");

    let list_output = String::from_utf8_lossy(&worktree_list.stdout);
    let worktree_count = list_output
        .lines()
        .filter(|l| l.starts_with("worktree "))
        .count();

    // Check for any rebase or merge worktree entries by name
    let has_stale_rebase = list_output.contains("rebase-");
    let has_stale_merge = list_output.contains("merge-");

    assert_eq!(
        worktree_count,
        1,
        "RC#4 Scenario 3: git worktree registry must have exactly 1 entry after merge (main only). \
         Got {}. Registry:\n{}",
        worktree_count,
        list_output
    );
    assert!(
        !has_stale_rebase,
        "RC#4 Scenario 3: no stale rebase-* entry should remain in git worktree registry. Registry:\n{}",
        list_output
    );
    assert!(
        !has_stale_merge,
        "RC#4 Scenario 3: no stale merge-* entry should remain in git worktree registry. Registry:\n{}",
        list_output
    );
}
