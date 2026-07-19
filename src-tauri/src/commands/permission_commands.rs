// Tauri commands for permission resolution
// Allows frontend to resolve pending permission requests from agents

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::interactive_notification_producer::permission_notification_key;
use crate::application::{PendingPermissionInfo, PermissionDecision, PERMISSION_RESOLVED_EVENT};
use crate::AppState;

/// Arguments for resolving a permission request
#[derive(Debug, Deserialize)]
pub struct ResolvePermissionArgs {
    pub request_id: String,
    pub decision: String, // "allow" or "deny"
    pub message: Option<String>,
}

/// Response for resolve_permission_request command
#[derive(Debug, Serialize)]
pub struct ResolvePermissionResponse {
    pub success: bool,
    pub message: Option<String>,
}

/// Resolve a pending permission request with a user decision
///
/// Called by the frontend PermissionDialog when the user clicks Allow or Deny.
/// Signals the waiting MCP long-poll request with the decision.
#[tauri::command]
pub async fn resolve_permission_request(
    state: State<'_, AppState>,
    args: ResolvePermissionArgs,
) -> Result<ResolvePermissionResponse, String> {
    // Validate decision value
    if args.decision != "allow" && args.decision != "deny" {
        return Err(format!(
            "Invalid decision '{}'. Must be 'allow' or 'deny'",
            args.decision
        ));
    }

    let decision = PermissionDecision {
        decision: args.decision.clone(),
        message: args.message,
    };

    let resolved = state
        .permission_state
        .resolve(&args.request_id, decision)
        .await;

    if resolved {
        state
            .notification_service()
            .resolve_workflow_notification(&permission_notification_key(&args.request_id))
            .await;
        state.events.emit(
            PERMISSION_RESOLVED_EVENT,
            serde_json::json!({ "request_id": &args.request_id }),
        );
        Ok(ResolvePermissionResponse {
            success: true,
            message: Some(format!("Permission request {} resolved", args.request_id)),
        })
    } else {
        Err(format!(
            "Permission request '{}' not found",
            args.request_id
        ))
    }
}

/// Get information about all pending permission requests
///
/// Used by the frontend to display any pending requests that might have been
/// missed (e.g., if the app was just opened while an agent was waiting for approval).
#[tauri::command]
pub async fn get_pending_permissions(
    state: State<'_, AppState>,
) -> Result<Vec<PendingPermissionInfo>, String> {
    let pending = state.permission_state.get_pending_info().await;
    Ok(pending)
}

#[cfg(test)]
#[path = "permission_commands_tests.rs"]
mod tests;
