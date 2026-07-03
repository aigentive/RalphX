// State Machine Flow Integration Tests
//
// These tests verify complete task lifecycle flows through the state machine,
// including happy path, QA flows, and human override scenarios.

#![allow(clippy::useless_vec)]
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_imports)]

use ralphx_lib::domain::entities::TaskId;
use ralphx_lib::domain::state_machine::types::QaFailure;
use ralphx_lib::domain::state_machine::{
    FailedData, QaFailedData, State, StateData, TaskContext, TaskEvent, TaskStateMachine,
};
use ralphx_lib::infrastructure::sqlite::TaskStateMachineRepository;
use ralphx_lib::testing::SqliteTestDb;

/// Helper to set up a test environment with a repository and task
fn setup_test() -> (SqliteTestDb, TaskStateMachineRepository, TaskId) {
    let db = SqliteTestDb::new("state-machine-flows");
    let conn = db.new_connection();

    // Insert a project and task
    conn.execute(
        "INSERT INTO projects (id, name, working_directory) VALUES ('proj-1', 'Test', '/path')",
        [],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO tasks (id, project_id, category, title, internal_status)
         VALUES ('task-1', 'proj-1', 'feature', 'Test Task', 'backlog')",
        [],
    )
    .unwrap();

    let repo = TaskStateMachineRepository::new(conn);
    let task_id = TaskId::from_string("task-1".to_string());

    (db, repo, task_id)
}

/// Helper to record transition history
fn record_transitions(
    repo: &TaskStateMachineRepository,
    task_id: &TaskId,
    events: &[TaskEvent],
) -> Vec<(State, State)> {
    let mut transitions = Vec::new();

    for event in events {
        let from = repo.load_state(task_id).unwrap();
        let to = repo.process_event(task_id, event).unwrap();
        transitions.push((from, to));
    }

    transitions
}

// ==================
// Happy Path Tests
// ==================

/// Test: Backlog → Ready → Executing → PendingReview → Reviewing → ReviewPassed → Approved
///
/// This is the simplest complete task lifecycle without QA.
#[test]
fn test_happy_path_without_qa() {
    let (_db, repo, task_id) = setup_test();

    // Track all transitions
    let events = vec![
        TaskEvent::Schedule, // Backlog -> Ready
                             // Ready -> Executing is normally triggered by scheduler picking up the task
                             // For this test, we manually set up Executing state
    ];

    // Schedule: Backlog -> Ready
    let state = repo.process_event(&task_id, &TaskEvent::Schedule).unwrap();
    assert_eq!(state, State::Ready);

    // Simulate starting execution by directly updating state
    // (In real app, the agent scheduler would do this)
    repo.persist_state(&task_id, &State::Executing).unwrap();
    assert_eq!(repo.load_state(&task_id).unwrap(), State::Executing);

    // ExecutionComplete: Executing -> PendingReview (direct, no ExecutionDone)
    let state = repo
        .process_event(&task_id, &TaskEvent::ExecutionComplete)
        .unwrap();
    assert_eq!(state, State::PendingReview);

    // PendingReview auto-transitions to Reviewing (normally done by state machine)
    repo.persist_state(&task_id, &State::Reviewing).unwrap();
    assert_eq!(repo.load_state(&task_id).unwrap(), State::Reviewing);

    // ReviewComplete with approved=true: Reviewing -> ReviewPassed
    let state = repo
        .process_event(
            &task_id,
            &TaskEvent::ReviewComplete {
                approved: true,
                feedback: None,
            },
        )
        .unwrap();
    assert_eq!(state, State::ReviewPassed);

    // Human approves: ReviewPassed -> Approved
    let state = repo
        .process_event(&task_id, &TaskEvent::HumanApprove)
        .unwrap();
    assert_eq!(state, State::Approved);

    // Verify final state is terminal
    if let State::Approved = repo.load_state(&task_id).unwrap() {
        // Success!
    } else {
        panic!("Expected Approved state");
    }
}

/// Test full flow tracking transitions
#[test]
fn test_happy_path_tracks_transitions() {
    let (_db, repo, task_id) = setup_test();

    let mut transition_log = Vec::new();

    // Track transitions using transition_atomically
    let new_state = repo
        .transition_atomically(&task_id, &TaskEvent::Schedule, |from, to| {
            transition_log.push((from.clone(), to.clone()));
            Ok(())
        })
        .unwrap();
    assert_eq!(new_state, State::Ready);

    // Move to executing manually
    repo.persist_state(&task_id, &State::Executing).unwrap();

    // Track ExecutionComplete
    let new_state = repo
        .transition_atomically(&task_id, &TaskEvent::ExecutionComplete, |from, to| {
            transition_log.push((from.clone(), to.clone()));
            Ok(())
        })
        .unwrap();
    assert_eq!(new_state, State::PendingReview);

    // Verify transitions were recorded
    assert_eq!(transition_log.len(), 2);
    assert_eq!(transition_log[0], (State::Backlog, State::Ready));
    assert_eq!(transition_log[1], (State::Executing, State::PendingReview));
}

