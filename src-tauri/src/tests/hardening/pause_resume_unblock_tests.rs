// Integration tests for pause/resume/unblock scenarios
//
// Tests cover three related areas:
//   1. Pause: agent-active states → Paused (all 6 pausable states)
//   2. Resume: Paused → previous agent-active state (PauseReason metadata round-trip)
//   3. Unblock: Blocked → Ready (BlockersResolved) and related guard conditions
//
// Follows the hardening test pattern: pure state machine + ExecutionState atomics.
// No database required — uses MemoryTaskRepository for tests that need persistence.

use std::sync::Arc;

use super::helpers::*;
use crate::application::chat_service::PauseReason;
use crate::application::execution_state::ExecutionState;
use crate::domain::entities::{InternalStatus, ProjectId};
use crate::domain::repositories::TaskRepository;
use crate::domain::state_machine::events::TaskEvent;
use crate::domain::state_machine::machine::State;
use crate::domain::state_machine::types::{Blocker, FailedData};

// ============================================================================
// Pause: all agent-active states can transition to Paused
// ============================================================================

#[tokio::test]
async fn test_pause_from_executing() {
    // Executing → Paused via TaskEvent::Pause
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-pause-exec", "proj-pause", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Executing, &TaskEvent::Pause)
        .await;

    assert!(result.is_success(), "Executing → Paused should succeed");
    assert_eq!(
        result.state(),
        Some(&State::Paused),
        "Should land in Paused"
    );
}

#[tokio::test]
async fn test_pause_from_re_executing() {
    // ReExecuting → Paused via TaskEvent::Pause
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-pause-reexec", "proj-pause", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::ReExecuting, &TaskEvent::Pause)
        .await;

    assert!(result.is_success());
    assert_eq!(result.state(), Some(&State::Paused));
}

#[tokio::test]
async fn test_pause_from_qa_refining() {
    // QaRefining → Paused via TaskEvent::Pause
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-pause-qa-ref", "proj-pause", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::QaRefining, &TaskEvent::Pause)
        .await;

    assert!(result.is_success());
    assert_eq!(result.state(), Some(&State::Paused));
}

#[tokio::test]
async fn test_pause_from_qa_testing() {
    // QaTesting → Paused via TaskEvent::Pause
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-pause-qa-test", "proj-pause", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::QaTesting, &TaskEvent::Pause)
        .await;

    assert!(result.is_success());
    assert_eq!(result.state(), Some(&State::Paused));
}

#[tokio::test]
async fn test_pause_from_reviewing() {
    // Reviewing → Paused via TaskEvent::Pause
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-pause-reviewing", "proj-pause", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Reviewing, &TaskEvent::Pause)
        .await;

    assert!(result.is_success());
    assert_eq!(result.state(), Some(&State::Paused));
}

#[tokio::test]
async fn test_pause_from_merging() {
    // Merging → Paused via TaskEvent::Pause
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-pause-merging", "proj-pause", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Merging, &TaskEvent::Pause)
        .await;

    assert!(result.is_success());
    assert_eq!(result.state(), Some(&State::Paused));
}

#[tokio::test]
async fn test_pause_from_pending_merge() {
    // PendingMerge → Paused via TaskEvent::Pause
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-pause-pmerge", "proj-pause", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::PendingMerge, &TaskEvent::Pause)
        .await;

    assert!(result.is_success());
    assert_eq!(result.state(), Some(&State::Paused));
}

// ============================================================================
// Pause: idle and terminal states do NOT pause
// ============================================================================

#[tokio::test]
async fn test_pause_ignored_from_backlog() {
    // Backlog does not handle Pause — event is not in its transition table
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-pause-backlog", "proj-pause", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Backlog, &TaskEvent::Pause)
        .await;

    assert!(
        !result.is_success(),
        "Backlog should NOT handle Pause — not in its transition table"
    );
}

#[tokio::test]
async fn test_pause_ignored_from_ready() {
    // Ready does not handle Pause
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-pause-ready", "proj-pause", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Ready, &TaskEvent::Pause)
        .await;

    assert!(!result.is_success(), "Ready should NOT handle Pause");
}

#[tokio::test]
async fn test_pause_ignored_from_blocked() {
    // Blocked is an idle state (waiting for human), Pause is not applicable
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-pause-blocked", "proj-pause", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Blocked, &TaskEvent::Pause)
        .await;

    assert!(!result.is_success(), "Blocked should NOT handle Pause");
}

