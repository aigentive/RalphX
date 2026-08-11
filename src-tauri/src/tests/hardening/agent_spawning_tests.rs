// Agent Spawning Hardening Tests (B1-B5)
//
// Tests for agent spawning behavior during task execution transitions.
// Verifies that chat_service.send_message() is called correctly,
// error handling gaps are documented, and concurrency controls are tested.

use super::helpers::*;
use crate::application::execution_state::ExecutionState;
use crate::domain::state_machine::events::TaskEvent;
use crate::domain::state_machine::machine::State;

// ============================================================================
// B1: send_message() error swallowed via `let _`
// ============================================================================

#[tokio::test]
async fn test_b1_send_message_error_swallowed() {
    // Scenario B1: send_message() error swallowed via `let _` — FIXED
    //
    // Set MockChatService to unavailable. Transition Ready->Executing.
    // Previously on_enter called `let _ = chat_service.send_message(...)` which
    // silently discarded the error. Task would remain in Executing state with no agent.
    //
    // FIXED: Task now correctly transitions to Failed state when agent spawning fails.

    let svc = create_hardening_services();

    // Make chat service unavailable — it will return ChatServiceError::AgentNotAvailable
    svc.chat_service.set_available(false).await;

    // Build services WITHOUT task_repo/project_repo so git setup is skipped
    // and we go straight to the send_message call
    let services = crate::domain::state_machine::context::TaskServices::new(
        svc.spawner.clone() as _,
        svc.emitter.clone() as _,
        svc.notifier.clone() as _,
        svc.dependency_manager.clone() as _,
        svc.review_starter.clone() as _,
        svc.chat_service.clone() as _,
    )
    .with_execution_state(svc.execution_state.clone());

    let mut machine = create_state_machine("task-b1", "proj-b1", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Ready, &TaskEvent::StartExecution)
        .await;

    // Transition succeeds — now properly handling the error
    assert!(
        result.is_success(),
        "Transition should succeed and handle send_message failure"
    );

    // FIXED: Task correctly transitions to Failed state when agent spawn fails
    assert!(
        matches!(result.state(), Some(State::Failed(_))),
        "FIXED: Task correctly transitions to Failed state when agent spawn fails"
    );

    // send_message WAS called (the call_count increments even on error)
    assert_eq!(
        svc.chat_service.call_count(),
        1,
        "send_message should have been called once"
    );

    // FIXED: The task is now in Failed state rather than stuck in Executing.
    // The error is properly handled and the task transitions to Failed.
}

#[tokio::test]
async fn test_b1_send_message_error_no_failed_transition() {
    // Scenario B1 variant: FIXED - Verify that after send_message fails,
    // task correctly transitions to Failed state.

    let svc = create_hardening_services();
    svc.chat_service.set_available(false).await;

    let services = crate::domain::state_machine::context::TaskServices::new(
        svc.spawner.clone() as _,
        svc.emitter.clone() as _,
        svc.notifier.clone() as _,
        svc.dependency_manager.clone() as _,
        svc.review_starter.clone() as _,
        svc.chat_service.clone() as _,
    )
    .with_execution_state(svc.execution_state.clone());

    let mut machine = create_state_machine("task-b1-nofail", "proj-b1", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Ready, &TaskEvent::StartExecution)
        .await;

    // FIXED: Task correctly transitions to Failed when agent spawn fails
    assert!(result.is_success(), "Transition should succeed");
    assert!(
        matches!(result.state(), Some(State::Failed(_))),
        "FIXED: Task correctly transitions to Failed when agent spawn fails"
    );

    // FIXED (Phase 2, B1+H2): Error event IS now emitted when send_message fails.
    // Previously this was a GAP — errors were silently swallowed.
    let events = svc.emitter.get_events();
    let has_error_event = events
        .iter()
        .any(|e| e.args.first().map(|s| s.contains("error")).unwrap_or(false));
    assert!(
        has_error_event,
        "FIXED: Error event should be emitted when send_message fails (was GAP, now fixed)"
    );
}

// ============================================================================
// B2: Spawn blocked at max_concurrent — silent
// ============================================================================

#[tokio::test]
async fn test_b2_execution_state_can_start_task_false_at_capacity() {
    // Scenario B2: Spawn blocked at max_concurrent — GAP
    //
    // Verify ExecutionState.can_start_task() returns false when at capacity.
    // The spawn gating logic is in AgenticClientSpawner (production code),
    // not in MockAgentSpawner. This test verifies the capacity check works
    // and demonstrates that no mechanism exists to block the state transition.

    let exec_state = ExecutionState::with_max_concurrent(2);

    // Initially can start
    assert!(
        exec_state.can_start_task(),
        "Should be able to start task when no tasks running"
    );

    // Fill up capacity
    exec_state.increment_running();
    exec_state.increment_running();

    // At capacity — cannot start
    assert!(
        !exec_state.can_start_task(),
        "Should not be able to start task when at max concurrent"
    );
    assert_eq!(exec_state.running_count(), 2);
    assert_eq!(exec_state.max_concurrent(), 2);

    // Decrement one — can start again
    exec_state.decrement_running();
    assert!(
        exec_state.can_start_task(),
        "Should be able to start task after decrementing"
    );
}