/// Test that terminal state Approved prevents further transitions
#[test]
fn test_approved_is_terminal() {
    let (_db, repo, task_id) = setup_test();

    // Fast-forward to Approved
    repo.persist_state(&task_id, &State::Approved).unwrap();

    // Try to cancel - should fail
    let result = repo.process_event(&task_id, &TaskEvent::Cancel);
    assert!(result.is_err());

    // State should still be Approved
    assert_eq!(repo.load_state(&task_id).unwrap(), State::Approved);
}

// ==================
// QA Flow Tests
// ==================

/// Test: Executing (with QA) → QaRefining → QaTesting → QaPassed → PendingReview
#[test]
fn test_qa_flow_success() {
    let (_db, repo, task_id) = setup_test();

    // Start in Executing state, then execute completion triggers QA flow
    repo.persist_state(&task_id, &State::Executing).unwrap();

    // ExecutionComplete with QA enabled would go to QaRefining
    // Simulate this transition
    repo.persist_state(&task_id, &State::QaRefining).unwrap();

    // Simulate QA being enabled and moving to QaRefining
    // (In real app, entry action would check context.qa_enabled)
    repo.persist_state(&task_id, &State::QaRefining).unwrap();

    // QaRefinementComplete: QaRefining -> QaTesting
    let state = repo
        .process_event(&task_id, &TaskEvent::QaRefinementComplete)
        .unwrap();
    assert_eq!(state, State::QaTesting);

    // QaTestsComplete with passed=true: QaTesting -> QaPassed
    let state = repo
        .process_event(&task_id, &TaskEvent::QaTestsComplete { passed: true })
        .unwrap();
    assert_eq!(state, State::QaPassed);

    // Simulate moving to PendingReview (entry action)
    repo.persist_state(&task_id, &State::PendingReview).unwrap();
    assert_eq!(repo.load_state(&task_id).unwrap(), State::PendingReview);
}

/// Test: QA failure and retry path
#[test]
fn test_qa_flow_failure_and_retry() {
    let (_db, repo, task_id) = setup_test();

    // Start in QaTesting state
    repo.persist_state(&task_id, &State::QaTesting).unwrap();

    // QaTestsComplete with passed=false: QaTesting -> QaFailed
    let state = repo
        .process_event(&task_id, &TaskEvent::QaTestsComplete { passed: false })
        .unwrap();

    if let State::QaFailed(_) = state {
        // Success - QaFailed state
    } else {
        panic!("Expected QaFailed state");
    }

    // Retry from QaFailed: QaFailed -> RevisionNeeded
    let state = repo.process_event(&task_id, &TaskEvent::Retry).unwrap();
    assert_eq!(state, State::RevisionNeeded);
}

/// Test: QaFailed state preserves failure data
#[test]
fn test_qa_failed_preserves_data() {
    let (_db, repo, task_id) = setup_test();

    // Set up QaFailed with failure data
    let qa_data = QaFailedData::single(QaFailure::new("test_login", "Expected 200, got 401"));
    repo.persist_state(&task_id, &State::QaFailed(qa_data.clone()))
        .unwrap();

    // Reload and verify data is preserved
    if let State::QaFailed(data) = repo.load_state(&task_id).unwrap() {
        assert!(data.has_failures());
        assert_eq!(data.first_error(), Some("Expected 200, got 401"));
    } else {
        panic!("Expected QaFailed state with data");
    }
}

/// Test: RevisionNeeded → Executing loop
#[test]
fn test_revision_needed_to_executing_loop() {
    let (_db, repo, task_id) = setup_test();

    // Start in RevisionNeeded
    repo.persist_state(&task_id, &State::RevisionNeeded)
        .unwrap();

    // Simulate starting re-execution (normally done by agent scheduler)
    repo.persist_state(&task_id, &State::Executing).unwrap();

    // ExecutionComplete again
    let state = repo
        .process_event(&task_id, &TaskEvent::ExecutionComplete)
        .unwrap();
    // With QA disabled (default), goes to PendingReview
    assert_eq!(state, State::PendingReview);
}

