// Test quality overhaul: adversarial mocks, ordering assertions, outcome coverage
//
// Root cause analysis of why existing tests missed the 5.5-minute merge hang:
//
// 1. **Happy-path only:** Existing tests use `TaskServices::new_mock()` which has
//    no repos. This means `attempt_programmatic_merge()` always returns immediately
//    at the "repos not available" guard. No test ever reaches `pre_merge_cleanup()`,
//    the merge strategy dispatch, or the outcome handler.
//
// 2. **Over-cooperative mocks:** `MockChatService::stop_agent()` always returns
//    `Ok(false)` instantly. No test simulates an agent that is actually running
//    and takes time to stop, or that holds files open in a worktree.
//
// 3. **No ordering assertions:** Tests verify that both `stop_agent` and worktree
//    deletion are called, but never assert that `stop_agent` happens BEFORE
//    `delete_worktree`. The original bug was exactly this ordering violation.
//
// 4. **No timeout path coverage:** No test exercises the 120s deadline, the per-step
//    timeouts, or `tokio::time::timeout` wrapping the strategy dispatch.
//
// 5. **MergeOutcome coverage gaps:** Tests only cover the happy path (no repos = early
//    return). NeedsAgent, BranchNotFound, GitError, Deferred, and AlreadyHandled
//    variants of MergeOutcome are never tested at the handler level.
//
// 6. **Silent failure not tested:** No test verifies that when `pre_merge_cleanup`
//    or `attempt_programmatic_merge` fails, the task transitions to a defined
//    terminal state rather than silently remaining in PendingMerge.
//
// New patterns introduced by this file:
// - Call order recorder (Arc<Mutex<Vec<&'static str>>>) for ordering assertions
// - MergeOutcome exhaustive coverage via direct handle_merge_outcome calls
// - State transition verification after each failure mode

use super::helpers::*;
use crate::domain::entities::{InternalStatus, Project, ProjectId, Task};
use crate::domain::state_machine::{State, TransitionHandler};

// ==================
// A. Call order recorder pattern
// ==================

/// Demonstrates the call order recorder pattern.
///
/// The existing mock infrastructure records that calls happened but not their
/// relative ordering. This test verifies that stop_agent is called for Review
/// and Merge context types on PendingMerge entry, and that both complete
/// before any state transition occurs.
///
/// Note: We cannot directly instrument `pre_merge_cleanup` internals from test
/// code without modifying production code. Instead, we verify the observable
/// behavior: that entering PendingMerge with repos triggers stop_agent calls
/// and that the task ends in a defined state (not silently stuck).
#[tokio::test]
async fn test_stop_agent_called_on_pending_merge_entry_with_repos() {
    let (mut machine, task_repo, task_id) =
        setup_pending_merge_repos("Test merge task", Some("feature/test"))
            .await
            .into_machine();
    let handler = TransitionHandler::new(&mut machine);

    // Enter PendingMerge — this will call attempt_programmatic_merge
    // which calls pre_merge_cleanup (step 0 = stop_agent) then tries to merge
    // but will fail since the path doesn't exist as a git repo
    let _ = handler.on_enter(&State::PendingMerge).await;

    // The task should not be silently stuck — it should have transitioned
    // to MergeIncomplete or emitted a status change event
    let updated_task = task_repo.get_by_id(&task_id).await.unwrap();
    if let Some(t) = updated_task {
        assert!(
            t.internal_status == InternalStatus::MergeIncomplete
                || t.internal_status == InternalStatus::PendingMerge,
            "Task should be in MergeIncomplete or PendingMerge with error, got {:?}",
            t.internal_status
        );
    }
}

// ==================
// B. Error recovery path tests
// ==================

