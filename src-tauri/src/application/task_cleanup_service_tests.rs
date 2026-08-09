use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use super::task_cleanup_service::*;
use crate::application::GitService;
use crate::domain::entities::{InternalStatus, Project, Task};
use crate::domain::repositories::{ProjectRepository, TaskRepository};
use crate::domain::services::{RunningAgentKey, RunningAgentRegistry};
use crate::infrastructure::memory::MemoryProjectRepository;
use crate::utils::path_safety::validate_absolute_non_root_path;
use ralphx_events::{NullEventSink, RecordingEventSink};

fn git_ok(repo: &Path, args: &[&str]) {
    let repo = validate_absolute_non_root_path(repo, "task cleanup test repository")
        .expect("test repository path should be safe");
    let output = Command::new("git")
        .args(args)
        // codeql[rust/path-injection]
        .current_dir(&repo)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn setup_git_repo() -> tempfile::TempDir {
    let repo = tempfile::tempdir().expect("repository should be created");
    git_ok(repo.path(), &["init", "-b", "main"]);
    git_ok(repo.path(), &["config", "user.email", "test@example.com"]);
    git_ok(repo.path(), &["config", "user.name", "Test User"]);
    git_ok(repo.path(), &["commit", "--allow-empty", "-m", "initial"]);
    repo
}

#[tokio::test]
async fn cleanup_single_task_emits_archived_event_through_explicit_sink() {
    let task_repo = Arc::new(crate::infrastructure::memory::MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let running_registry = Arc::new(crate::domain::services::MemoryRunningAgentRegistry::new());
    let project = project_repo
        .create(Project::new(
            "Event Project".to_string(),
            "/tmp/event-project".to_string(),
        ))
        .await
        .unwrap();
    let task = task_repo
        .create(Task::new(project.id.clone(), "Event task".to_string()))
        .await
        .unwrap();
    let events = RecordingEventSink::new();
    let service = TaskCleanupService::new(
        Arc::clone(&task_repo) as Arc<dyn crate::domain::repositories::TaskRepository>,
        Arc::clone(&project_repo) as Arc<dyn crate::domain::repositories::ProjectRepository>,
        Arc::clone(&running_registry) as Arc<dyn crate::domain::services::RunningAgentRegistry>,
        Arc::new(events.clone()),
    );

    service
        .cleanup_single_task(&task, StopMode::DirectStop, true)
        .await
        .unwrap();

    let recorded = events.events();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].event, "task:archived");
    assert_eq!(recorded[0].payload["taskId"], task.id.as_str());
    assert_eq!(recorded[0].payload["projectId"], project.id.as_str());
}