// ==================
// Human Override Tests
// ==================

/// Test: HumanApprove from ReviewPassed completes task
#[test]
fn test_human_approve_from_review_passed() {
    let (_db, repo, task_id) = setup_test();

    // Set up ReviewPassed state (AI approved, awaiting human)
    repo.persist_state(&task_id, &State::ReviewPassed).unwrap();

    // HumanApprove: ReviewPassed -> Approved
    let state = repo
        .process_event(&task_id, &TaskEvent::HumanApprove)
        .unwrap();
    assert_eq!(state, State::Approved);
}

/// Test: SkipQa from QaFailed moves to PendingReview
#[test]
fn test_skip_qa_from_qa_failed() {
    let (_db, repo, task_id) = setup_test();

    // Set up QaFailed state with failure data
    let qa_data = QaFailedData::single(QaFailure::new("flaky_test", "Intermittent failure"));
    repo.persist_state(&task_id, &State::QaFailed(qa_data))
        .unwrap();

    // SkipQa: QaFailed -> PendingReview
    let state = repo.process_event(&task_id, &TaskEvent::SkipQa).unwrap();
    assert_eq!(state, State::PendingReview);
}

/// Test: Retry from Failed state
#[test]
fn test_retry_from_failed() {
    let (_db, repo, task_id) = setup_test();

    // Set up Failed state with error data
    let failed_data = FailedData::new("Build timeout").with_details("CI timed out after 60m");
    repo.persist_state(&task_id, &State::Failed(failed_data))
        .unwrap();

    // Retry: Failed -> Ready
    let state = repo.process_event(&task_id, &TaskEvent::Retry).unwrap();
    assert_eq!(state, State::Ready);
}

/// Test: Retry from Cancelled state
#[test]
fn test_retry_from_cancelled() {
    let (_db, repo, task_id) = setup_test();

    // Set up Cancelled state
    repo.persist_state(&task_id, &State::Cancelled).unwrap();

    // Retry: Cancelled -> Ready
    let state = repo.process_event(&task_id, &TaskEvent::Retry).unwrap();
    assert_eq!(state, State::Ready);
}

/// Test: Retry from Approved state (re-open completed task)
#[test]
fn test_retry_from_approved() {
    let (_db, repo, task_id) = setup_test();

    // Set up Approved state
    repo.persist_state(&task_id, &State::Approved).unwrap();

    // Retry: Approved -> Ready
    let state = repo.process_event(&task_id, &TaskEvent::Retry).unwrap();
    assert_eq!(state, State::Ready);
}

/// Test: Retry clears error state (state data is cleaned up)
#[test]
fn test_retry_clears_error_state() {
    let (_db, repo, task_id) = setup_test();

    // Set up Failed state with error data
    let failed_data = FailedData::new("Some error");
    repo.persist_state(&task_id, &State::Failed(failed_data))
        .unwrap();

    // Retry to Ready
    repo.process_event(&task_id, &TaskEvent::Retry).unwrap();

    // Verify state is Ready (simple state, no data)
    assert_eq!(repo.load_state(&task_id).unwrap(), State::Ready);

    // The state_data table entry should be cleaned up by persist_state
    // (tested in unit tests, here we just verify the state)
}

// ==================
// Blocking Flow Tests
// ==================

/// Test: Ready → Blocked (blocker detected) → Ready (blockers resolved)
#[test]
fn test_blocking_flow() {
    let (_db, repo, task_id) = setup_test();

    // Schedule to Ready
    repo.process_event(&task_id, &TaskEvent::Schedule).unwrap();
    assert_eq!(repo.load_state(&task_id).unwrap(), State::Ready);

    // BlockerDetected: Ready -> Blocked
    let state = repo
        .process_event(
            &task_id,
            &TaskEvent::BlockerDetected {
                blocker_id: "task-blocker".to_string(),
            },
        )
        .unwrap();
    assert_eq!(state, State::Blocked);

    // BlockersResolved: Blocked -> Ready
    let state = repo
        .process_event(&task_id, &TaskEvent::BlockersResolved)
        .unwrap();
    assert_eq!(state, State::Ready);
}

/// Test: NeedsHumanInput during execution creates Blocked state
#[test]
fn test_needs_human_input_blocks_execution() {
    let (_db, repo, task_id) = setup_test();

    // Set up Executing state
    repo.persist_state(&task_id, &State::Executing).unwrap();

    // NeedsHumanInput: Executing -> Blocked
    let state = repo
        .process_event(
            &task_id,
            &TaskEvent::NeedsHumanInput {
                reason: "Need API key configuration".to_string(),
            },
        )
        .unwrap();
    assert_eq!(state, State::Blocked);

    // After human provides input, BlockersResolved -> Ready
    let state = repo
        .process_event(&task_id, &TaskEvent::BlockersResolved)
        .unwrap();
    assert_eq!(state, State::Ready);
}

