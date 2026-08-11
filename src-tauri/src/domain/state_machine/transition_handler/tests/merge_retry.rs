// Merge retry tests: reload continuation, event emission, deferred merge regression, main merge retry

use super::helpers::*;
use crate::domain::state_machine::{State, TaskEvent, TransitionHandler};

// ==================
// Reload continuation tests
// ==================

/// Callback drop during merge workflow handled gracefully (no panic without event_sink).
#[tokio::test]
async fn test_reload_continuation_callback_drop() {
    use crate::commands::ExecutionState;

    let execution_state = Arc::new(ExecutionState::new());
    execution_state.increment_running();

    let services = TaskServices::new_mock().with_execution_state(Arc::clone(&execution_state));
    assert!(
        services.event_sink.is_none(),
        "Mock services should not have event_sink"
    );

    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    // None should panic (graceful handling even without event_sink)
    handler.on_exit(&State::PendingMerge, &State::Merged).await;
    handler
        .on_exit(&State::PendingMerge, &State::MergeIncomplete)
        .await;

    assert_eq!(
        execution_state.running_count(),
        1,
        "Running count should be unchanged (PendingMerge is not agent-active)"
    );
}

// Tests early-return guard — does not reach merge strategy dispatch
/// Without repos or event_sink, on_enter for PendingMerge/Merged/Merging does not panic.
#[tokio::test]
async fn test_guard_no_repos_enter_merge_states_no_panic() {
    let services = TaskServices::new_mock();
    assert!(services.event_sink.is_none());

    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::PendingMerge).await;
    assert!(
        result.is_ok(),
        "on_enter(PendingMerge) should succeed without event_sink"
    );

    let result = handler.on_enter(&State::Merged).await;
    assert!(
        result.is_ok(),
        "on_enter(Merged) should succeed without event_sink"
    );

    let result = handler.on_enter(&State::Merging).await;
    assert!(
        result.is_ok(),
        "on_enter(Merging) should succeed without event_sink"
    );
}

// Tests early-return guard — does not reach merge strategy dispatch
/// Without repos, re-entering PendingMerge after simulated reload works without panic.
#[tokio::test]
async fn test_guard_no_repos_reload_recovery_no_panic() {
    // Phase 1: Task enters PendingMerge
    let (mut machine1, _scheduler1) = new_machine_with_scheduler("task-1", "proj-1");
    let handler1 = TransitionHandler::new(&mut machine1);
    let _ = handler1.on_enter(&State::PendingMerge).await;

    // Phase 2: Simulate "reload" — create fresh context for same task
    let (mut machine2, scheduler2) = new_machine_with_scheduler("task-1", "proj-1");
    let handler2 = TransitionHandler::new(&mut machine2);

    let result = handler2.on_enter(&State::PendingMerge).await;
    assert!(
        result.is_ok(),
        "Re-entering PendingMerge after reload should succeed"
    );

    handler2.on_exit(&State::PendingMerge, &State::Merged).await;

    let sched = Arc::clone(&scheduler2);
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
        "Deferred merge retry should work after reload"
    );
}

// ==================
// Event emission tests
// ==================

/// Approved entry emits task_completed event.
#[tokio::test]
async fn test_event_emission_approved_emits_task_completed() {
    let (_spawner, emitter, _notifier, _dep_manager, _review_starter, services) =
        create_test_services();
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    let result = handler
        .handle_transition(&State::ReviewPassed, &TaskEvent::HumanApprove)
        .await;

    assert_eq!(result.state(), Some(&State::PendingMerge));
    assert!(
        emitter.has_event("task_completed"),
        "Should emit task_completed when entering Approved"
    );
}

/// Merged entry triggers unblock_dependents side effect.
#[tokio::test]
async fn test_event_emission_merged_entry_side_effects() {
    let dep_manager = Arc::new(MockDependencyManager::new());
    let emitter = Arc::new(MockEventEmitter::new());

    let mut services = TaskServices::new_mock();
    services.dependency_manager = Arc::clone(&dep_manager) as Arc<dyn DependencyManager>;
    services.event_emitter = Arc::clone(&emitter) as Arc<dyn EventEmitter>;

    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);
    let _ = handler.on_enter(&State::Merged).await;

    let dep_calls = dep_manager.get_calls();
    assert!(
        dep_calls.iter().any(|c| c.method == "unblock_dependents"),
        "on_enter(Merged) should unblock dependents"
    );
}

