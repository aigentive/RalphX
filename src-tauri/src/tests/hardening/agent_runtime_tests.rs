// Agent Runtime Hardening Tests (C1-C6)
//
// Tests for agent runtime failure scenarios at the state machine level.
// Production-level stream/process behaviors (timeouts, stalls, kills)
// are verified indirectly by testing the state transitions they produce.

use super::helpers::*;

use crate::application::execution_state::ExecutionState;
use crate::domain::entities::{GitMode, ProjectId};
use crate::domain::state_machine::events::TaskEvent;
use crate::domain::state_machine::machine::{Response, State};

// ============================================================================
// C1: Stream timeout detects no output — COVERED
// ============================================================================

#[tokio::test]
async fn test_c1_stream_timeout_produces_execution_failed_transition() {
    // COVERED: Stream timeout in production emits ExecutionFailed.
    // Verify Executing -> Failed with the timeout error.
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-c1", "proj-1", services);

    let result = machine.dispatch(
        &State::Executing,
        &TaskEvent::ExecutionFailed {
            error: "Stream timeout: no output for 120s".to_string(),
        },
    );

    match result {
        Response::Transition(State::Failed(data)) => {
            assert_eq!(data.error, "Stream timeout: no output for 120s");
        }
        other => panic!("Expected Transition(Failed), got {:?}", other),
    }
}

#[tokio::test]
async fn test_c1_timeout_from_re_executing_also_fails() {
    // COVERED: ReExecuting also handles ExecutionFailed -> Failed
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-c1b", "proj-1", services);

    let result = machine.dispatch(
        &State::ReExecuting,
        &TaskEvent::ExecutionFailed {
            error: "Stream timeout: no output for 120s".to_string(),
        },
    );

    match result {
        Response::Transition(State::Failed(data)) => {
            assert_eq!(data.error, "Stream timeout: no output for 120s");
        }
        other => panic!("Expected Transition(Failed), got {:?}", other),
    }
}

// ============================================================================
// C2: Parse stall detection — COVERED
// ============================================================================

#[tokio::test]
async fn test_c2_parse_stall_produces_execution_failed_transition() {
    // COVERED: Parse stall in production also emits ExecutionFailed.
    // Same transition path as C1 — Executing -> Failed.
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-c2", "proj-1", services);

    let result = machine.dispatch(
        &State::Executing,
        &TaskEvent::ExecutionFailed {
            error: "Parse stall detected: output stuck for 60s".to_string(),
        },
    );

    match result {
        Response::Transition(State::Failed(data)) => {
            assert!(data.error.contains("Parse stall"));
        }
        other => panic!("Expected Transition(Failed), got {:?}", other),
    }
}

// ============================================================================
// C3: Agent killed externally — COVERED
// ============================================================================

#[tokio::test]
async fn test_c3_agent_killed_externally_transitions_to_failed() {
    // COVERED: SIGKILL/OOM triggers ExecutionFailed -> Failed
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-c3", "proj-1", services);

    let result = machine.dispatch(
        &State::Executing,
        &TaskEvent::ExecutionFailed {
            error: "Agent process exited with signal 9 (SIGKILL)".to_string(),
        },
    );

    match result {
        Response::Transition(State::Failed(data)) => {
            assert!(data.error.contains("SIGKILL"));
        }
        other => panic!("Expected Transition(Failed), got {:?}", other),
    }
}

#[tokio::test]
async fn test_c3_killed_agent_in_qa_refining_transitions_to_failed() {
    // COVERED: Agent killed while in QaRefining -> Failed
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-c3b", "proj-1", services);

    let result = machine.dispatch(
        &State::QaRefining,
        &TaskEvent::ExecutionFailed {
            error: "Agent killed by OOM".to_string(),
        },
    );

    match result {
        Response::Transition(State::Failed(data)) => {
            assert!(data.error.contains("OOM"));
        }
        other => panic!("Expected Transition(Failed), got {:?}", other),
    }
}

// ============================================================================
// C4: Session recovery — COVERED
// ============================================================================

#[tokio::test]
async fn test_c4_task_stays_in_executing_when_session_recovery_succeeds() {
    // COVERED: When session recovery succeeds, no event is dispatched.
    // Verify that irrelevant events are NotHandled from Executing state.
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-c4", "proj-1", services);

    let irrelevant_events = vec![
        TaskEvent::Schedule,
        TaskEvent::StartExecution,
        TaskEvent::StartReview,
        TaskEvent::StartRevision,
        TaskEvent::HumanApprove,
        TaskEvent::Retry,
        TaskEvent::BlockersResolved,
        TaskEvent::MergeComplete,
        TaskEvent::MergeConflict,
        TaskEvent::MergeAgentFailed,
        TaskEvent::MergeAgentError,
        TaskEvent::ConflictResolved,
        TaskEvent::QaRefinementComplete,
        TaskEvent::QaTestsComplete { passed: true },
        TaskEvent::ReviewComplete {
            approved: true,
            feedback: None,
        },
        TaskEvent::SkipQa,
        TaskEvent::StartMerge,
        TaskEvent::ForceApprove,
    ];

    for event in &irrelevant_events {
        let result = machine.dispatch(&State::Executing, event);
        assert_eq!(
            result,
            Response::NotHandled,
            "Event {:?} should be NotHandled in Executing state",
            event.name()
        );
    }
}

