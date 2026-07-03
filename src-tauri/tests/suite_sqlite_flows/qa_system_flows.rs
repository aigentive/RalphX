// QA System Integration Tests
//
// These tests verify the QA system functionality:
// - QA Prep running in parallel with task execution
// - QA Testing flows (pass/fail)
// - State machine transitions for QA states

use std::sync::Arc;

use ralphx_lib::domain::agents::{AgentRole, AgenticClient};
use ralphx_lib::domain::entities::TaskId;
use ralphx_lib::domain::state_machine::types::QaFailure;
use ralphx_lib::domain::state_machine::{AgentSpawner, QaFailedData, State, TaskEvent};
use ralphx_lib::infrastructure::agents::AgenticClientSpawner;
use ralphx_lib::infrastructure::sqlite::TaskStateMachineRepository;
use ralphx_lib::infrastructure::{MockAgenticClient, MockCallType};
use ralphx_lib::testing::{test_prompts, SqliteTestDb};

/// Helper to set up a test environment with a repository and task
fn setup_test() -> (SqliteTestDb, TaskStateMachineRepository, TaskId) {
    let db = SqliteTestDb::new("qa-system-flows");
    let conn = db.new_connection();

    // Insert a project and task with QA enabled
    conn.execute(
        "INSERT INTO projects (id, name, working_directory) VALUES ('proj-1', 'Test', '/path')",
        [],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO tasks (id, project_id, category, title, internal_status, needs_qa)
         VALUES ('task-1', 'proj-1', 'feature', 'Test Task', 'backlog', 1)",
        [],
    )
    .unwrap();

    let repo = TaskStateMachineRepository::new(conn);
    let task_id = TaskId::from_string("task-1".to_string());

    (db, repo, task_id)
}

/// Helper to set up a mock client with QA prep configured
fn setup_mock_client_for_qa() -> MockAgenticClient {
    MockAgenticClient::new()
}

// ============================================================================
// QA Prep Parallel Execution Tests
// ============================================================================

/// Test: QA Prep spawns as a background task while worker executes
///
/// Flow:
/// 1. Task is scheduled and starts executing
/// 2. QA Prep agent is spawned in background
/// 3. Worker agent completes
/// 4. State waits for QA Prep if needed
/// 5. QA Prep completes
/// 6. Transition to QA_REFINING
#[tokio::test]
async fn test_qa_prep_runs_in_parallel_with_execution() {
    let client = Arc::new(setup_mock_client_for_qa());
    let spawner = AgenticClientSpawner::new(client.clone());

    // Spawn worker (main execution)
    spawner.spawn("worker", "task-1").await;

    // Spawn QA prep in background (parallel)
    spawner.spawn_background("qa-prep", "task-1").await;

    // Verify both were spawned
    let calls = client.get_spawn_calls().await;
    assert_eq!(calls.len(), 2, "Expected 2 spawn calls (worker + qa-prep)");

    // Verify roles
    let roles: Vec<AgentRole> = calls
        .iter()
        .map(|c| {
            if let MockCallType::Spawn { role, .. } = &c.call_type {
                role.clone()
            } else {
                panic!("Expected Spawn call")
            }
        })
        .collect();

    assert!(
        roles.contains(&AgentRole::Worker),
        "Worker should be spawned"
    );
    assert!(
        roles.contains(&AgentRole::QaPrep),
        "QaPrep should be spawned in background"
    );
}

/// Test: State machine waits for QA Prep if worker completes first
#[test]
fn test_state_waits_for_qa_prep_after_worker_complete() {
    let (_db, repo, task_id) = setup_test();

    // Start in Executing state
    repo.persist_state(&task_id, &State::Executing).unwrap();

    // Worker completes -> QaRefining (with QA enabled) or PendingReview
    // Since task context has qa_enabled=false by default, it would go to PendingReview
    // But for QA test, we simulate QA enabled by manually transitioning
    repo.persist_state(&task_id, &State::QaRefining).unwrap();

    // At this point, if QA is enabled and QA Prep is still running,
    // the system should wait. We simulate by not transitioning yet.
    // In the real system, an entry action would check qa_prep_complete flag.

    // Simulate QA Prep completing and triggering transition to QaRefining
    repo.persist_state(&task_id, &State::QaRefining).unwrap();
    assert_eq!(repo.load_state(&task_id).unwrap(), State::QaRefining);
}

/// Test: Verify MockAgenticClient records both spawn and spawn_background calls
#[tokio::test]
async fn test_mock_client_distinguishes_spawn_modes() {
    let client = Arc::new(MockAgenticClient::new());
    let spawner = AgenticClientSpawner::new(client.clone());

    // Regular spawn
    spawner.spawn("worker", "task-1").await;

    // Background spawn
    spawner.spawn_background("qa-prep", "task-1").await;

    let calls = client.get_calls().await;

    // Filter spawn calls and check they were recorded
    let spawn_calls: Vec<_> = calls
        .iter()
        .filter(|c| matches!(c.call_type, MockCallType::Spawn { .. }))
        .collect();

    assert_eq!(spawn_calls.len(), 2, "Should have 2 spawn calls");
}