/// Exiting PendingMerge does NOT decrement execution running count.
#[tokio::test]
async fn test_event_emission_pending_merge_exit_preserves_execution_state() {
    use crate::commands::ExecutionState;

    let execution_state = Arc::new(ExecutionState::new());
    execution_state.increment_running();

    let emitter = Arc::new(MockEventEmitter::new());
    let mut services = TaskServices::new_mock();
    services.event_emitter = Arc::clone(&emitter) as Arc<dyn EventEmitter>;
    let services = services.with_execution_state(Arc::clone(&execution_state));

    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);
    handler.on_exit(&State::PendingMerge, &State::Merged).await;

    assert_eq!(
        execution_state.running_count(),
        1,
        "PendingMerge exit should NOT decrement running count (not agent-active)"
    );
}

/// Full merge event sequence from ReviewPassed through PendingMerge.
#[tokio::test]
async fn test_event_emission_full_merge_event_sequence() {
    let (_spawner, emitter, _notifier, dep_manager, _review_starter, services) =
        create_test_services();
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    let result = handler
        .handle_transition(&State::ReviewPassed, &TaskEvent::HumanApprove)
        .await;
    assert_eq!(result.state(), Some(&State::PendingMerge));

    let events = emitter.get_events();
    assert!(
        events
            .iter()
            .any(|e| e.args.first().map(|s| s.as_str()) == Some("task_completed")),
        "Event sequence should include task_completed"
    );

    let dep_calls = dep_manager.get_calls();
    assert!(
        !dep_calls.iter().any(|c| c.method == "unblock_dependents"),
        "Dependents should NOT be unblocked at Approved"
    );
}

// ==================
// Deferred merge compatibility regression tests
// ==================

/// Regression: deferred merge retry preserved on all PendingMerge exits.
#[tokio::test]
async fn test_deferred_merge_retry_on_all_pending_merge_exits() {
    let target_states = [State::Merged, State::MergeIncomplete, State::Merging];

    for target in &target_states {
        let (mut machine, scheduler) = new_machine_with_scheduler("task-1", "proj-1");
        let handler = TransitionHandler::new(&mut machine);
        handler.on_exit(&State::PendingMerge, target).await;

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
            "Expected deferred merge retry when exiting PendingMerge to {:?}",
            target
        );

        let calls = scheduler.get_calls();
        let retry_calls: Vec<_> = calls
            .iter()
            .filter(|c| c.method == "try_retry_deferred_merges")
            .collect();

        assert_eq!(
            retry_calls.len(),
            1,
            "Expected deferred merge retry when exiting PendingMerge to {:?}",
            target
        );
        assert_eq!(
            retry_calls[0].args,
            vec!["proj-1"],
            "Deferred retry should use correct project_id for target {:?}",
            target
        );
    }
}

/// Regression: deferred merge retry preserved on all Merging exits.
#[tokio::test]
async fn test_deferred_merge_retry_on_all_merging_exits() {
    let target_states = [State::Merged, State::MergeIncomplete, State::MergeConflict];

    for target in &target_states {
        let (mut machine, scheduler) = new_machine_with_scheduler("task-1", "proj-1");
        let handler = TransitionHandler::new(&mut machine);
        handler.on_exit(&State::Merging, target).await;

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
            "Expected deferred merge retry when exiting Merging to {:?}",
            target
        );

        let calls = scheduler.get_calls();
        let retry_calls: Vec<_> = calls
            .iter()
            .filter(|c| c.method == "try_retry_deferred_merges")
            .collect();

        assert_eq!(
            retry_calls.len(),
            1,
            "Expected deferred merge retry when exiting Merging to {:?}",
            target
        );
    }
}

/// Regression: single on_exit produces exactly one deferred retry (no duplicates).
#[tokio::test]
async fn test_deferred_merge_no_duplicate_retries() {
    let (mut machine, scheduler) = new_machine_with_scheduler("task-1", "proj-1");
    let handler = TransitionHandler::new(&mut machine);
    handler.on_exit(&State::PendingMerge, &State::Merged).await;

    // Wait for the call to appear, then verify no duplicates
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
    // Brief additional wait to catch any duplicate calls
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let calls = scheduler.get_calls();
    let retry_calls: Vec<_> = calls
        .iter()
        .filter(|c| c.method == "try_retry_deferred_merges")
        .collect();

    assert_eq!(
        retry_calls.len(),
        1,
        "Single on_exit should produce exactly one deferred retry call, got {}",
        retry_calls.len()
    );
}

