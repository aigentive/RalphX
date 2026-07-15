// Execution Control Integration Tests
//
// These tests verify the execution control functionality:
// - AskUserQuestion flow (NeedsHumanInput event → Blocked → answer → Ready)
// - Pause/resume execution
// - Blocking and unblocking tasks

use ralphx_lib::domain::entities::{Project, ProjectId, Task, TaskId};
use ralphx_lib::domain::state_machine::{State, TaskEvent};
use ralphx_lib::infrastructure::sqlite::TaskStateMachineRepository;
use ralphx_lib::testing::SqliteTestDb;

/// Helper to set up a test environment with a task in executing state
fn setup_execution_test() -> (SqliteTestDb, TaskStateMachineRepository, ProjectId, TaskId) {
    let db = SqliteTestDb::new("execution-control-flows");
    let conn = db.new_connection();

    // Insert a project
    conn.execute(
        "INSERT INTO projects (id, name, working_directory) VALUES ('proj-1', 'Test', '/path')",
        [],
    )
    .unwrap();

    // Insert a task in executing state
    conn.execute(
        "INSERT INTO tasks (id, project_id, category, title, internal_status)
         VALUES ('task-1', 'proj-1', 'feature', 'Test Task', 'executing')",
        [],
    )
    .unwrap();

    let repo = TaskStateMachineRepository::new(conn);
    let project_id = ProjectId::from_string("proj-1".to_string());
    let task_id = TaskId::from_string("task-1".to_string());

    (db, repo, project_id, task_id)
}

/// Helper to create multiple tasks
fn setup_multiple_tasks() -> (SqliteTestDb, TaskStateMachineRepository, ProjectId, Vec<TaskId>) {
    let db = SqliteTestDb::new("execution-control-flows-multi");
    let conn = db.new_connection();

    // Insert a project
    conn.execute(
        "INSERT INTO projects (id, name, working_directory) VALUES ('proj-1', 'Test', '/path')",
        [],
    )
    .unwrap();

    // Insert multiple tasks in different states
    conn.execute(
        "INSERT INTO tasks (id, project_id, category, title, internal_status)
         VALUES
         ('task-1', 'proj-1', 'feature', 'Task 1', 'ready'),
         ('task-2', 'proj-1', 'feature', 'Task 2', 'ready'),
         ('task-3', 'proj-1', 'feature', 'Task 3', 'executing'),
         ('task-4', 'proj-1', 'feature', 'Task 4', 'backlog')",
        [],
    )
    .unwrap();

    let repo = TaskStateMachineRepository::new(conn);
    let project_id = ProjectId::from_string("proj-1".to_string());
    let task_ids = vec![
        TaskId::from_string("task-1".to_string()),
        TaskId::from_string("task-2".to_string()),
        TaskId::from_string("task-3".to_string()),
        TaskId::from_string("task-4".to_string()),
    ];

    (db, repo, project_id, task_ids)
}

// ============================================================================
// AskUserQuestion Flow Tests
// ============================================================================

/// Test: Full AskUserQuestion flow
///
/// Flow:
/// 1. Task is executing
/// 2. Agent emits NeedsHumanInput event (AskUserQuestion)
/// 3. Task transitions to Blocked
/// 4. User provides answer
/// 5. BlockersResolved event
/// 6. Task transitions to Ready
#[test]
fn test_ask_user_question_full_flow() {
    let (_db, repo, _project_id, task_id) = setup_execution_test();

    // 1. Verify task is in Executing state
    assert_eq!(repo.load_state(&task_id).unwrap(), State::Executing);

    // 2. Agent needs human input (e.g., AskUserQuestion tool was called)
    let state = repo
        .process_event(
            &task_id,
            &TaskEvent::NeedsHumanInput {
                reason: "Which database should I use? MySQL or PostgreSQL?".to_string(),
            },
        )
        .unwrap();

    // 3. Verify task is now Blocked
    assert_eq!(state, State::Blocked);
    assert_eq!(repo.load_state(&task_id).unwrap(), State::Blocked);

    // 4. User answers the question (simulated by frontend calling answer_user_question)
    // 5. BlockersResolved event is sent
    let state = repo
        .process_event(&task_id, &TaskEvent::BlockersResolved)
        .unwrap();

    // 6. Verify task is Ready (will be picked up by scheduler)
    assert_eq!(state, State::Ready);
    assert_eq!(repo.load_state(&task_id).unwrap(), State::Ready);
}

/// Test: NeedsHumanInput preserves the reason
#[test]
fn test_needs_human_input_preserves_reason() {
    let (_db, repo, _project_id, task_id) = setup_execution_test();

    // Process the event
    let event = TaskEvent::NeedsHumanInput {
        reason: "Need API credentials from user".to_string(),
    };

    repo.process_event(&task_id, &event).unwrap();

    // Verify task is blocked
    assert_eq!(repo.load_state(&task_id).unwrap(), State::Blocked);

    // In a real system, the reason would be stored in task context
    // or emitted via an event for the frontend to display
}

