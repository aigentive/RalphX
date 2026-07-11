// Tests for was_queued guard in on_enter for all 4 agent-spawning states.
//
// RC#2 fix (updated): when chat_service.send_message() returns Ok(was_queued: true),
// on_enter should log info and return Ok(()) (no-op), NOT record a spawn failure.
// Gate 2 now returns Ok(was_queued: true) instead of Err(AgentAlreadyRunning).
//
// Per CLAUDE.md rule 1.5: MemoryTaskRepository + MockChatService.
// Mock agent spawning only → verify call_count() and absence of spawn_failure metadata.

use super::helpers::*;
use crate::domain::entities::{InternalStatus, MergeRecoveryMetadata, Project, ProjectId, Task};
use crate::domain::state_machine::services::TaskScheduler;
use crate::domain::state_machine::{State, TransitionHandler};

// ──────────────────────────────────────────────────────────────────────────────
// Shared helper: TaskServices with a MockChatService that returns
// AgentAlreadyRunning after the first successful call.
// ──────────────────────────────────────────────────────────────────────────────

async fn make_services_already_running_after_1(
    task_repo: Arc<MemoryTaskRepository>,
    project_repo: Arc<MemoryProjectRepository>,
) -> (Arc<MockChatService>, TaskServices) {
    let chat_service = Arc::new(MockChatService::new());
    chat_service.set_already_running_after(1).await;
    let services = TaskServices::new(
        Arc::new(MockAgentSpawner::new()) as Arc<dyn AgentSpawner>,
        Arc::new(MockEventEmitter::new()) as Arc<dyn EventEmitter>,
        Arc::new(MockNotifier::new()) as Arc<dyn Notifier>,
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(MockReviewStarter::new()) as Arc<dyn ReviewStarter>,
        Arc::clone(&chat_service) as Arc<dyn ChatService>,
    )
    .with_task_scheduler(Arc::new(MockTaskScheduler::new()) as Arc<dyn TaskScheduler>)
    .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
    .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>);
    (chat_service, services)
}

fn build_services_from_chat(
    chat_service: &Arc<MockChatService>,
    task_repo: &Arc<MemoryTaskRepository>,
    project_repo: &Arc<MemoryProjectRepository>,
) -> TaskServices {
    TaskServices::new(
        Arc::new(MockAgentSpawner::new()) as Arc<dyn AgentSpawner>,
        Arc::new(MockEventEmitter::new()) as Arc<dyn EventEmitter>,
        Arc::new(MockNotifier::new()) as Arc<dyn Notifier>,
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(MockReviewStarter::new()) as Arc<dyn ReviewStarter>,
        Arc::clone(chat_service) as Arc<dyn ChatService>,
    )
    .with_task_scheduler(Arc::new(MockTaskScheduler::new()) as Arc<dyn TaskScheduler>)
    .with_task_repo(Arc::clone(task_repo) as Arc<dyn TaskRepository>)
    .with_project_repo(Arc::clone(project_repo) as Arc<dyn ProjectRepository>)
}

// ──────────────────────────────────────────────────────────────────────────────
// Test: on_enter(Merging) double call → call_count == 1, no spawn_failure
// ──────────────────────────────────────────────────────────────────────────────

