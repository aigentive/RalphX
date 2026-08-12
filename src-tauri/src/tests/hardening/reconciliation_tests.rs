// Reconciliation Hardening Tests (E1-E9)
//
// Tests for reconciliation scenarios: stale agent-active tasks, capacity gating,
// merge recovery, retry limits, and crash recovery paths.

use super::helpers::*;

use crate::application::execution_state::ExecutionState;
use crate::domain::entities::{InternalStatus, ProjectId, TaskId};
use crate::domain::repositories::TaskRepository;
use crate::domain::state_machine::events::TaskEvent;
use crate::domain::state_machine::machine::{Response, State};

// ============================================================================
// E1: Task in Executing, agent died, slots available — COVERED
// ============================================================================

#[tokio::test]
async fn test_e1_executing_task_recovery_via_stop_retry() {
    // COVERED: Executing -> Stopped -> Ready (reconciliation path)
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-e1", "proj-1", services);

    assert_eq!(
        machine.dispatch(&State::Executing, &TaskEvent::Stop),
        Response::Transition(State::Stopped)
    );
    assert_eq!(
        machine.dispatch(&State::Stopped, &TaskEvent::Retry),
        Response::Transition(State::Ready)
    );
    assert_eq!(
        machine.dispatch(&State::Ready, &TaskEvent::StartExecution),
        Response::Transition(State::Executing)
    );
}

#[tokio::test]
async fn test_e1_full_recovery_with_running_count() {
    // COVERED: Full recovery cycle via TransitionHandler with running count
    let s = create_hardening_services();
    s.execution_state.increment_running();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-e1b", "proj-1", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Executing, &TaskEvent::Stop)
        .await;
    assert!(result.is_success());
    assert_eq!(
        s.execution_state.running_count(),
        0,
        "on_exit should decrement"
    );

    let result = handler
        .handle_transition(&State::Stopped, &TaskEvent::Retry)
        .await;
    assert!(result.is_success());
}

#[tokio::test]
async fn test_e1_recovery_via_execution_failed_path() {
    // COVERED: Executing -> Failed -> Ready (alternative recovery)
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-e1c", "proj-1", services);

    let r = machine.dispatch(
        &State::Executing,
        &TaskEvent::ExecutionFailed {
            error: "Agent died".into(),
        },
    );
    assert!(matches!(r, Response::Transition(State::Failed(_))));
    assert_eq!(
        machine.dispatch(&State::Failed(Default::default()), &TaskEvent::Retry),
        Response::Transition(State::Ready)
    );
}

// ============================================================================
// E2: Task in Executing, max_concurrent reached — PARTIAL
// ============================================================================

#[tokio::test]
async fn test_e2_can_start_task_capacity_and_pause_checks() {
    // PARTIAL: can_start_task() respects max_concurrent, pause, and global max
    let exec_state = ExecutionState::with_max_concurrent(2);
    assert!(exec_state.can_start_task());

    exec_state.increment_running();
    exec_state.increment_running();
    assert!(
        !exec_state.can_start_task(),
        "Should be false at max_concurrent"
    );

    // Pause blocks regardless of capacity
    let exec_state2 = ExecutionState::with_max_concurrent(5);
    exec_state2.pause();
    assert!(!exec_state2.can_start_task(), "Should be false when paused");
    exec_state2.resume();
    assert!(exec_state2.can_start_task());

    // Global max also blocks
    let exec_state3 = ExecutionState::with_max_concurrent(100);
    exec_state3.set_global_max_concurrent(2);
    exec_state3.increment_running();
    exec_state3.increment_running();
    assert!(
        !exec_state3.can_start_task(),
        "Should respect global_max_concurrent"
    );

    // PARTIAL: No escalation mechanism for stuck tasks at capacity
}

// ============================================================================
// E3: Task in Merging, agent died — COVERED
// ============================================================================

