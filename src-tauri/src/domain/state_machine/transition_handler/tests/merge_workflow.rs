// Merge workflow tests: deferred retry, non-blocking retry, background execution, blocking isolation
//
// Tests for the merge state machine workflow including:
// - Deferred merge retry on PendingMerge/Merging exits
// - Non-blocking retry_merge command latency
// - Background execution correctness (state ordering, terminal state, retry)
// - Blocking isolation (concurrent operations, execution state)

use super::helpers::*;
use crate::domain::state_machine::{State, TaskEvent, TransitionHandler, TransitionResult};

// ==================
// Deferred merge retry tests
// ==================

#[tokio::test]
async fn test_exiting_pending_merge_triggers_retry_deferred_merges() {
    let (mut machine, scheduler) = new_machine_with_scheduler("task-1", "proj-1");
    let handler = TransitionHandler::new(&mut machine);

    // Transition from PendingMerge to Merged (successful merge)
    handler.on_exit(&State::PendingMerge, &State::Merged).await;

    // Wait for spawned task to call try_retry_deferred_merges
    let sched = Arc::clone(&scheduler);
    assert!(
        wait_for_condition(
            || {
                let s = Arc::clone(&sched);
                async move {
                    s.get_calls()
                        .iter()
                        .any(|c| c.method == "try_retry_deferred_merges")
                }
            },
            5000
        )
        .await,
        "Expected try_retry_deferred_merges to be called"
    );

    let calls = scheduler.get_calls();
    let retry_calls: Vec<_> = calls
        .iter()
        .filter(|c| c.method == "try_retry_deferred_merges")
        .collect();

    assert_eq!(
        retry_calls.len(),
        1,
        "Expected exactly one try_retry_deferred_merges call"
    );
    assert_eq!(
        retry_calls[0].args,
        vec!["proj-1"],
        "Expected project_id to be passed"
    );
}

#[tokio::test]
async fn test_exiting_pending_merge_to_merge_incomplete_triggers_retry() {
    let (mut machine, scheduler) = new_machine_with_scheduler("task-1", "proj-1");
    let handler = TransitionHandler::new(&mut machine);

    // Transition from PendingMerge to MergeIncomplete (failed merge)
    handler
        .on_exit(&State::PendingMerge, &State::MergeIncomplete)
        .await;

    // Wait for spawned task to call try_retry_deferred_merges
    let sched = Arc::clone(&scheduler);
    assert!(
        wait_for_condition(
            || {
                let s = Arc::clone(&sched);
                async move {
                    s.get_calls()
                        .iter()
                        .any(|c| c.method == "try_retry_deferred_merges")
                }
            },
            5000
        )
        .await,
        "Expected retry even on merge_incomplete"
    );

    let calls = scheduler.get_calls();
    let retry_calls: Vec<_> = calls
        .iter()
        .filter(|c| c.method == "try_retry_deferred_merges")
        .collect();

    assert_eq!(
        retry_calls.len(),
        1,
        "Expected retry even on merge_incomplete"
    );
    assert_eq!(retry_calls[0].args, vec!["proj-1"]);
}

#[tokio::test]
async fn test_exiting_merging_to_merged_triggers_retry() {
    let (mut machine, scheduler) = new_machine_with_scheduler("task-1", "proj-1");
    let handler = TransitionHandler::new(&mut machine);

    // Transition from Merging to Merged (manual merge completion)
    handler.on_exit(&State::Merging, &State::Merged).await;

    // Wait for spawned task to call try_retry_deferred_merges
    let sched = Arc::clone(&scheduler);
    assert!(
        wait_for_condition(
            || {
                let s = Arc::clone(&sched);
                async move {
                    s.get_calls()
                        .iter()
                        .any(|c| c.method == "try_retry_deferred_merges")
                }
            },
            5000
        )
        .await,
        "Expected try_retry_deferred_merges to be called"
    );

    let calls = scheduler.get_calls();
    let retry_calls: Vec<_> = calls
        .iter()
        .filter(|c| c.method == "try_retry_deferred_merges")
        .collect();

    assert_eq!(retry_calls.len(), 1);
    assert_eq!(retry_calls[0].args, vec!["proj-1"]);
}

