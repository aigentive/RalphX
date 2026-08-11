// Phase 2 MERGE + Phase 3 CLEANUP tests
//
// TDD tests for the merge pipeline speed overhaul:
// - Phase 2: complete_merge_internal marks Merged immediately, sets pending_cleanup metadata
// - Phase 3: deferred_merge_cleanup runs in background, clears metadata after cleanup
// - Startup: resume_pending_cleanup detects and resumes unfinished cleanup

use std::path::Path;
use std::sync::Arc;

use crate::domain::entities::{InternalStatus, MergeStrategy, Project, ProjectId, Task};
use crate::domain::repositories::TaskRepository;
use crate::domain::state_machine::transition_handler::merge_completion::{
    clear_pending_cleanup_metadata, deferred_merge_cleanup, has_pending_cleanup_metadata,
    set_pending_cleanup_metadata,
};
use crate::infrastructure::memory::MemoryTaskRepository;

// ==================
// Metadata helpers
// ==================

#[test]
fn set_pending_cleanup_metadata_on_empty_metadata() {
    let mut task = Task::new(ProjectId::from_string("proj-1".to_string()), "test".into());
    assert!(task.metadata.is_none());

    set_pending_cleanup_metadata(&mut task);

    assert!(task.metadata.is_some());
    assert!(has_pending_cleanup_metadata(&task));
}

#[test]
fn set_pending_cleanup_metadata_preserves_existing() {
    let mut task = Task::new(ProjectId::from_string("proj-1".to_string()), "test".into());
    task.metadata = Some(r#"{"merge_attempt_count":3}"#.to_string());

    set_pending_cleanup_metadata(&mut task);

    assert!(has_pending_cleanup_metadata(&task));
    // Verify existing metadata preserved
    let meta: serde_json::Value = serde_json::from_str(task.metadata.as_deref().unwrap()).unwrap();
    assert_eq!(meta["merge_attempt_count"], 3);
    assert_eq!(meta["pending_cleanup"], true);
}

#[test]
fn clear_pending_cleanup_metadata_removes_flag() {
    let mut task = Task::new(ProjectId::from_string("proj-1".to_string()), "test".into());
    task.metadata = Some(r#"{"pending_cleanup":true,"other":"data"}"#.to_string());

    clear_pending_cleanup_metadata(&mut task);

    assert!(!has_pending_cleanup_metadata(&task));
    // Verify other metadata preserved
    let meta: serde_json::Value = serde_json::from_str(task.metadata.as_deref().unwrap()).unwrap();
    assert_eq!(meta["other"], "data");
    assert!(meta.get("pending_cleanup").is_none());
}

#[test]
fn has_pending_cleanup_false_when_no_metadata() {
    let task = Task::new(ProjectId::from_string("proj-1".to_string()), "test".into());
    assert!(!has_pending_cleanup_metadata(&task));
}

#[test]
fn has_pending_cleanup_false_when_not_set() {
    let mut task = Task::new(ProjectId::from_string("proj-1".to_string()), "test".into());
    task.metadata = Some(r#"{"other":"data"}"#.to_string());
    assert!(!has_pending_cleanup_metadata(&task));
}

// ==================
// Phase 2: complete_merge_internal sets pending_cleanup
// ==================

fn make_test_repo() -> (tempfile::TempDir, String) {
    let dir = tempfile::TempDir::new().expect("create temp dir");
    let path = dir.path();
    for args in [
        vec!["init", "-b", "main"],
        vec!["config", "user.email", "t@t.com"],
        vec!["config", "user.name", "T"],
    ] {
        let _ = std::process::Command::new("git")
            .args(&args)
            .current_dir(path)
            .output();
    }
    std::fs::write(path.join("README.md"), "# test").unwrap();
    for args in [vec!["add", "."], vec!["commit", "-m", "init"]] {
        let _ = std::process::Command::new("git")
            .args(&args)
            .current_dir(path)
            .output();
    }
    let path_str = path.to_string_lossy().to_string();
    (dir, path_str)
}

fn make_merge_commit(repo_path: &Path) -> String {
    // Create a feature branch with a commit, then merge it
    let _ = std::process::Command::new("git")
        .args(["checkout", "-b", "feature/test-merge"])
        .current_dir(repo_path)
        .output();
    std::fs::write(repo_path.join("feature.rs"), "// feature").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(repo_path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "add feature"])
        .current_dir(repo_path)
        .output();

    // Get the feature commit SHA
    let sha_output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_path)
        .output()
        .unwrap();
    let _feature_sha = String::from_utf8_lossy(&sha_output.stdout)
        .trim()
        .to_string();

    // Switch back to main and merge
    let _ = std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(repo_path)
        .output();
    let _ = std::process::Command::new("git")
        .args([
            "merge",
            "feature/test-merge",
            "--no-ff",
            "-m",
            "merge feature",
        ])
        .current_dir(repo_path)
        .output();

    // Get the merge commit SHA
    let merge_sha_output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_path)
        .output()
        .unwrap();
    String::from_utf8_lossy(&merge_sha_output.stdout)
        .trim()
        .to_string()
}