#[tokio::test]
async fn test_pause_ignored_from_failed() {
    // Failed is terminal — Pause should be ignored
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-pause-failed", "proj-pause", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Failed(FailedData::default()), &TaskEvent::Pause)
        .await;

    assert!(
        !result.is_success(),
        "Failed (terminal) should NOT handle Pause"
    );
}

// ============================================================================
// Stop: agent-active states can stop (produces Stopped, not Paused)
// ============================================================================

#[tokio::test]
async fn test_stop_from_executing_produces_stopped_not_paused() {
    // Stop should land in Stopped, which is terminal (unlike Paused)
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-stop-exec", "proj-stop", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Executing, &TaskEvent::Stop)
        .await;

    assert!(result.is_success());
    assert_eq!(
        result.state(),
        Some(&State::Stopped),
        "Stop produces Stopped, not Paused"
    );
}

#[tokio::test]
async fn test_paused_is_not_terminal_but_stopped_is() {
    // Semantic difference: Paused can resume, Stopped requires manual restart
    assert!(
        !State::Paused.is_terminal(),
        "Paused is NOT terminal — can resume"
    );
    assert!(
        State::Stopped.is_terminal(),
        "Stopped IS terminal — requires manual restart"
    );
}

#[tokio::test]
async fn test_paused_is_not_active_or_idle() {
    // Paused is its own category (suspended), not active, not idle, not terminal
    assert!(!State::Paused.is_active(), "Paused is not active");
    assert!(!State::Paused.is_idle(), "Paused is not idle");
    assert!(!State::Paused.is_terminal(), "Paused is not terminal");
    assert!(State::Paused.is_paused(), "Paused.is_paused() returns true");
}

// ============================================================================
// Resume: Paused state does NOT directly handle events (command-layer concern)
// ============================================================================

#[tokio::test]
async fn test_paused_state_does_not_handle_any_event() {
    // The Paused state ignores all events in the state machine.
    // Resume is done at the command layer (resume_execution) which reads
    // pause_reason metadata and directly transitions to the pre-pause status.
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-paused-noop", "proj-resume", services);
    let mut handler = create_transition_handler(&mut machine);

    // None of these events should be handled by the Paused state
    for event in &[
        TaskEvent::ExecutionComplete,
        TaskEvent::Cancel,
        TaskEvent::Retry,
        TaskEvent::Stop,
        TaskEvent::Pause, // Self-transition: Paused cannot re-pause
    ] {
        let result = handler.handle_transition(&State::Paused, event).await;
        assert!(
            !result.is_success(),
            "Paused state should NOT handle {:?} — resume is done at command layer",
            event
        );
    }
}

// ============================================================================
// Resume: PauseReason metadata round-trip
// ============================================================================

#[test]
fn test_pause_reason_user_initiated_round_trip() {
    // PauseReason::UserInitiated written to metadata and read back intact
    let reason = PauseReason::UserInitiated {
        previous_status: "executing".to_string(),
        paused_at: "2026-02-18T10:00:00Z".to_string(),
        scope: "global".to_string(),
    };

    let metadata = reason.write_to_task_metadata(None);
    let recovered = PauseReason::from_task_metadata(Some(&metadata));

    assert!(
        recovered.is_some(),
        "Should recover PauseReason from metadata"
    );
    assert_eq!(
        recovered.unwrap().previous_status(),
        "executing",
        "previous_status must survive round-trip"
    );
}

#[test]
fn test_pause_reason_provider_error_round_trip() {
    // PauseReason::ProviderError written to metadata and read back intact
    use crate::application::chat_service::ProviderErrorCategory;

    let reason = PauseReason::ProviderError {
        category: ProviderErrorCategory::RateLimit,
        message: "429 Too Many Requests".to_string(),
        retry_after: Some("2026-02-18T11:00:00Z".to_string()),
        previous_status: "reviewing".to_string(),
        paused_at: "2026-02-18T10:00:00Z".to_string(),
        auto_resumable: true,
        resume_attempts: 0,
    };

    let metadata = reason.write_to_task_metadata(None);
    let recovered = PauseReason::from_task_metadata(Some(&metadata));

    assert!(recovered.is_some());
    let r = recovered.unwrap();
    assert_eq!(r.previous_status(), "reviewing");
    assert!(r.is_provider_error());
}