#[tokio::test]
async fn test_direct_cleanup_stops_task_using_current_repo_state_not_stale_snapshot() {
    let task_repo = Arc::new(crate::infrastructure::memory::MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let running_registry = Arc::new(crate::domain::services::MemoryRunningAgentRegistry::new());

    let project = Project::new("Test Project".to_string(), "/tmp/test-project".to_string());
    project_repo.create(project.clone()).await.unwrap();

    let mut stored_task = Task::new(project.id.clone(), "Leaked Task".to_string());
    stored_task.internal_status = InternalStatus::Executing;
    let stored_task = task_repo.create(stored_task).await.unwrap();

    let mut stale_snapshot = stored_task.clone();
    stale_snapshot.internal_status = InternalStatus::Ready;

    running_registry
        .register(
            RunningAgentKey::new("task_execution", stored_task.id.as_str()),
            424242,
            "conv-stale".to_string(),
            "run-stale".to_string(),
            None,
            None,
        )
        .await;
    running_registry
        .register(
            RunningAgentKey::new("branch_update", stored_task.id.as_str()),
            424243,
            "conv-branch-update".to_string(),
            "run-branch-update".to_string(),
            None,
            None,
        )
        .await;

    let service = TaskCleanupService::new(
        Arc::clone(&task_repo) as Arc<dyn crate::domain::repositories::TaskRepository>,
        Arc::clone(&project_repo) as Arc<dyn crate::domain::repositories::ProjectRepository>,
        Arc::clone(&running_registry) as Arc<dyn crate::domain::services::RunningAgentRegistry>,
        Arc::new(NullEventSink),
    );

    let report = service
        .cleanup_tasks(&[stale_snapshot], StopMode::DirectStop, false)
        .await;

    assert_eq!(report.errors.len(), 0, "cleanup should not report errors");

    let key = RunningAgentKey::new("task_execution", stored_task.id.as_str());
    assert!(
        !running_registry.is_running(&key).await,
        "direct cleanup must stop the live task_execution context even when the input snapshot is stale"
    );
    let branch_update_key = RunningAgentKey::new("branch_update", stored_task.id.as_str());
    assert!(
        !running_registry.is_running(&branch_update_key).await,
        "direct cleanup must also stop an active branch-update runtime"
    );

    let archived = task_repo
        .get_by_id(&stored_task.id)
        .await
        .unwrap()
        .expect("task should still exist after archive");
    assert!(
        archived.archived_at.is_some(),
        "cleanup should archive the task after stopping its live context"
    );
}

#[tokio::test]
async fn replacement_cleanup_removes_only_derived_task_worktrees_and_reports_errors() {
    let repo = setup_git_repo();
    let worktree_parent = tempfile::tempdir().expect("worktree parent should be created");
    let task_repo = Arc::new(crate::infrastructure::memory::MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let running_registry = Arc::new(crate::domain::services::MemoryRunningAgentRegistry::new());

    let mut project = Project::new(
        "Strict replacement cleanup".to_string(),
        repo.path().to_string_lossy().into_owned(),
    );
    project.worktree_parent_directory = Some(worktree_parent.path().to_string_lossy().into_owned());
    let project = project_repo.create(project).await.unwrap();
    let mut task = Task::new(project.id.clone(), "Owned worktree".to_string());
    let task_branch = format!("cleanup/task-{}", task.id.as_str());
    let task_worktree = project.task_worktree_path(task.id.as_str());
    GitService::create_worktree(repo.path(), &task_worktree, &task_branch, "main")
        .await
        .expect("derived task worktree should be created");
    task.task_branch = Some(task_branch.clone());
    task.worktree_path = Some(task_worktree.to_string_lossy().into_owned());
    let task = task_repo.create(task).await.unwrap();

    let service = TaskCleanupService::new(
        Arc::clone(&task_repo) as Arc<dyn crate::domain::repositories::TaskRepository>,
        Arc::clone(&project_repo) as Arc<dyn crate::domain::repositories::ProjectRepository>,
        Arc::clone(&running_registry) as Arc<dyn crate::domain::services::RunningAgentRegistry>,
        Arc::new(NullEventSink),
    );
    let report = service
        .prepare_tasks_for_replacement(&[task], StopMode::DirectStop, None)
        .await;

    assert!(
        report.errors.is_empty(),
        "unexpected errors: {:?}",
        report.errors
    );
    assert_eq!(report.git_cleanups, 1);
    assert!(!task_worktree.exists());
    assert!(!GitService::branch_exists(repo.path(), &task_branch)
        .await
        .expect("branch lookup should succeed"));

    let mut mismatched_task = Task::new(project.id.clone(), "Mismatched owner".to_string());
    let mismatched_worktree = project.task_worktree_path(mismatched_task.id.as_str());
    let unexpected_branch = format!("cleanup/unexpected-{}", mismatched_task.id.as_str());
    GitService::create_worktree(
        repo.path(),
        &mismatched_worktree,
        &unexpected_branch,
        "main",
    )
    .await
    .expect("mismatched worktree should be created");
    mismatched_task.task_branch = Some(format!("cleanup/expected-{}", mismatched_task.id.as_str()));
    mismatched_task.worktree_path = Some(mismatched_worktree.to_string_lossy().into_owned());
    let report = service
        .prepare_tasks_for_replacement(&[mismatched_task], StopMode::DirectStop, None)
        .await;

    assert_eq!(report.git_cleanups, 0);
    assert_eq!(report.errors.len(), 1);
    assert!(
        mismatched_worktree.is_dir(),
        "a worktree registered to another branch must remain untouched"
    );

    let outside = tempfile::tempdir().expect("unowned path should be created");
    let mut unsafe_task = Task::new(project.id.clone(), "Unowned worktree".to_string());
    unsafe_task.worktree_path = Some(outside.path().to_string_lossy().into_owned());
    let report = service
        .prepare_tasks_for_replacement(&[unsafe_task], StopMode::DirectStop, None)
        .await;

    assert_eq!(report.git_cleanups, 0);
    assert_eq!(report.errors.len(), 1);
    assert!(
        outside.path().is_dir(),
        "unknown path must remain untouched"
    );
}

#[tokio::test]
async fn replacement_cleanup_validates_the_whole_batch_before_mutating_any_worktree() {
    let repo = setup_git_repo();
    let worktree_parent = tempfile::tempdir().expect("worktree parent should be created");
    let task_repo = Arc::new(crate::infrastructure::memory::MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let running_registry = Arc::new(crate::domain::services::MemoryRunningAgentRegistry::new());

    let mut project = Project::new(
        "Replacement preflight".to_string(),
        repo.path().to_string_lossy().into_owned(),
    );
    project.worktree_parent_directory = Some(worktree_parent.path().to_string_lossy().into_owned());
    let project = project_repo.create(project).await.unwrap();

    let mut valid_task = Task::new(project.id.clone(), "Valid first task".to_string());
    let valid_branch = format!("cleanup/valid-{}", valid_task.id.as_str());
    let valid_worktree = project.task_worktree_path(valid_task.id.as_str());
    GitService::create_worktree(repo.path(), &valid_worktree, &valid_branch, "main")
        .await
        .expect("valid worktree should be created");
    valid_task.task_branch = Some(valid_branch);
    valid_task.worktree_path = Some(valid_worktree.to_string_lossy().into_owned());
    let valid_task = task_repo.create(valid_task).await.unwrap();

    let outside = tempfile::tempdir().expect("unknown path should be created");
    let mut unsafe_task = Task::new(project.id.clone(), "Unsafe later task".to_string());
    unsafe_task.worktree_path = Some(outside.path().to_string_lossy().into_owned());
    let unsafe_task = task_repo.create(unsafe_task).await.unwrap();

    let service = TaskCleanupService::new(
        Arc::clone(&task_repo) as Arc<dyn crate::domain::repositories::TaskRepository>,
        Arc::clone(&project_repo) as Arc<dyn crate::domain::repositories::ProjectRepository>,
        Arc::clone(&running_registry) as Arc<dyn crate::domain::services::RunningAgentRegistry>,
        Arc::new(NullEventSink),
    );
    let report = service
        .prepare_tasks_for_replacement(&[valid_task, unsafe_task], StopMode::DirectStop, None)
        .await;

    assert_eq!(report.errors.len(), 1);
    assert!(
        valid_worktree.is_dir(),
        "a later validation failure must preserve earlier valid worktrees"
    );
    assert!(
        outside.path().is_dir(),
        "unknown data must remain untouched"
    );
}

#[tokio::test]
async fn replacement_cleanup_deletes_task_branch_checked_out_in_project_root() {
    let repo = setup_git_repo();
    let task_repo = Arc::new(crate::infrastructure::memory::MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let running_registry = Arc::new(crate::domain::services::MemoryRunningAgentRegistry::new());

    let project = project_repo
        .create(Project::new(
            "Project-root branch cleanup".to_string(),
            repo.path().to_string_lossy().into_owned(),
        ))
        .await
        .unwrap();
    let mut task = Task::new(project.id.clone(), "Current root branch".to_string());
    let task_branch = format!("cleanup/root-{}", task.id.as_str());
    git_ok(repo.path(), &["checkout", "-b", &task_branch]);
    task.task_branch = Some(task_branch.clone());
    let task = task_repo.create(task).await.unwrap();

    let service = TaskCleanupService::new(
        Arc::clone(&task_repo) as Arc<dyn crate::domain::repositories::TaskRepository>,
        Arc::clone(&project_repo) as Arc<dyn crate::domain::repositories::ProjectRepository>,
        Arc::clone(&running_registry) as Arc<dyn crate::domain::services::RunningAgentRegistry>,
        Arc::new(NullEventSink),
    );
    let report = service
        .prepare_tasks_for_replacement(&[task], StopMode::DirectStop, None)
        .await;

    assert!(
        report.errors.is_empty(),
        "unexpected cleanup errors: {:?}",
        report.errors
    );
    assert_eq!(
        GitService::get_current_branch(repo.path()).await.unwrap(),
        "main"
    );
    assert!(
        !GitService::branch_exists(repo.path(), &task_branch)
            .await
            .expect("task branch lookup should succeed"),
        "cleanup should delete the task branch after moving the checkout to base"
    );
}

#[tokio::test]
async fn replacement_cleanup_preserves_explicitly_preserved_branch() {
    let repo = setup_git_repo();
    let task_repo = Arc::new(crate::infrastructure::memory::MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let running_registry = Arc::new(crate::domain::services::MemoryRunningAgentRegistry::new());

    let project = project_repo
        .create(Project::new(
            "Preserved branch cleanup".to_string(),
            repo.path().to_string_lossy().into_owned(),
        ))
        .await
        .unwrap();
    let mut task = Task::new(project.id.clone(), "Preserved branch".to_string());
    let preserved_branch = format!("cleanup/preserved-{}", task.id.as_str());
    git_ok(repo.path(), &["branch", &preserved_branch, "main"]);
    task.task_branch = Some(preserved_branch.clone());
    let task = task_repo.create(task).await.unwrap();

    let service = TaskCleanupService::new(
        Arc::clone(&task_repo) as Arc<dyn crate::domain::repositories::TaskRepository>,
        Arc::clone(&project_repo) as Arc<dyn crate::domain::repositories::ProjectRepository>,
        Arc::clone(&running_registry) as Arc<dyn crate::domain::services::RunningAgentRegistry>,
        Arc::new(NullEventSink),
    );
    service
        .preflight_tasks_for_replacement(std::slice::from_ref(&task), Some(&preserved_branch))
        .await
        .expect("preserved branch should pass strict preflight");
    let report = service
        .prepare_tasks_for_replacement(&[task], StopMode::DirectStop, Some(&preserved_branch))
        .await;

    assert!(
        report.errors.is_empty(),
        "unexpected cleanup errors: {:?}",
        report.errors
    );
    assert!(
        GitService::branch_exists(repo.path(), &preserved_branch)
            .await
            .expect("preserved branch lookup should succeed"),
        "preserved branch must not be deleted during replacement cleanup"
    );
}

#[tokio::test]
async fn replacement_cleanup_fails_closed_when_a_task_row_disappeared() {
    let repo = setup_git_repo();
    let task_repo = Arc::new(crate::infrastructure::memory::MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let running_registry = Arc::new(crate::domain::services::MemoryRunningAgentRegistry::new());
    let project = project_repo
        .create(Project::new(
            "Missing task row".to_string(),
            repo.path().to_string_lossy().into_owned(),
        ))
        .await
        .unwrap();
    let missing = Task::new(project.id, "Missing snapshot".to_string());
    let service = TaskCleanupService::new(
        Arc::clone(&task_repo) as Arc<dyn crate::domain::repositories::TaskRepository>,
        Arc::clone(&project_repo) as Arc<dyn crate::domain::repositories::ProjectRepository>,
        Arc::clone(&running_registry) as Arc<dyn crate::domain::services::RunningAgentRegistry>,
        Arc::new(NullEventSink),
    );

    let report = service
        .prepare_tasks_for_replacement(&[missing], StopMode::DirectStop, None)
        .await;

    assert_eq!(report.errors.len(), 1);
    assert!(report.errors[0].contains("no longer exists"));
}
