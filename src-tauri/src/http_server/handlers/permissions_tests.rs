use axum::http::StatusCode;
use std::sync::Arc;

use async_trait::async_trait;
use axum::extract::State;
use axum::Json;
use chrono::{DateTime, Utc};

use super::{expire_permission_and_emit, request_permission, resolve_permission};
use crate::application::app_state::AppState;
use crate::application::interactive_notification_producer::permission_notification_key;
use crate::application::permission_state::{PendingPermissionInfo, PERMISSION_RESOLVED_EVENT};
use crate::application::execution_state::ExecutionState;
use crate::domain::entities::Notification;
use crate::domain::entities::{
    ChatConversation, NewNotification, NotificationCategory, NotificationSeverity,
    NotificationTarget, NotificationTargetKind, ProjectId,
};
use crate::domain::repositories::{NotificationPage, NotificationRepository};
use crate::error::{AppError, AppResult};
use crate::http_server::types::{HttpServerState, PermissionRequestInput, ResolvePermissionInput};
use ralphx_events::RecordingEventSink;

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
    HttpServerState {
        app_state,
        execution_state,
        delegation_service: Default::default(),
        external_mcp_supervisor: None,
    }
}

fn make_test_state_with_event_sink() -> (HttpServerState, RecordingEventSink) {
    let event_sink = RecordingEventSink::new();
    let mut app_state = AppState::new_test();
    app_state.events = Arc::new(event_sink.clone());
    let app_state = Arc::new(app_state);
    let execution_state = Arc::new(ExecutionState::new());
    (
        HttpServerState {
            app_state,
            execution_state,
            delegation_service: Default::default(),
            external_mcp_supervisor: None,
        },
        event_sink,
    )
}

struct FailingNotificationRepository;

#[async_trait]
impl NotificationRepository for FailingNotificationRepository {
    async fn create_with_dedupe(&self, _notification: Notification) -> AppResult<bool> {
        Err(AppError::Database("injected notification failure".into()))
    }

    async fn list(
        &self,
        _project_id: Option<&str>,
        _cursor: Option<&str>,
        _limit: u32,
    ) -> AppResult<NotificationPage> {
        Err(AppError::Database("injected notification failure".into()))
    }

    async fn unread_count(&self, _project_id: Option<&str>) -> AppResult<u64> {
        Err(AppError::Database("injected notification failure".into()))
    }

    async fn mark_read(
        &self,
        _id: &str,
        _read_at: DateTime<Utc>,
    ) -> AppResult<Option<Notification>> {
        Err(AppError::Database("injected notification failure".into()))
    }

    async fn mark_read_by_dedupe_key(
        &self,
        _dedupe_key: &str,
        _read_at: DateTime<Utc>,
    ) -> AppResult<Option<Notification>> {
        Err(AppError::Database("injected notification failure".into()))
    }

    async fn mark_all_read(
        &self,
        _project_id: Option<&str>,
        _read_at: DateTime<Utc>,
    ) -> AppResult<u64> {
        Err(AppError::Database("injected notification failure".into()))
    }

    async fn prune(&self, _read_before: DateTime<Utc>, _max_rows: u32) -> AppResult<()> {
        Err(AppError::Database("injected notification failure".into()))
    }
}