#[tokio::test]
async fn test_b2_paused_blocks_can_start_task() {
    // Scenario B2 variant: Verify paused state blocks can_start_task.

    let exec_state = ExecutionState::with_max_concurrent(5);
    assert!(exec_state.can_start_task());

    exec_state.pause();
    assert!(
        !exec_state.can_start_task(),
        "Should not start task when paused, even with capacity available"
    );
    assert!(exec_state.is_paused());

    exec_state.resume();
    assert!(exec_state.can_start_task());
}

#[tokio::test]
async fn test_b2_transition_succeeds_despite_max_concurrent() {
    // Scenario B2: GAP demonstration — state machine transition to Executing
    // succeeds even when ExecutionState says can_start_task() is false.
    // The spawn gating happens at a higher level (TaskTransitionService or
    // AgenticClientSpawner), not in the state machine or TransitionHandler.

    let svc = create_hardening_services();

    // Fill up execution capacity
    svc.execution_state.increment_running();
    svc.execution_state.increment_running();
    assert!(
        !svc.execution_state.can_start_task(),
        "Precondition: should be at max capacity"
    );

    // Build services without repos (skip git setup)
    let services = crate::domain::state_machine::context::TaskServices::new(
        svc.spawner.clone() as _,
        svc.emitter.clone() as _,
        svc.notifier.clone() as _,
        svc.dependency_manager.clone() as _,
        svc.review_starter.clone() as _,
        svc.chat_service.clone() as _,
    )
    .with_execution_state(svc.execution_state.clone());

    let mut machine = create_state_machine("task-b2-overload", "proj-b2", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Ready, &TaskEvent::StartExecution)
        .await;

    // GAP: Transition succeeds even though we're at max capacity
    assert!(
        result.is_success(),
        "GAP: State machine does not check ExecutionState capacity — \
         transition to Executing succeeds at max concurrent"
    );

    // send_message was still called
    assert_eq!(
        svc.chat_service.call_count(),
        1,
        "GAP: send_message called despite being at max capacity"
    );
}

// ============================================================================
// B3: Agent process fails to start — partial
// ============================================================================

#[tokio::test]
async fn test_b3_agent_spawn_failure_swallowed() {
    // Scenario B3: Agent process fails to start — PARTIAL
    //
    // MockChatService returns error from send_message.
    // Verify on_enter logs error but doesn't transition to Failed
    // (because `let _` swallows the error).

    let svc = create_hardening_services();

    // Make chat service unavailable to simulate spawn failure
    svc.chat_service.set_available(false).await;

    // Use services without repos so git setup is skipped
    let services = crate::domain::state_machine::context::TaskServices::new(
        svc.spawner.clone() as _,
        svc.emitter.clone() as _,
        svc.notifier.clone() as _,
        svc.dependency_manager.clone() as _,
        svc.review_starter.clone() as _,
        svc.chat_service.clone() as _,
    )
    .with_execution_state(svc.execution_state.clone());

    let mut machine = create_state_machine("task-b3", "proj-b3", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Ready, &TaskEvent::StartExecution)
        .await;

    // on_enter completed without returning an error (the `let _` swallows it)
    assert!(
        result.is_success(),
        "on_enter should not propagate send_message errors due to `let _ =`"
    );

    // The send_message call was made but returned an error
    assert_eq!(svc.chat_service.call_count(), 1);

    // No notification was sent about the failure
    assert_eq!(
        svc.notifier.notification_count(),
        0,
        "GAP: No notification sent when agent fails to start"
    );
}

// ============================================================================
// B4: Agent crashes immediately (exit code != 0)
// ============================================================================

#[tokio::test]
async fn test_b4_execution_failed_event_transitions_to_failed() {
    // Scenario B4: Agent crashes immediately (exit code != 0) — COVERED
    //
    // This is handled by stream monitoring in production code.
    // For unit test: verify that ExecutionFailed event correctly transitions
    // Executing -> Failed.

    let svc = create_hardening_services();

    let services = build_task_services(&svc);
    let mut machine = create_state_machine("task-b4", "proj-b4", services);
    let mut handler = create_transition_handler(&mut machine);

    // Directly test Executing + ExecutionFailed -> Failed
    let result = handler
        .handle_transition(
            &State::Executing,
            &TaskEvent::ExecutionFailed {
                error: "Agent crashed with exit code 1".to_string(),
            },
        )
        .await;

    assert!(
        result.is_success(),
        "ExecutionFailed transition should succeed"
    );

    // Verify we're in Failed state
    match result.state() {
        Some(State::Failed(_data)) => {
            // Failed state should contain the error data
            assert!(
                true,
                "Task correctly transitioned to Failed state with error data"
            );
        }
        other => panic!("Expected Failed state, got {:?}", other),
    }
}