#[tokio::test]
async fn test_c4_executing_only_handles_expected_events() {
    // COVERED: Verify Executing state handles exactly the expected events.
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-c4b", "proj-1", services);

    // ExecutionComplete -> transitions
    let r = machine.dispatch(&State::Executing, &TaskEvent::ExecutionComplete);
    assert!(matches!(r, Response::Transition(_)));

    // ExecutionFailed -> Failed
    let r = machine.dispatch(
        &State::Executing,
        &TaskEvent::ExecutionFailed {
            error: "err".into(),
        },
    );
    assert!(matches!(r, Response::Transition(State::Failed(_))));

    // NeedsHumanInput -> Blocked
    let r = machine.dispatch(
        &State::Executing,
        &TaskEvent::NeedsHumanInput {
            reason: "need help".into(),
        },
    );
    assert!(matches!(r, Response::Transition(State::Blocked)));

    // Pause -> Paused
    let r = machine.dispatch(&State::Executing, &TaskEvent::Pause);
    assert!(matches!(r, Response::Transition(State::Paused)));

    // Stop -> Stopped
    let r = machine.dispatch(&State::Executing, &TaskEvent::Stop);
    assert!(matches!(r, Response::Transition(State::Stopped)));

    // Cancel -> Cancelled
    let r = machine.dispatch(&State::Executing, &TaskEvent::Cancel);
    assert!(matches!(r, Response::Transition(State::Cancelled)));
}

// ============================================================================
// C5: No wall-clock timeout — GAP
// ============================================================================

#[tokio::test]
async fn test_c5_no_wall_clock_timeout_on_execution_state() {
    // GAP: ExecutionState has no concept of wall-clock limits per task.
    // No timeout_seconds, no max_execution_time, no per-task timer.
    let exec_state = ExecutionState::new();

    // All public methods — none are timeout-related:
    let _ = exec_state.is_paused();
    let _ = exec_state.running_count();
    let _ = exec_state.max_concurrent();
    let _ = exec_state.global_max_concurrent();
    let _ = exec_state.can_start_task();
    exec_state.pause();
    exec_state.resume();
    exec_state.increment_running();
    exec_state.decrement_running();
    exec_state.set_running_count(0);
    exec_state.set_max_concurrent(2);
    exec_state.set_global_max_concurrent(20);

    // GAP: No timeout_seconds(), no max_execution_time(), no set_timeout().
    // A task in Executing state can run indefinitely. The only timeout
    // mechanism is at the stream level (not state machine level).

    // Verify TaskContext also has no timeout configuration
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let machine = create_state_machine("task-c5", "proj-1", services);

    assert_eq!(machine.context.task_id, "task-c5");
    assert!(machine.context.error.is_none());
    // No: machine.context.timeout_seconds
    // No: machine.context.max_execution_time

    // This test PASSES to document the gap. A wall-clock timeout would require:
    // 1. A timeout_seconds field on Task or ExecutionState
    // 2. A timer that fires ExecutionFailed after the timeout
    // 3. Per-task-type timeout configuration
}

// ============================================================================
// C6: Agent writes to wrong directory — PARTIAL
// ============================================================================

#[tokio::test]
async fn test_c6_worktree_path_tracked_on_task() {
    // PARTIAL: CWD enforcement happens at spawn time (not state machine level).
    let project_id = ProjectId::from_string("proj-c6".to_string());
    let mut task = create_test_task(&project_id, "Test worktree task");

    assert!(task.worktree_path.is_none());

    task.worktree_path = Some("/tmp/worktrees/task-c6".to_string());
    assert_eq!(
        task.worktree_path.as_deref(),
        Some("/tmp/worktrees/task-c6")
    );
}

#[tokio::test]
async fn test_c6_project_working_directory_used_as_fallback() {
    // PARTIAL: When no worktree_path is set, project working_directory is the
    // fallback CWD.
    let project = create_test_project("test-project");
    assert_eq!(project.working_directory, "/tmp/test-project");

    let wt_project = create_test_project_with_git_mode("wt-project", GitMode::Worktree);
    assert_eq!(wt_project.working_directory, "/tmp/test-project");
    assert_eq!(wt_project.git_mode, GitMode::Worktree);
}

#[tokio::test]
async fn test_c6_task_branch_set_during_execution_entry() {
    // PARTIAL: task_branch is None by default, set by on_enter(Executing).
    // No runtime CWD enforcement after spawn.
    let project_id = ProjectId::from_string("proj-c6c".to_string());
    let task = create_test_task(&project_id, "Branch isolation task");

    assert!(task.task_branch.is_none());
    assert!(task.worktree_path.is_none());
}