#[tokio::test]
async fn test_exiting_merging_to_merge_incomplete_triggers_retry() {
    let (mut machine, scheduler) = new_machine_with_scheduler("task-1", "proj-1");
    let handler = TransitionHandler::new(&mut machine);

    // Transition from Merging to MergeIncomplete (merge failed during conflict resolution)
    handler
        .on_exit(&State::Merging, &State::MergeIncomplete)
        .await;

    // Wait for spawned task to call try_retry_deferred_merges
    let sched = Arc::clone(&scheduler);
    assert!(
        wait_for_condition(
            || {
                let s = Arc::clone(&sched);
                async move {
                    s.get_calls()
                        .iter()
                        .any(|c| c.method == "try_retry_deferred_merges")
                }
            },
            5000
        )
        .await,
        "Expected try_retry_deferred_merges to be called"
    );

    let calls = scheduler.get_calls();
    let retry_calls: Vec<_> = calls
        .iter()
        .filter(|c| c.method == "try_retry_deferred_merges")
        .collect();

    assert_eq!(retry_calls.len(), 1);
    assert_eq!(retry_calls[0].args, vec!["proj-1"]);
}

#[tokio::test]
async fn test_exiting_other_states_does_not_trigger_retry() {
    let (mut machine, scheduler) = new_machine_with_scheduler("task-1", "proj-1");
    let handler = TransitionHandler::new(&mut machine);

    // Transition from Ready to Executing (normal execution start)
    handler.on_exit(&State::Ready, &State::Executing).await;

    // Wait briefly to confirm no spawned tasks fire (negative test)
    let sched = Arc::clone(&scheduler);
    let _ = wait_for_condition(
        || {
            let s = Arc::clone(&sched);
            async move {
                s.get_calls()
                    .iter()
                    .any(|c| c.method == "try_retry_deferred_merges")
            }
        },
        500,
    )
    .await;

    // Verify try_retry_deferred_merges was NOT called for non-merge states
    let calls = scheduler.get_calls();
    let retry_calls: Vec<_> = calls
        .iter()
        .filter(|c| c.method == "try_retry_deferred_merges")
        .collect();

    assert_eq!(
        retry_calls.len(),
        0,
        "Expected no retry calls for non-merge state transitions"
    );
}

#[tokio::test]
async fn test_no_scheduler_does_not_panic_on_exit() {
    // Create services without a scheduler
    let services = TaskServices::new_mock();
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);

    let handler = TransitionHandler::new(&mut machine);

    // Should not panic when scheduler is None
    handler.on_exit(&State::PendingMerge, &State::Merged).await;

    // Yield to runtime to ensure no panic from spawned task
    tokio::task::yield_now().await;
}

// ==================
// Non-blocking retry merge tests
// ==================

// Tests early-return guard — does not reach merge strategy dispatch
/// Without repos, on_enter(PendingMerge) returns immediately.
/// Validates the structural non-blocking property of the auto-transition chain.
#[tokio::test]
async fn test_guard_no_repos_pending_merge_returns_quickly() {
    use std::time::Instant;

    let (_spawner, _emitter, _notifier, _dep_manager, _review_starter, services) =
        create_test_services();
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    // Measure time for the transition from ReviewPassed -> Approved -> PendingMerge
    // (Approved auto-transitions to PendingMerge)
    let start = Instant::now();
    let result = handler
        .handle_transition(&State::ReviewPassed, &TaskEvent::HumanApprove)
        .await;
    let elapsed = start.elapsed();

    // The transition should complete quickly (without repos, on_enter(PendingMerge)
    // skips the heavy merge attempt and returns immediately).
    assert!(
        elapsed.as_millis() < 100,
        "Transition to PendingMerge should complete in <100ms, took {}ms",
        elapsed.as_millis()
    );

    // Verify correct auto-transition chain: ReviewPassed -> Approved -> PendingMerge
    if let TransitionResult::AutoTransition(state) = &result {
        assert_eq!(*state, State::PendingMerge);
    } else {
        panic!("Expected AutoTransition to PendingMerge, got {:?}", result);
    }
}