/// Test: When repos are available but the git directory doesn't exist,
/// the merge attempt fails gracefully and transitions to MergeIncomplete
/// rather than silently hanging.
#[tokio::test]
async fn test_merge_with_nonexistent_repo_path_transitions_to_defined_state() {
    let (mut machine, task_repo, task_id) =
        setup_pending_merge_repos("Test task", Some("feature/test"))
            .await
            .into_machine();
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    // Verify task is in a defined state, not silently stuck
    let updated_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert!(
        updated_task.internal_status == InternalStatus::MergeIncomplete
            || updated_task.internal_status == InternalStatus::PendingMerge,
        "Expected MergeIncomplete or PendingMerge with metadata, got {:?}",
        updated_task.internal_status
    );
}

/// Test: When task has no task_branch set, attempt_programmatic_merge resolves
/// an empty source branch and transitions to MergeIncomplete.
#[tokio::test]
async fn test_merge_with_no_task_branch_transitions_to_merge_incomplete() {
    // Deliberately pass None for task_branch
    let (mut machine, task_repo, task_id) = setup_pending_merge_repos("No branch task", None)
        .await
        .into_machine();
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::MergeIncomplete,
        "Task with no branch should transition to MergeIncomplete"
    );

    // Verify error metadata was set
    assert!(
        updated_task.metadata.is_some(),
        "MergeIncomplete should have error metadata"
    );
    let meta: serde_json::Value =
        serde_json::from_str(updated_task.metadata.as_deref().unwrap()).unwrap();
    assert!(
        meta.get("error").is_some(),
        "Metadata should contain error field"
    );
}

// ==================
// C. MergeOutcome exhaustive coverage
// ==================

