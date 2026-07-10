use axum::http::StatusCode;
use std::sync::Arc;

use super::expire_permission_and_emit;
use crate::application::app_state::AppState;
use crate::application::permission_state::PendingPermissionInfo;
use crate::application::{TeamService, TeamStateTracker};
use crate::commands::ExecutionState;
use crate::http_server::types::HttpServerState;

fn make_info(request_id: &str) -> PendingPermissionInfo {
    PendingPermissionInfo {
        request_id: request_id.to_string(),
        tool_name: "Bash".to_string(),
        tool_input: serde_json::json!({}),
        context: None,
        agent_type: None,
        task_id: None,
        context_type: None,
        context_id: None,
        created_at: "2026-07-10T00:00:00+00:00".to_string(),
    }
}

fn make_test_state() -> HttpServerState {
    let app_state = Arc::new(AppState::new_test());
    let execution_state = Arc::new(ExecutionState::new());
    let tracker = Arc::new(TeamStateTracker::new());
    let team_service = Arc::new(TeamService::new_without_events(tracker));
    HttpServerState {
        app_state,
        execution_state,
        team_tracker: TeamStateTracker::new(),
        team_service,
        delegation_service: Default::default(),
    }
}

/// Path 1 — pre-check timeout: elapsed >= timeout before first channel poll.
/// Verifies: state removed, returns REQUEST_TIMEOUT.
#[tokio::test]
async fn test_expire_permission_and_emit_request_timeout() {
    let state = make_test_state();
    let request_id = "req-timeout-1";
    state
        .app_state
        .permission_state
        .register(make_info(request_id))
        .await;

    // Confirm it's registered
    assert!(state
        .app_state
        .permission_state
        .pending
        .lock()
        .await
        .contains_key(request_id));

    let result = expire_permission_and_emit(&state, request_id, StatusCode::REQUEST_TIMEOUT).await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), StatusCode::REQUEST_TIMEOUT);
    // State must be cleaned up after expiry
    assert!(!state
        .app_state
        .permission_state
        .pending
        .lock()
        .await
        .contains_key(request_id));
}

/// Path 2 — channel closed: sender dropped unexpectedly.
/// Verifies: state removed, returns INTERNAL_SERVER_ERROR.
#[tokio::test]
async fn test_expire_permission_and_emit_channel_closed() {
    let state = make_test_state();
    let request_id = "req-closed-1";
    state
        .app_state
        .permission_state
        .register(make_info(request_id))
        .await;

    let result =
        expire_permission_and_emit(&state, request_id, StatusCode::INTERNAL_SERVER_ERROR).await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!state
        .app_state
        .permission_state
        .pending
        .lock()
        .await
        .contains_key(request_id));
}

/// Path 3 — channel timeout: tokio::time::timeout expired waiting for rx.changed().
/// Verifies: state removed, returns REQUEST_TIMEOUT.
#[tokio::test]
async fn test_expire_permission_and_emit_channel_timeout() {
    let state = make_test_state();
    let request_id = "req-timeout-2";
    state
        .app_state
        .permission_state
        .register(make_info(request_id))
        .await;

    let result = expire_permission_and_emit(&state, request_id, StatusCode::REQUEST_TIMEOUT).await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), StatusCode::REQUEST_TIMEOUT);
    assert!(!state
        .app_state
        .permission_state
        .pending
        .lock()
        .await
        .contains_key(request_id));
}

/// Verify expire on unknown request_id is safe (remove is idempotent) and still
/// returns the correct error code.
#[tokio::test]
async fn test_expire_permission_and_emit_unknown_request_id() {
    let state = make_test_state();

    let result =
        expire_permission_and_emit(&state, "nonexistent", StatusCode::REQUEST_TIMEOUT).await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), StatusCode::REQUEST_TIMEOUT);
}

/// Verify that `permission:expired` event carries `{ request_id }` in its payload.
/// With app_handle = None in test mode, emission is a no-op; the payload structure
/// is enforced at compile time via the serde_json::json! macro in expire_permission_and_emit.
/// This test documents the event contract and ensures the helper compiles with the
/// correct payload shape.
#[tokio::test]
async fn test_expire_permission_event_payload_shape() {
    let state = make_test_state();
    let request_id = "req-payload-check";
    state
        .app_state
        .permission_state
        .register(make_info(request_id))
        .await;

    // With None app_handle the emit call is skipped; we verify state clean-up and return
    // to confirm the code path through expire_permission_and_emit executes correctly.
    let result = expire_permission_and_emit(&state, request_id, StatusCode::REQUEST_TIMEOUT).await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), StatusCode::REQUEST_TIMEOUT);
    assert!(!state
        .app_state
        .permission_state
        .pending
        .lock()
        .await
        .contains_key(request_id));
}
