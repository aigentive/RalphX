use super::helpers::{create_context_with_services, create_test_services};
use crate::domain::entities::{ProjectId, Task, TaskId};
use crate::domain::state_machine::services::{NotificationContext, TaskNotification};
use crate::domain::state_machine::types::QaFailedData;
use crate::domain::state_machine::{
    State, TaskEvent, TaskStateMachine, TransitionHandler, TransitionResult,
};

// ==================
// TransitionResult tests
// ==================

#[test]
fn test_transition_result_success() {
    let result = TransitionResult::Success(State::Ready);
    assert!(result.is_success());
    assert_eq!(result.state(), Some(&State::Ready));
}

#[test]
fn test_transition_result_auto_transition() {
    let result = TransitionResult::AutoTransition(State::QaRefining);
    assert!(result.is_success());
    assert_eq!(result.state(), Some(&State::QaRefining));
}

#[test]
fn test_transition_result_not_handled() {
    let result = TransitionResult::NotHandled;
    assert!(!result.is_success());
    assert!(result.state().is_none());
}

// ==================
// Basic transition tests
// ==================

#[tokio::test]
async fn test_backlog_to_ready_transition() {
    let (spawner, _emitter, _notifier, _dep_manager, _review_starter, services) =
        create_test_services();
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    let result = handler
        .handle_transition(&State::Backlog, &TaskEvent::Schedule)
        .await;

    assert!(result.is_success());
    // Without QA enabled, no agent should be spawned on Ready entry
    assert_eq!(spawner.spawn_count(), 0);
}

#[tokio::test]
async fn test_backlog_to_ready_with_qa_enabled() {
    let (spawner, _emitter, _notifier, _dep_manager, _review_starter, services) =
        create_test_services();
    let context = create_context_with_services("task-1", "proj-1", services).with_qa_enabled();
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    let result = handler
        .handle_transition(&State::Backlog, &TaskEvent::Schedule)
        .await;

    assert!(result.is_success());
    // With QA enabled, QA prep should be spawned in background
    let calls = spawner.get_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "spawn_background");
    assert_eq!(calls[0].args, vec!["qa-prep", "task-1"]);
}

// ==================
// ExecutionDone auto-transition tests
// ==================

#[tokio::test]
async fn test_executing_complete_transitions_to_qa_refining_with_qa() {
    let (spawner, _emitter, _notifier, _dep_manager, _review_starter, services) =
        create_test_services();
    let context = create_context_with_services("task-1", "proj-1", services).with_qa_enabled();
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    let result = handler
        .handle_transition(&State::Executing, &TaskEvent::ExecutionComplete)
        .await;

    // Should transition directly to QaRefining
    assert_eq!(result.state(), Some(&State::QaRefining));

    // Should wait for qa-prep and spawn qa-refiner
    let calls = spawner.get_calls();
    assert!(calls
        .iter()
        .any(|c| c.method == "wait_for" && c.args[0] == "qa-prep"));
    assert!(calls
        .iter()
        .any(|c| c.method == "spawn" && c.args[0] == "qa-refiner"));
}

#[tokio::test]
async fn test_executing_complete_transitions_to_pending_review_without_qa() {
    let (_spawner, _emitter, _notifier, _dep_manager, _review_starter, services) =
        create_test_services();
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    let result = handler
        .handle_transition(&State::Executing, &TaskEvent::ExecutionComplete)
        .await;

    // Should transition to PendingReview, then auto-transition to Reviewing
    if let TransitionResult::AutoTransition(state) = &result {
        assert_eq!(*state, State::Reviewing);
    } else {
        panic!("Expected AutoTransition to Reviewing, got {:?}", result);
    }
}