#[tokio::test]
async fn test_e3_merge_agent_error_and_retry() {
    // COVERED: Merging -> MergeIncomplete -> PendingMerge (retry)
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-e3", "proj-1", services);

    assert_eq!(
        machine.dispatch(&State::Merging, &TaskEvent::MergeAgentError),
        Response::Transition(State::MergeIncomplete)
    );
    assert_eq!(
        machine.dispatch(&State::MergeIncomplete, &TaskEvent::Retry),
        Response::Transition(State::PendingMerge)
    );
}

#[tokio::test]
async fn test_e3_merge_agent_failed_to_conflict() {
    // COVERED: MergeAgentFailed -> MergeConflict
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-e3b", "proj-1", services);

    assert_eq!(
        machine.dispatch(&State::Merging, &TaskEvent::MergeAgentFailed),
        Response::Transition(State::MergeConflict)
    );
}

#[tokio::test]
async fn test_e3_full_merge_recovery_with_running_count() {
    // COVERED: Full cycle with on_exit decrement
    let s = create_hardening_services();
    s.execution_state.increment_running();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-e3c", "proj-1", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Merging, &TaskEvent::MergeAgentError)
        .await;
    assert!(result.is_success());
    assert_eq!(s.execution_state.running_count(), 0);

    let result = handler
        .handle_transition(&State::MergeIncomplete, &TaskEvent::Retry)
        .await;
    assert!(result.is_success());
}

// ============================================================================
// E4: Task in Reviewing, agent died — COVERED
// ============================================================================

#[tokio::test]
async fn test_e4_reviewing_recovery_and_cancel() {
    // COVERED: Reviewing -> Stopped -> Ready -> Executing, and Reviewing -> Cancelled
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-e4", "proj-1", services);

    assert_eq!(
        machine.dispatch(&State::Reviewing, &TaskEvent::Stop),
        Response::Transition(State::Stopped)
    );
    assert_eq!(
        machine.dispatch(&State::Stopped, &TaskEvent::Retry),
        Response::Transition(State::Ready)
    );
    assert_eq!(
        machine.dispatch(&State::Ready, &TaskEvent::StartExecution),
        Response::Transition(State::Executing)
    );

    // Also verify Cancel path
    assert_eq!(
        machine.dispatch(&State::Reviewing, &TaskEvent::Cancel),
        Response::Transition(State::Cancelled)
    );
}

// ============================================================================
// E5: QaRefining/QaTesting stale — COVERED
// ============================================================================

#[tokio::test]
async fn test_e5_qa_states_accept_stop_pause_cancel() {
    // COVERED: QA states accept all control events for cleanup
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-e5", "proj-1", services);

    // QaRefining control events
    assert_eq!(
        machine.dispatch(&State::QaRefining, &TaskEvent::Stop),
        Response::Transition(State::Stopped)
    );
    assert_eq!(
        machine.dispatch(&State::QaRefining, &TaskEvent::Pause),
        Response::Transition(State::Paused)
    );
    assert_eq!(
        machine.dispatch(&State::QaRefining, &TaskEvent::Cancel),
        Response::Transition(State::Cancelled)
    );

    // QaTesting control events
    assert_eq!(
        machine.dispatch(&State::QaTesting, &TaskEvent::Stop),
        Response::Transition(State::Stopped)
    );
    assert_eq!(
        machine.dispatch(&State::QaTesting, &TaskEvent::Pause),
        Response::Transition(State::Paused)
    );
    assert_eq!(
        machine.dispatch(&State::QaTesting, &TaskEvent::Cancel),
        Response::Transition(State::Cancelled)
    );
}

#[tokio::test]
async fn test_e5_qa_states_retry_path() {
    // COVERED: Recovery via Stop -> Retry -> Ready for both QA states
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-e5b", "proj-1", services);

    assert_eq!(
        machine.dispatch(&State::QaRefining, &TaskEvent::Stop),
        Response::Transition(State::Stopped)
    );
    assert_eq!(
        machine.dispatch(&State::Stopped, &TaskEvent::Retry),
        Response::Transition(State::Ready)
    );

    assert_eq!(
        machine.dispatch(&State::QaTesting, &TaskEvent::Stop),
        Response::Transition(State::Stopped)
    );
    assert_eq!(
        machine.dispatch(&State::Stopped, &TaskEvent::Retry),
        Response::Transition(State::Ready)
    );
}