/// Phase 2: complete_merge_internal sets pending_cleanup metadata.
#[tokio::test]
async fn complete_merge_sets_pending_cleanup_metadata() {
    use crate::domain::state_machine::transition_handler::complete_merge_internal;

    let (_dir, repo_path_str) = make_test_repo();
    let repo_path = Path::new(&repo_path_str);
    let commit_sha = make_merge_commit(repo_path);

    let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());
    let project_id = ProjectId::from_string("proj-phase2".to_string());

    let mut task = Task::new(project_id.clone(), "Phase 2 test".into());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some("feature/test-merge".to_string());
    task.worktree_path = Some("/tmp/some-worktree".to_string());
    task_repo.create(task.clone()).await.unwrap();

    let mut project = Project::new("phase2-project".to_string(), repo_path_str.clone());
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project.merge_strategy = MergeStrategy::Merge;

    let result = complete_merge_internal(
        &mut task,
        &project,
        &commit_sha,
        "",
        "main",
        &task_repo,
        None,
        None,
        None,
        None,
    )
    .await;

    assert!(result.is_ok(), "complete_merge_internal should succeed");

    // Task must be Merged
    assert_eq!(task.internal_status, InternalStatus::Merged);

    // pending_cleanup metadata must be set
    assert!(
        has_pending_cleanup_metadata(&task),
        "pending_cleanup metadata must be set after complete_merge_internal"
    );

    // task_branch and worktree_path must NOT be cleared yet (cleanup hasn't run)
    assert!(
        task.task_branch.is_some(),
        "task_branch must still be set (cleanup deferred)"
    );
    assert!(
        task.worktree_path.is_some(),
        "worktree_path must still be set (cleanup deferred)"
    );
}

/// Phase 2: complete_merge_internal does NOT block on cleanup.
/// Verifies the function returns quickly (no process killing, no worktree deletion).
#[tokio::test]
async fn complete_merge_returns_quickly_without_cleanup_blocking() {
    use crate::domain::state_machine::transition_handler::complete_merge_internal;

    let (_dir, repo_path_str) = make_test_repo();
    let repo_path = Path::new(&repo_path_str);
    let commit_sha = make_merge_commit(repo_path);

    let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());
    let project_id = ProjectId::from_string("proj-fast".to_string());

    let mut task = Task::new(project_id.clone(), "Fast return test".into());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some("feature/test-merge".to_string());
    task_repo.create(task.clone()).await.unwrap();

    let mut project = Project::new("fast-project".to_string(), repo_path_str);
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project.merge_strategy = MergeStrategy::Merge;

    let start = std::time::Instant::now();
    let result = complete_merge_internal(
        &mut task,
        &project,
        &commit_sha,
        "",
        "main",
        &task_repo,
        None,
        None,
        None,
        None,
    )
    .await;
    let elapsed = start.elapsed();

    assert!(result.is_ok());
    // Without synchronous cleanup, this should complete in well under 2 seconds.
    // The old code with worktree deletion could take 5-10s.
    assert!(
        elapsed.as_secs() < 2,
        "complete_merge_internal should return quickly without cleanup blocking, took {}ms",
        elapsed.as_millis()
    );
}

// ==================
// Phase 3: deferred_merge_cleanup
// ==================

/// Phase 3: deferred_merge_cleanup clears pending_cleanup metadata.
#[tokio::test]
async fn deferred_cleanup_clears_pending_cleanup_metadata() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let task_repo_dyn: Arc<dyn TaskRepository> = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;

    let project_id = ProjectId::from_string("proj-cleanup".to_string());
    let mut task = Task::new(project_id, "Cleanup test".into());
    task.internal_status = InternalStatus::Merged;
    task.task_branch = Some("task/cleanup-test".to_string());
    task.worktree_path = Some("/tmp/nonexistent-worktree-cleanup".to_string());
    set_pending_cleanup_metadata(&mut task);
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    deferred_merge_cleanup(
        task_id.clone(),
        task_repo_dyn,
        "/tmp/nonexistent-repo".to_string(),
        Some("task/cleanup-test".to_string()),
        Some("/tmp/nonexistent-worktree-cleanup".to_string()),
        None,
    )
    .await;

    // Verify pending_cleanup is cleared
    let updated_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert!(
        !has_pending_cleanup_metadata(&updated_task),
        "pending_cleanup metadata must be cleared after deferred cleanup"
    );
}

