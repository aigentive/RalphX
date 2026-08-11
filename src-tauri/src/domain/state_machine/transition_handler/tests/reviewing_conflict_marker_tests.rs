// Integration tests for conflict marker scan before reviewer spawn (Fix 2)
// and BranchFreshnessConflict routing during auto-transition (Fix 1 coverage).
//
// Fix 2: on_enter(Reviewing) scans task worktree for conflict markers via
//   GitService::has_conflict_markers(). Marker-only evidence is not directional,
//   so it returns ReviewWorktreeConflictMarkers and persists only attention metadata.
//
// Fix 1: TaskTransitionService catches BranchFreshnessConflict from on_enter during
//   auto-transition and routes the task to Merging (tested indirectly via on_enter result).
//
// KEY INSIGHT: The freshness check runs on project.working_directory (repo_path),
// but the conflict marker scan runs on task.worktree_path. To test the scan
// independently, we use separate directories: a clean project repo for freshness
// and a task worktree containing conflict markers.

use super::helpers::*;
use crate::domain::entities::{InternalStatus, Project, ProjectId, Task, TaskId};
use crate::domain::state_machine::context::TaskContext;
use crate::domain::state_machine::{State, TransitionHandler};
use crate::infrastructure::memory::{MemoryProjectRepository, MemoryTaskRepository};
use crate::AppError;

/// Helper: init a minimal git repo at `path` with user config and initial commit.
fn init_git_repo(path: &std::path::Path) {
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .expect("git command failed");
    };
    run(&["init", "-b", "main"]);
    run(&["config", "user.email", "test@test.com"]);
    run(&["config", "user.name", "Test"]);
    std::fs::write(path.join("README.md"), "# test").unwrap();
    run(&["add", "."]);
    run(&["commit", "-m", "initial"]);
}

/// Set up in-memory repos with a task in Reviewing state.
///
/// `project_dir`: project.working_directory (used by freshness check)
/// `worktree_path`: task.worktree_path (used by conflict marker scan)
async fn setup_reviewing_task(
    project_dir: &str,
    worktree_path: Option<&str>,
    task_branch: Option<&str>,
) -> (
    TaskId,
    Arc<MemoryTaskRepository>,
    Arc<MemoryProjectRepository>,
) {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());

    let mut task = Task::new(project_id.clone(), "Review conflict test".to_string());
    task.internal_status = InternalStatus::Reviewing;
    task.worktree_path = worktree_path.map(|s| s.to_string());
    task.task_branch = task_branch.map(|s| s.to_string());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = Project::new("test-project".to_string(), project_dir.to_string());
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project_repo.create(project).await.unwrap();

    (task_id, task_repo, project_repo)
}

/// Build a TaskStateMachine for the given task/project repos.
fn build_machine(
    task_id: &TaskId,
    task_repo: &Arc<MemoryTaskRepository>,
    project_repo: &Arc<MemoryProjectRepository>,
) -> crate::domain::state_machine::TaskStateMachine {
    let services = crate::domain::state_machine::context::TaskServices::new_mock()
        .with_task_repo(Arc::clone(task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(project_repo) as Arc<dyn ProjectRepository>);
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    crate::domain::state_machine::TaskStateMachine::new(context)
}

// ==========================================================================
// Test 1: Marker-only task worktree conflict → review-worktree attention
//
// Uses separate project dir (clean) and task worktree (has conflict markers).
// The freshness check runs on the clean project dir, so the auto-commit guard
// doesn't interfere with the conflict marker scan on the task worktree.
// ==========================================================================

#[tokio::test]
async fn test_reviewing_on_enter_marker_only_conflicts_do_not_route_to_branch_update() {
    // Clean project repo (freshness check runs here)
    let project_temp = tempfile::TempDir::new().unwrap();
    init_git_repo(project_temp.path());

    // Task worktree with conflict markers (conflict marker scan runs here)
    let worktree_temp = tempfile::TempDir::new().unwrap();
    init_git_repo(worktree_temp.path());

    // Create a tracked file, commit it, then overwrite with conflict markers
    let conflict_file = worktree_temp.path().join("conflict.rs");
    std::fs::write(&conflict_file, "fn clean() {}").unwrap();
    std::process::Command::new("git")
        .args(["add", "conflict.rs"])
        .current_dir(worktree_temp.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "add conflict.rs"])
        .current_dir(worktree_temp.path())
        .output()
        .unwrap();

    // Overwrite with conflict markers (unstaged modification)
    std::fs::write(
        &conflict_file,
        "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> theirs\n",
    )
    .unwrap();

    // Precondition: markers are detectable
    let has_markers =
        crate::application::git_service::GitService::has_conflict_markers(worktree_temp.path())
            .await
            .expect("has_conflict_markers should succeed");
    assert!(
        has_markers,
        "Precondition: has_conflict_markers should detect markers in task worktree"
    );

    let project_dir = project_temp.path().to_string_lossy().to_string();
    let worktree_dir = worktree_temp.path().to_string_lossy().to_string();
    let (task_id, task_repo, project_repo) =
        setup_reviewing_task(&project_dir, Some(&worktree_dir), Some("main")).await;

    let mut machine = build_machine(&task_id, &task_repo, &project_repo);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::Reviewing).await;

    assert!(
        result.is_err(),
        "on_enter(Reviewing) should fail when conflict markers are present in task worktree"
    );
    assert!(
        matches!(
            result.as_ref().unwrap_err(),
            AppError::ReviewWorktreeConflictMarkers
        ),
        "Error should be ReviewWorktreeConflictMarkers, got: {:?}",
        result.unwrap_err()
    );
}

