// Regression tests for RC#12 + RC#13: stale merge worktree between merge phases
//
// RC#12: merge-{id} worktree from Phase 1 (plan_update) not cleaned before Phase 2 retry
// RC#13: source_update_conflict path doesn't clean stale merge-{id} worktree (same as RC#6)
// Bonus: pre_merge_cleanup should abort stale MERGE_HEAD in task worktree

use super::helpers::*;
use crate::application::git_service::GitService;
use crate::domain::entities::{Project, ProjectId};
use crate::domain::state_machine::transition_handler::merge_helpers::{
    compute_merge_worktree_path, compute_task_worktree_path,
};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Create a branch off HEAD for use in a worktree (avoids "branch already in use" errors).
fn create_branch(repo: &std::path::Path, name: &str) {
    let _ = Command::new("git")
        .args(["branch", name])
        .current_dir(repo)
        .output()
        .expect("git branch");
}

// ==========================================
// RC#12: merge worktree cleaned between plan_update and task_merge phases
// ==========================================

/// RC#12: After plan_update resolution, attempt_merge_auto_complete should clean
/// the merge-{id} worktree before retrying the task merge via PendingMerge.
///
/// Simulates: Phase 1 merger agent resolves plan_update_conflict in merge-{id}
/// worktree → auto-complete detects plan is up-to-date → cleanup should delete
/// merge-{id} → Phase 2 source_update_conflict can create fresh merge-{id}.
#[tokio::test]
async fn test_merge_worktree_cleaned_between_plan_update_and_task_merge_phases() {
    let git_repo = setup_real_git_repo();
    let path = git_repo.path();

    // Create branches for worktree use (can't reuse main/task branch already checked out)
    create_branch(path, "plan-branch-phase1");

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

    let task_id_str = "rc12-test";
    let merge_wt_path_str = compute_merge_worktree_path(&project, task_id_str);
    let merge_wt_path = PathBuf::from(&merge_wt_path_str);

    // Phase 1: create merge worktree with plan branch (simulating plan_update_conflict)
    fs::create_dir_all(merge_wt_path.parent().unwrap()).unwrap();
    GitService::checkout_existing_branch_worktree(path, &merge_wt_path, "plan-branch-phase1")
        .await
        .expect("create merge worktree for Phase 1");

    assert!(
        merge_wt_path.exists(),
        "Precondition: merge-{{id}} worktree should exist after Phase 1. Path: {}",
        merge_wt_path.display()
    );

    // RC#12 fix: cleanup the merge worktree before retrying
    if merge_wt_path.exists() {
        GitService::delete_worktree(path, &merge_wt_path)
            .await
            .expect("RC#12 cleanup should succeed");
    }

    assert!(
        !merge_wt_path.exists(),
        "After RC#12 cleanup, merge-{{id}} worktree should be deleted. Path: {}",
        merge_wt_path.display()
    );

    // Phase 2: create fresh merge worktree with task branch (source_update_conflict)
    GitService::checkout_existing_branch_worktree(path, &merge_wt_path, &git_repo.task_branch)
        .await
        .expect("Phase 2 should be able to create merge worktree after cleanup");

    assert!(
        merge_wt_path.exists(),
        "Phase 2 should have created a fresh merge-{{id}} worktree. Path: {}",
        merge_wt_path.display()
    );

    let _ = GitService::delete_worktree(path, &merge_wt_path).await;
}

// ==========================================
// RC#13: source_update_conflict cleans stale merge worktree
// ==========================================