#[test]
fn test_pause_reason_clear_removes_from_metadata() {
    // After clear_from_task_metadata, from_task_metadata returns None
    let reason = PauseReason::UserInitiated {
        previous_status: "executing".to_string(),
        paused_at: "2026-02-18T10:00:00Z".to_string(),
        scope: "task".to_string(),
    };

    let metadata_with = reason.write_to_task_metadata(None);
    let metadata_cleared = PauseReason::clear_from_task_metadata(Some(&metadata_with));
    let recovered = PauseReason::from_task_metadata(Some(&metadata_cleared));

    assert!(
        recovered.is_none(),
        "PauseReason should be absent after clear"
    );
}

#[test]
fn test_pause_reason_preserves_other_metadata_keys() {
    // Writing/clearing pause_reason must not clobber other metadata keys
    let existing = r#"{"trigger_origin": "scheduler", "some_key": 42}"#;

    let reason = PauseReason::UserInitiated {
        previous_status: "executing".to_string(),
        paused_at: "2026-02-18T10:00:00Z".to_string(),
        scope: "global".to_string(),
    };

    let with_pause = reason.write_to_task_metadata(Some(existing));
    let json: serde_json::Value = serde_json::from_str(&with_pause).unwrap();
    assert_eq!(
        json["trigger_origin"], "scheduler",
        "Other keys preserved on write"
    );
    assert_eq!(json["some_key"], 42, "Numeric keys preserved on write");

    let cleared = PauseReason::clear_from_task_metadata(Some(&with_pause));
    let json2: serde_json::Value = serde_json::from_str(&cleared).unwrap();
    assert_eq!(
        json2["trigger_origin"], "scheduler",
        "Other keys preserved on clear"
    );
    assert!(json2.get("pause_reason").is_none(), "pause_reason removed");
}

#[test]
fn test_pause_reason_previous_status_parses_to_internal_status() {
    // The resume_execution command parses previous_status back to InternalStatus.
    // Verify that all agent-active statuses survive this round-trip.
    let agent_active_statuses = [
        "executing",
        "re_executing",
        "qa_refining",
        "qa_testing",
        "reviewing",
        "merging",
        "pending_merge",
    ];

    for status_str in &agent_active_statuses {
        let reason = PauseReason::UserInitiated {
            previous_status: status_str.to_string(),
            paused_at: "2026-02-18T10:00:00Z".to_string(),
            scope: "global".to_string(),
        };

        let metadata = reason.write_to_task_metadata(None);
        let recovered = PauseReason::from_task_metadata(Some(&metadata)).unwrap();
        let parsed: Result<InternalStatus, _> = recovered.previous_status().parse();

        assert!(
            parsed.is_ok(),
            "previous_status '{}' should parse to InternalStatus",
            status_str
        );
    }
}

// ============================================================================
// Resume: ExecutionState semantics
// ============================================================================

#[tokio::test]
async fn test_resume_execution_state_allows_new_tasks_after_pause() {
    // pause() → resume() restores can_start_task when capacity is available
    let exec_state = Arc::new(ExecutionState::with_max_concurrent(3));
    exec_state.increment_running(); // 1 of 3 running

    exec_state.pause();
    assert!(
        !exec_state.can_start_task(),
        "Paused: cannot start even with capacity"
    );

    exec_state.resume();
    assert!(
        exec_state.can_start_task(),
        "Resumed with capacity: can start (running=1, max=3)"
    );
}

#[tokio::test]
async fn test_resume_execution_state_still_blocked_at_capacity() {
    // Resuming when at capacity does not allow starts until a slot frees
    let exec_state = Arc::new(ExecutionState::with_max_concurrent(2));
    exec_state.increment_running();
    exec_state.increment_running(); // at capacity

    exec_state.pause();
    exec_state.resume();

    assert!(
        !exec_state.can_start_task(),
        "Resumed but at capacity (running=2, max=2) — still blocked"
    );

    exec_state.decrement_running(); // slot freed
    assert!(
        exec_state.can_start_task(),
        "Slot freed after resume: can start now"
    );
}

#[tokio::test]
async fn test_pause_resume_cycle_does_not_change_running_count() {
    // pause/resume is about scheduling gating, not running count
    let exec_state = Arc::new(ExecutionState::with_max_concurrent(5));
    exec_state.increment_running();
    exec_state.increment_running();

    let count_before = exec_state.running_count();
    exec_state.pause();
    exec_state.resume();
    let count_after = exec_state.running_count();

    assert_eq!(
        count_before, count_after,
        "pause/resume cycle must not modify running_count"
    );
}

// ============================================================================
// Unblock: Blocked → Ready via BlockersResolved
// ============================================================================