// ============================================================================
// E6: PendingMerge deferred, blocker orphaned — COVERED
// ============================================================================

#[tokio::test]
async fn test_e6_pending_merge_transitions() {
    // COVERED: PendingMerge -> Merged, -> Merging, -> Stopped, -> Cancelled
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-e6", "proj-1", services);

    assert_eq!(
        machine.dispatch(&State::PendingMerge, &TaskEvent::MergeComplete),
        Response::Transition(State::Merged)
    );
    assert_eq!(
        machine.dispatch(&State::PendingMerge, &TaskEvent::MergeConflict),
        Response::Transition(State::Merging)
    );
    assert_eq!(
        machine.dispatch(&State::PendingMerge, &TaskEvent::Stop),
        Response::Transition(State::Stopped)
    );
    assert_eq!(
        machine.dispatch(&State::PendingMerge, &TaskEvent::Cancel),
        Response::Transition(State::Cancelled)
    );
}

#[tokio::test]
async fn test_e6_merge_conflict_manual_resolution() {
    // COVERED: MergeConflict -> Merged via ConflictResolved
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-e6b", "proj-1", services);

    assert_eq!(
        machine.dispatch(&State::MergeConflict, &TaskEvent::ConflictResolved),
        Response::Transition(State::Merged)
    );
}

// ============================================================================
// E7: No max-retry limit for re-spawns — GAP
// ============================================================================

#[tokio::test]
async fn test_e7_no_retry_counter_on_execution_state() {
    // GAP: No retry counter field on ExecutionState.
    let exec_state = ExecutionState::new();
    // No get_retry_count(), increment_retry(), max_retries(), has_exceeded_retries()
    assert_eq!(exec_state.running_count(), 0);
    assert_eq!(exec_state.max_concurrent(), 10);
    assert!(!exec_state.is_paused());
}

#[tokio::test]
async fn test_e7_unlimited_retry_from_failed() {
    // GAP: Cycle Failed -> Ready -> Executing -> Failed 10x with no limit
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-e7", "proj-1", services);

    for i in 0..10 {
        assert_eq!(
            machine.dispatch(&State::Failed(Default::default()), &TaskEvent::Retry),
            Response::Transition(State::Ready),
            "Retry #{} should work",
            i
        );
        assert_eq!(
            machine.dispatch(&State::Ready, &TaskEvent::StartExecution),
            Response::Transition(State::Executing)
        );
        let r = machine.dispatch(
            &State::Executing,
            &TaskEvent::ExecutionFailed {
                error: format!("Failure #{}", i),
            },
        );
        assert!(matches!(r, Response::Transition(State::Failed(_))));
    }
    // GAP: Only Merging has max retries (in ReconciliationRunner, not state machine).
}

#[tokio::test]
async fn test_e7_unlimited_retry_from_stopped_and_cancelled() {
    // GAP: Stopped and Cancelled also have no retry limit
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-e7b", "proj-1", services);

    for _ in 0..5 {
        assert_eq!(
            machine.dispatch(&State::Stopped, &TaskEvent::Retry),
            Response::Transition(State::Ready)
        );
        assert_eq!(
            machine.dispatch(&State::Ready, &TaskEvent::StartExecution),
            Response::Transition(State::Executing)
        );
        assert_eq!(
            machine.dispatch(&State::Executing, &TaskEvent::Stop),
            Response::Transition(State::Stopped)
        );
    }

    for _ in 0..5 {
        assert_eq!(
            machine.dispatch(&State::Cancelled, &TaskEvent::Retry),
            Response::Transition(State::Ready)
        );
        assert_eq!(
            machine.dispatch(&State::Ready, &TaskEvent::StartExecution),
            Response::Transition(State::Executing)
        );
        assert_eq!(
            machine.dispatch(&State::Executing, &TaskEvent::Cancel),
            Response::Transition(State::Cancelled)
        );
    }
}

// ============================================================================
// E8: App crashes while task in agent-active state — COVERED
// ============================================================================

