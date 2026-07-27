// Integration tests for branch discovery, worktree recovery, and ExecutionBlocked handling.
// These tests create REAL git repos using tempfile.

use super::helpers::*;
use crate::domain::entities::InternalStatus;
use crate::domain::state_machine::{State, TaskEvent, TaskStateMachine, TransitionHandler};
use crate::utils::path_safety::validate_absolute_non_root_path;
use std::sync::Arc;

/// Helper: initialize a git repo in the given directory with an initial commit.
fn init_git_repo(repo_path: &std::path::Path) {
    use std::process::Command;
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo_path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(repo_path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(repo_path)
        .output()
        .unwrap();
}

/// Helper: stage all and commit with given message.
fn git_add_and_commit(repo_path: &std::path::Path, message: &str) {
    use std::process::Command;
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo_path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(repo_path)
        .output()
        .unwrap();
}

fn git_rev_parse(repo_path: &std::path::Path, ref_name: &str) -> String {
    use std::process::Command;
    let repo_path = validate_absolute_non_root_path(repo_path, "branch discovery test repository")
        .expect("safe test repo");
    let output = Command::new("git")
        .args(["rev-parse", ref_name])
        .current_dir(&repo_path)
        .output()
        .expect("git rev-parse failed");
    assert!(
        output.status.success(),
        "git rev-parse {ref_name} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

// ==================
// Branch discovery integration tests
// ==================

/// Branch discovery with attempt_programmatic_merge: task_branch=None but git branch exists.
///
/// A discovered branch with no committed work must not be marked merged.
#[tokio::test]
async fn test_branch_discovery_integrates_with_pending_merge() {
    use crate::domain::entities::{Project, Task};
    use crate::infrastructure::memory::{MemoryProjectRepository, MemoryTaskRepository};
    use std::process::Command;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let repo_path = temp_dir.path().to_path_buf();
    init_git_repo(&repo_path);

    std::fs::write(repo_path.join("README.md"), "test").unwrap();
    git_add_and_commit(&repo_path, "Initial commit");

    let project = Project::new(
        "test-project".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    let mut task = Task::new(project.id.clone(), "Test task".to_string());
    task.internal_status = InternalStatus::PendingMerge;

    let expected_branch = format!("ralphx/test-project/task-{}", task.id.as_str());
    Command::new("git")
        .args(["branch", &expected_branch])
        .current_dir(&repo_path)
        .output()
        .unwrap();
    assert_eq!(task.task_branch, None);

    let task_repo: Arc<dyn crate::domain::repositories::TaskRepository> =
        Arc::new(MemoryTaskRepository::new());
    let project_repo: Arc<dyn crate::domain::repositories::ProjectRepository> =
        Arc::new(MemoryProjectRepository::new());
    task_repo.create(task.clone()).await.unwrap();
    project_repo.create(project.clone()).await.unwrap();

    let services =
        with_default_test_branch_update_authority(TaskServices::new_mock(), Arc::clone(&task_repo))
            .with_task_repo(Arc::clone(&task_repo))
            .with_project_repo(Arc::clone(&project_repo));

    let context = create_context_with_services(task.id.as_str(), project.id.as_str(), services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated = task_repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert_eq!(
        updated.task_branch.as_deref(),
        Some(expected_branch.as_str()),
        "Task branch should be discovered and attached"
    );
    assert_eq!(
        updated.internal_status,
        InternalStatus::MergeIncomplete,
        "Discovered branch has no committed source work and must not be marked Merged"
    );
}

/// MergeIncomplete with task_branch=None, git branch exists with commits → retry → Merged.
#[tokio::test]
async fn test_merge_retry_recovery_discovers_branch_and_merges() {
    use crate::domain::entities::{Project, Task};
    use crate::infrastructure::memory::{MemoryProjectRepository, MemoryTaskRepository};
    use std::process::Command;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let repo_path = temp_dir.path().to_path_buf();
    init_git_repo(&repo_path);

    std::fs::write(repo_path.join("README.md"), "initial content").unwrap();
    git_add_and_commit(&repo_path, "Initial commit");

    let project = Project::new(
        "test-project".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    let mut task = Task::new(project.id.clone(), "Test recovery task".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = None;

    let expected_branch = format!("ralphx/test-project/task-{}", task.id.as_str());
    Command::new("git")
        .args(["checkout", "-b", &expected_branch])
        .current_dir(&repo_path)
        .output()
        .unwrap();
    std::fs::write(repo_path.join("feature.txt"), "feature work").unwrap();
    git_add_and_commit(&repo_path, "Add feature");
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(&repo_path)
        .output()
        .unwrap();

    assert_eq!(task.task_branch, None);

    let task_repo: Arc<dyn crate::domain::repositories::TaskRepository> =
        Arc::new(MemoryTaskRepository::new());
    let project_repo: Arc<dyn crate::domain::repositories::ProjectRepository> =
        Arc::new(MemoryProjectRepository::new());
    task_repo.create(task.clone()).await.unwrap();
    project_repo.create(project.clone()).await.unwrap();

    let services =
        with_default_test_branch_update_authority(TaskServices::new_mock(), Arc::clone(&task_repo))
            .with_task_repo(Arc::clone(&task_repo))
            .with_project_repo(Arc::clone(&project_repo));

    let context = create_context_with_services(task.id.as_str(), project.id.as_str(), services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let repo = Arc::clone(&task_repo);
    let tid = task.id.clone();
    let merged = wait_for_condition(
        || {
            let r = Arc::clone(&repo);
            let t = tid.clone();
            async move {
                r.get_by_id(&t).await.unwrap().unwrap().internal_status == InternalStatus::Merged
            }
        },
        5000,
    )
    .await;
    let updated = task_repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert!(
        merged,
        "Task should transition to Merged after successful programmatic merge (branch={expected_branch}); got {:?} with metadata {:?}",
        updated.internal_status,
        updated.metadata,
    );
}

/// Conflicting changes on target → retry → branch discovered → dedicated task update.
#[tokio::test]
async fn test_merge_retry_recovery_detects_conflicts_and_enters_task_branch_update() {
    use crate::domain::entities::{Project, Task};
    use crate::infrastructure::memory::{MemoryProjectRepository, MemoryTaskRepository};
    use std::process::Command;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let repo_path = temp_dir.path().to_path_buf();
    init_git_repo(&repo_path);

    std::fs::write(repo_path.join("conflict.txt"), "original line\n").unwrap();
    git_add_and_commit(&repo_path, "Initial commit");

    let project = Project::new(
        "test-project".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    let mut task = Task::new(project.id.clone(), "Test conflict task".to_string());
    task.internal_status = InternalStatus::MergeIncomplete;
    task.task_branch = None;

    let expected_branch = format!("ralphx/test-project/task-{}", task.id.as_str());
    Command::new("git")
        .args(["checkout", "-b", &expected_branch])
        .current_dir(&repo_path)
        .output()
        .unwrap();
    std::fs::write(repo_path.join("conflict.txt"), "branch change\n").unwrap();
    git_add_and_commit(&repo_path, "Branch modification");

    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(&repo_path)
        .output()
        .unwrap();
    std::fs::write(repo_path.join("conflict.txt"), "main change\n").unwrap();
    git_add_and_commit(&repo_path, "Main modification");

    assert_eq!(task.task_branch, None);

    let task_repo: Arc<dyn crate::domain::repositories::TaskRepository> =
        Arc::new(MemoryTaskRepository::new());
    let project_repo: Arc<dyn crate::domain::repositories::ProjectRepository> =
        Arc::new(MemoryProjectRepository::new());
    task_repo.create(task.clone()).await.unwrap();
    project_repo.create(project.clone()).await.unwrap();

    let branch_update_repo = crate::testing::memory_branch_update_repository_with_task_repository(
        Arc::clone(&task_repo),
    );
    let chat_service = Arc::new(MockChatService::new());
    let services = TaskServices::new_mock()
        .with_chat_service(Arc::clone(&chat_service) as Arc<dyn ChatService>)
        .with_branch_update_repo(Arc::clone(&branch_update_repo))
        .with_branch_update_workflow(crate::testing::branch_update_workflow(Arc::clone(
            &chat_service,
        )
            as Arc<dyn ChatService>))
        .with_task_repo(Arc::clone(&task_repo))
        .with_project_repo(Arc::clone(&project_repo));

    let context = create_context_with_services(task.id.as_str(), project.id.as_str(), services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let repo = Arc::clone(&task_repo);
    let tid = task.id.clone();
    assert!(
        wait_for_condition(
            || {
                let r = Arc::clone(&repo);
                let t = tid.clone();
                async move {
                    let status = r.get_by_id(&t).await.unwrap().unwrap().internal_status;
                    status == InternalStatus::UpdatingTaskBranch
                }
            },
            5000
        )
        .await,
        "Task should transition to UpdatingTaskBranch when freshness conflicts are detected"
    );

    let updated_task = task_repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert_eq!(
        updated_task.task_branch,
        Some(expected_branch.clone()),
        "Branch should be discovered and re-attached during retry"
    );
    let operation = branch_update_repo
        .get_active_operation(&task.id)
        .await
        .unwrap()
        .expect("dedicated update state must have an active operation");
    assert_eq!(
        operation.direction,
        crate::domain::entities::BranchUpdateDirection::TaskBranch
    );
    assert!(
        operation
            .workspace_path
            .as_ref()
            .is_some_and(|path| path.exists()),
        "Conflicted branch update must retain its operation-owned workspace"
    );
    assert!(
        chat_service.call_count() >= 1,
        "Conflicted branch update must spawn the dedicated resolver"
    );
}

/// Failed task with task_branch=None, git branch exists → Executing → worktree created.
#[tokio::test]
async fn test_executing_entry_recovers_existing_branch_into_worktree() {
    use crate::domain::entities::{GitMode, Project, Task};
    use crate::infrastructure::memory::{MemoryProjectRepository, MemoryTaskRepository};
    use std::process::Command;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let repo_path = temp_dir.path().to_path_buf();
    init_git_repo(&repo_path);

    std::fs::write(repo_path.join("README.md"), "initial content").unwrap();
    git_add_and_commit(&repo_path, "Initial commit");
    let base_sha = git_rev_parse(&repo_path, "main");

    let worktree_parent = temp_dir.path().join("worktrees");
    std::fs::create_dir_all(&worktree_parent).unwrap();
    let mut project = Project::new(
        "test-project".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.git_mode = GitMode::Worktree;
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());

    let mut task = Task::new(project.id.clone(), "Test worktree recovery".to_string());
    task.internal_status = InternalStatus::Failed;
    task.task_branch = None;
    task.task_branch_base_ref = Some("main".to_string());
    task.task_branch_base_sha = Some(base_sha);

    let expected_branch = format!("ralphx/test-project/task-{}", task.id.as_str());
    Command::new("git")
        .args(["branch", &expected_branch])
        .current_dir(&repo_path)
        .output()
        .unwrap();
    assert_eq!(task.task_branch, None);

    let task_repo: Arc<dyn crate::domain::repositories::TaskRepository> =
        Arc::new(MemoryTaskRepository::new());
    let project_repo: Arc<dyn crate::domain::repositories::ProjectRepository> =
        Arc::new(MemoryProjectRepository::new());
    task_repo.create(task.clone()).await.unwrap();
    project_repo.create(project.clone()).await.unwrap();

    let services =
        with_default_test_branch_update_authority(TaskServices::new_mock(), Arc::clone(&task_repo))
            .with_task_repo(Arc::clone(&task_repo))
            .with_project_repo(Arc::clone(&project_repo));

    let context = create_context_with_services(task.id.as_str(), project.id.as_str(), services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::Executing).await;
    assert!(
        result.is_ok(),
        "Executing entry should succeed even with existing branch: {:?}",
        result
    );

    let updated_task = task_repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert_eq!(
        updated_task.task_branch,
        Some(expected_branch.clone()),
        "Existing branch should be attached during Executing entry"
    );

    let expected_worktree = format!(
        "{}/test-project/task-{}",
        worktree_parent.to_string_lossy(),
        task.id.as_str()
    );
    assert_eq!(
        updated_task.worktree_path,
        Some(expected_worktree.clone()),
        "Worktree path should be set"
    );

    let worktree_path = std::path::Path::new(&expected_worktree);
    assert!(
        worktree_path.exists(),
        "Worktree directory should exist at {}",
        expected_worktree
    );

    let branch_check = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(worktree_path)
        .output()
        .unwrap();
    let current_branch = String::from_utf8_lossy(&branch_check.stdout)
        .trim()
        .to_string();
    assert_eq!(
        current_branch, expected_branch,
        "Worktree should be on the existing branch"
    );
}

// ==================
// ExecutionBlocked error handling tests
// ==================

/// ExecutionFailed event transitions Executing to Failed.
#[tokio::test]
async fn test_execution_blocked_triggers_execution_failed() {
    let (_spawner, emitter, _notifier, _dep_manager, _review_starter, services) =
        create_test_services();
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    let result = handler
        .handle_transition(
            &State::Executing,
            &TaskEvent::ExecutionFailed {
                error: "Execution blocked: uncommitted changes in working directory".to_string(),
            },
        )
        .await;

    assert!(matches!(result.state(), Some(State::Failed(_))));
    assert!(emitter.has_event("task_failed"));
}