#[tokio::test]
async fn test_unblock_blocked_task_transitions_to_ready() {
    // BlockersResolved event transitions Blocked → Ready
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-unblock-1", "proj-unblock", services);
    // Manually add a blocker so context is in the "blocked" state
    machine
        .context
        .add_blocker(Blocker::new("blocking-task-id"));

    let result = {
        let mut handler = create_transition_handler(&mut machine);
        handler
            .handle_transition(&State::Blocked, &TaskEvent::BlockersResolved)
            .await
    };

    assert!(result.is_success(), "BlockersResolved should succeed");
    assert_eq!(
        result.state(),
        Some(&State::Ready),
        "Blocked → Ready on BlockersResolved"
    );
    // handler dropped; access machine.context directly
    assert!(
        !machine.context.has_blockers(),
        "All blockers should be cleared after BlockersResolved"
    );
}

#[tokio::test]
async fn test_unblock_clears_all_blockers_from_context() {
    // Multiple blockers are all cleared on BlockersResolved
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-unblock-multi", "proj-unblock", services);

    // Add multiple blockers
    machine.context.add_blocker(Blocker::new("dep-task-a"));
    machine.context.add_blocker(Blocker::new("dep-task-b"));
    machine.context.add_blocker(Blocker::new("dep-task-c"));
    assert!(machine.context.has_blockers(), "Should have 3 blockers");

    let result = {
        let mut handler = create_transition_handler(&mut machine);
        handler
            .handle_transition(&State::Blocked, &TaskEvent::BlockersResolved)
            .await
    };

    assert!(result.is_success());
    // handler dropped; access machine.context directly
    assert!(
        !machine.context.has_blockers(),
        "All 3 blockers cleared after BlockersResolved"
    );
}

#[tokio::test]
async fn test_ready_to_blocked_via_blocker_detected() {
    // Ready → Blocked on BlockerDetected; context gains the blocker
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-block-ready", "proj-unblock", services);

    let result = {
        let mut handler = create_transition_handler(&mut machine);
        handler
            .handle_transition(
                &State::Ready,
                &TaskEvent::BlockerDetected {
                    blocker_id: "blocking-task-xyz".to_string(),
                },
            )
            .await
    };

    assert!(result.is_success());
    assert_eq!(result.state(), Some(&State::Blocked));
    // handler dropped; access machine.context directly
    assert!(
        machine.context.has_blockers(),
        "Blocker should be in context after BlockerDetected"
    );
}

#[tokio::test]
async fn test_executing_needs_human_input_transitions_to_blocked() {
    // NeedsHumanInput during execution blocks the task
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-human-block", "proj-unblock", services);

    let result = {
        let mut handler = create_transition_handler(&mut machine);
        handler
            .handle_transition(
                &State::Executing,
                &TaskEvent::NeedsHumanInput {
                    reason: "Need API credentials from user".to_string(),
                },
            )
            .await
    };

    assert!(result.is_success());
    assert_eq!(result.state(), Some(&State::Blocked));
    // handler dropped; access machine.context directly
    assert!(
        machine.context.has_blockers(),
        "Human input blocker should be tracked in context"
    );
}

#[tokio::test]
async fn test_blocked_task_can_be_cancelled() {
    // Blocked tasks can be cancelled — not just unblocked
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-cancel-blocked", "proj-unblock", services);
    machine.context.add_blocker(Blocker::new("some-dep"));
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Blocked, &TaskEvent::Cancel)
        .await;

    assert!(result.is_success());
    assert_eq!(result.state(), Some(&State::Cancelled));
}

// ============================================================================
// Unblock: MemoryTaskRepository integration
// ============================================================================

#[tokio::test]
async fn test_blocked_task_status_persists_in_repo() {
    // Verify that a task stored in MemoryTaskRepository with Blocked status
    // is queryable and its status survives a read-back
    let s = create_hardening_services();
    let project_id = ProjectId::from_string("proj-repo-unblock".to_string());

    let task =
        create_test_task_with_status(&project_id, "Waiting for dep", InternalStatus::Blocked);
    let task_id = task.id.clone();

    s.task_repo.create(task).await.unwrap();

    let fetched = s.task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        fetched.internal_status,
        InternalStatus::Blocked,
        "Blocked status persists in repo"
    );
}

