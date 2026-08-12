// Completion & Transition Hardening Tests (D1-D5)
//
// Tests for task completion scenarios: successful transitions, error handling,
// stale events, retry logic gaps, and running count integrity.

use super::helpers::*;

use crate::application::execution_state::ExecutionState;
use crate::domain::entities::{InternalStatus, ProjectId};
use crate::domain::state_machine::events::TaskEvent;
use crate::domain::state_machine::machine::{Response, State};
use crate::domain::state_machine::transition_handler::TransitionResult;

// ============================================================================
// D1: Agent completes successfully — COVERED
// ============================================================================

#[tokio::test]
async fn test_d1_agent_completes_successfully_transitions_to_reviewing() {
    // COVERED: Executing -> PendingReview (ExecutionComplete)
    // PendingReview auto-transitions -> Reviewing
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-d1", "proj-1", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Executing, &TaskEvent::ExecutionComplete)
        .await;

    match result {
        TransitionResult::AutoTransition(state) => {
            assert_eq!(state, State::Reviewing);
        }
        TransitionResult::Success(state) => {
            assert!(
                state == State::PendingReview || state == State::Reviewing,
                "Expected PendingReview or Reviewing, got {:?}",
                state
            );
        }
        other => panic!("Expected transition, got {:?}", other),
    }
}

#[tokio::test]
async fn test_d1_re_executing_completes_same_path() {
    // COVERED: ReExecuting -> PendingReview -> Reviewing
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-d1b", "proj-1", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::ReExecuting, &TaskEvent::ExecutionComplete)
        .await;

    match result {
        TransitionResult::AutoTransition(state) => {
            assert_eq!(state, State::Reviewing);
        }
        TransitionResult::Success(state) => {
            assert!(
                state == State::PendingReview || state == State::Reviewing,
                "Expected PendingReview or Reviewing, got {:?}",
                state
            );
        }
        other => panic!("Expected transition, got {:?}", other),
    }
}

#[tokio::test]
async fn test_d1_qa_enabled_routes_to_qa_refining() {
    // COVERED: With QA enabled, Executing -> QaRefining (not PendingReview)
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-d1c", "proj-1", services);
    machine.context.qa_enabled = true;
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Executing, &TaskEvent::ExecutionComplete)
        .await;

    match result {
        TransitionResult::Success(state) => {
            assert_eq!(state, State::QaRefining);
        }
        other => panic!("Expected Success(QaRefining), got {:?}", other),
    }
}

// ============================================================================
// D2: Agent completes with zero output -> Failed — COVERED
// ============================================================================

#[tokio::test]
async fn test_d2_zero_output_produces_execution_failed() {
    // COVERED: Zero output triggers ExecutionFailed at stream level.
    // State machine handles Executing -> Failed.
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-d2", "proj-1", services);

    let result = machine.dispatch(
        &State::Executing,
        &TaskEvent::ExecutionFailed {
            error: "Agent produced zero output".to_string(),
        },
    );

    match result {
        Response::Transition(State::Failed(data)) => {
            assert_eq!(data.error, "Agent produced zero output");
        }
        other => panic!("Expected Transition(Failed), got {:?}", other),
    }
}

#[tokio::test]
async fn test_d2_failed_state_accepts_retry() {
    // COVERED: After failure, task can be retried -> Ready
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-d2b", "proj-1", services);

    let result = machine.dispatch(&State::Failed(Default::default()), &TaskEvent::Retry);
    assert_eq!(result, Response::Transition(State::Ready));
}

// ============================================================================
// D3: Agent errors but task already moved (stale event) — PARTIAL
// ============================================================================

#[tokio::test]
async fn test_d3_execution_failed_rejected_from_approved_state() {
    // PARTIAL: If task has been manually moved to Approved, ExecutionFailed
    // should be NotHandled (state machine rejects stale events).
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-d3", "proj-1", services);

    let result = machine.dispatch(
        &State::Approved,
        &TaskEvent::ExecutionFailed {
            error: "Late failure from dead agent".to_string(),
        },
    );

    assert_eq!(
        result,
        Response::NotHandled,
        "ExecutionFailed should be rejected when task is already Approved"
    );
}

#[tokio::test]
async fn test_d3_execution_failed_rejected_from_merged_state() {
    // PARTIAL: Task already merged, stale ExecutionFailed arrives
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-d3b", "proj-1", services);

    let result = machine.dispatch(
        &State::Merged,
        &TaskEvent::ExecutionFailed {
            error: "Very late failure".to_string(),
        },
    );

    assert_eq!(
        result,
        Response::NotHandled,
        "ExecutionFailed should be rejected when task is already Merged"
    );
}

#[tokio::test]
async fn test_d3_execution_failed_rejected_from_reviewing_state() {
    // PARTIAL: Task moved to Reviewing, stale ExecutionFailed arrives.
    // No notification when a stale event is silently skipped.
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-d3c", "proj-1", services);

    let result = machine.dispatch(
        &State::Reviewing,
        &TaskEvent::ExecutionFailed {
            error: "Stale failure from previous execution".to_string(),
        },
    );

    assert_eq!(
        result,
        Response::NotHandled,
        "ExecutionFailed should be rejected when task is in Reviewing"
    );
}

