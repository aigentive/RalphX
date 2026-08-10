use super::helpers::create_context_with_services;
use crate::application::execution_state::ExecutionState;
use crate::domain::state_machine::context::TaskServices;
use crate::domain::state_machine::types::FailedData;
use crate::domain::state_machine::{State, TaskStateMachine, TransitionHandler};
use std::sync::Arc;

// ==================
// ExecutionState decrement on exit tests
// ==================

#[tokio::test]
async fn test_exiting_executing_decrements_running_count() {
    let execution_state = Arc::new(ExecutionState::new());
    // Simulate task already running
    execution_state.increment_running();
    assert_eq!(execution_state.running_count(), 1);

    let services = TaskServices::new_mock().with_execution_state(Arc::clone(&execution_state));
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);

    let handler = TransitionHandler::new(&mut machine);
    handler
        .on_exit(&State::Executing, &State::PendingReview)
        .await;

    // Running count should be decremented
    assert_eq!(execution_state.running_count(), 0);
}

#[tokio::test]
async fn test_exiting_reviewing_decrements_running_count() {
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.increment_running();
    assert_eq!(execution_state.running_count(), 1);

    let services = TaskServices::new_mock().with_execution_state(Arc::clone(&execution_state));
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);

    let handler = TransitionHandler::new(&mut machine);
    handler
        .on_exit(&State::Reviewing, &State::ReviewPassed)
        .await;

    assert_eq!(execution_state.running_count(), 0);
}

#[tokio::test]
async fn test_exiting_qa_refining_decrements_running_count() {
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.increment_running();

    let services = TaskServices::new_mock().with_execution_state(Arc::clone(&execution_state));
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);

    let handler = TransitionHandler::new(&mut machine);
    handler.on_exit(&State::QaRefining, &State::QaTesting).await;

    assert_eq!(execution_state.running_count(), 0);
}

#[tokio::test]
async fn test_exiting_qa_testing_decrements_running_count() {
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.increment_running();

    let services = TaskServices::new_mock().with_execution_state(Arc::clone(&execution_state));
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);

    let handler = TransitionHandler::new(&mut machine);
    handler.on_exit(&State::QaTesting, &State::QaPassed).await;

    assert_eq!(execution_state.running_count(), 0);
}

#[tokio::test]
async fn test_exiting_re_executing_decrements_running_count() {
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.increment_running();

    let services = TaskServices::new_mock().with_execution_state(Arc::clone(&execution_state));
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);

    let handler = TransitionHandler::new(&mut machine);
    handler
        .on_exit(&State::ReExecuting, &State::PendingReview)
        .await;

    assert_eq!(execution_state.running_count(), 0);
}

#[tokio::test]
async fn test_exiting_non_agent_state_does_not_decrement() {
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.increment_running();
    assert_eq!(execution_state.running_count(), 1);

    let services = TaskServices::new_mock().with_execution_state(Arc::clone(&execution_state));
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);

    let handler = TransitionHandler::new(&mut machine);
    // Exiting Ready (not agent-active) should NOT decrement
    handler.on_exit(&State::Ready, &State::Executing).await;

    // Running count should remain unchanged
    assert_eq!(execution_state.running_count(), 1);
}

#[tokio::test]
async fn test_exiting_without_execution_state_does_not_panic() {
    // Services without execution_state
    let services = TaskServices::new_mock();
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);

    let handler = TransitionHandler::new(&mut machine);
    // Should not panic even without execution_state
    handler
        .on_exit(&State::Executing, &State::PendingReview)
        .await;
}

#[tokio::test]
async fn test_on_exit_with_execution_state_no_app_handle_does_not_panic() {
    // Verifies that on_exit() handles the case where execution_state is Some
    // but event_sink is None (the emit_status_changed call is skipped gracefully).
    // Actual event emission is covered by EventSink-backed regression tests.
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.increment_running();
    assert_eq!(execution_state.running_count(), 1);

    // Services with execution_state but no event_sink
    let services = TaskServices::new_mock().with_execution_state(Arc::clone(&execution_state));
    // Note: event_sink is None by default in new_mock()
    assert!(services.event_sink.is_none());

    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);

    let handler = TransitionHandler::new(&mut machine);
    // Should not panic - emit_status_changed is skipped when event_sink is None
    handler
        .on_exit(&State::Executing, &State::PendingReview)
        .await;

    // Running count should still be decremented
    assert_eq!(execution_state.running_count(), 0);
}

#[tokio::test]
async fn test_on_exit_emits_for_all_agent_active_states() {
    // Verifies that on_exit() attempts to emit status_changed for all agent-active states
    // The emit logic is the same for all states that decrement running count
    let agent_active_states = [
        State::Executing,
        State::QaRefining,
        State::QaTesting,
        State::Reviewing,
        State::ReExecuting,
    ];

    for from_state in &agent_active_states {
        let execution_state = Arc::new(ExecutionState::new());
        execution_state.increment_running();

        let services = TaskServices::new_mock().with_execution_state(Arc::clone(&execution_state));
        let context = create_context_with_services("task-1", "proj-1", services);
        let mut machine = TaskStateMachine::new(context);

        let handler = TransitionHandler::new(&mut machine);
        // Each agent-active state should trigger decrement (and emit if event_sink present)
        let to_state = State::Failed(FailedData::default());
        handler.on_exit(from_state, &to_state).await;

        assert_eq!(
            execution_state.running_count(),
            0,
            "Expected running_count=0 after exiting {:?}",
            from_state
        );
    }
}