/// Phase 3: deferred_merge_cleanup clears task_branch and worktree_path.
#[tokio::test]
async fn deferred_cleanup_clears_branch_and_worktree_fields() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let task_repo_dyn: Arc<dyn TaskRepository> = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;

    let project_id = ProjectId::from_string("proj-fields".to_string());
    let mut task = Task::new(project_id, "Fields test".into());
    task.internal_status = InternalStatus::Merged;
    task.task_branch = Some("task/fields-test".to_string());
    task.worktree_path = Some("/tmp/nonexistent-worktree-fields".to_string());
    set_pending_cleanup_metadata(&mut task);
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    deferred_merge_cleanup(
        task_id.clone(),
        task_repo_dyn,
        "/tmp/nonexistent-repo".to_string(),
        Some("task/fields-test".to_string()),
        Some("/tmp/nonexistent-worktree-fields".to_string()),
        None,
    )
    .await;

    let updated_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert!(
        updated_task.task_branch.is_none(),
        "task_branch must be cleared after deferred cleanup"
    );
    assert!(
        updated_task.worktree_path.is_none(),
        "worktree_path must be cleared after deferred cleanup"
    );
}

/// Phase 3: deferred_merge_cleanup failure does not affect Merged status.
#[tokio::test]
async fn deferred_cleanup_failure_preserves_merged_status() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let task_repo_dyn: Arc<dyn TaskRepository> = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;

    let project_id = ProjectId::from_string("proj-fail".to_string());
    let mut task = Task::new(project_id, "Fail test".into());
    task.internal_status = InternalStatus::Merged;
    task.task_branch = Some("task/fail-test".to_string());
    // Point to a non-existent worktree path — cleanup will "fail" but non-fatally
    task.worktree_path = Some("/tmp/nonexistent-worktree-fail-test".to_string());
    set_pending_cleanup_metadata(&mut task);
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    deferred_merge_cleanup(
        task_id.clone(),
        task_repo_dyn,
        "/tmp/nonexistent-repo".to_string(),
        Some("task/fail-test".to_string()),
        Some("/tmp/nonexistent-worktree-fail-test".to_string()),
        None,
    )
    .await;

    // Task must still be Merged
    let updated_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::Merged,
        "Merged status must be preserved even if cleanup fails"
    );
}

/// Phase 3: deferred_merge_cleanup with real git repo cleans up branch.
#[tokio::test]
async fn deferred_cleanup_deletes_branch_in_real_repo() {
    let (_dir, repo_path_str) = make_test_repo();
    let repo_path_for_git = repo_path_str.clone();

    // Create a task branch
    let branch_name = "task/deferred-cleanup-test";
    let git_dir = Path::new(&repo_path_for_git);
    let _ = std::process::Command::new("git")
        .args(["checkout", "-b", branch_name])
        .current_dir(git_dir)
        .output();
    let _ = std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(git_dir)
        .output();

    // Verify branch exists
    let branch_check = std::process::Command::new("git")
        .args(["branch", "--list", branch_name])
        .current_dir(git_dir)
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&branch_check.stdout).contains(branch_name),
        "Branch should exist before cleanup"
    );

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let task_repo_dyn: Arc<dyn TaskRepository> = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;

    let project_id = ProjectId::from_string("proj-real".to_string());
    let mut task = Task::new(project_id, "Real cleanup test".into());
    task.internal_status = InternalStatus::Merged;
    task.task_branch = Some(branch_name.to_string());
    set_pending_cleanup_metadata(&mut task);
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    deferred_merge_cleanup(
        task_id.clone(),
        task_repo_dyn,
        repo_path_str,
        Some(branch_name.to_string()),
        None, // no worktree
        None,
    )
    .await;

    // Branch must be deleted
    let branch_check_after = std::process::Command::new("git")
        .args(["branch", "--list", branch_name])
        .current_dir(git_dir)
        .output()
        .unwrap();
    assert!(
        !String::from_utf8_lossy(&branch_check_after.stdout).contains(branch_name),
        "Branch should be deleted after deferred cleanup"
    );

    // Metadata must be cleared
    let updated_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert!(!has_pending_cleanup_metadata(&updated_task));
    assert!(updated_task.task_branch.is_none());
}

/// Phase 3: deferred_merge_cleanup handles no-op gracefully (no branch, no worktree).
#[tokio::test]
async fn deferred_cleanup_noop_when_no_branch_or_worktree() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let task_repo_dyn: Arc<dyn TaskRepository> = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;

    let project_id = ProjectId::from_string("proj-noop".to_string());
    let mut task = Task::new(project_id, "Noop test".into());
    task.internal_status = InternalStatus::Merged;
    // No branch or worktree
    set_pending_cleanup_metadata(&mut task);
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    deferred_merge_cleanup(
        task_id.clone(),
        task_repo_dyn,
        "/tmp/nonexistent-repo".to_string(),
        None,
        None,
        None,
    )
    .await;

    let updated_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert!(!has_pending_cleanup_metadata(&updated_task));
    assert_eq!(updated_task.internal_status, InternalStatus::Merged);
}