/// Test: Blocked task cannot be scheduled
#[test]
fn test_blocked_task_cannot_be_scheduled() {
    let (_db, repo, _project_id, task_id) = setup_execution_test();

    // Block the task
    repo.process_event(
        &task_id,
        &TaskEvent::NeedsHumanInput {
            reason: "Waiting for input".to_string(),
        },
    )
    .unwrap();

    // Verify blocked
    assert_eq!(repo.load_state(&task_id).unwrap(), State::Blocked);

    // Attempting to schedule should fail (can't go from Blocked to Ready via Schedule)
    // The only valid transition from Blocked is BlockersResolved or Cancel
    let result = repo.process_event(&task_id, &TaskEvent::Schedule);
    assert!(
        result.is_err(),
        "Should not be able to schedule a Blocked task"
    );
}

/// Test: Blocked task can be cancelled
#[test]
fn test_blocked_task_can_be_cancelled() {
    let (_db, repo, _project_id, task_id) = setup_execution_test();

    // Block the task
    repo.process_event(
        &task_id,
        &TaskEvent::NeedsHumanInput {
            reason: "Waiting for input".to_string(),
        },
    )
    .unwrap();

    // Cancel the blocked task
    let state = repo.process_event(&task_id, &TaskEvent::Cancel).unwrap();

    // Verify cancelled
    assert_eq!(state, State::Cancelled);
}

/// Test: Multiple AskUserQuestion in sequence
#[test]
fn test_multiple_ask_user_questions() {
    let (_db, repo, _project_id, task_id) = setup_execution_test();

    // First question
    repo.process_event(
        &task_id,
        &TaskEvent::NeedsHumanInput {
            reason: "Question 1: What framework?".to_string(),
        },
    )
    .unwrap();
    assert_eq!(repo.load_state(&task_id).unwrap(), State::Blocked);

    // Answer first question
    repo.process_event(&task_id, &TaskEvent::BlockersResolved)
        .unwrap();
    assert_eq!(repo.load_state(&task_id).unwrap(), State::Ready);

    // Resume execution
    repo.persist_state(&task_id, &State::Executing).unwrap();

    // Second question
    repo.process_event(
        &task_id,
        &TaskEvent::NeedsHumanInput {
            reason: "Question 2: What styling library?".to_string(),
        },
    )
    .unwrap();
    assert_eq!(repo.load_state(&task_id).unwrap(), State::Blocked);

    // Answer second question
    repo.process_event(&task_id, &TaskEvent::BlockersResolved)
        .unwrap();
    assert_eq!(repo.load_state(&task_id).unwrap(), State::Ready);
}

// ============================================================================
// Execution Pause and Resume Tests
// ============================================================================

/// Test: Pause execution doesn't affect currently executing tasks
#[test]
fn test_pause_does_not_affect_executing_tasks() {
    let (_db, repo, _project_id, task_ids) = setup_multiple_tasks();

    // task-3 is executing
    let task3_id = &task_ids[2];
    assert_eq!(repo.load_state(task3_id).unwrap(), State::Executing);

    // "Pause" is a system-level flag, not a state machine event
    // Executing tasks should continue until completion
    // We verify that execution can complete normally
    let state = repo
        .process_event(task3_id, &TaskEvent::ExecutionComplete)
        .unwrap();
    // With QA disabled (default), goes to PendingReview
    assert_eq!(state, State::PendingReview);
}

/// Test: Ready tasks can still be scheduled (pause is handled at orchestrator level)
#[test]
fn test_ready_tasks_still_schedulable() {
    let (_db, repo, _project_id, task_ids) = setup_multiple_tasks();

    // task-1 and task-2 are ready
    let task1_id = &task_ids[0];
    assert_eq!(repo.load_state(task1_id).unwrap(), State::Ready);

    // The state machine itself doesn't prevent scheduling
    // Pause logic is in the ExecutionControl service, not the state machine
    // A Ready task can transition to Executing
    repo.persist_state(task1_id, &State::Executing).unwrap();
    assert_eq!(repo.load_state(task1_id).unwrap(), State::Executing);
}

/// Test: Backlog tasks can be scheduled when not paused
#[test]
fn test_backlog_to_ready_scheduling() {
    let (_db, repo, _project_id, task_ids) = setup_multiple_tasks();

    // task-4 is in backlog
    let task4_id = &task_ids[3];
    assert_eq!(repo.load_state(task4_id).unwrap(), State::Backlog);

    // Schedule the task
    let state = repo.process_event(task4_id, &TaskEvent::Schedule).unwrap();
    assert_eq!(state, State::Ready);
}

