// Execution Plan Cascade Hardening Tests
//
// Tests covering four fixes from the execution-plan-cascade session:
//
// Fix 1 (skipped): resolve_task_base_branch with plan_branch_repo returns plan branch.
//   Already covered by `resolve_task_base_branch_returns_feature_branch_when_branch_exists`
//   in transition_handler/tests/merge_helpers_branch_resolution.rs.
//
// Fix 2: Per-task scheduling guard in ExecutionState::try_start_scheduling prevents
//   double-scheduling the same task under concurrent scheduler invocations.
//
// Fix 3: Registry cleanup (stop) before reconciliation auto-recovery re-spawn prevents
//   infinite thrash loop when try_register fails on an occupied slot.
//
// Fix 4: Registry-aware fallback in load_execution_run prevents reconciliation from
//   loading a stale/cancelled AgentRun when status_history metadata hasn't been linked
//   yet (async race between persist_status_change and update_latest_state_history_metadata).

use std::sync::Arc;

use crate::application::execution_state::ExecutionState;
use crate::domain::services::{MemoryRunningAgentRegistry, RunningAgentKey, RunningAgentRegistry};

// ============================================================================
// Fix 4: Helper — builds a ReconciliationRunner using AppState::new_test()
// ============================================================================

fn build_reconciler_for_tests(
    app_state: &crate::application::AppState,
    execution_state: &Arc<ExecutionState>,
) -> crate::application::ReconciliationRunner {
    use crate::application::TaskTransitionService;
    let transition_service = Arc::new(TaskTransitionService::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(execution_state),
        None,
        Arc::clone(&app_state.memory_event_repo),
    ));
    crate::application::ReconciliationRunner::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.artifact_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&app_state.memory_event_repo),
        Arc::clone(&app_state.agent_run_repo),
        transition_service,
        Arc::clone(execution_state),
        None,
    )
}

// ============================================================================
// Fix 2: Per-task scheduling guard (ExecutionState::try_start_scheduling)
// ============================================================================

#[test]
fn test_per_task_scheduling_guard_prevents_double_scheduling() {
    // Guard prevents two concurrent scheduler invocations from both scheduling
    // the same task (TOCTOU race on async on_enter_Executing).
    let state = ExecutionState::new();

    // First caller acquires the slot
    assert!(
        state.try_start_scheduling("task-1"),
        "First call should succeed"
    );

    // Second concurrent caller for same task is rejected
    assert!(
        !state.try_start_scheduling("task-1"),
        "Second call for same task should be rejected (already in flight)"
    );

    // A different task is not affected
    assert!(
        state.try_start_scheduling("task-2"),
        "A different task should not be blocked by task-1's guard"
    );

    // After finish, the slot is released
    state.finish_scheduling("task-1");
    assert!(
        state.try_start_scheduling("task-1"),
        "After finish_scheduling, the slot should be available again"
    );
}

#[test]
fn test_scheduling_guard_multiple_tasks_independent() {
    // Guards for distinct tasks are independent — acquiring one does not block others.
    let state = ExecutionState::new();

    assert!(state.try_start_scheduling("task-a"));
    assert!(state.try_start_scheduling("task-b"));
    assert!(state.try_start_scheduling("task-c"));

    // Each is individually blocked while in flight
    assert!(!state.try_start_scheduling("task-a"));
    assert!(!state.try_start_scheduling("task-b"));
    assert!(!state.try_start_scheduling("task-c"));

    // Release one; only that slot becomes free
    state.finish_scheduling("task-a");
    assert!(
        state.try_start_scheduling("task-a"),
        "task-a slot should be free after finish"
    );
    assert!(
        !state.try_start_scheduling("task-b"),
        "task-b slot should still be held"
    );
    assert!(
        !state.try_start_scheduling("task-c"),
        "task-c slot should still be held"
    );
}

#[test]
fn test_scheduling_guard_finish_on_nonexistent_is_noop() {
    // finish_scheduling for an ID that was never started must not panic.
    let state = ExecutionState::new();
    state.finish_scheduling("never-started"); // should not panic

    // And a subsequent try_start should succeed (not poisoned)
    assert!(state.try_start_scheduling("never-started"));
}

// ============================================================================
// Fix 3: Registry cleanup before reconciliation re-spawn prevents thrashing
// ============================================================================