#[tokio::test]
async fn test_executing_complete_with_qa_prep_complete_skips_wait() {
    let (spawner, _emitter, _notifier, _dep_manager, _review_starter, services) =
        create_test_services();
    let context = create_context_with_services("task-1", "proj-1", services)
        .with_qa_enabled()
        .with_qa_prep_complete();
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    let result = handler
        .handle_transition(&State::Executing, &TaskEvent::ExecutionComplete)
        .await;

    assert!(result.is_success());

    // Should NOT wait for qa-prep since it's already complete
    let calls = spawner.get_calls();
    assert!(!calls.iter().any(|c| c.method == "wait_for"));
    // But should still spawn qa-refiner
    assert!(calls
        .iter()
        .any(|c| c.method == "spawn" && c.args[0] == "qa-refiner"));
}

// ==================
// QA state tests
// ==================

#[tokio::test]
async fn test_qa_refining_to_qa_testing() {
    let (spawner, _emitter, _notifier, _dep_manager, _review_starter, services) =
        create_test_services();
    let context = create_context_with_services("task-1", "proj-1", services).with_qa_enabled();
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    let result = handler
        .handle_transition(&State::QaRefining, &TaskEvent::QaRefinementComplete)
        .await;

    assert_eq!(result.state(), Some(&State::QaTesting));

    // Should spawn qa-tester
    let calls = spawner.get_calls();
    assert!(calls
        .iter()
        .any(|c| c.method == "spawn" && c.args[0] == "qa-tester"));
}

#[tokio::test]
async fn test_qa_testing_passed() {
    let (_spawner, emitter, _notifier, _dep_manager, _review_starter, services) =
        create_test_services();
    let context = create_context_with_services("task-1", "proj-1", services).with_qa_enabled();
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    let result = handler
        .handle_transition(
            &State::QaTesting,
            &TaskEvent::QaTestsComplete { passed: true },
        )
        .await;

    // Should auto-transition from QaPassed to PendingReview
    if let TransitionResult::AutoTransition(state) = &result {
        assert_eq!(*state, State::PendingReview);
    } else {
        panic!("Expected AutoTransition, got {:?}", result);
    }

    // Should emit qa_passed event
    assert!(emitter.has_event("qa_passed"));
}

#[tokio::test]
async fn test_qa_testing_failed_notifies_user() {
    let (_spawner, emitter, notifier, _dep_manager, _review_starter, services) =
        create_test_services();
    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "QA notification task".to_string());
    task.id = TaskId::from_string("task-1".to_string());
    let context = create_context_with_services(
        "task-1",
        "proj-1",
        services.with_notification_context(NotificationContext {
            task,
            history_entry_id: "committed-history-qa-failed".to_string(),
            project_id,
        }),
    )
    .with_qa_enabled();
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    let result = handler
        .handle_transition(
            &State::QaTesting,
            &TaskEvent::QaTestsComplete { passed: false },
        )
        .await;

    // Should transition to QaFailed
    assert!(matches!(result.state(), Some(State::QaFailed(_))));

    // Should emit qa_failed event
    assert!(emitter.has_event("qa_failed"));

    // The notification must retain the exact history entry committed by the transition owner.
    let notifications = notifier.notification_records();
    assert_eq!(notifications.len(), 1);
    assert!(matches!(
        notifications[0].notification,
        TaskNotification::StateEntered(crate::domain::entities::InternalStatus::QaFailed)
    ));
    assert_eq!(notifications[0].context.task.id.as_str(), "task-1");
    assert_eq!(
        notifications[0].context.history_entry_id,
        "committed-history-qa-failed"
    );
}

#[tokio::test]
async fn test_qa_failed_skip_qa_transitions_to_pending_review() {
    let (_spawner, _emitter, _notifier, _dep_manager, _review_starter, services) =
        create_test_services();
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    let result = handler
        .handle_transition(
            &State::QaFailed(QaFailedData::default()),
            &TaskEvent::SkipQa,
        )
        .await;

    // Should transition to PendingReview, then auto-transition to Reviewing
    if let TransitionResult::AutoTransition(state) = &result {
        assert_eq!(*state, State::Reviewing);
    } else {
        panic!("Expected AutoTransition to Reviewing, got {:?}", result);
    }
}