// ==========================================================================
// Test 2: has_conflict_markers directly detects unstaged conflict markers
// ==========================================================================

#[tokio::test]
async fn test_has_conflict_markers_detects_unstaged_changes() {
    let temp = tempfile::TempDir::new().unwrap();
    let repo_path = temp.path();
    init_git_repo(repo_path);

    // Create and commit a tracked file
    std::fs::write(repo_path.join("file.rs"), "fn clean() {}").unwrap();
    std::process::Command::new("git")
        .args(["add", "file.rs"])
        .current_dir(repo_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "add file.rs"])
        .current_dir(repo_path)
        .output()
        .unwrap();

    // Modify to have conflict markers (unstaged)
    std::fs::write(
        repo_path.join("file.rs"),
        "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> branch\n",
    )
    .unwrap();

    let has_markers = crate::application::git_service::GitService::has_conflict_markers(repo_path)
        .await
        .unwrap();
    assert!(
        has_markers,
        "has_conflict_markers should detect unstaged conflict markers"
    );
}

// ==========================================================================
// Test 3: has_conflict_markers with real merge conflict (unmerged entries)
// ==========================================================================

#[tokio::test]
async fn test_has_conflict_markers_detects_real_merge_conflict() {
    let temp = tempfile::TempDir::new().unwrap();
    let repo_path = temp.path();
    init_git_repo(repo_path);

    // Create a real merge conflict
    let conflict_file = repo_path.join("shared.txt");
    std::fs::write(&conflict_file, "original\n").unwrap();
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(repo_path)
            .output()
            .expect("git failed")
    };
    run(&["add", "shared.txt"]);
    run(&["commit", "-m", "add shared"]);
    run(&["checkout", "-b", "other"]);
    std::fs::write(&conflict_file, "other branch\n").unwrap();
    run(&["add", "shared.txt"]);
    run(&["commit", "-m", "change on other"]);
    run(&["checkout", "main"]);
    std::fs::write(&conflict_file, "main branch\n").unwrap();
    run(&["add", "shared.txt"]);
    run(&["commit", "-m", "change on main"]);
    let merge_out = run(&["merge", "other", "--no-edit"]);
    assert!(
        !merge_out.status.success(),
        "Merge should fail with conflict"
    );

    let has_markers = crate::application::git_service::GitService::has_conflict_markers(repo_path)
        .await
        .unwrap();
    assert!(
        has_markers,
        "has_conflict_markers should detect markers from real git merge conflict"
    );
}

// ==========================================================================
// Test 4: No conflict markers → proceeds past scan (no BranchFreshnessConflict)
// ==========================================================================

#[tokio::test]
async fn test_reviewing_on_enter_no_conflict_markers_proceeds() {
    let temp = tempfile::TempDir::new().unwrap();
    let repo_path = temp.path();
    init_git_repo(repo_path);

    // Clean worktree — no conflict markers
    let dir_str = repo_path.to_string_lossy().to_string();
    let (task_id, task_repo, project_repo) =
        setup_reviewing_task(&dir_str, Some(&dir_str), Some("main")).await;

    let mut machine = build_machine(&task_id, &task_repo, &project_repo);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::Reviewing).await;

    // It may fail for other reasons (mock chat service, missing reviewers), but
    // it should NOT be BranchFreshnessConflict.
    match result {
        Ok(_) => {} // Fine — no error at all
        Err(ref e) => {
            assert!(
                !matches!(e, AppError::BranchFreshnessConflict),
                "Clean worktree should not produce BranchFreshnessConflict, got: {:?}",
                e
            );
        }
    }
}