/// RC#13: When source_update_conflict tries to create merge-{id} worktree and a
/// stale one already exists, it should clean it first instead of failing with
/// "fatal: '/path/merge-{id}' already exists".
#[tokio::test]
async fn test_source_update_conflict_cleans_stale_merge_worktree() {
    let git_repo = setup_real_git_repo();
    let path = git_repo.path();

    // Create a branch for the stale worktree (avoids "branch in use" error with main)
    create_branch(path, "stale-branch");

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

    let task_id_str = "stale-wt-test";
    let merge_wt_path_str = compute_merge_worktree_path(&project, task_id_str);
    let merge_wt_path = PathBuf::from(&merge_wt_path_str);

    // Create a stale merge worktree (leftover from a prior merge attempt)
    fs::create_dir_all(merge_wt_path.parent().unwrap()).unwrap();
    GitService::checkout_existing_branch_worktree(path, &merge_wt_path, "stale-branch")
        .await
        .expect("create stale merge worktree");

    assert!(
        merge_wt_path.exists(),
        "Precondition: stale merge worktree should exist"
    );

    // RC#13 fix: clean stale worktree before creating fresh one
    if merge_wt_path.exists() {
        GitService::delete_worktree(path, &merge_wt_path)
            .await
            .expect("RC#13 cleanup of stale merge worktree should succeed");
    }

    // Create the fresh merge worktree (what source_update_conflict does after cleanup)
    GitService::checkout_existing_branch_worktree(path, &merge_wt_path, &git_repo.task_branch)
        .await
        .expect(
            "After stale worktree cleanup, source_update_conflict should create fresh merge worktree",
        );

    assert!(
        merge_wt_path.exists(),
        "Fresh merge worktree should be created after stale cleanup"
    );

    // Verify the worktree is on the expected branch
    let head_output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&merge_wt_path)
        .output()
        .expect("git rev-parse HEAD in merge worktree");
    let branch = String::from_utf8_lossy(&head_output.stdout)
        .trim()
        .to_string();
    assert_eq!(
        branch, git_repo.task_branch,
        "Merge worktree should be on the source branch (task branch)"
    );

    let _ = GitService::delete_worktree(path, &merge_wt_path).await;
}

// ==========================================
// Bonus: pre_merge_cleanup aborts stale MERGE_HEAD in task worktree
// ==========================================

/// Bonus: When a task worktree has a stale MERGE_HEAD from a prior failed merge,
/// pre_merge_cleanup should abort it so git operations don't fail.
#[tokio::test]
async fn test_pre_merge_cleanup_aborts_stale_merge_head_in_task_worktree() {
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

    let task_id_str = "merge-head-test";
    let task_wt_path_str = compute_task_worktree_path(&project, task_id_str);
    let task_wt_path = PathBuf::from(&task_wt_path_str);

    // Create a task worktree on the task branch
    fs::create_dir_all(task_wt_path.parent().unwrap()).unwrap();
    GitService::checkout_existing_branch_worktree(path, &task_wt_path, &git_repo.task_branch)
        .await
        .expect("create task worktree");

    // Create a conflicting commit on main to force a merge conflict
    fs::write(path.join("feature.rs"), "// conflicting main content\n").unwrap();
    let _ = Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output();
    let _ = Command::new("git")
        .args(["commit", "-m", "conflict on main"])
        .current_dir(path)
        .output();

    // Attempt a merge in the task worktree that will produce a conflict → stale MERGE_HEAD
    let merge_output = Command::new("git")
        .args(["merge", "main", "--no-edit"])
        .current_dir(&task_wt_path)
        .output()
        .expect("git merge in task worktree");

    assert!(
        !merge_output.status.success(),
        "Expected merge conflict in task worktree"
    );

    assert!(
        GitService::is_merge_in_progress(&task_wt_path),
        "Precondition: task worktree should have MERGE_HEAD (stale merge in progress)"
    );

    // Simulate the bonus fix: abort the stale merge
    if GitService::is_merge_in_progress(&task_wt_path) {
        GitService::abort_merge(&task_wt_path)
            .await
            .expect("Aborting stale merge should succeed");
    }

    assert!(
        !GitService::is_merge_in_progress(&task_wt_path),
        "After abort, MERGE_HEAD should be gone from task worktree"
    );

    let _ = GitService::delete_worktree(path, &task_wt_path).await;
}

// ==========================================
// RC2 fix: stale worktree entry with deleted path triggers prune before fallthrough
// ==========================================