/// Regression: non-merge state exits do NOT trigger deferred merge retry.
#[tokio::test]
async fn test_deferred_merge_not_triggered_by_non_merge_exits() {
    let non_merge_transitions = [
        (State::Executing, State::PendingReview),
        (State::Reviewing, State::ReviewPassed),
        (State::ReExecuting, State::PendingReview),
        (State::Ready, State::Executing),
        (State::QaTesting, State::QaPassed),
        (State::QaRefining, State::QaTesting),
    ];

    for (from, to) in &non_merge_transitions {
        let (mut machine, scheduler) = new_machine_with_scheduler("task-1", "proj-1");
        let handler = TransitionHandler::new(&mut machine);
        handler.on_exit(from, to).await;

        // Negative test: wait briefly to confirm nothing fires
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

        let calls = scheduler.get_calls();
        let retry_calls: Vec<_> = calls
            .iter()
            .filter(|c| c.method == "try_retry_deferred_merges")
            .collect();

        assert_eq!(
            retry_calls.len(),
            0,
            "Non-merge exit {:?} -> {:?} should NOT trigger deferred retry",
            from,
            to
        );
    }
}

// ==================
// Merge-exit main merge retry tests
// ==================

/// Exiting PendingMerge calls try_retry_main_merges when running_count == 0.
#[tokio::test]
async fn test_merge_exit_triggers_main_merge_retry_when_all_idle() {
    use crate::commands::ExecutionState;

    let scheduler = Arc::new(MockTaskScheduler::new());
    let execution_state = Arc::new(ExecutionState::new());

    let services = TaskServices::new_mock()
        .with_task_scheduler(Arc::clone(&scheduler)
            as Arc<dyn crate::domain::state_machine::services::TaskScheduler>)
        .with_execution_state(Arc::clone(&execution_state));

    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);
    handler.on_exit(&State::PendingMerge, &State::Merged).await;

    let sched = Arc::clone(&scheduler);
    assert!(
        wait_for_condition(
            || {
                let s = Arc::clone(&sched);
                async move {
                    s.get_calls()
                        .iter()
                        .any(|c| c.method == "try_retry_main_merges")
                }
            },
            5000
        )
        .await,
        "Expected try_retry_main_merges when all agents idle (running_count == 0)"
    );
}

/// Exiting PendingMerge ALWAYS calls try_retry_main_merges, even when agents are running.
///
/// The running_count guard was removed from spawn_deferred_merge_retry to fix a TOCTOU race:
/// by the time the tokio::spawn future runs, auto-transitions (e.g. PendingReview→Reviewing)
/// may have already incremented running_count. The authoritative deferral gate is
/// check_main_merge_deferral() inside attempt_programmatic_merge(), which reads running_count
/// fresh at merge-start time and re-defers correctly if agents are still active.
#[tokio::test]
async fn test_merge_exit_always_triggers_main_merge_retry_regardless_of_running_count() {
    use crate::commands::ExecutionState;

    let scheduler = Arc::new(MockTaskScheduler::new());
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.increment_running(); // running_count > 0 — retry must still fire

    let services = TaskServices::new_mock()
        .with_task_scheduler(Arc::clone(&scheduler)
            as Arc<dyn crate::domain::state_machine::services::TaskScheduler>)
        .with_execution_state(Arc::clone(&execution_state));

    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);
    handler.on_exit(&State::PendingMerge, &State::Merged).await;

    let sched = Arc::clone(&scheduler);
    assert!(
        wait_for_condition(
            || {
                let s = Arc::clone(&sched);
                async move {
                    s.get_calls()
                        .iter()
                        .any(|c| c.method == "try_retry_main_merges")
                }
            },
            5000,
        )
        .await,
        "Expected try_retry_main_merges even when running_count > 0 (TOCTOU fix: guard is inside check_main_merge_deferral)"
    );
}

/// Cascading merge unblocking — Merging exit also triggers main merge retry when idle.
#[tokio::test]
async fn test_merging_exit_triggers_main_merge_retry_when_all_idle() {
    use crate::commands::ExecutionState;

    let scheduler = Arc::new(MockTaskScheduler::new());
    let execution_state = Arc::new(ExecutionState::new());

    let services = TaskServices::new_mock()
        .with_task_scheduler(Arc::clone(&scheduler)
            as Arc<dyn crate::domain::state_machine::services::TaskScheduler>)
        .with_execution_state(Arc::clone(&execution_state));

    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);
    handler.on_exit(&State::Merging, &State::Merged).await;

    let sched = Arc::clone(&scheduler);
    assert!(
        wait_for_condition(
            || {
                let s = Arc::clone(&sched);
                async move {
                    s.get_calls()
                        .iter()
                        .any(|c| c.method == "try_retry_main_merges")
                }
            },
            5000
        )
        .await,
        "Merging exit should trigger try_retry_main_merges when all agents idle"
    );
}