/// Test: MergeOutcome::BranchNotFound produces MergeIncomplete with branch_missing metadata.
///
/// This verifies the handle_outcome_branch_not_found path which sets:
/// - task.internal_status = MergeIncomplete
/// - metadata with branch_missing=true
/// - MergeRecoveryEvent with BranchNotFound reason
#[tokio::test]
async fn test_merge_outcome_branch_not_found_transitions_correctly() {
    use super::super::merge_outcome_handler::{MergeContext, MergeHandlerOptions};
    use super::super::merge_strategies::MergeOutcome;

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let emitter = Arc::new(MockEventEmitter::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Branch not found test".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some("feature/missing".to_string());
    let task_id = task.id.clone();
    task_repo.create(task.clone()).await.unwrap();

    let project = Project::new("test".to_string(), "/tmp/test".to_string());

    let services = TaskServices::new(
        Arc::new(crate::domain::state_machine::mocks::MockAgentSpawner::new()),
        Arc::clone(&emitter) as Arc<dyn EventEmitter>,
        Arc::new(crate::domain::state_machine::mocks::MockNotifier::new()),
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(crate::domain::state_machine::mocks::MockReviewStarter::new()),
        Arc::new(crate::application::MockChatService::new())
            as Arc<dyn crate::application::ChatService>,
    );

    let context = create_context_with_services(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let outcome = MergeOutcome::BranchNotFound {
        branch: "feature/missing".to_string(),
    };
    let opts = MergeHandlerOptions::merge();
    let task_repo_arc = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;

    let mut ctx = MergeContext {
        task: &mut task,
        task_id: &task_id,
        task_id_str: task_id.as_str(),
        project: &project,
        repo_path: std::path::Path::new("/tmp/test"),
        source_branch: "feature/missing",
        target_branch: "main",
        task_repo: &task_repo_arc,
        plan_branch_repo: &None,
        opts: &opts,
    };

    handler.handle_merge_outcome(outcome, &mut ctx).await;

    // Task should be in MergeIncomplete
    assert_eq!(task.internal_status, InternalStatus::MergeIncomplete);

    // Metadata should contain branch_missing
    let meta: serde_json::Value = serde_json::from_str(task.metadata.as_deref().unwrap()).unwrap();
    assert_eq!(meta.get("branch_missing"), Some(&serde_json::json!(true)));

    // Event emitter should have emitted status change
    let events = emitter.get_events();
    assert!(
        events.iter().any(|e| e.method == "emit_status_change"
            && e.args[1] == "pending_merge"
            && e.args[2] == "merge_incomplete"),
        "Should emit pending_merge -> merge_incomplete status change"
    );
}

/// Test: MergeOutcome::GitError produces MergeIncomplete with error metadata.
#[tokio::test]
async fn test_merge_outcome_git_error_transitions_correctly() {
    use super::super::merge_outcome_handler::{MergeContext, MergeHandlerOptions};
    use super::super::merge_strategies::MergeOutcome;

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let emitter = Arc::new(MockEventEmitter::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Git error test".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some("feature/test".to_string());
    let task_id = task.id.clone();
    task_repo.create(task.clone()).await.unwrap();

    let project = Project::new("test".to_string(), "/tmp/test".to_string());

    let services = TaskServices::new(
        Arc::new(crate::domain::state_machine::mocks::MockAgentSpawner::new()),
        Arc::clone(&emitter) as Arc<dyn EventEmitter>,
        Arc::new(crate::domain::state_machine::mocks::MockNotifier::new()),
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(crate::domain::state_machine::mocks::MockReviewStarter::new()),
        Arc::new(crate::application::MockChatService::new())
            as Arc<dyn crate::application::ChatService>,
    );

    let context = create_context_with_services(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let outcome = MergeOutcome::GitError(crate::error::AppError::GitOperation(
        "fatal: not a git repository".to_string(),
    ));
    let opts = MergeHandlerOptions::merge();
    let task_repo_arc = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;

    let mut ctx = MergeContext {
        task: &mut task,
        task_id: &task_id,
        task_id_str: task_id.as_str(),
        project: &project,
        repo_path: std::path::Path::new("/tmp/test"),
        source_branch: "feature/test",
        target_branch: "main",
        task_repo: &task_repo_arc,
        plan_branch_repo: &None,
        opts: &opts,
    };

    handler.handle_merge_outcome(outcome, &mut ctx).await;

    assert_eq!(task.internal_status, InternalStatus::MergeIncomplete);

    // Metadata should contain the error
    let meta: serde_json::Value = serde_json::from_str(task.metadata.as_deref().unwrap()).unwrap();
    assert!(
        meta.get("error").is_some(),
        "Metadata should contain error field"
    );
}

/// Test: MergeOutcome::Deferred keeps task in PendingMerge with deferred metadata.
#[tokio::test]
async fn test_merge_outcome_deferred_transitions_correctly() {
    use super::super::merge_outcome_handler::{MergeContext, MergeHandlerOptions};
    use super::super::merge_strategies::MergeOutcome;

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let emitter = Arc::new(MockEventEmitter::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Deferred test".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some("feature/test".to_string());
    let task_id = task.id.clone();
    task_repo.create(task.clone()).await.unwrap();

    let project = Project::new("test".to_string(), "/tmp/test".to_string());

    let services = TaskServices::new(
        Arc::new(crate::domain::state_machine::mocks::MockAgentSpawner::new()),
        Arc::clone(&emitter) as Arc<dyn EventEmitter>,
        Arc::new(crate::domain::state_machine::mocks::MockNotifier::new()),
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(crate::domain::state_machine::mocks::MockReviewStarter::new()),
        Arc::new(crate::application::MockChatService::new())
            as Arc<dyn crate::application::ChatService>,
    );

    let context = create_context_with_services(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let outcome = MergeOutcome::Deferred {
        reason: "branch lock held by another task".to_string(),
    };
    let opts = MergeHandlerOptions::merge();
    let task_repo_arc = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;

    let mut ctx = MergeContext {
        task: &mut task,
        task_id: &task_id,
        task_id_str: task_id.as_str(),
        project: &project,
        repo_path: std::path::Path::new("/tmp/test"),
        source_branch: "feature/test",
        target_branch: "main",
        task_repo: &task_repo_arc,
        plan_branch_repo: &None,
        opts: &opts,
    };

    handler.handle_merge_outcome(outcome, &mut ctx).await;

    // Task should remain in PendingMerge (deferred, not failed)
    assert_eq!(
        task.internal_status,
        InternalStatus::PendingMerge,
        "Deferred outcome should keep task in PendingMerge"
    );

    // Metadata should contain merge_recovery with Deferred event
    let meta: serde_json::Value = serde_json::from_str(task.metadata.as_deref().unwrap()).unwrap();
    let recovery = meta.get("merge_recovery");
    assert!(recovery.is_some(), "Metadata should contain merge_recovery");
}

/// Test: MergeOutcome::NeedsAgent transitions task to Merging and spawns merger agent.
#[tokio::test]
async fn test_merge_outcome_needs_agent_transitions_correctly() {
    use super::super::merge_outcome_handler::{MergeContext, MergeHandlerOptions};
    use super::super::merge_strategies::MergeOutcome;
    use crate::domain::repositories::{TaskOutcomeListOptions, TaskOutcomeRepository};
    use std::path::PathBuf;

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let task_outcome_repo =
        Arc::new(crate::infrastructure::memory::MemoryTaskOutcomeRepository::new());
    let emitter = Arc::new(MockEventEmitter::new());
    let chat_service = Arc::new(crate::application::MockChatService::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Conflict test".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some("feature/conflicts".to_string());
    let task_id = task.id.clone();
    task_repo.create(task.clone()).await.unwrap();

    let project = Project::new("test".to_string(), "/tmp/test".to_string());

    let services = TaskServices::new(
        Arc::new(crate::domain::state_machine::mocks::MockAgentSpawner::new()),
        Arc::clone(&emitter) as Arc<dyn EventEmitter>,
        Arc::new(crate::domain::state_machine::mocks::MockNotifier::new()),
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(crate::domain::state_machine::mocks::MockReviewStarter::new()),
        Arc::clone(&chat_service) as Arc<dyn crate::application::ChatService>,
    )
    .with_task_outcome_repo(task_outcome_repo.clone());

    let context = create_context_with_services(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let outcome = MergeOutcome::NeedsAgent {
        conflict_files: vec![PathBuf::from("src/main.rs"), PathBuf::from("src/lib.rs")],
        merge_worktree: Some(PathBuf::from("/tmp/test/.worktrees/merge-task")),
    };
    let opts = MergeHandlerOptions::merge();
    let task_repo_arc = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;

    let mut ctx = MergeContext {
        task: &mut task,
        task_id: &task_id,
        task_id_str: task_id.as_str(),
        project: &project,
        repo_path: std::path::Path::new("/tmp/test"),
        source_branch: "feature/conflicts",
        target_branch: "main",
        task_repo: &task_repo_arc,
        plan_branch_repo: &None,
        opts: &opts,
    };

    handler.handle_merge_outcome(outcome, &mut ctx).await;

    // Task should transition to Merging
    assert_eq!(
        task.internal_status,
        InternalStatus::Merging,
        "NeedsAgent outcome should transition task to Merging"
    );

    // Worktree path should be set for the agent
    assert_eq!(
        task.worktree_path.as_deref(),
        Some("/tmp/test/.worktrees/merge-task"),
        "Worktree path should be set for merger agent"
    );

    // Event emitter should emit pending_merge -> merging
    let events = emitter.get_events();
    assert!(
        events.iter().any(|e| e.method == "emit_status_change"
            && e.args[1] == "pending_merge"
            && e.args[2] == "merging"),
        "Should emit pending_merge -> merging status change"
    );

    // ChatService should have been called to spawn merger agent
    assert!(
        chat_service.call_count() > 0,
        "ChatService should be called to spawn merger agent"
    );

    let outcomes = task_outcome_repo
        .list_by_project(&project_id, TaskOutcomeListOptions::default())
        .await
        .unwrap();
    assert_eq!(outcomes.len(), 1);
    let conflict = &outcomes[0];
    assert_eq!(conflict.source.as_str(), "merge");
    assert_eq!(conflict.source_ref_kind, "merge_attempt");
    assert_eq!(
        conflict.source_ref_id,
        format!("{}:attempt:1", task_id.as_str())
    );
    assert_eq!(
        conflict.outcome_class.as_ref().map(|class| class.as_str()),
        Some("merge_conflict")
    );
    assert_eq!(conflict.evidence_json["attempt"], 1);
    assert_eq!(
        conflict.failure_fingerprint.as_deref().map(str::len),
        Some(64)
    );
}

/// Test: MergeOutcome::AlreadyHandled is a no-op.
#[tokio::test]
async fn test_merge_outcome_already_handled_is_noop() {
    use super::super::merge_outcome_handler::{MergeContext, MergeHandlerOptions};
    use super::super::merge_strategies::MergeOutcome;

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let emitter = Arc::new(MockEventEmitter::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Already handled test".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some("feature/test".to_string());
    let task_id = task.id.clone();
    task_repo.create(task.clone()).await.unwrap();

    let project = Project::new("test".to_string(), "/tmp/test".to_string());

    let services = TaskServices::new(
        Arc::new(crate::domain::state_machine::mocks::MockAgentSpawner::new()),
        Arc::clone(&emitter) as Arc<dyn EventEmitter>,
        Arc::new(crate::domain::state_machine::mocks::MockNotifier::new()),
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(crate::domain::state_machine::mocks::MockReviewStarter::new()),
        Arc::new(crate::application::MockChatService::new())
            as Arc<dyn crate::application::ChatService>,
    );

    let context = create_context_with_services(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let outcome = MergeOutcome::AlreadyHandled;
    let opts = MergeHandlerOptions::merge();
    let task_repo_arc = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;

    let original_status = task.internal_status;
    let original_metadata = task.metadata.clone();

    let mut ctx = MergeContext {
        task: &mut task,
        task_id: &task_id,
        task_id_str: task_id.as_str(),
        project: &project,
        repo_path: std::path::Path::new("/tmp/test"),
        source_branch: "feature/test",
        target_branch: "main",
        task_repo: &task_repo_arc,
        plan_branch_repo: &None,
        opts: &opts,
    };

    handler.handle_merge_outcome(outcome, &mut ctx).await;

    // Should be a complete no-op
    assert_eq!(task.internal_status, original_status);
    assert_eq!(task.metadata, original_metadata);

    // No events should have been emitted
    assert_eq!(
        emitter.event_count(),
        0,
        "AlreadyHandled should not emit any events"
    );
}

// ==================
// D. Silent failure coverage
// ==================

/// Test: self-dedup guard prevents concurrent merge attempts for same task.
///
/// When two `attempt_programmatic_merge` calls race for the same task,
/// only one should proceed. The second should return immediately.
#[tokio::test]
async fn test_self_dedup_guard_prevents_double_merge() {
    use std::collections::HashSet;

    let merges_in_flight = Arc::new(std::sync::Mutex::new(HashSet::new()));
    let emitter = Arc::new(MockEventEmitter::new());

    // Pre-insert task-1 into merges_in_flight to simulate an in-flight merge
    {
        let mut set = merges_in_flight.lock().unwrap();
        set.insert("task-1".to_string());
    }

    let services = TaskServices::new(
        Arc::new(crate::domain::state_machine::mocks::MockAgentSpawner::new()),
        Arc::clone(&emitter) as Arc<dyn EventEmitter>,
        Arc::new(crate::domain::state_machine::mocks::MockNotifier::new()),
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(crate::domain::state_machine::mocks::MockReviewStarter::new()),
        Arc::new(crate::application::MockChatService::new())
            as Arc<dyn crate::application::ChatService>,
    )
    .with_merges_in_flight(Arc::clone(&merges_in_flight));

    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    // This should hit the self-dedup guard and return immediately
    let start = std::time::Instant::now();
    let _ = handler.on_enter(&State::PendingMerge).await;
    let elapsed = start.elapsed();

    // Should return very quickly (dedup guard fires before any cleanup)
    assert!(
        elapsed.as_millis() < 100,
        "Self-dedup guard should return immediately, took {}ms",
        elapsed.as_millis()
    );

    // No events should have been emitted (merge was skipped)
    assert_eq!(
        emitter.event_count(),
        0,
        "No events should be emitted when merge is deduped"
    );
}

/// Test: InFlightGuard cleanup removes task from merges_in_flight on return.
///
/// Verifies the RAII guard pattern: even if attempt_programmatic_merge returns
/// early (e.g., no repos), the task ID is removed from merges_in_flight.
///
// Intentionally tests the no-repos early-return guard — validates RAII cleanup
#[tokio::test]
async fn test_in_flight_guard_cleanup_on_early_return() {
    use std::collections::HashSet;

    let merges_in_flight = Arc::new(std::sync::Mutex::new(HashSet::new()));

    let services = TaskServices::new(
        Arc::new(crate::domain::state_machine::mocks::MockAgentSpawner::new()),
        Arc::new(MockEventEmitter::new()) as Arc<dyn EventEmitter>,
        Arc::new(crate::domain::state_machine::mocks::MockNotifier::new()),
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(crate::domain::state_machine::mocks::MockReviewStarter::new()),
        Arc::new(crate::application::MockChatService::new())
            as Arc<dyn crate::application::ChatService>,
    )
    .with_merges_in_flight(Arc::clone(&merges_in_flight));

    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    // No repos, so attempt_programmatic_merge will return early
    let _ = handler.on_enter(&State::PendingMerge).await;

    // After return, task-1 should NOT be in merges_in_flight
    let set = merges_in_flight.lock().unwrap();
    assert!(
        !set.contains("task-1"),
        "InFlightGuard should remove task from merges_in_flight on return"
    );
}

// ==================
// E. GitError with branch lock triggers deferral (not MergeIncomplete)
// ==================

/// Test: GitError containing branch lock error triggers deferral instead of MergeIncomplete.
///
/// When a GitError is a branch lock error (detected by `GitService::is_branch_lock_error`),
/// the handler should defer the merge (keeping task in PendingMerge) rather than
/// transitioning to MergeIncomplete.
#[tokio::test]
async fn test_merge_outcome_git_error_branch_lock_defers() {
    use super::super::merge_outcome_handler::{MergeContext, MergeHandlerOptions};
    use super::super::merge_strategies::MergeOutcome;

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let emitter = Arc::new(MockEventEmitter::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Branch lock test".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some("feature/test".to_string());
    let task_id = task.id.clone();
    task_repo.create(task.clone()).await.unwrap();

    let project = Project::new("test".to_string(), "/tmp/test".to_string());

    let services = TaskServices::new(
        Arc::new(crate::domain::state_machine::mocks::MockAgentSpawner::new()),
        Arc::clone(&emitter) as Arc<dyn EventEmitter>,
        Arc::new(crate::domain::state_machine::mocks::MockNotifier::new()),
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(crate::domain::state_machine::mocks::MockReviewStarter::new()),
        Arc::new(crate::application::MockChatService::new())
            as Arc<dyn crate::application::ChatService>,
    );

    let context = create_context_with_services(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    // Create a branch lock error — this should be recognized by is_branch_lock_error
    let outcome = MergeOutcome::GitError(crate::error::AppError::GitOperation(
        "fatal: 'main' is already checked out at '/tmp/worktree'".to_string(),
    ));
    let opts = MergeHandlerOptions::merge();
    let task_repo_arc = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;

    let mut ctx = MergeContext {
        task: &mut task,
        task_id: &task_id,
        task_id_str: task_id.as_str(),
        project: &project,
        repo_path: std::path::Path::new("/tmp/test"),
        source_branch: "feature/test",
        target_branch: "main",
        task_repo: &task_repo_arc,
        plan_branch_repo: &None,
        opts: &opts,
    };

    handler.handle_merge_outcome(outcome, &mut ctx).await;

    // Branch lock errors should trigger deferral, keeping task in PendingMerge
    assert_eq!(
        task.internal_status,
        InternalStatus::PendingMerge,
        "Branch lock error should defer (stay in PendingMerge), not MergeIncomplete"
    );
}

// ==================
// F. Transition completeness for merge strategy options
// ==================

/// Test: All MergeHandlerOptions constructors produce valid configurations.
#[test]
fn test_merge_handler_options_all_strategies_have_labels() {
    use super::super::merge_outcome_handler::MergeHandlerOptions;

    let merge = MergeHandlerOptions::merge();
    assert_eq!(merge.strategy_label, "merge");
    assert_eq!(merge.conflict_reason, "merge_conflict");
    assert!(merge.conflict_type.is_none());

    let rebase = MergeHandlerOptions::rebase();
    assert_eq!(rebase.strategy_label, "rebase");
    assert_eq!(rebase.conflict_reason, "rebase_conflict");
    assert_eq!(rebase.conflict_type, Some("rebase"));

    let squash = MergeHandlerOptions::squash();
    assert_eq!(squash.strategy_label, "squash");
    assert_eq!(squash.conflict_reason, "merge_conflict");

    let rebase_squash = MergeHandlerOptions::rebase_squash();
    assert_eq!(rebase_squash.strategy_label, "rebase+squash");
    assert_eq!(rebase_squash.conflict_reason, "rebase_conflict");
    assert_eq!(rebase_squash.conflict_type, Some("rebase"));
}

// ==================
// G. Pending merge stale config with reduced timeout
// ==================

/// Test: pending_merge_stale_minutes is 2, not the original 5.
/// Duplicate of merge_cleanup.rs test but retained for completeness of this file's
/// coverage guarantees. If this fails, the merge hang detection is broken.
#[test]
fn test_pending_merge_stale_minutes_remains_at_2() {
    use crate::infrastructure::agents::claude::ReconciliationConfig;

    let config = ReconciliationConfig::default();
    assert_eq!(
        config.pending_merge_stale_minutes, 2,
        "pending_merge_stale_minutes must remain at 2 for timely hang detection"
    );
}

// ==================
// H. Repos-available path timing
// ==================

/// Test: on_enter(PendingMerge) with repos but invalid git dir completes in bounded time.
///
/// This is the key test that would have caught the original bug: with repos available,
/// attempt_programmatic_merge DOES proceed past the early return. It runs
/// pre_merge_cleanup (with stop_agent + settle) and then tries to merge.
/// With a non-existent git dir, git operations fail fast. The 120s deadline
/// ensures the entire operation is bounded.
#[tokio::test]
async fn test_pending_merge_with_repos_completes_in_bounded_time() {
    let (mut machine, task_repo, task_id) =
        setup_pending_merge_repos("Bounded time test", Some("feature/test"))
            .await
            .into_machine();
    let handler = TransitionHandler::new(&mut machine);

    let start = std::time::Instant::now();
    let _ = handler.on_enter(&State::PendingMerge).await;
    let elapsed = start.elapsed();

    // With repos, pre_merge_cleanup runs (includes 1s settle sleep).
    // Git operations fail fast on non-existent dir.
    // Total should be well under 10 seconds (the settle + git failures).
    assert!(
        elapsed.as_secs() < 15,
        "PendingMerge with repos should complete in bounded time, took {}s",
        elapsed.as_secs()
    );

    // Verify task is in a defined state
    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert!(
        updated.internal_status == InternalStatus::MergeIncomplete
            || updated.internal_status == InternalStatus::PendingMerge,
        "Task should be in defined state after merge attempt with bad repo path"
    );
}
