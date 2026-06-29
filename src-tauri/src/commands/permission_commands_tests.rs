use super::*;
use crate::application::AppState;
use tauri::Manager;

fn permission_command_app() -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build")
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
async fn resolve_permission_request_command_accepts_allow_and_deny() {
    let app = permission_command_app();
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