#[tokio::test]
async fn test_registry_cleanup_before_respawn_prevents_thrash() {
    // Without the fix: apply_recovery_decision called execute_entry_actions while a
    // stale registry entry for the same task still existed. try_register then returned
    // Err (slot occupied), the spawn was skipped, and the reconciler retried forever.
    //
    // With the fix: apply_recovery_decision calls registry.stop() before spawning,
    // so try_register finds an empty slot and succeeds.

    let registry = MemoryRunningAgentRegistry::new();
    let key = RunningAgentKey::new("task_execution", "task-stale-123");

    // Simulate a stale entry left over from a crashed agent
    registry
        .register(
            key.clone(),
            99999, // stale PID — process no longer exists
            "conv-old".to_string(),
            "run-old".to_string(),
            None,
            None,
        )
        .await;

    // Stale entry is present
    assert!(
        registry.is_running(&key).await,
        "Stale entry should be present before cleanup"
    );

    // Without cleanup: try_register returns Err because the slot is occupied
    let result_without_cleanup = registry
        .try_register(
            key.clone(),
            "conv-new".to_string(),
            "run-new".to_string(),
        )
        .await;
    assert!(
        result_without_cleanup.is_err(),
        "try_register must fail when stale entry is present (no cleanup)"
    );

    // The fix: clear the stale entry first (stop removes it from the registry)
    let stop_result = registry.stop(&key).await;
    assert!(
        stop_result.is_ok(),
        "stop() should succeed even for a stale entry"
    );

    // Entry is now cleared
    assert!(
        !registry.is_running(&key).await,
        "Registry entry should be absent after stop()"
    );

    // After cleanup: try_register succeeds and the new agent can be spawned
    let result_after_cleanup = registry
        .try_register(
            key.clone(),
            "conv-new".to_string(),
            "run-new".to_string(),
        )
        .await;
    assert!(
        result_after_cleanup.is_ok(),
        "try_register should succeed after the stale entry is cleaned up"
    );
}

#[tokio::test]
async fn test_registry_cleanup_idempotent_when_no_entry() {
    // stop() on a key with no entry should return Ok(None) without panicking.
    // Ensures the cleanup call in apply_recovery_decision is safe even if the
    // entry was already removed by a concurrent prune.
    let registry = MemoryRunningAgentRegistry::new();
    let key = RunningAgentKey::new("task_execution", "task-no-entry");

    assert!(
        !registry.is_running(&key).await,
        "No entry should exist initially"
    );

    // stop on absent key should not panic and should return Ok
    let result = registry.stop(&key).await;
    assert!(
        result.is_ok(),
        "stop() on absent entry should return Ok (idempotent)"
    );
    let stopped_info = result.unwrap();
    assert!(
        stopped_info.is_none(),
        "stop() on absent entry should return None (no entry was removed)"
    );
}

// ============================================================================
// Fix 4: Registry-aware fallback in load_execution_run (startup race condition)
// ============================================================================

#[tokio::test]
async fn test_load_execution_run_uses_registry_when_metadata_missing() {
    // Regression test for the startup race condition:
    // persist_status_change records the Executing transition BEFORE
    // update_latest_state_history_metadata links the agent_run_id. When
    // reconciliation fires in this window the history entry has agent_run_id=None,
    // so the old code fell through to the conversation-based lookup and returned
    // a stale/cancelled run — which triggered a re-spawn and killed the fresh agent.
    //
    // The fix adds a registry-aware fallback: when no history entry with
    // agent_run_id is found, check the running_agent_registry before falling back
    // to the conversation lookup. This test verifies that fallback returns the
    // REGISTRY's running agent (not None / not the stale one).
    use crate::application::AppState;
    use crate::domain::entities::{AgentRun, AgentRunStatus, ChatConversationId, InternalStatus, Project, Task};


    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let reconciler = build_reconciler_for_tests(&app_state, &execution_state);

    // Create a project and task in Executing state.
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state.project_repo.create(project.clone()).await.unwrap();
    let mut task = Task::new(project.id.clone(), "Race Condition Task".to_string());
    task.internal_status = InternalStatus::Executing;
    app_state.task_repo.create(task.clone()).await.unwrap();

    // Simulate the async race: status transition recorded but agent_run_id not yet linked.
    app_state
        .task_repo
        .persist_status_change(&task.id, InternalStatus::Ready, InternalStatus::Executing, "test")
        .await
        .unwrap();

    // Create the fresh Running agent run and store it in the repo.
    let conv_id = ChatConversationId::new();
    let fresh_run = AgentRun::new(conv_id);
    let fresh_run = app_state.agent_run_repo.create(fresh_run).await.unwrap();

    // Register the fresh agent in the running registry.
    let key = RunningAgentKey::new("task_execution", task.id.as_str());
    app_state
        .running_agent_registry
        .register(
            key.clone(),
            12345,
            "conv-fresh".to_string(),
            fresh_run.id.as_str(),
            None,
            None,
        )
        .await;

    // Act: load_execution_run should fall back to the registry since history has no agent_run_id.
    let loaded = reconciler
        .load_execution_run(&task, InternalStatus::Executing)
        .await;

    // Assert: returned the registry's fresh Running run, not None.
    let loaded = loaded.expect("load_execution_run should return the registry's run");
    assert_eq!(
        loaded.id, fresh_run.id,
        "Should load the registry's run, not fall through to stale conversation lookup"
    );
    assert_eq!(
        loaded.status,
        AgentRunStatus::Running,
        "Registry's run should be in Running state"
    );
}