#[tokio::test]
async fn test_qa_failed_retry_transitions_to_revision_needed() {
    let (_spawner, _emitter, _notifier, _dep_manager, _review_starter, services) =
        create_test_services();
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    let result = handler
        .handle_transition(&State::QaFailed(QaFailedData::default()), &TaskEvent::Retry)
        .await;

    // Should auto-transition from RevisionNeeded to ReExecuting
    if let TransitionResult::AutoTransition(state) = &result {
        assert_eq!(*state, State::ReExecuting);
    } else {
        panic!("Expected AutoTransition, got {:?}", result);
    }
}

// ==================
// Review and terminal state tests
// ==================

#[tokio::test]
async fn test_reviewing_approved_transitions_to_review_passed() {
    let (_spawner, _emitter, _notifier, _dep_manager, _review_starter, services) =
        create_test_services();
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    let result = handler
        .handle_transition(
            &State::Reviewing,
            &TaskEvent::ReviewComplete {
                approved: true,
                feedback: None,
            },
        )
        .await;

    // Reviewing -> ReviewPassed (awaiting human)
    assert_eq!(result.state(), Some(&State::ReviewPassed));
}

#[tokio::test]
async fn test_review_passed_human_approve_transitions_to_pending_merge() {
    let (_spawner, emitter, _notifier, dep_manager, _review_starter, services) =
        create_test_services();
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    let result = handler
        .handle_transition(&State::ReviewPassed, &TaskEvent::HumanApprove)
        .await;

    // Should auto-transition from Approved to PendingMerge (Phase 66 - merge workflow)
    if let TransitionResult::AutoTransition(state) = &result {
        assert_eq!(*state, State::PendingMerge);
    } else {
        panic!("Expected AutoTransition to PendingMerge, got {:?}", result);
    }

    // Should emit task_completed event (during Approved entry)
    assert!(emitter.has_event("task_completed"));

    // Should NOT unblock dependents at Approved - only at Merged (after successful merge)
    let calls = dep_manager.get_calls();
    assert!(
        !calls.iter().any(|c| c.method == "unblock_dependents"),
        "unblock_dependents should NOT be called at Approved - only at Merged"
    );
}

#[tokio::test]
async fn test_reviewing_rejected_auto_transitions_to_re_executing() {
    let (_spawner, _emitter, _notifier, _dep_manager, _review_starter, services) =
        create_test_services();
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    let result = handler
        .handle_transition(
            &State::Reviewing,
            &TaskEvent::ReviewComplete {
                approved: false,
                feedback: Some("Needs tests".to_string()),
            },
        )
        .await;

    // Should auto-transition from RevisionNeeded to ReExecuting
    if let TransitionResult::AutoTransition(state) = &result {
        assert_eq!(*state, State::ReExecuting);
    } else {
        panic!("Expected AutoTransition, got {:?}", result);
    }

    // Worker is spawned via ExecutionChatService (not agent_spawner)
    // Test passes if the auto-transition completes without panic
}

#[tokio::test]
async fn test_execution_failed_emits_event() {
    let (_spawner, emitter, _notifier, _dep_manager, _review_starter, services) =
        create_test_services();
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    let result = handler
        .handle_transition(
            &State::Executing,
            &TaskEvent::ExecutionFailed {
                error: "Build failed".to_string(),
            },
        )
        .await;

    assert!(matches!(result.state(), Some(State::Failed(_))));

    // Should emit task_failed event
    assert!(emitter.has_event("task_failed"));
}

// ==================
// Event not handled tests
// ==================

#[tokio::test]
async fn test_event_not_handled() {
    let (_spawner, _emitter, _notifier, _dep_manager, _review_starter, services) =
        create_test_services();
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    // Try to schedule from Executing (not allowed)
    let result = handler
        .handle_transition(&State::Executing, &TaskEvent::Schedule)
        .await;

    assert!(!result.is_success());
    assert_eq!(result, TransitionResult::NotHandled);
}