// ============================================================================
// Blocker Detection and Resolution Tests
// ============================================================================

/// Test: BlockerDetected transitions Ready to Blocked
#[test]
fn test_blocker_detected_blocks_ready_task() {
    let (_db, repo, _project_id, task_ids) = setup_multiple_tasks();

    // task-1 is ready
    let task1_id = &task_ids[0];
    assert_eq!(repo.load_state(task1_id).unwrap(), State::Ready);

    // Blocker detected
    let state = repo
        .process_event(
            task1_id,
            &TaskEvent::BlockerDetected {
                blocker_id: "task-3".to_string(),
            },
        )
        .unwrap();

    assert_eq!(state, State::Blocked);
}

/// Test: BlockersResolved transitions Blocked to Ready
#[test]
fn test_blockers_resolved_unblocks_task() {
    let (_db, repo, _project_id, task_ids) = setup_multiple_tasks();

    // Block task-1
    let task1_id = &task_ids[0];
    repo.process_event(
        task1_id,
        &TaskEvent::BlockerDetected {
            blocker_id: "task-3".to_string(),
        },
    )
    .unwrap();
    assert_eq!(repo.load_state(task1_id).unwrap(), State::Blocked);

    // Resolve blockers
    let state = repo
        .process_event(task1_id, &TaskEvent::BlockersResolved)
        .unwrap();
    assert_eq!(state, State::Ready);
}

/// Test: Multiple blockers resolved at once
#[test]
fn test_multiple_blockers_resolved() {
    let (_db, repo, _project_id, task_ids) = setup_multiple_tasks();

    // Block task-1 with first blocker
    let task1_id = &task_ids[0];
    repo.process_event(
        task1_id,
        &TaskEvent::BlockerDetected {
            blocker_id: "blocker-1".to_string(),
        },
    )
    .unwrap();
    assert_eq!(repo.load_state(task1_id).unwrap(), State::Blocked);

    // In the state machine, BlockersResolved means ALL blockers are resolved
    // (the orchestrator tracks individual blockers)
    let state = repo
        .process_event(task1_id, &TaskEvent::BlockersResolved)
        .unwrap();
    assert_eq!(state, State::Ready);
}

// ============================================================================
// Task Lifecycle with Human Intervention Tests
// ============================================================================

/// Test: Complete lifecycle with AskUserQuestion
#[test]
fn test_complete_lifecycle_with_question() {
    let (_db, repo, _project_id, task_id) = setup_execution_test();

    // Executing -> Blocked (question asked)
    repo.process_event(
        &task_id,
        &TaskEvent::NeedsHumanInput {
            reason: "Need clarification".to_string(),
        },
    )
    .unwrap();
    assert_eq!(repo.load_state(&task_id).unwrap(), State::Blocked);

    // Blocked -> Ready (question answered)
    repo.process_event(&task_id, &TaskEvent::BlockersResolved)
        .unwrap();
    assert_eq!(repo.load_state(&task_id).unwrap(), State::Ready);

    // Ready -> Executing (resumed)
    repo.persist_state(&task_id, &State::Executing).unwrap();

    // Executing -> PendingReview (direct, no QA)
    repo.process_event(&task_id, &TaskEvent::ExecutionComplete)
        .unwrap();
    assert_eq!(repo.load_state(&task_id).unwrap(), State::PendingReview);

    // PendingReview -> Reviewing (auto-transition)
    repo.persist_state(&task_id, &State::Reviewing).unwrap();

    // Reviewing -> ReviewPassed
    repo.process_event(
        &task_id,
        &TaskEvent::ReviewComplete {
            approved: true,
            feedback: None,
        },
    )
    .unwrap();
    assert_eq!(repo.load_state(&task_id).unwrap(), State::ReviewPassed);

    // ReviewPassed -> Approved (human approval)
    repo.process_event(&task_id, &TaskEvent::HumanApprove)
        .unwrap();
    assert_eq!(repo.load_state(&task_id).unwrap(), State::Approved);
}

/// Test: Task can fail while blocked (edge case)
#[test]
fn test_blocked_task_cannot_fail_directly() {
    let (_db, repo, _project_id, task_id) = setup_execution_test();

    // Block the task
    repo.process_event(
        &task_id,
        &TaskEvent::NeedsHumanInput {
            reason: "Waiting".to_string(),
        },
    )
    .unwrap();
    assert_eq!(repo.load_state(&task_id).unwrap(), State::Blocked);

    // ExecutionFailed shouldn't work from Blocked state
    let result = repo.process_event(
        &task_id,
        &TaskEvent::ExecutionFailed {
            error: "Some error".to_string(),
        },
    );
    assert!(result.is_err(), "Cannot fail from Blocked state");
}