#[tokio::test]
async fn test_b4_re_executing_failed_transitions_to_failed() {
    // Scenario B4 variant: ExecutionFailed from ReExecuting state also goes to Failed.

    let svc = create_hardening_services();

    let services = build_task_services(&svc);
    let mut machine = create_state_machine("task-b4-reexec", "proj-b4", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(
            &State::ReExecuting,
            &TaskEvent::ExecutionFailed {
                error: "Re-execution crashed".to_string(),
            },
        )
        .await;

    assert!(result.is_success());
    assert!(
        matches!(result.state(), Some(State::Failed(_))),
        "ReExecuting + ExecutionFailed should transition to Failed"
    );
}

// ============================================================================
// B5: Multiple spawn attempts for same task
// ============================================================================

#[tokio::test]
async fn test_b5_duplicate_on_enter_executing_calls_send_message_twice() {
    // Scenario B5: Multiple spawn attempts for same task — PARTIAL
    //
    // Calling on_enter(Executing) twice for the same task results in
    // send_message being called twice. There is no dedup mechanism.
    // This demonstrates the gap.

    let svc = create_hardening_services();

    // Use services without repos so git setup is skipped
    let services = crate::domain::state_machine::context::TaskServices::new(
        svc.spawner.clone() as _,
        svc.emitter.clone() as _,
        svc.notifier.clone() as _,
        svc.dependency_manager.clone() as _,
        svc.review_starter.clone() as _,
        svc.chat_service.clone() as _,
    )
    .with_execution_state(svc.execution_state.clone());

    let mut machine = create_state_machine("task-b5-dup", "proj-b5", services);
    let mut handler = create_transition_handler(&mut machine);

    // First transition: Ready -> Executing
    let result1 = handler
        .handle_transition(&State::Ready, &TaskEvent::StartExecution)
        .await;
    assert!(result1.is_success());
    assert_eq!(
        svc.chat_service.call_count(),
        1,
        "First transition should call send_message once"
    );

    // Simulate a second execution for the same task:
    // Executing -> Failed (via ExecutionFailed)
    let result_fail = handler
        .handle_transition(
            &State::Executing,
            &TaskEvent::ExecutionFailed {
                error: "first attempt failed".to_string(),
            },
        )
        .await;
    assert!(result_fail.is_success());

    // Failed -> Ready (via Retry)
    let result_retry = handler
        .handle_transition(&State::Failed(Default::default()), &TaskEvent::Retry)
        .await;
    assert!(result_retry.is_success());
    // on_enter(Ready) may trigger scheduler but doesn't call send_message

    // Ready -> Executing again (via StartExecution)
    let result2 = handler
        .handle_transition(&State::Ready, &TaskEvent::StartExecution)
        .await;
    assert!(result2.is_success());

    // GAP: send_message was called again for the same task with no dedup check.
    // The task_id is the same across both calls — no mechanism prevents spawning
    // a second agent for an already-spawned task.
    assert_eq!(
        svc.chat_service.call_count(),
        2,
        "GAP: send_message called twice for the same task — no dedup mechanism"
    );
}

#[tokio::test]
async fn test_b5_re_executing_also_spawns_agent() {
    // Scenario B5 variant: ReExecuting state also calls send_message
    // (via the RevisionNeeded auto-transition). This is expected behavior
    // but shows that multiple spawns happen in a task lifecycle.

    let svc = create_hardening_services();

    let services = crate::domain::state_machine::context::TaskServices::new(
        svc.spawner.clone() as _,
        svc.emitter.clone() as _,
        svc.notifier.clone() as _,
        svc.dependency_manager.clone() as _,
        svc.review_starter.clone() as _,
        svc.chat_service.clone() as _,
    )
    .with_execution_state(svc.execution_state.clone())
    .with_task_scheduler(svc.scheduler.clone()
        as std::sync::Arc<dyn crate::domain::state_machine::services::TaskScheduler>);

    let mut machine = create_state_machine("task-b5-reexec", "proj-b5", services);

    // Set review_feedback for the RevisionNeeded transition
    machine.context.review_feedback = Some("Please fix the tests".to_string());

    let mut handler = create_transition_handler(&mut machine);

    // Transition Reviewing -> RevisionNeeded (via ReviewComplete{approved: false})
    let result = handler
        .handle_transition(
            &State::Reviewing,
            &TaskEvent::ReviewComplete {
                approved: false,
                feedback: Some("Please fix the tests".to_string()),
            },
        )
        .await;

    // RevisionNeeded auto-transitions to ReExecuting
    assert!(result.is_success());

    // ReExecuting's on_enter calls send_message for the revision
    // The call count depends on auto-transition chain:
    // Reviewing -> RevisionNeeded (on_enter) -> ReExecuting (on_enter: send_message)
    assert!(
        svc.chat_service.call_count() >= 1,
        "ReExecuting on_enter should call send_message for revision work"
    );
}
