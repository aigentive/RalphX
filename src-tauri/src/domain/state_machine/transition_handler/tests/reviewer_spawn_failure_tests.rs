// Tests for reviewer spawn failure recovery: on_enter(Reviewing) with missing worktree or failing chat service.
//
// Covers three scenarios:
//   1. worktree_path = Some("/nonexistent") → directory does not exist → ReviewWorktreeMissing
//   2. worktree_path = None → no path set at all → ReviewWorktreeMissing
//   3. worktree_path exists but chat service unavailable → reviewer_spawn_failure_count recorded
//
// Per CLAUDE.md rule 1.5: MemoryTaskRepository + MockChatService.
// No real git repo needed for tests 1 and 2 — they exercise the worktree guard, not git.
// Test 3 uses std::env::temp_dir() which always exists, so wt_path.exists() passes.

use super::helpers::*;
use crate::domain::entities::{InternalStatus, Project, ProjectId, Task};
use crate::domain::state_machine::context::{TaskContext, TaskServices};
use crate::domain::state_machine::{State, TaskStateMachine, TransitionHandler};
use crate::infrastructure::memory::{MemoryProjectRepository, MemoryTaskRepository};

// ──────────────────────────────────────────────────────────────────────────────
// Shared helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Seed in-memory repos with a Reviewing task and a project.
///
/// `worktree_path`: task.worktree_path value (None = not set, Some(path) = directory reference)
async fn setup_reviewing_task(
    worktree_path: Option<&str>,
) -> (
    crate::domain::entities::TaskId,
    Arc<MemoryTaskRepository>,
    Arc<MemoryProjectRepository>,
) {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());

    let mut task = Task::new(
        project_id.clone(),
        "Reviewer spawn failure test".to_string(),
    );
    task.internal_status = InternalStatus::Reviewing;
    task.worktree_path = worktree_path.map(|s| s.to_string());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = Project::new(
        "test-project".to_string(),
        "/tmp/nonexistent-reviewer-project".to_string(),
    );
    project.id = project_id;
    project_repo.create(project).await.unwrap();

    (task_id, task_repo, project_repo)
}

/// Build a machine with a WORKING MockChatService (default available = true).
///
/// Wires task_repo and project_repo into services so the worktree guard fires.
fn build_machine_with_working_chat(
    task_id: &crate::domain::entities::TaskId,
    task_repo: &Arc<MemoryTaskRepository>,
    project_repo: &Arc<MemoryProjectRepository>,
) -> TaskStateMachine {
    let services = TaskServices::new_mock()
        .with_task_repo(
            Arc::clone(task_repo) as Arc<dyn crate::domain::repositories::TaskRepository>
        )
        .with_project_repo(
            Arc::clone(project_repo) as Arc<dyn crate::domain::repositories::ProjectRepository>
        );
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    TaskStateMachine::new(context)
}