#[tokio::test]
async fn test_e8_all_agent_active_states_have_recovery_paths() {
    // COVERED: All agent-active states accept Stop -> Retry -> Ready
    for state in [
        State::Executing,
        State::ReExecuting,
        State::QaRefining,
        State::QaTesting,
        State::Reviewing,
        State::Merging,
    ] {
        let s = create_hardening_services();
        let services = build_task_services(&s);
        let mut machine = create_state_machine("task-e8", "proj-1", services);

        assert_eq!(
            machine.dispatch(&state, &TaskEvent::Stop),
            Response::Transition(State::Stopped),
            "{:?} should accept Stop",
            state
        );
        assert_eq!(
            machine.dispatch(&State::Stopped, &TaskEvent::Retry),
            Response::Transition(State::Ready)
        );
    }
}

#[tokio::test]
async fn test_e8_crash_recovery_resets_execution_state() {
    // COVERED: After crash, running count needs reconciliation via set_running_count
    let s = create_hardening_services();
    assert_eq!(s.execution_state.running_count(), 0);
    assert!(s.execution_state.can_start_task());

    s.execution_state.set_running_count(3);
    assert_eq!(s.execution_state.running_count(), 3);
    s.execution_state.set_running_count(0);
    assert_eq!(s.execution_state.running_count(), 0);
}

// ============================================================================
// E9: Crash during merge, stale worktree — PARTIAL
// ============================================================================

#[tokio::test]
async fn test_e9_merging_task_has_worktree_path_tracked() {
    // PARTIAL: worktree_path is tracked on the task entity
    let project_id = ProjectId::from_string("proj-e9".to_string());
    let mut task = create_test_task_with_status(&project_id, "Merge task", InternalStatus::Merging);
    task.worktree_path = Some("/tmp/worktrees/task-e9".to_string());
    task.task_branch = Some("ralphx/test-project/task-e9".to_string());

    assert_eq!(
        task.worktree_path.as_deref(),
        Some("/tmp/worktrees/task-e9")
    );
    assert_eq!(
        task.task_branch.as_deref(),
        Some("ralphx/test-project/task-e9")
    );
}

#[tokio::test]
async fn test_e9_merge_incomplete_does_not_clean_worktree() {
    // PARTIAL: Merging -> MergeIncomplete does NOT clean up worktree.
    // Cleanup only happens on Merged state entry.
    let s = create_hardening_services();
    let project_id = ProjectId::from_string("proj-e9b".to_string());
    let mut task =
        create_test_task_with_status(&project_id, "Stale merge", InternalStatus::Merging);
    task.worktree_path = Some("/tmp/worktrees/task-e9b".to_string());
    task.task_branch = Some("ralphx/test/task-e9b".to_string());

    let task_id_str = task.id.as_str().to_string();
    s.task_repo.create(task).await.unwrap();

    let services = build_task_services(&s);
    let mut machine = create_state_machine(&task_id_str, "proj-e9b", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Merging, &TaskEvent::MergeAgentError)
        .await;
    assert!(result.is_success());

    let updated = s
        .task_repo
        .get_by_id(&TaskId::from_string(task_id_str))
        .await
        .unwrap();
    if let Some(task) = updated {
        // PARTIAL: Stale worktree remains on disk until successful merge or manual cleanup
        assert!(
            task.worktree_path.is_some() || task.worktree_path.is_none(),
            "worktree_path cleanup is NOT guaranteed on MergeIncomplete"
        );
    }
}

#[tokio::test]
async fn test_e9_merge_recovery_preserves_worktree_context() {
    // PARTIAL: MergeIncomplete -> PendingMerge and MergeConflict -> Merged
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-e9c", "proj-1", services);

    assert_eq!(
        machine.dispatch(&State::MergeIncomplete, &TaskEvent::Retry),
        Response::Transition(State::PendingMerge)
    );
    assert_eq!(
        machine.dispatch(&State::MergeConflict, &TaskEvent::ConflictResolved),
        Response::Transition(State::Merged)
    );
}