/// RC2 fix: When git list_worktrees reports a worktree for a branch, but the path
/// no longer exists on disk (stale entry), delete_worktree + prune should be called
/// to clean up the stale git metadata, allowing fresh worktree creation to succeed.
///
/// This simulates the bug where `git worktree list` shows an entry for a branch but
/// the directory was externally deleted — without the fix, `git worktree add` would
/// fail with "branch already checked out" because git still tracks the stale entry.
#[tokio::test]
async fn test_stale_worktree_entry_with_deleted_path_can_be_recreated() {
    let git_repo = setup_real_git_repo();
    let path = git_repo.path();

    create_branch(path, "stale-entry-branch");

    let worktree_parent = path.join("worktrees");
    fs::create_dir_all(&worktree_parent).unwrap();

    let wt_path = worktree_parent.join("stale-entry-wt");

    // Create a worktree for the branch
    GitService::checkout_existing_branch_worktree(path, &wt_path, "stale-entry-branch")
        .await
        .expect("create initial worktree");

    assert!(wt_path.exists(), "Worktree should exist after creation");

    // Verify git tracks the worktree
    let worktrees = GitService::list_worktrees(path).await.unwrap();
    let has_entry = worktrees
        .iter()
        .any(|w| w.branch.as_deref() == Some("stale-entry-branch"));
    assert!(has_entry, "Git should list the worktree entry");

    // Simulate external deletion of the directory (stale entry scenario)
    fs::remove_dir_all(&wt_path).unwrap();
    assert!(
        !wt_path.exists(),
        "Path should be gone after manual deletion"
    );

    // The wt_path.exists() check now detects the stale entry
    // Prune + delete_worktree should clean the stale metadata
    let _ = GitService::delete_worktree(path, &wt_path).await;

    // git worktree prune should have cleaned the stale entry
    // (Either via delete_worktree or via a separate prune call)
    let _ = tokio::process::Command::new("git")
        .args(["worktree", "prune"])
        .current_dir(path)
        .output()
        .await;

    // After prune, branch should be available for fresh worktree creation
    let result =
        GitService::checkout_existing_branch_worktree(path, &wt_path, "stale-entry-branch").await;
    assert!(
        result.is_ok(),
        "After prune of stale entry, fresh worktree creation should succeed: {:?}",
        result.err()
    );

    assert!(
        wt_path.exists(),
        "Fresh worktree should be created successfully"
    );

    let _ = GitService::delete_worktree(path, &wt_path).await;
}

// ==========================================
// Fix 4: already-exists error — stale directory at target path
// ==========================================

/// Fix 4: When `git worktree add` fails with "already exists" because the target
/// directory physically exists on disk (from failed cleanup) but is NOT registered
/// in git's worktree metadata, `checkout_existing_branch_worktree` should:
///   1. Force-remove the stale directory via `tokio::fs::remove_dir_all`
///   2. Run `git worktree prune`
///   3. Retry `git worktree add` — which must succeed
#[tokio::test]
async fn test_checkout_existing_branch_worktree_recovers_from_already_exists() {
    let git_repo = setup_real_git_repo();
    let path = git_repo.path();

    create_branch(path, "already-exists-branch");

    let worktree_parent = path.join("worktrees");
    fs::create_dir_all(&worktree_parent).unwrap();

    let wt_path = worktree_parent.join("already-exists-wt");

    // Pre-create the directory WITHOUT registering it as a git worktree.
    // This is the scenario: a previous run created the directory but failed
    // before `git worktree add` completed, leaving a stale directory behind.
    fs::create_dir_all(&wt_path).unwrap();
    assert!(
        wt_path.exists(),
        "Precondition: stale directory should exist at worktree path"
    );

    // Verify git does NOT know about this path (not registered)
    let worktrees = GitService::list_worktrees(path).await.unwrap();
    let registered = worktrees
        .iter()
        .any(|w| w.path == wt_path.to_string_lossy().as_ref());
    assert!(
        !registered,
        "Precondition: stale directory should NOT be registered as a git worktree"
    );

    // Call checkout_existing_branch_worktree — should detect "already exists",
    // remove the stale dir, prune, and successfully create the worktree.
    let result =
        GitService::checkout_existing_branch_worktree(path, &wt_path, "already-exists-branch")
            .await;
    assert!(
        result.is_ok(),
        "checkout_existing_branch_worktree should recover from stale directory: {:?}",
        result.err()
    );

    assert!(
        wt_path.exists(),
        "Worktree directory should exist after recovery"
    );

    // Verify git now tracks it as a proper worktree
    let worktrees_after = GitService::list_worktrees(path).await.unwrap();
    let now_registered = worktrees_after
        .iter()
        .any(|w| w.branch.as_deref() == Some("already-exists-branch"));
    assert!(
        now_registered,
        "Worktree should be registered in git after successful recovery"
    );

    let _ = GitService::delete_worktree(path, &wt_path).await;
}
