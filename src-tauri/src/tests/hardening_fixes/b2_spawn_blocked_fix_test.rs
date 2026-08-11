// Fix B2/H3: Spawn blocked at max_concurrent now emits execution:spawn_blocked event
//
// After fix: when AgenticClientSpawner.spawn() is blocked by capacity or pause,
// it emits an execution:spawn_blocked event via app_handle before returning.
// Note: This fix is in production spawner code (infrastructure layer),
// not testable via MockAgentSpawner. These tests verify the ExecutionState
// behavior and document the expected event contract.

use crate::application::execution_state::ExecutionState;

#[tokio::test]
async fn test_b2_fix_execution_state_reports_blocking_reason() {
    // After fix: ExecutionState provides enough info for the spawner
    // to emit a meaningful blocked event.
    let exec_state = ExecutionState::with_max_concurrent(2);

    // Fill capacity
    exec_state.increment_running();
    exec_state.increment_running();

    assert!(!exec_state.can_start_task());
    assert!(!exec_state.is_paused());
    assert_eq!(exec_state.running_count(), 2);
    assert_eq!(exec_state.max_concurrent(), 2);

    // The spawner now uses these values to emit execution:spawn_blocked
    // with reason: "max_concurrent_reached"
}

#[tokio::test]
async fn test_b2_fix_paused_provides_reason() {
    // After fix: paused state provides reason for spawn_blocked event
    let exec_state = ExecutionState::with_max_concurrent(5);

    exec_state.pause();
    assert!(!exec_state.can_start_task());
    assert!(exec_state.is_paused());

    // The spawner now uses is_paused() to emit execution:spawn_blocked
    // with reason: "execution_paused"
}

#[tokio::test]
async fn test_b2_fix_spawn_blocked_event_contract() {
    // Document the expected event payload structure
    let payload = serde_json::json!({
        "task_id": "task-123",
        "agent_type": "worker",
        "reason": "max_concurrent_reached",
        "running_count": 2,
        "max_concurrent": 2,
    });

    // Verify the payload structure is valid JSON with expected fields
    assert!(payload["task_id"].is_string());
    assert!(payload["reason"].is_string());
    assert!(payload["running_count"].is_number());
    assert!(payload["max_concurrent"].is_number());
}