// ==========================================================================
// Test 5: Missing worktree → skips scan, no BranchFreshnessConflict
// ==========================================================================

#[tokio::test]
async fn test_reviewing_on_enter_missing_worktree_skips_scan() {
    let temp = tempfile::TempDir::new().unwrap();
    let repo_path = temp.path();
    init_git_repo(repo_path);

    let dir_str = repo_path.to_string_lossy().to_string();
    let (task_id, task_repo, project_repo) =
        setup_reviewing_task(&dir_str, Some("/nonexistent/worktree/path"), Some("main")).await;

    let mut machine = build_machine(&task_id, &task_repo, &project_repo);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::Reviewing).await;

    match result {
        Ok(_) => {}
        Err(ref e) => {
            assert!(
                !matches!(e, AppError::BranchFreshnessConflict),
                "Nonexistent worktree should skip scan, got: {:?}",
                e
            );
        }
    }
}

// ==========================================================================
// Test 6: None worktree_path → skips scan, no BranchFreshnessConflict
// ==========================================================================

#[tokio::test]
async fn test_reviewing_on_enter_none_worktree_skips_scan() {
    let temp = tempfile::TempDir::new().unwrap();
    let repo_path = temp.path();
    init_git_repo(repo_path);

    let dir_str = repo_path.to_string_lossy().to_string();
    let (task_id, task_repo, project_repo) =
        setup_reviewing_task(&dir_str, None, Some("main")).await;

    let mut machine = build_machine(&task_id, &task_repo, &project_repo);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::Reviewing).await;

    match result {
        Ok(_) => {}
        Err(ref e) => {
            assert!(
                !matches!(e, AppError::BranchFreshnessConflict),
                "None worktree_path should skip scan, got: {:?}",
                e
            );
        }
    }
}

// ==========================================================================
// Test 7: Conflict markers → metadata persistence
//
// Verifies marker-only review failures do not masquerade as branch freshness.
// ==========================================================================

#[tokio::test]
async fn test_reviewing_conflict_markers_metadata_persistence() {
    // Separate project dir (clean) from task worktree (conflict markers)
    let project_temp = tempfile::TempDir::new().unwrap();
    init_git_repo(project_temp.path());

    let worktree_temp = tempfile::TempDir::new().unwrap();
    init_git_repo(worktree_temp.path());

    // Create tracked file + commit + overwrite with markers
    let conflict_file = worktree_temp.path().join("merge_conflict.txt");
    std::fs::write(&conflict_file, "original content").unwrap();
    std::process::Command::new("git")
        .args(["add", "merge_conflict.txt"])
        .current_dir(worktree_temp.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "add merge_conflict.txt"])
        .current_dir(worktree_temp.path())
        .output()
        .unwrap();
    std::fs::write(
        &conflict_file,
        "<<<<<<< HEAD\nour changes\n=======\ntheir changes\n>>>>>>> branch\n",
    )
    .unwrap();

    let project_dir = project_temp.path().to_string_lossy().to_string();
    let worktree_dir = worktree_temp.path().to_string_lossy().to_string();
    let (task_id, task_repo, project_repo) =
        setup_reviewing_task(&project_dir, Some(&worktree_dir), Some("main")).await;

    let mut machine = build_machine(&task_id, &task_repo, &project_repo);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::Reviewing).await;
    assert!(
        matches!(result, Err(AppError::ReviewWorktreeConflictMarkers)),
        "Expected ReviewWorktreeConflictMarkers, got: {:?}",
        result
    );

    // Verify metadata was persisted
    let updated_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    let metadata: serde_json::Value =
        serde_json::from_str(updated_task.metadata.as_deref().unwrap_or("{}")).unwrap();

    assert_eq!(
        metadata.get("conflict_markers_detected"),
        Some(&serde_json::json!(true)),
        "Metadata should have conflict_markers_detected=true. Full metadata: {:?}",
        metadata
    );
    assert_ne!(
        metadata.get("branch_freshness_conflict"),
        Some(&serde_json::json!(true)),
        "Marker-only evidence must not claim a freshness conflict. Full metadata: {:?}",
        metadata
    );
    assert!(
        metadata.get("branch_update_operation_id").is_none(),
        "Marker-only evidence must not create branch-update authority. Full metadata: {:?}",
        metadata
    );
}