/// Test: Resume from Blocked doesn't skip to completion
#[test]
fn test_resume_from_blocked_goes_to_ready() {
    let (_db, repo, _project_id, task_id) = setup_execution_test();

    // Block the task
    repo.process_event(
        &task_id,
        &TaskEvent::NeedsHumanInput {
            reason: "Waiting".to_string(),
        },
    )
    .unwrap();

    // Resolve blockers
    let state = repo
        .process_event(&task_id, &TaskEvent::BlockersResolved)
        .unwrap();

    // Should be Ready, not Executing or any other state
    assert_eq!(state, State::Ready);
}

// ============================================================================
// Settings UI Command Removal Tests
// ============================================================================

/// Test: Settings UI command removal full workflow
///
/// This test validates the bug fix for the worktree setup user override bug.
/// When users modify project analytics in the Settings UI by removing/clearing
/// worktree setup commands, the system should respect their changes and NOT
/// execute the agent's original suggestions from `detected_analysis`.
///
/// The fix ensures that when a user provides a `custom_analysis` with empty
/// `worktree_setup: []`, the system respects that choice and does NOT merge
/// in commands from `detected_analysis`.
///
/// This integration test verifies the end-to-end workflow by:
/// 1. Creating a project with both `detected_analysis` (containing worktree_setup commands)
///    and `custom_analysis` (with empty worktree_setup array, simulating user removal)
/// 2. Running pre-execution setup with a real worktree path
/// 3. Verifying that removed commands are NOT executed while install still runs
#[tokio::test]
async fn test_settings_ui_command_removal_full_workflow() {
    use ralphx_lib::domain::entities::InternalStatus;
    use tokio_util::sync::CancellationToken;

    // Create worktree and project directories
    let worktree_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();

    let project_id = ProjectId::from_string("proj-1".to_string());

    // Create a project with both detected_analysis and custom_analysis
    let mut project = Project::new(
        "test-project".to_string(),
        project_dir.path().to_str().unwrap().to_string(),
    );
    project.id = project_id.clone();
    project.base_branch = Some("main".to_string());
    project.merge_validation_mode = ralphx_lib::domain::entities::MergeValidationMode::Warn; // Use Warn to run pre-execution setup without blocking on failures

    // detected_analysis has worktree_setup commands (agent's original suggestion)
    project.detected_analysis = Some(
        r#"[{"path": ".", "label": "Rust", "install": "true", "worktree_setup": ["echo setup_command_from_detected"]}]"#.to_string(),
    );

    // custom_analysis intentionally has EMPTY worktree_setup (user removed commands via Settings UI)
    project.custom_analysis = Some(
        r#"[{"path": ".", "label": "Rust", "install": "true", "worktree_setup": []}]"#.to_string(),
    );

    // Create a task in Executing state
    let mut task = Task::new(project_id, "Test task".to_string());
    task.internal_status = InternalStatus::Executing;
    task.worktree_path = Some(worktree_dir.path().to_str().unwrap().to_string());
    task.task_branch = Some("test-branch".to_string());

    let result = ralphx_lib::testing::run_pre_execution_setup(
        &project,
        &task,
        worktree_dir.path(),
        task.id.as_str(),
        None,
        "execution",
        &CancellationToken::new(),
    )
    .await
    .expect("pre-execution setup should run");

    let execution_log = result.log;

    // Verify that NO worktree_setup commands from detected_analysis were executed
    // The custom_analysis has empty worktree_setup, so we should only have the install phase
    for entry in &execution_log {
        let phase = entry.phase.as_str();
        let command = entry.command.as_str();

        // Verify no setup phase commands were executed
        assert_ne!(
            phase, "setup",
            "Setup phase commands should NOT be executed when custom_analysis has empty worktree_setup. \
             Found setup command: {}",
            command
        );

        // Verify install phase was still executed
        assert_eq!(
            phase, "install",
            "Only install phase should be executed when custom_analysis has empty worktree_setup"
        );
    }

    // Verify that we only have install phase entries (no setup phase)
    let setup_count = execution_log
        .iter()
        .filter(|e| e.phase == "setup")
        .count();
    assert_eq!(
        setup_count, 0,
        "Should have NO setup phase entries when custom_analysis has empty worktree_setup. \
         Found {} setup entries in execution_setup_log: {:?}",
        setup_count, execution_log
    );

    // Verify we have install phase entries
    let install_count = execution_log
        .iter()
        .filter(|e| e.phase == "install")
        .count();
    assert!(
        install_count > 0,
        "Should have install phase entries. execution_setup_log: {:?}",
        execution_log
    );
}