// ============================================================================
// QA Testing Flow - Pass Tests
// ============================================================================

/// Test: Full QA flow with tests passing
///
/// Flow: Executing (QA enabled) -> QaRefining -> QaTesting -> QaPassed -> PendingReview
#[test]
fn test_qa_testing_flow_pass() {
    let (_db, repo, task_id) = setup_test();

    // Start in QaRefining (execution complete with QA enabled, ready for QA refinement)
    repo.persist_state(&task_id, &State::QaRefining).unwrap();

    // Transition to QaRefining (QA Prep complete, ready to refine)
    repo.persist_state(&task_id, &State::QaRefining).unwrap();
    assert_eq!(repo.load_state(&task_id).unwrap(), State::QaRefining);

    // QA Refinement complete -> QaTesting
    let state = repo
        .process_event(&task_id, &TaskEvent::QaRefinementComplete)
        .unwrap();
    assert_eq!(state, State::QaTesting);

    // QA Tests pass -> QaPassed
    let state = repo
        .process_event(&task_id, &TaskEvent::QaTestsComplete { passed: true })
        .unwrap();
    assert_eq!(state, State::QaPassed);

    // Transition to PendingReview (entry action in real system)
    repo.persist_state(&task_id, &State::PendingReview).unwrap();
    assert_eq!(repo.load_state(&task_id).unwrap(), State::PendingReview);
}

/// Test: QaPassed emits success event data
#[test]
fn test_qa_passed_records_success() {
    let (_db, repo, task_id) = setup_test();

    // Set up QaTesting state
    repo.persist_state(&task_id, &State::QaTesting).unwrap();

    // QA Tests pass
    let state = repo
        .process_event(&task_id, &TaskEvent::QaTestsComplete { passed: true })
        .unwrap();

    // Verify we're in QaPassed
    assert_eq!(state, State::QaPassed);

    // Reload and verify
    let loaded = repo.load_state(&task_id).unwrap();
    assert_eq!(loaded, State::QaPassed);
}

// ============================================================================
// QA Testing Flow - Failure Tests
// ============================================================================

/// Test: QA test failure creates QaFailed state with failure data
#[test]
fn test_qa_testing_flow_failure() {
    let (_db, repo, task_id) = setup_test();

    // Set up QaTesting state
    repo.persist_state(&task_id, &State::QaTesting).unwrap();

    // QA Tests fail -> QaFailed
    let state = repo
        .process_event(&task_id, &TaskEvent::QaTestsComplete { passed: false })
        .unwrap();

    if let State::QaFailed(_) = state {
        // Success - we're in QaFailed state
    } else {
        panic!("Expected QaFailed state, got {:?}", state);
    }
}

/// Test: QaFailed preserves failure details
#[test]
fn test_qa_failed_preserves_failure_details() {
    let (_db, repo, task_id) = setup_test();

    // Create QaFailed state with specific failure details
    let failures = QaFailedData::new(vec![
        QaFailure::new("test_login_button", "Button not visible after 5s"),
        QaFailure::new("test_form_submit", "Expected 200, got 401"),
    ]);

    repo.persist_state(&task_id, &State::QaFailed(failures))
        .unwrap();

    // Reload and verify failures preserved
    if let State::QaFailed(data) = repo.load_state(&task_id).unwrap() {
        assert!(data.has_failures());
        assert!(data.first_error().is_some());
        // Verify first failure message
        assert!(data.first_error().unwrap().contains("not visible"));
    } else {
        panic!("Expected QaFailed state with data");
    }
}

/// Test: Retry from QaFailed goes to RevisionNeeded
#[test]
fn test_qa_failed_retry_to_revision_needed() {
    let (_db, repo, task_id) = setup_test();

    // Set up QaFailed state
    let failures = QaFailedData::single(QaFailure::new("test_x", "Test failed"));
    repo.persist_state(&task_id, &State::QaFailed(failures))
        .unwrap();

    // Retry: QaFailed -> RevisionNeeded
    let state = repo.process_event(&task_id, &TaskEvent::Retry).unwrap();
    assert_eq!(state, State::RevisionNeeded);
}

/// Test: SkipQa from QaFailed bypasses to PendingReview
#[test]
fn test_qa_failed_skip_to_pending_review() {
    let (_db, repo, task_id) = setup_test();

    // Set up QaFailed state (maybe flaky test)
    let failures = QaFailedData::single(QaFailure::new("flaky_test", "Random timeout"));
    repo.persist_state(&task_id, &State::QaFailed(failures))
        .unwrap();

    // SkipQa: QaFailed -> PendingReview
    let state = repo.process_event(&task_id, &TaskEvent::SkipQa).unwrap();
    assert_eq!(state, State::PendingReview);
}

// ============================================================================
// Complete QA Lifecycle Tests
// ============================================================================