// Tests early-return guard — does not reach merge strategy dispatch
/// Without repos, on_enter(PendingMerge) is a no-op and returns near-instantly.
#[tokio::test]
async fn test_guard_no_repos_on_enter_pending_merge_is_instant() {
    use std::time::Instant;

    let services = TaskServices::new_mock(); // No task_repo or project_repo
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);

    let handler = TransitionHandler::new(&mut machine);

    let start = Instant::now();
    let result = handler.on_enter(&State::PendingMerge).await;
    let elapsed = start.elapsed();

    // Without repos, on_enter(PendingMerge) returns immediately (skips merge attempt)
    assert!(result.is_ok());
    assert!(
        elapsed.as_millis() < 50,
        "on_enter(PendingMerge) without repos should be near-instant, took {}ms",
        elapsed.as_millis()
    );
}

// ==================
// Background execution correctness tests
// ==================

/// Test: State transitions for merge workflow occur in correct order.
///
/// Validates the expected progression:
/// ReviewPassed -> Approved -> PendingMerge (via auto-transitions)
/// with correct side effects at each stage.
#[tokio::test]
async fn test_background_execution_correctness_state_ordering() {
    let (_spawner, emitter, _notifier, dep_manager, _review_starter, services) =
        create_test_services();
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    // Trigger: ReviewPassed -> HumanApprove
    // Expected chain: ReviewPassed -> Approved -> PendingMerge (auto-transition)
    let result = handler
        .handle_transition(&State::ReviewPassed, &TaskEvent::HumanApprove)
        .await;

    // Final state should be PendingMerge (via Approved auto-transition)
    assert_eq!(
        result.state(),
        Some(&State::PendingMerge),
        "Should auto-transition to PendingMerge"
    );

    // Approved entry action should have emitted task_completed
    assert!(
        emitter.has_event("task_completed"),
        "Should emit task_completed on entering Approved"
    );

    // Approved should NOT unblock dependents (that happens at Merged)
    let dep_calls = dep_manager.get_calls();
    assert!(
        !dep_calls.iter().any(|c| c.method == "unblock_dependents"),
        "Should NOT unblock dependents at Approved — only at Merged"
    );
}

/// Test: On entering Merged state, dependents are unblocked and scheduling triggered.
///
/// Verifies the terminal merge state correctly handles:
/// 1. Dependency unblocking
/// 2. Ready task scheduling
/// 3. Deferred merge retry
#[tokio::test]
async fn test_background_execution_merged_terminal_state() {
    let scheduler = Arc::new(MockTaskScheduler::new());
    let dep_manager = Arc::new(MockDependencyManager::new());

    let mut services = TaskServices::new_mock();
    services.dependency_manager = Arc::clone(&dep_manager) as Arc<dyn DependencyManager>;
    services.task_scheduler =
        Some(Arc::clone(&scheduler)
            as Arc<
                dyn crate::domain::state_machine::services::TaskScheduler,
            >);

    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);

    let handler = TransitionHandler::new(&mut machine);
    let _ = handler.on_enter(&State::Merged).await;

    // Dependents should be unblocked
    let dep_calls = dep_manager.get_calls();
    assert!(
        dep_calls
            .iter()
            .any(|c| c.method == "unblock_dependents" && c.args[0] == "task-1"),
        "Should unblock dependents on Merged entry"
    );

    // Wait for spawned scheduling/retry tasks
    let sched = Arc::clone(&scheduler);
    assert!(
        wait_for_condition(
            || {
                let s = Arc::clone(&sched);
                async move {
                    let calls = s.get_calls();
                    calls.iter().any(|c| c.method == "try_schedule_ready_tasks")
                        && calls
                            .iter()
                            .any(|c| c.method == "try_retry_deferred_merges")
                }
            },
            5000
        )
        .await,
        "Should trigger both ready task scheduling and deferred merge retry"
    );

    let sched_calls = scheduler.get_calls();

    // Should trigger ready task scheduling
    assert!(
        sched_calls
            .iter()
            .any(|c| c.method == "try_schedule_ready_tasks"),
        "Should schedule ready tasks after merge"
    );

    // Should trigger deferred merge retry
    assert!(
        sched_calls
            .iter()
            .any(|c| c.method == "try_retry_deferred_merges"),
        "Should retry deferred merges after merge"
    );
}