// ==================
// Cancel Flow Tests
// ==================

/// Test: Cancel from various states
#[test]
fn test_cancel_from_various_states() {
    // Cancel from Backlog
    let (_db, repo, task_id) = setup_test();
    let state = repo.process_event(&task_id, &TaskEvent::Cancel).unwrap();
    assert_eq!(state, State::Cancelled);

    // Cancel from Ready
    let (_db, repo, task_id) = setup_test();
    repo.process_event(&task_id, &TaskEvent::Schedule).unwrap();
    let state = repo.process_event(&task_id, &TaskEvent::Cancel).unwrap();
    assert_eq!(state, State::Cancelled);

    // Cancel from Blocked
    let (_db, repo, task_id) = setup_test();
    repo.persist_state(&task_id, &State::Blocked).unwrap();
    let state = repo.process_event(&task_id, &TaskEvent::Cancel).unwrap();
    assert_eq!(state, State::Cancelled);

    // Cancel from Executing
    let (_db, repo, task_id) = setup_test();
    repo.persist_state(&task_id, &State::Executing).unwrap();
    let state = repo.process_event(&task_id, &TaskEvent::Cancel).unwrap();
    assert_eq!(state, State::Cancelled);
}

// ==================
// Failure Flow Tests
// ==================

/// Test: ExecutionFailed creates Failed state with error data
#[test]
fn test_execution_failed_stores_error() {
    let (_db, repo, task_id) = setup_test();

    // Set up Executing state
    repo.persist_state(&task_id, &State::Executing).unwrap();

    // ExecutionFailed: Executing -> Failed
    let state = repo
        .process_event(
            &task_id,
            &TaskEvent::ExecutionFailed {
                error: "Compilation failed: missing semicolon".to_string(),
            },
        )
        .unwrap();

    if let State::Failed(data) = state {
        assert_eq!(data.error, "Compilation failed: missing semicolon");
    } else {
        panic!("Expected Failed state");
    }
}

// ==================
// Review Flow Tests
// ==================

/// Test: Review rejection leads to RevisionNeeded
#[test]
fn test_review_rejection_to_revision_needed() {
    let (_db, repo, task_id) = setup_test();

    // Set up Reviewing state (AI is reviewing)
    repo.persist_state(&task_id, &State::Reviewing).unwrap();

    // ReviewComplete with approved=false: Reviewing -> RevisionNeeded
    let state = repo
        .process_event(
            &task_id,
            &TaskEvent::ReviewComplete {
                approved: false,
                feedback: Some("Please add error handling".to_string()),
            },
        )
        .unwrap();
    assert_eq!(state, State::RevisionNeeded);
}

/// Test: Full review cycle (reject, fix, approve)
#[test]
fn test_full_review_cycle() {
    let (_db, repo, task_id) = setup_test();

    // Start in Reviewing (AI is reviewing)
    repo.persist_state(&task_id, &State::Reviewing).unwrap();

    // Reject: Reviewing -> RevisionNeeded
    repo.process_event(
        &task_id,
        &TaskEvent::ReviewComplete {
            approved: false,
            feedback: Some("Add tests".to_string()),
        },
    )
    .unwrap();
    assert_eq!(repo.load_state(&task_id).unwrap(), State::RevisionNeeded);

    // Re-execute: RevisionNeeded auto-transitions to ReExecuting
    repo.persist_state(&task_id, &State::ReExecuting).unwrap();
    repo.process_event(&task_id, &TaskEvent::ExecutionComplete)
        .unwrap();

    // Skip QA, go to PendingReview then Reviewing
    repo.persist_state(&task_id, &State::Reviewing).unwrap();

    // Approve: Reviewing -> ReviewPassed
    repo.process_event(
        &task_id,
        &TaskEvent::ReviewComplete {
            approved: true,
            feedback: None,
        },
    )
    .unwrap();
    assert_eq!(repo.load_state(&task_id).unwrap(), State::ReviewPassed);

    // Human approves: ReviewPassed -> Approved
    repo.process_event(&task_id, &TaskEvent::HumanApprove)
        .unwrap();
    assert_eq!(repo.load_state(&task_id).unwrap(), State::Approved);
}