#[tokio::test]
async fn test_reconciliation_skips_fresh_registry_entry() {
    // When a freshly spawned agent is Running and registered, load_execution_run
    // (via registry fallback) returns the Running run, and build_run_evidence sets
    // run_status=Running + registry_running=true. has_conflict() must be false so
    // reconciliation does NOT kill or re-spawn the healthy agent.
    use crate::application::AppState;
    use crate::domain::entities::{AgentRun, AgentRunStatus, ChatContextType, ChatConversationId, InternalStatus, Project, Task};


    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::with_max_concurrent(2));
    let reconciler = build_reconciler_for_tests(&app_state, &execution_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state.project_repo.create(project.clone()).await.unwrap();
    let mut task = Task::new(project.id.clone(), "Fresh Agent Task".to_string());
    task.internal_status = InternalStatus::Executing;
    app_state.task_repo.create(task.clone()).await.unwrap();

    // History entry without agent_run_id (async race window).
    app_state
        .task_repo
        .persist_status_change(&task.id, InternalStatus::Ready, InternalStatus::Executing, "test")
        .await
        .unwrap();

    // Create a fresh Running agent run.
    let conv_id = ChatConversationId::new();
    let fresh_run = AgentRun::new(conv_id);
    let fresh_run = app_state.agent_run_repo.create(fresh_run).await.unwrap();

    let key = RunningAgentKey::new("task_execution", task.id.as_str());
    app_state
        .running_agent_registry
        .register(key.clone(), 12345, "conv-fresh".to_string(), fresh_run.id.as_str(), None, None)
        .await;

    // load_execution_run uses registry fallback → Running run.
    let loaded_run = reconciler
        .load_execution_run(&task, InternalStatus::Executing)
        .await;

    // build_run_evidence should show: Running + registry present → no conflict.
    let evidence = reconciler
        .build_run_evidence(&task, ChatContextType::TaskExecution, loaded_run.as_ref())
        .await;

    assert_eq!(
        evidence.run_status,
        Some(AgentRunStatus::Running),
        "Fresh agent must report Running status"
    );
    assert!(
        evidence.registry_running,
        "Fresh agent must be marked as running in registry"
    );
    assert!(
        !evidence.has_conflict(),
        "Running agent in registry is healthy — must NOT trigger conflict action"
    );
}

#[tokio::test]
async fn test_reconciliation_kills_stale_registry_entry() {
    // When the registry holds a stale entry pointing to a Cancelled agent run
    // (e.g., crash left registry uncleared), load_execution_run (registry fallback)
    // correctly loads the Cancelled run and build_run_evidence detects a conflict:
    // run_status=Cancelled + registry_running=true → has_conflict() == true.
    // Reconciliation then takes corrective action (re-spawn / escalation).
    use crate::application::AppState;
    use crate::domain::entities::{AgentRun, AgentRunStatus, ChatContextType, ChatConversationId, InternalStatus, Project, Task};


    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::with_max_concurrent(2));
    let reconciler = build_reconciler_for_tests(&app_state, &execution_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state.project_repo.create(project.clone()).await.unwrap();
    let mut task = Task::new(project.id.clone(), "Stale Agent Task".to_string());
    task.internal_status = InternalStatus::Executing;
    app_state.task_repo.create(task.clone()).await.unwrap();

    // History entry without agent_run_id (same race condition scenario).
    app_state
        .task_repo
        .persist_status_change(&task.id, InternalStatus::Ready, InternalStatus::Executing, "test")
        .await
        .unwrap();

    // Create and cancel an agent run (simulates a crashed agent).
    let conv_id = ChatConversationId::new();
    let mut stale_run = AgentRun::new(conv_id);
    stale_run.cancel();
    let stale_run = app_state.agent_run_repo.create(stale_run).await.unwrap();

    // Registry still holds the stale entry (process died without cleanup).
    let key = RunningAgentKey::new("task_execution", task.id.as_str());
    app_state
        .running_agent_registry
        .register(key.clone(), 99999, "conv-stale".to_string(), stale_run.id.as_str(), None, None)
        .await;

    // load_execution_run finds Cancelled run via registry fallback.
    let loaded_run = reconciler
        .load_execution_run(&task, InternalStatus::Executing)
        .await;

    // build_run_evidence: Cancelled run + registry still present → conflict.
    let evidence = reconciler
        .build_run_evidence(&task, ChatContextType::TaskExecution, loaded_run.as_ref())
        .await;

    assert_eq!(
        evidence.run_status,
        Some(AgentRunStatus::Cancelled),
        "Stale run should report Cancelled status"
    );
    assert!(
        evidence.registry_running,
        "Stale registry entry should still show as running"
    );
    assert!(
        evidence.has_conflict(),
        "Cancelled run + live registry entry must be flagged as conflict for corrective action"
    );
}
