use super::*;
use crate::application::AppState;
use crate::application::PERMISSION_RESOLVED_EVENT;
use crate::domain::entities::{
    NewNotification, NotificationCategory, NotificationSeverity, NotificationTarget,
};
use ralphx_events::RecordingEventSink;
use std::sync::Arc;
use tauri::Manager;

fn permission_command_app() -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build")
}

fn permission_command_app_with_event_sink(
) -> (tauri::App<tauri::test::MockRuntime>, RecordingEventSink) {
    let event_sink = RecordingEventSink::new();
    let mut state = AppState::new_test();
    state.events = Arc::new(event_sink.clone());
    let app = tauri::test::mock_builder()
        .manage(state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build");

    (app, event_sink)
}

fn pending_permission(request_id: &str) -> PendingPermissionInfo {
    PendingPermissionInfo {
        request_id: request_id.to_string(),
        tool_name: "mcp__ralphx__get_task_context".to_string(),
        tool_input: serde_json::json!({ "task_id": "task-1" }),
        context: Some("Needs task context".to_string()),
        agent_type: Some("worker".to_string()),
        task_id: Some("task-1".to_string()),
        context_type: Some("task".to_string()),
        context_id: Some("task-1".to_string()),
        created_at: "2026-07-10T00:00:00+00:00".to_string(),
    }
}

#[test]
fn test_resolve_permission_args_deserialize() {
    let json = r#"{"request_id": "abc-123", "decision": "allow", "message": "User approved"}"#;
    let args: ResolvePermissionArgs = serde_json::from_str(json).unwrap();
    assert_eq!(args.request_id, "abc-123");
    assert_eq!(args.decision, "allow");
    assert_eq!(args.message, Some("User approved".to_string()));
}

#[test]
fn test_resolve_permission_args_without_message() {
    let json = r#"{"request_id": "abc-123", "decision": "deny"}"#;
    let args: ResolvePermissionArgs = serde_json::from_str(json).unwrap();
    assert_eq!(args.request_id, "abc-123");
    assert_eq!(args.decision, "deny");
    assert!(args.message.is_none());
}

#[test]
fn test_resolve_permission_response_serialize() {
    let response = ResolvePermissionResponse {
        success: true,
        message: Some("Resolved".to_string()),
    };
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("\"success\":true"));
    assert!(json.contains("\"message\":\"Resolved\""));
}

#[tokio::test]
async fn get_pending_permissions_command_returns_registered_requests() {
    let app = permission_command_app();
    let info = pending_permission("permission-1");
    app.state::<AppState>()
        .permission_state
        .register(info.clone())
        .await;

    let pending = get_pending_permissions(app.state::<AppState>())
        .await
        .expect("pending permissions load");

    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].request_id, info.request_id);
    assert_eq!(pending[0].tool_name, info.tool_name);
    assert_eq!(pending[0].agent_type.as_deref(), Some("worker"));
    assert_eq!(pending[0].context_id.as_deref(), Some("task-1"));
}

#[tokio::test]
async fn list_pending_permission_gates_rehydrates_only_still_pending_disconnect_gates() {
    let app = permission_command_app();
    let mut survives_disconnect = pending_permission("permission-disconnected");
    survives_disconnect.created_at = chrono::Utc::now().to_rfc3339();
    let mut resolved_before_reconnect = pending_permission("permission-resolved");
    resolved_before_reconnect.created_at = chrono::Utc::now().to_rfc3339();
    app.state::<AppState>()
        .permission_state
        .register(survives_disconnect)
        .await;
    app.state::<AppState>()
        .permission_state
        .register(resolved_before_reconnect)
        .await;

    let while_disconnected = list_pending_permission_gates(app.state::<AppState>())
        .await
        .expect("pending gates load while the client is disconnected");
    assert_eq!(while_disconnected.len(), 2);

    assert!(
        app.state::<AppState>()
            .permission_state
            .resolve(
                "permission-resolved",
                PermissionDecision {
                    decision: "deny".to_string(),
                    message: None,
                },
            )
            .await
    );

    let after_reconnect = list_pending_permission_gates(app.state::<AppState>())
        .await
        .expect("pending gates reload on reconnect");
    assert_eq!(
        after_reconnect
            .iter()
            .map(|permission| permission.request_id.as_str())
            .collect::<Vec<_>>(),
        vec!["permission-disconnected"],
        "the unresolved disconnect-window gate is rehydrated and the resolved gate is absent"
    );
}

#[tokio::test]
async fn resolve_permission_request_command_emits_resolved_events_for_allow_and_deny() {
    let (app, event_sink) = permission_command_app_with_event_sink();
    for request_id in ["permission-allow", "permission-deny"] {
        app.state::<AppState>()
            .notification_service()
            .record(NewNotification {
                project_id: None,
                category: NotificationCategory::PermissionRequest,
                severity: NotificationSeverity::ActionRequired,
                title: "Permission required".to_string(),
                body: None,
                target: NotificationTarget::none(),
                dedupe_key: Some(permission_notification_key(request_id)),
            })
            .await;
    }
    app.state::<AppState>()
        .permission_state
        .register(pending_permission("permission-allow"))
        .await;
    app.state::<AppState>()
        .permission_state
        .register(pending_permission("permission-deny"))
        .await;

    let allow = resolve_permission_request(
        app.state::<AppState>(),
        ResolvePermissionArgs {
            request_id: "permission-allow".to_string(),
            decision: "allow".to_string(),
            message: Some("Approved".to_string()),
        },
    )
    .await
    .expect("allow resolution succeeds");
    assert!(allow.success);
    assert_eq!(
        allow.message.as_deref(),
        Some("Permission request permission-allow resolved")
    );

    let deny = resolve_permission_request(
        app.state::<AppState>(),
        ResolvePermissionArgs {
            request_id: "permission-deny".to_string(),
            decision: "deny".to_string(),
            message: None,
        },
    )
    .await
    .expect("deny resolution succeeds");
    assert!(deny.success);

    assert_eq!(
        event_sink.events(),
        vec![
            ralphx_events::RecordedEvent {
                event: PERMISSION_RESOLVED_EVENT.to_string(),
                payload: serde_json::json!({ "request_id": "permission-allow" }),
            },
            ralphx_events::RecordedEvent {
                event: PERMISSION_RESOLVED_EVENT.to_string(),
                payload: serde_json::json!({ "request_id": "permission-deny" }),
            },
        ]
    );
    let notifications = app
        .state::<AppState>()
        .notification_repo
        .list(None, None, 10)
        .await
        .expect("notifications should load")
        .notifications;
    assert_eq!(notifications.len(), 2);
    assert!(notifications
        .iter()
        .all(|notification| notification.read_at.is_some()));
}

#[tokio::test]
async fn resolve_permission_request_command_rejects_invalid_or_missing_requests() {
    let app = permission_command_app();
    app.state::<AppState>()
        .permission_state
        .register(pending_permission("permission-valid"))
        .await;

    let invalid_decision = resolve_permission_request(
        app.state::<AppState>(),
        ResolvePermissionArgs {
            request_id: "permission-valid".to_string(),
            decision: "maybe".to_string(),
            message: None,
        },
    )
    .await
    .expect_err("invalid decision should error");
    assert!(invalid_decision.contains("Invalid decision 'maybe'"));

    let missing = resolve_permission_request(
        app.state::<AppState>(),
        ResolvePermissionArgs {
            request_id: "missing-permission".to_string(),
            decision: "allow".to_string(),
            message: None,
        },
    )
    .await
    .expect_err("missing permission should error");
    assert!(missing.contains("Permission request 'missing-permission' not found"));
}