#[tokio::test]
async fn request_permission_records_one_deduplicated_notification_without_event_listener() {
    let state = make_test_state();
    let conversation = ChatConversation::new_project(ProjectId::from_string("project-1".into()));
    state
        .app_state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .unwrap();
    let request = PermissionRequestInput {
        request_id: Some("permission-request-1".into()),
        tool_name: "Bash".into(),
        tool_input: serde_json::json!({"command": "git status"}),
        context: Some("Repository setup".into()),
        agent_type: Some("worker".into()),
        task_id: None,
        context_type: Some("project".into()),
        context_id: Some(conversation.id.to_string()),
    };

    let first = request_permission(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(request),
    )
    .await
    .0;
    let second = request_permission(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(PermissionRequestInput {
            request_id: Some(first.request_id.clone()),
            tool_name: "Bash".into(),
            tool_input: serde_json::json!({}),
            context: Some("Repository setup".into()),
            agent_type: Some("worker".into()),
            task_id: None,
            context_type: Some("project".into()),
            context_id: Some(conversation.id.to_string()),
        }),
    )
    .await
    .0;

    assert_eq!(second.request_id, first.request_id);
    let rows = state
        .app_state
        .notification_repo
        .list(None, None, 50)
        .await
        .unwrap()
        .notifications;
    assert_eq!(rows.len(), 1, "server-side record must not need a listener");
    let row = &rows[0];
    assert_eq!(row.category, NotificationCategory::PermissionRequest);
    assert_eq!(row.dedupe_key.as_deref(), Some("perm:permission-request-1"));
    assert_eq!(row.target.kind, NotificationTargetKind::AgentConversation);
    let conversation_id = conversation.id.as_str();
    assert_eq!(
        row.target.conversation_id.as_deref(),
        Some(conversation_id.as_str())
    );
    assert_eq!(row.project_id.as_deref(), Some("project-1"));
    assert_eq!(
        row.body.as_deref(),
        Some("worker wants to run Bash on “Repository setup” — expires in 5m")
    );

    assert_eq!(
        resolve_permission(
            State(state.clone()),
            Json(ResolvePermissionInput {
                request_id: first.request_id,
                decision: "allow".into(),
                message: None,
            }),
        )
        .await,
        StatusCode::OK
    );
    let settled = state
        .app_state
        .notification_repo
        .list(None, None, 50)
        .await
        .unwrap()
        .notifications;
    assert!(settled[0].read_at.is_some());
}

#[tokio::test]
async fn request_permission_returns_after_notification_repository_failure() {
    let mut app_state = AppState::new_test();
    app_state.notification_repo = Arc::new(FailingNotificationRepository);
    let state = HttpServerState::new_test(Arc::new(app_state));

    let response = request_permission(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(PermissionRequestInput {
            request_id: Some("permission-failure".into()),
            tool_name: "Bash".into(),
            tool_input: serde_json::json!({}),
            context: None,
            agent_type: None,
            task_id: None,
            context_type: None,
            context_id: None,
        }),
    )
    .await
    .0;

    assert_eq!(response.request_id, "permission-failure");
    assert!(state
        .app_state
        .permission_state
        .pending
        .lock()
        .await
        .contains_key("permission-failure"));
}

#[tokio::test]
async fn resolve_permission_emits_resolved_event_once_with_request_id() {
    let (state, event_sink) = make_test_state_with_event_sink();
    state
        .app_state
        .permission_state
        .register(make_info("permission-resolve-1"))
        .await;

    let status = resolve_permission(
        State(state),
        Json(ResolvePermissionInput {
            request_id: "permission-resolve-1".into(),
            decision: "allow".into(),
            message: Some("Approved".into()),
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        event_sink.events(),
        vec![ralphx_events::RecordedEvent {
            event: PERMISSION_RESOLVED_EVENT.to_string(),
            payload: serde_json::json!({ "request_id": "permission-resolve-1" }),
        }]
    );
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
    state
        .app_state
        .notification_service()
        .record(NewNotification {
            project_id: None,
            category: NotificationCategory::PermissionRequest,
            severity: NotificationSeverity::ActionRequired,
            title: "Permission needed".into(),
            body: None,
            target: NotificationTarget::none(),
            dedupe_key: Some(permission_notification_key("nonexistent")),
        })
        .await;

    let result =
        expire_permission_and_emit(&state, "nonexistent", StatusCode::REQUEST_TIMEOUT).await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), StatusCode::REQUEST_TIMEOUT);
    let notifications = state
        .app_state
        .notification_repo
        .list(None, None, 50)
        .await
        .unwrap()
        .notifications;
    assert!(notifications[0].read_at.is_none());
}

/// Verify that `permission:expired` event carries `{ request_id }` in its payload.
/// With app_handle = None in test mode, emission is a no-op; the payload structure
/// is enforced at compile time via the serde_json::json! macro in expire_permission_and_emit.
/// This test documents the event contract and ensures the helper compiles with the
/// correct payload shape.
#[tokio::test]
async fn test_expire_permission_event_payload_shape() {
    let (state, event_sink) = make_test_state_with_event_sink();
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
    let events = event_sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, "permission:expired");
    assert_eq!(
        events[0].payload,
        serde_json::json!({ "request_id": request_id })
    );
    assert!(events
        .iter()
        .all(|event| event.event != PERMISSION_RESOLVED_EVENT));
}