/// Build a machine with a FAILING MockChatService (available = false).
///
/// The worktree guard is bypassed when the worktree exists; the chat service
/// returns AgentNotAvailable which triggers record_reviewer_spawn_failure().
async fn build_machine_with_failing_chat(
    task_id: &crate::domain::entities::TaskId,
    task_repo: &Arc<MemoryTaskRepository>,
    project_repo: &Arc<MemoryProjectRepository>,
) -> TaskStateMachine {
    let chat_service = Arc::new(MockChatService::new());
    chat_service.set_available(false).await;

    let services = TaskServices::new(
        Arc::new(MockAgentSpawner::new()) as Arc<dyn AgentSpawner>,
        Arc::new(MockEventEmitter::new()) as Arc<dyn EventEmitter>,
        Arc::new(MockNotifier::new()) as Arc<dyn Notifier>,
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(MockReviewStarter::new()) as Arc<dyn ReviewStarter>,
        Arc::clone(&chat_service) as Arc<dyn crate::application::ChatService>,
    )
    .with_task_repo(Arc::clone(task_repo) as Arc<dyn crate::domain::repositories::TaskRepository>)
    .with_project_repo(
        Arc::clone(project_repo) as Arc<dyn crate::domain::repositories::ProjectRepository>
    );

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    TaskStateMachine::new(context)
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 1: worktree_path = Some("/nonexistent") → ReviewWorktreeMissing
// ──────────────────────────────────────────────────────────────────────────────

/// on_enter(Reviewing) returns ReviewWorktreeMissing when the worktree directory
/// is set but does not exist on disk.
///
/// Orchestration chain:
///   on_enter(&State::Reviewing)
///   → task_repo.get_by_id → task.worktree_path = Some("/tmp/nonexistent-reviewer-test-dir")
///   → Path::new(wt_path).exists() → false
///   → persist worktree_missing_at_review = true in metadata
///   → return Err(AppError::ReviewWorktreeMissing)
///
/// Verified: error variant + task metadata flag.
#[tokio::test]
async fn on_enter_reviewing_missing_worktree_returns_error() {
    let (task_id, task_repo, project_repo) =
        setup_reviewing_task(Some("/tmp/nonexistent-reviewer-test-dir")).await;

    let mut machine = build_machine_with_working_chat(&task_id, &task_repo, &project_repo);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::Reviewing).await;

    assert!(
        matches!(result, Err(crate::error::AppError::ReviewWorktreeMissing)),
        "Expected ReviewWorktreeMissing when worktree directory does not exist, got: {:?}",
        result
    );

    // Metadata flag must be persisted
    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    let meta: serde_json::Value = updated
        .metadata
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    assert_eq!(
        meta.get("worktree_missing_at_review")
            .and_then(|v| v.as_bool()),
        Some(true),
        "worktree_missing_at_review must be true in task metadata after ReviewWorktreeMissing. \
         Metadata: {:?}",
        updated.metadata
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 2: worktree_path = None → ReviewWorktreeMissing
// ──────────────────────────────────────────────────────────────────────────────

/// on_enter(Reviewing) returns ReviewWorktreeMissing when the task has no
/// worktree_path at all (None).
///
/// Orchestration chain:
///   on_enter(&State::Reviewing)
///   → task.worktree_path = None
///   → return Err(AppError::ReviewWorktreeMissing)
///
/// Verified: error variant is ReviewWorktreeMissing.
#[tokio::test]
async fn on_enter_reviewing_none_worktree_path_returns_error() {
    let (task_id, task_repo, project_repo) = setup_reviewing_task(None).await;

    let mut machine = build_machine_with_working_chat(&task_id, &task_repo, &project_repo);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::Reviewing).await;

    assert!(
        matches!(result, Err(crate::error::AppError::ReviewWorktreeMissing)),
        "Expected ReviewWorktreeMissing when worktree_path is None, got: {:?}",
        result
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 3: worktree exists but chat service fails → spawn failure metadata recorded
// ──────────────────────────────────────────────────────────────────────────────

/// on_enter(Reviewing) records reviewer_spawn_failure_count = 1 and
/// last_reviewer_spawn_error in task metadata when the chat service is unavailable.
///
/// Uses std::env::temp_dir() as the worktree_path because it always exists,
/// so the worktree guard passes and execution reaches the chat service call.
///
/// Orchestration chain:
///   on_enter(&State::Reviewing)
///   → task.worktree_path = Some(temp_dir) → dir.exists() = true
///   → conflict marker scan: no markers in temp_dir → passes
///   → chat_service.send_message(Review, ...) → Err(AgentNotAvailable)
///   → record_reviewer_spawn_failure(task_repo, task_id, error_msg)
///   → metadata["reviewer_spawn_failure_count"] = 1
///   → metadata["last_reviewer_spawn_error"] = "<error>"
///
/// Verified: reviewer_spawn_failure_count == 1 and last_reviewer_spawn_error is non-empty.
#[tokio::test]
async fn on_enter_reviewing_spawn_failure_records_metadata() {
    // temp_dir always exists — passes the wt_path.exists() check
    let temp_dir = std::env::temp_dir();
    let temp_dir_str = temp_dir.to_string_lossy().to_string();

    let (task_id, task_repo, project_repo) = setup_reviewing_task(Some(&temp_dir_str)).await;

    let mut machine = build_machine_with_failing_chat(&task_id, &task_repo, &project_repo).await;
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::Reviewing).await;

    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    let meta: serde_json::Value = updated
        .metadata
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    let failure_count = meta
        .get("reviewer_spawn_failure_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    assert_eq!(
        failure_count, 1,
        "reviewer_spawn_failure_count must be 1 after one spawn failure. \
         Got {}. Metadata: {:?}",
        failure_count, updated.metadata
    );

    let last_error = meta
        .get("last_reviewer_spawn_error")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    assert!(
        !last_error.is_empty(),
        "last_reviewer_spawn_error must be non-empty after a spawn failure. \
         Metadata: {:?}",
        updated.metadata
    );
}