/// Double on_enter(Merging): second call returns AgentAlreadyRunning → no-op.
///
/// Verifies:
///   1. First on_enter(Merging) succeeds (agent spawned, call_count == 1)
///   2. Second on_enter(Merging) returns Ok (no error propagated)
///   3. call_count == 2 (both calls reached chat_service) but only 1 agent spawned
///   4. No AttemptFailed / spawn_failure metadata written (record_merger_spawn_failure skipped)
#[tokio::test]
async fn merging_double_on_enter_agent_already_running_is_noop() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(
        project_id.clone(),
        "Merging double on_enter test".to_string(),
    );
    task.internal_status = InternalStatus::Merging;
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = Project::new(
        "test-project".to_string(),
        "/tmp/nonexistent-already-running-merge".to_string(),
    );
    project.id = project_id;
    project_repo.create(project).await.unwrap();

    let (chat_service, _) =
        make_services_already_running_after_1(Arc::clone(&task_repo), Arc::clone(&project_repo))
            .await;

    // First on_enter(Merging): succeeds
    {
        let services = build_services_from_chat(&chat_service, &task_repo, &project_repo);
        let context = TaskContext::new(task_id.as_str(), "proj-1", services);
        let mut machine = TaskStateMachine::new(context);
        let handler = TransitionHandler::new(&mut machine);
        let result = handler.on_enter(&State::Merging).await;
        assert!(
            result.is_ok(),
            "First on_enter(Merging) should succeed: {:?}",
            result.err()
        );
    }

    assert_eq!(
        chat_service.call_count(),
        1,
        "After first on_enter, call_count should be 1"
    );

    // Second on_enter(Merging): AgentAlreadyRunning → should be no-op (Ok)
    {
        let services = build_services_from_chat(&chat_service, &task_repo, &project_repo);
        let context = TaskContext::new(task_id.as_str(), "proj-1", services);
        let mut machine = TaskStateMachine::new(context);
        let handler = TransitionHandler::new(&mut machine);
        let result = handler.on_enter(&State::Merging).await;
        assert!(
            result.is_ok(),
            "Second on_enter(Merging) must return Ok (no-op), not propagate AgentAlreadyRunning: {:?}",
            result.err()
        );
    }

    assert_eq!(
        chat_service.call_count(),
        2,
        "Both on_enter calls should have reached chat_service (call_count == 2)"
    );

    // Key assertion: no spawn_failure metadata written (record_merger_spawn_failure was skipped)
    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    let recovery =
        MergeRecoveryMetadata::from_task_metadata(updated.metadata.as_deref()).unwrap_or(None);

    if let Some(ref r) = recovery {
        let attempt_failed_count = r
            .events
            .iter()
            .filter(|e| {
                matches!(
                    e.kind,
                    crate::domain::entities::MergeRecoveryEventKind::AttemptFailed
                )
            })
            .count();
        assert_eq!(
            attempt_failed_count, 0,
            "AgentAlreadyRunning must NOT record AttemptFailed events. Got {}. Metadata: {:?}",
            attempt_failed_count, updated.metadata,
        );
    }
    // If recovery is None, no merge_recovery metadata was written — that's correct.
}

// ──────────────────────────────────────────────────────────────────────────────
// Test: on_enter(Reviewing) double call → call_count == 1, returns Ok
// ──────────────────────────────────────────────────────────────────────────────