/// Test: MergeIncomplete -> PendingMerge (retry) handles transition correctly.
///
/// Simulates the retry_merge path: user clicks Retry from MergeIncomplete,
/// which transitions to PendingMerge.
#[tokio::test]
async fn test_background_execution_retry_from_merge_incomplete() {
    let (_spawner, _emitter, _notifier, _dep_manager, _review_starter, services) =
        create_test_services();
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    // MergeIncomplete -> Retry -> PendingMerge
    let result = handler
        .handle_transition(&State::MergeIncomplete, &TaskEvent::Retry)
        .await;

    assert_eq!(
        result.state(),
        Some(&State::PendingMerge),
        "Retry from MergeIncomplete should go to PendingMerge"
    );
}

// ==================
// Blocking isolation tests
// ==================

// Tests early-return guard — does not reach merge strategy dispatch
/// Task 1 enters PendingMerge with no repos (guard returns immediately).
/// Validates that Task 2 is not blocked by the no-op merge path.
#[tokio::test]
async fn test_guard_no_repos_pending_merge_does_not_block_other_tasks() {
    use std::time::Instant;

    // Task 1: enters PendingMerge (merge workflow in progress)
    let services1 = TaskServices::new_mock();
    let context1 = create_context_with_services("task-1", "proj-1", services1);
    let mut machine1 = TaskStateMachine::new(context1);
    let handler1 = TransitionHandler::new(&mut machine1);

    // Trigger on_enter for PendingMerge (starts merge attempt, which is a no-op without repos)
    let _ = handler1.on_enter(&State::PendingMerge).await;

    // Task 2: independent operation (Backlog -> Ready) should not be affected
    let (spawner2, _emitter2, _notifier2, _dep_manager2, _review_starter2, services2) =
        create_test_services();
    let context2 = create_context_with_services("task-2", "proj-1", services2);
    let mut machine2 = TaskStateMachine::new(context2);
    let mut handler2 = TransitionHandler::new(&mut machine2);

    let start = Instant::now();
    let result = handler2
        .handle_transition(&State::Backlog, &TaskEvent::Schedule)
        .await;
    let elapsed = start.elapsed();

    // Unrelated transition should complete quickly
    assert!(result.is_success());
    assert_eq!(result.state(), Some(&State::Ready));
    assert!(
        elapsed.as_millis() < 50,
        "Unrelated transition should not be blocked by merge workflow, took {}ms",
        elapsed.as_millis()
    );

    // Verify the second task's operations completed correctly
    assert_eq!(spawner2.spawn_count(), 0); // No QA, so no agent spawned
}

/// Test: ExecutionState running count is not affected by PendingMerge transitions.
///
/// PendingMerge is NOT an agent-active state (only Executing, QaRefining,
/// QaTesting, Reviewing, ReExecuting, and Merging are). Exiting PendingMerge
/// should not affect the execution concurrency counter.
#[tokio::test]
async fn test_blocking_isolation_execution_state_unaffected_by_pending_merge() {
    use crate::application::execution_state::ExecutionState;

    let execution_state = Arc::new(ExecutionState::new());
    // Simulate one task running
    execution_state.increment_running();
    assert_eq!(execution_state.running_count(), 1);

    let services = TaskServices::new_mock().with_execution_state(Arc::clone(&execution_state));
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);

    let handler = TransitionHandler::new(&mut machine);

    // Exiting PendingMerge to Merged should NOT decrement (PendingMerge is not agent-active)
    handler.on_exit(&State::PendingMerge, &State::Merged).await;

    // PendingMerge is NOT agent-active, running count should be unchanged
    assert_eq!(
        execution_state.running_count(),
        1,
        "PendingMerge exit should NOT decrement running count (not agent-active)"
    );
}