#[tokio::test]
async fn test_pause_reason_metadata_survives_task_update() {
    // PauseReason metadata written to task.metadata round-trips through repo.update()
    let s = create_hardening_services();
    let project_id = ProjectId::from_string("proj-meta-unblock".to_string());

    let mut task =
        create_test_task_with_status(&project_id, "Paused worker", InternalStatus::Paused);

    let pause_reason = PauseReason::UserInitiated {
        previous_status: "executing".to_string(),
        paused_at: "2026-02-18T10:00:00Z".to_string(),
        scope: "global".to_string(),
    };
    task.metadata = Some(pause_reason.write_to_task_metadata(task.metadata.as_deref()));
    let task_id = task.id.clone();

    s.task_repo.create(task).await.unwrap();

    let fetched = s.task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    let recovered = PauseReason::from_task_metadata(fetched.metadata.as_deref());

    assert!(
        recovered.is_some(),
        "PauseReason should survive repo round-trip"
    );
    assert_eq!(
        recovered.unwrap().previous_status(),
        "executing",
        "previous_status should survive repo round-trip"
    );
}

// ============================================================================
// Full scenario: pause → task stored → resume restores correct previous state
// ============================================================================

#[tokio::test]
async fn test_full_pause_resume_scenario_with_metadata() {
    // Simulate the full pause/resume flow:
    //   1. Task is Executing
    //   2. Pause: task transitions to Paused, PauseReason written to metadata
    //   3. Resume: read previous_status from metadata, parse back to InternalStatus
    //   4. Verify the recovered status matches the original
    let s = create_hardening_services();
    let project_id = ProjectId::from_string("proj-full-pause".to_string());

    // Step 1: Create task in Executing
    let mut task =
        create_test_task_with_status(&project_id, "Worker task", InternalStatus::Executing);
    let task_id = task.id.clone();

    // Step 2: Simulate pause — write pause metadata and change status
    let pause_reason = PauseReason::UserInitiated {
        previous_status: InternalStatus::Executing.to_string(),
        paused_at: "2026-02-18T10:00:00Z".to_string(),
        scope: "global".to_string(),
    };
    task.metadata = Some(pause_reason.write_to_task_metadata(task.metadata.as_deref()));
    task.internal_status = InternalStatus::Paused;

    s.task_repo.create(task).await.unwrap();

    // Step 3: Simulate resume — read PauseReason and recover previous_status
    let fetched = s.task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(fetched.internal_status, InternalStatus::Paused);

    let recovered_reason = PauseReason::from_task_metadata(fetched.metadata.as_deref()).unwrap();
    let restore_status: InternalStatus = recovered_reason
        .previous_status()
        .parse()
        .expect("previous_status should parse to InternalStatus");

    // Step 4: Verify the recovered status is correct
    assert_eq!(
        restore_status,
        InternalStatus::Executing,
        "Resume should restore task to Executing"
    );

    // Step 5: Simulate clearing metadata on successful resume
    let mut resumed_task = fetched;
    resumed_task.metadata = Some(PauseReason::clear_from_task_metadata(
        resumed_task.metadata.as_deref(),
    ));
    resumed_task.internal_status = restore_status;
    s.task_repo.update(&resumed_task).await.unwrap();

    let after_resume = s.task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(after_resume.internal_status, InternalStatus::Executing);
    assert!(
        PauseReason::from_task_metadata(after_resume.metadata.as_deref()).is_none(),
        "pause_reason metadata cleared after resume"
    );
}

#[tokio::test]
async fn test_full_unblock_scenario_with_repo() {
    // Simulate the full unblock flow:
    //   1. Task starts Ready
    //   2. BlockerDetected → Blocked (state machine transition)
    //   3. Dependency completes → BlockersResolved → Ready (state machine transition)
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-full-unblock", "proj-full-unblock", services);

    // Step 1: Ready → Blocked
    let r1 = {
        let mut handler = create_transition_handler(&mut machine);
        handler
            .handle_transition(
                &State::Ready,
                &TaskEvent::BlockerDetected {
                    blocker_id: "dependency-task".to_string(),
                },
            )
            .await
    };
    assert_eq!(r1.state(), Some(&State::Blocked));
    assert!(machine.context.has_blockers());

    // Step 2: Blocked → Ready (dependency completed)
    let r2 = {
        let mut handler = create_transition_handler(&mut machine);
        handler
            .handle_transition(&State::Blocked, &TaskEvent::BlockersResolved)
            .await
    };
    assert_eq!(r2.state(), Some(&State::Ready));
    assert!(
        !machine.context.has_blockers(),
        "No blockers remain after unblock"
    );
}