/// Double on_enter(Reviewing): second call returns AgentAlreadyRunning → no-op.
///
/// Verifies:
///   1. First on_enter(Reviewing) succeeds (agent spawned, call_count == 1)
///   2. Second on_enter(Reviewing) returns Ok (no error propagated)
///   3. call_count == 2 (both calls reached chat_service) but only 1 agent spawned
#[tokio::test]
async fn reviewing_double_on_enter_agent_already_running_is_noop() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(
        project_id.clone(),
        "Reviewing double on_enter test".to_string(),
    );
    task.internal_status = InternalStatus::Reviewing;
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    // Set worktree_path to an existing dir so the worktree guard passes
    let temp_dir = std::env::temp_dir().to_string_lossy().to_string();
    let mut task_stored = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    task_stored.worktree_path = Some(temp_dir);
    task_repo.update(&task_stored).await.unwrap();

    let mut project = Project::new(
        "test-project".to_string(),
        "/tmp/nonexistent-already-running-review".to_string(),
    );
    project.id = project_id;
    project_repo.create(project).await.unwrap();

    let (chat_service, _) =
        make_services_already_running_after_1(Arc::clone(&task_repo), Arc::clone(&project_repo))
            .await;

    // First on_enter(Reviewing): succeeds
    {
        let services = build_services_from_chat(&chat_service, &task_repo, &project_repo);
        let context = TaskContext::new(task_id.as_str(), "proj-1", services);
        let mut machine = TaskStateMachine::new(context);
        let handler = TransitionHandler::new(&mut machine);
        let result = handler.on_enter(&State::Reviewing).await;
        assert!(
            result.is_ok(),
            "First on_enter(Reviewing) should succeed: {:?}",
            result.err()
        );
    }

    assert_eq!(
        chat_service.call_count(),
        1,
        "After first on_enter, call_count should be 1"
    );

    // Second on_enter(Reviewing): AgentAlreadyRunning → should be no-op (Ok)
    {
        let services = build_services_from_chat(&chat_service, &task_repo, &project_repo);
        let context = TaskContext::new(task_id.as_str(), "proj-1", services);
        let mut machine = TaskStateMachine::new(context);
        let handler = TransitionHandler::new(&mut machine);
        let result = handler.on_enter(&State::Reviewing).await;
        assert!(
            result.is_ok(),
            "Second on_enter(Reviewing) must return Ok (no-op), not propagate AgentAlreadyRunning: {:?}",
            result.err()
        );
    }

    assert_eq!(
        chat_service.call_count(),
        2,
        "Both on_enter calls should have reached chat_service (call_count == 2)"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Helpers: services without repos (Executing/ReExecuting skip git operations
// when no task_repo/project_repo is wired into TaskServices)
// ──────────────────────────────────────────────────────────────────────────────

async fn make_services_queued_no_repos() -> (Arc<MockChatService>, TaskServices) {
    // set_already_running_after(0) → first call returns Ok(was_queued: true)
    let chat_service = Arc::new(MockChatService::new());
    chat_service.set_already_running_after(0).await;
    let services = TaskServices::new(
        Arc::new(MockAgentSpawner::new()) as Arc<dyn AgentSpawner>,
        Arc::new(MockEventEmitter::new()) as Arc<dyn EventEmitter>,
        Arc::new(MockNotifier::new()) as Arc<dyn Notifier>,
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(MockReviewStarter::new()) as Arc<dyn ReviewStarter>,
        Arc::clone(&chat_service) as Arc<dyn ChatService>,
    )
    .with_task_scheduler(Arc::new(MockTaskScheduler::new()) as Arc<dyn TaskScheduler>);
    (chat_service, services)
}

async fn make_services_unavailable_no_repos() -> (Arc<MockChatService>, TaskServices) {
    // set_available(false) → all calls return Err(AgentNotAvailable)
    let chat_service = Arc::new(MockChatService::new());
    chat_service.set_available(false).await;
    let services = TaskServices::new(
        Arc::new(MockAgentSpawner::new()) as Arc<dyn AgentSpawner>,
        Arc::new(MockEventEmitter::new()) as Arc<dyn EventEmitter>,
        Arc::new(MockNotifier::new()) as Arc<dyn Notifier>,
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(MockReviewStarter::new()) as Arc<dyn ReviewStarter>,
        Arc::clone(&chat_service) as Arc<dyn ChatService>,
    )
    .with_task_scheduler(Arc::new(MockTaskScheduler::new()) as Arc<dyn TaskScheduler>);
    (chat_service, services)
}

async fn make_services_normal_no_repos() -> (Arc<MockChatService>, TaskServices) {
    // Normal mock — first call returns Ok(was_queued: false)
    let chat_service = Arc::new(MockChatService::new());
    let services = TaskServices::new(
        Arc::new(MockAgentSpawner::new()) as Arc<dyn AgentSpawner>,
        Arc::new(MockEventEmitter::new()) as Arc<dyn EventEmitter>,
        Arc::new(MockNotifier::new()) as Arc<dyn Notifier>,
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(MockReviewStarter::new()) as Arc<dyn ReviewStarter>,
        Arc::clone(&chat_service) as Arc<dyn ChatService>,
    )
    .with_task_scheduler(Arc::new(MockTaskScheduler::new()) as Arc<dyn TaskScheduler>);
    (chat_service, services)
}

// ──────────────────────────────────────────────────────────────────────────────
// Executing: positive (was_queued), negative (genuine error), normal-spawn
// ──────────────────────────────────────────────────────────────────────────────

/// on_enter(Executing) with was_queued: true → no-op (Ok).
#[tokio::test]
async fn executing_was_queued_is_noop() {
    let (chat_service, services) = make_services_queued_no_repos().await;
    let context = TaskContext::new("task-exec-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);
    let result = handler.on_enter(&State::Executing).await;
    assert!(
        result.is_ok(),
        "on_enter(Executing) with was_queued must return Ok (no-op): {:?}",
        result.err()
    );
    assert_eq!(
        chat_service.call_count(),
        1,
        "send_message must be called once"
    );
}

/// on_enter(Executing) with genuine Err → error is propagated as Err.
#[tokio::test]
async fn executing_genuine_error_is_propagated() {
    let (chat_service, services) = make_services_unavailable_no_repos().await;
    let context = TaskContext::new("task-exec-err", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);
    let result = handler.on_enter(&State::Executing).await;
    assert!(
        result.is_err(),
        "on_enter(Executing) with genuine Err must propagate error, not silently swallow it"
    );
    assert_eq!(
        chat_service.call_count(),
        1,
        "send_message must be attempted once"
    );
}

/// on_enter(Executing) with was_queued: false → normal spawn, returns Ok.
#[tokio::test]
async fn executing_normal_spawn_succeeds() {
    let (chat_service, services) = make_services_normal_no_repos().await;
    let context = TaskContext::new("task-exec-ok", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);
    let result = handler.on_enter(&State::Executing).await;
    assert!(
        result.is_ok(),
        "on_enter(Executing) normal spawn must return Ok: {:?}",
        result.err()
    );
    assert_eq!(
        chat_service.call_count(),
        1,
        "send_message must be called once for normal spawn"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// ReExecuting: positive (was_queued), negative (genuine error), normal-spawn
// ──────────────────────────────────────────────────────────────────────────────

/// on_enter(ReExecuting) with was_queued: true → no-op (Ok).
#[tokio::test]
async fn re_executing_was_queued_is_noop() {
    let (chat_service, services) = make_services_queued_no_repos().await;
    let context = TaskContext::new("task-reexec-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);
    let result = handler.on_enter(&State::ReExecuting).await;
    assert!(
        result.is_ok(),
        "on_enter(ReExecuting) with was_queued must return Ok (no-op): {:?}",
        result.err()
    );
    assert_eq!(
        chat_service.call_count(),
        1,
        "send_message must be called once"
    );
}

/// on_enter(ReExecuting) with genuine Err → error is propagated as Err.
#[tokio::test]
async fn re_executing_genuine_error_is_propagated() {
    let (chat_service, services) = make_services_unavailable_no_repos().await;
    let context = TaskContext::new("task-reexec-err", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);
    let result = handler.on_enter(&State::ReExecuting).await;
    assert!(
        result.is_err(),
        "on_enter(ReExecuting) with genuine Err must propagate error, not silently swallow it"
    );
    assert_eq!(
        chat_service.call_count(),
        1,
        "send_message must be attempted once"
    );
}

/// on_enter(ReExecuting) with was_queued: false → normal spawn, returns Ok.
#[tokio::test]
async fn re_executing_normal_spawn_succeeds() {
    let (chat_service, services) = make_services_normal_no_repos().await;
    let context = TaskContext::new("task-reexec-ok", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);
    let result = handler.on_enter(&State::ReExecuting).await;
    assert!(
        result.is_ok(),
        "on_enter(ReExecuting) normal spawn must return Ok: {:?}",
        result.err()
    );
    assert_eq!(
        chat_service.call_count(),
        1,
        "send_message must be called once for normal spawn"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Negative tests: Reviewing and Merging — genuine Err is NOT propagated (logged only)
// ──────────────────────────────────────────────────────────────────────────────

/// on_enter(Reviewing) with genuine Err → handler returns Ok (error logged, not propagated).
/// Guards against regressions that accidentally convert Err arm to was_queued no-op.
#[tokio::test]
async fn reviewing_genuine_error_is_not_propagated() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(
        project_id.clone(),
        "Reviewing genuine error test".to_string(),
    );
    task.internal_status = InternalStatus::Reviewing;
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    // Set worktree_path to an existing dir so the worktree guard passes and
    // execution reaches send_message (which will fail due to set_available(false))
    let temp_dir = std::env::temp_dir().to_string_lossy().to_string();
    let mut task_stored = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    task_stored.worktree_path = Some(temp_dir);
    task_repo.update(&task_stored).await.unwrap();

    let mut project = Project::new(
        "test-project".to_string(),
        "/tmp/nonexistent-reviewing-err".to_string(),
    );
    project.id = project_id;
    project_repo.create(project).await.unwrap();

    let chat_service = Arc::new(MockChatService::new());
    chat_service.set_available(false).await;

    let services = build_services_from_chat(&chat_service, &task_repo, &project_repo);
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);
    let result = handler.on_enter(&State::Reviewing).await;
    assert!(
        result.is_ok(),
        "on_enter(Reviewing) must not propagate spawn errors (errors are logged only): {:?}",
        result.err()
    );
    assert_eq!(
        chat_service.call_count(),
        1,
        "send_message must be attempted once"
    );
}

/// on_enter(Merging) with genuine Err → handler returns Ok (error logged + recorded, not propagated).
/// Guards against regressions that accidentally convert Err arm to was_queued no-op.
#[tokio::test]
async fn merging_genuine_error_is_not_propagated() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Merging genuine error test".to_string());
    task.internal_status = InternalStatus::Merging;
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = Project::new(
        "test-project".to_string(),
        "/tmp/nonexistent-merging-err".to_string(),
    );
    project.id = project_id;
    project_repo.create(project).await.unwrap();

    let chat_service = Arc::new(MockChatService::new());
    chat_service.set_available(false).await;

    let services = build_services_from_chat(&chat_service, &task_repo, &project_repo);
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);
    let result = handler.on_enter(&State::Merging).await;
    assert!(
        result.is_ok(),
        "on_enter(Merging) must not propagate spawn errors (errors are logged + recorded only): {:?}",
        result.err()
    );
    assert_eq!(
        chat_service.call_count(),
        1,
        "send_message must be attempted once"
    );
}