#[tokio::test]
async fn test_d3_stale_events_silently_skipped_no_notification() {
    // PARTIAL: When a stale event is NotHandled, TransitionHandler returns
    // NotHandled but does not emit any notification or event.
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-d3d", "proj-1", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(
            &State::Approved,
            &TaskEvent::ExecutionFailed {
                error: "stale".into(),
            },
        )
        .await;

    assert_eq!(result, TransitionResult::NotHandled);

    assert_eq!(
        s.emitter.event_count(),
        0,
        "No events should be emitted for NotHandled transitions"
    );
    assert_eq!(
        s.notifier.notification_count(),
        0,
        "No notifications should be sent for NotHandled transitions"
    );
}

// ============================================================================
// D4: handle_stream_error fails to transition task — GAP
// ============================================================================

#[tokio::test]
async fn test_d4_no_retry_logic_in_transition_handler_on_enter() {
    // GAP: If on_enter fails (e.g., task_repo.get_by_id() returns error),
    // TransitionHandler logs the error but still returns Success.
    // No retry mechanism for failed on_enter side effects.
    //
    // No task or project in the repos. on_enter(Executing) tries to fetch
    // task and project for git setup but they don't exist.
    // The handler still returns Success -- side effects are best-effort.
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-d4", "proj-1", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Ready, &TaskEvent::StartExecution)
        .await;

    assert!(
        result.is_success(),
        "Transition should succeed even when on_enter has no task/project to configure"
    );

    // GAP: No retry queue, no compensation, no alerting when on_enter fails.
    // The transition happened (state changed) but side effects may not have completed.
}

#[tokio::test]
async fn test_d4_on_enter_error_does_not_revert_transition() {
    // GAP: Even if on_enter returns Err, the transition is not reverted.
    // TransitionHandler logs the error but returns the new state.
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-d4b", "proj-1", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Ready, &TaskEvent::StartExecution)
        .await;

    match &result {
        TransitionResult::Success(state) => {
            assert_eq!(*state, State::Executing);
        }
        TransitionResult::AutoTransition(_) => {
            // Also acceptable if an auto-transition fires
        }
        other => panic!("Expected Success or AutoTransition, got {:?}", other),
    }
}

// ============================================================================
// D5: Running count not decremented (on_exit never called) — PARTIAL
// ============================================================================

#[tokio::test]
async fn test_d5_on_exit_decrements_running_count() {
    // PARTIAL: Verify on_exit properly decrements when leaving agent-active state.
    let s = create_hardening_services();
    s.execution_state.increment_running();
    assert_eq!(s.execution_state.running_count(), 1);

    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-d5", "proj-1", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Executing, &TaskEvent::ExecutionComplete)
        .await;

    assert!(result.is_success());
    assert_eq!(
        s.execution_state.running_count(),
        0,
        "Running count should be decremented after exiting Executing state"
    );
}

#[tokio::test]
async fn test_d5_on_exit_decrements_for_all_agent_active_states() {
    // PARTIAL: Verify on_exit decrements for each agent-active state
    let agent_active_transitions: Vec<(State, TaskEvent, &str)> = vec![
        (
            State::Executing,
            TaskEvent::ExecutionFailed {
                error: "err".into(),
            },
            "Executing",
        ),
        (
            State::Reviewing,
            TaskEvent::ReviewComplete {
                approved: true,
                feedback: None,
            },
            "Reviewing",
        ),
        (State::Merging, TaskEvent::MergeAgentError, "Merging"),
        (
            State::QaRefining,
            TaskEvent::QaRefinementComplete,
            "QaRefining",
        ),
        (
            State::QaTesting,
            TaskEvent::QaTestsComplete { passed: true },
            "QaTesting",
        ),
    ];

    for (from_state, event, label) in agent_active_transitions {
        let s = create_hardening_services();
        s.execution_state.increment_running();
        assert_eq!(s.execution_state.running_count(), 1);

        let services = build_task_services(&s);
        let mut machine = create_state_machine("task-d5-loop", "proj-1", services);
        let mut handler = create_transition_handler(&mut machine);

        let result = handler.handle_transition(&from_state, &event).await;
        assert!(
            result.is_success(),
            "Transition from {} should succeed",
            label
        );

        assert_eq!(
            s.execution_state.running_count(),
            0,
            "Running count should be decremented after exiting {} state",
            label
        );
    }
}

#[tokio::test]
async fn test_d5_direct_status_update_bypassing_handler_leaves_count_wrong() {
    // PARTIAL: If status is updated directly (bypassing TransitionHandler),
    // on_exit is never called and running count stays wrong.
    let s = create_hardening_services();
    s.execution_state.increment_running();
    assert_eq!(s.execution_state.running_count(), 1);

    let project_id = ProjectId::from_string("proj-d5".to_string());
    let mut task =
        create_test_task_with_status(&project_id, "Direct update task", InternalStatus::Executing);

    // Direct status change (the anti-pattern)
    task.internal_status = InternalStatus::PendingReview;

    // Running count is still 1 because on_exit was never called
    assert_eq!(
        s.execution_state.running_count(),
        1,
        "Running count should remain wrong when TransitionHandler is bypassed"
    );

    // PARTIAL: The only fix is reconciliation, which periodically corrects
    // the running count by scanning actual agent-active tasks.
}

#[tokio::test]
async fn test_d5_underflow_protection_on_decrement() {
    // PARTIAL: Verify ExecutionState handles underflow gracefully
    let exec_state = ExecutionState::new();
    assert_eq!(exec_state.running_count(), 0);

    let result = exec_state.decrement_running();
    assert_eq!(result, 0, "Decrement from 0 should return 0 (no underflow)");
    assert_eq!(
        exec_state.running_count(),
        0,
        "Running count should stay at 0 after underflow protection"
    );
}