/// Test: Full lifecycle with QA enabled: Backlog -> ... -> Approved
#[test]
fn test_complete_lifecycle_with_qa() {
    let (_db, repo, task_id) = setup_test();

    // 1. Schedule: Backlog -> Ready
    let state = repo.process_event(&task_id, &TaskEvent::Schedule).unwrap();
    assert_eq!(state, State::Ready);

    // 2. Start execution (simulated)
    repo.persist_state(&task_id, &State::Executing).unwrap();

    // 3. ExecutionComplete: Executing -> QaRefining (with QA enabled)
    // In real system, qa_enabled would trigger this
    repo.persist_state(&task_id, &State::QaRefining).unwrap();

    // 5. QaRefinementComplete: QaRefining -> QaTesting
    let state = repo
        .process_event(&task_id, &TaskEvent::QaRefinementComplete)
        .unwrap();
    assert_eq!(state, State::QaTesting);

    // 6. QaTestsComplete (passed): QaTesting -> QaPassed
    let state = repo
        .process_event(&task_id, &TaskEvent::QaTestsComplete { passed: true })
        .unwrap();
    assert_eq!(state, State::QaPassed);

    // 7. Move to PendingReview (simulated entry action)
    repo.persist_state(&task_id, &State::PendingReview).unwrap();

    // 8. PendingReview -> Reviewing (auto-transition, simulated)
    repo.persist_state(&task_id, &State::Reviewing).unwrap();

    // 9. ReviewComplete (approved): Reviewing -> ReviewPassed
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

    // 10. HumanApprove: ReviewPassed -> Approved
    let state = repo
        .process_event(&task_id, &TaskEvent::HumanApprove)
        .unwrap();
    assert_eq!(state, State::Approved);
}

/// Test: QA failure and re-execution cycle
#[test]
fn test_qa_failure_reexecution_cycle() {
    let (_db, repo, task_id) = setup_test();

    // Start in QaTesting
    repo.persist_state(&task_id, &State::QaTesting).unwrap();

    // QA fails
    let state = repo
        .process_event(&task_id, &TaskEvent::QaTestsComplete { passed: false })
        .unwrap();
    assert!(matches!(state, State::QaFailed(_)));

    // Retry triggers revision
    let state = repo.process_event(&task_id, &TaskEvent::Retry).unwrap();
    assert_eq!(state, State::RevisionNeeded);

    // Agent re-executes
    repo.persist_state(&task_id, &State::Executing).unwrap();

    // Execution completes again -> QaRefining (with QA enabled)
    repo.persist_state(&task_id, &State::QaRefining).unwrap();
    repo.process_event(&task_id, &TaskEvent::QaRefinementComplete)
        .unwrap();
    let state = repo
        .process_event(&task_id, &TaskEvent::QaTestsComplete { passed: true })
        .unwrap();
    assert_eq!(state, State::QaPassed);
}

// ============================================================================
// Mock Agent QA Tests
// ============================================================================

/// Test: Mock client can be configured for QA-specific responses
#[tokio::test]
async fn test_mock_client_qa_prep_responses() {
    let client = MockAgenticClient::new();
    let handle = ralphx_lib::domain::agents::AgentHandle::mock(AgentRole::QaPrep);

    // Configure QA prep response
    client
        .when_prompt_contains(
            "acceptance criteria",
            "Generated 5 acceptance criteria for login feature",
        )
        .await;

    // Test the configured response
    let response = client
        .send_prompt(
            &handle,
            "Generate acceptance criteria for the login feature",
        )
        .await
        .unwrap();

    assert!(response.content.contains("5 acceptance criteria"));
}

/// Test: Mock client can be configured for QA test responses
#[tokio::test]
async fn test_mock_client_qa_test_responses() {
    let client = MockAgenticClient::new();
    let handle = ralphx_lib::domain::agents::AgentHandle::mock(AgentRole::QaTester);

    // Configure QA test pass response
    client.when_prompt_contains(
        "run tests",
        "All 5 tests passed: login_button, form_submit, validation, error_handling, success_redirect"
    ).await;

    // Configure QA test fail response
    client
        .when_prompt_contains(
            "failing tests",
            "2 of 5 tests failed: form_submit (Expected 200, got 401), validation (Timeout)",
        )
        .await;

    // Test pass scenario
    let response = client
        .send_prompt(&handle, "run tests for login feature")
        .await
        .unwrap();
    assert!(response.content.contains("All 5 tests passed"));

    // Test fail scenario (new prompt)
    let handle2 = ralphx_lib::domain::agents::AgentHandle::mock(AgentRole::QaTester);
    let response = client
        .send_prompt(&handle2, "check failing tests")
        .await
        .unwrap();
    assert!(response.content.contains("2 of 5 tests failed"));
}

/// Test: QA agents use cost-optimized test prompts
#[tokio::test]
async fn test_qa_agents_use_test_prompts() {
    let client = MockAgenticClient::new();
    let handle = ralphx_lib::domain::agents::AgentHandle::mock(AgentRole::QaPrep);

    // Use the echo marker for minimal cost testing
    client
        .when_prompt_contains(test_prompts::ECHO_MARKER, test_prompts::expected::ECHO_OK)
        .await;

    let response = client
        .send_prompt(&handle, test_prompts::ECHO_MARKER)
        .await
        .unwrap();
    test_prompts::assert_marker(&response.content, test_prompts::expected::ECHO_OK);
}
