use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use uuid::Uuid;

use super::*;
use crate::application::interactive_notification_producer::{
    permission_notification_key, InteractiveNotificationProducer,
};
use crate::application::permission_state::PendingPermissionInfo;
use crate::application::{
    NotificationContextResolver, PermissionDecision, PERMISSION_REQUEST_TTL,
    PERMISSION_RESOLVED_EVENT,
};
use crate::domain::entities::NotificationTargetKind;

pub async fn request_permission(
    State(state): State<HttpServerState>,
    headers: axum::http::HeaderMap,
    Json(input): Json<PermissionRequestInput>,
) -> Json<PermissionRequestResponse> {
    let request_id = input
        .request_id
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let info = PendingPermissionInfo {
        request_id: request_id.clone(),
        tool_name: input.tool_name.clone(),
        tool_input: input.tool_input.clone(),
        context: input.context.clone(),
        agent_type: input.agent_type.clone(),
        task_id: input.task_id.clone(),
        context_type: input.context_type.clone(),
        context_id: input.context_id.clone(),
        created_at: Utc::now().to_rfc3339(),
    };

    // Store pending request with metadata
    state
        .app_state
        .permission_state
        .register(info.clone())
        .await;

    let notification_context = NotificationContextResolver::from_app_state(&state.app_state);
    match notification_context
        .resolve_permission_target_with_trusted_conversation(
            info.task_id.as_deref(),
            info.context_type.as_deref(),
            info.context_id.as_deref(),
            trusted_conversation_id(&headers),
        )
        .await
    {
        Ok(resolved) if resolved.target.kind != NotificationTargetKind::None => {
            state
                .app_state
                .notification_service()
                .record(InteractiveNotificationProducer::permission_request(
                    &info, resolved,
                ))
                .await;
        }
        Ok(_) => {
            tracing::warn!(request_id = %request_id, "Skipped permission notification without a navigable target")
        }
        Err(error) => {
            tracing::warn!(error = %error, request_id = %request_id, "Failed to resolve permission notification context");
        }
    }

    crate::http_server::emit_http_event(
        &state,
        "permission:request",
        serde_json::json!({
            "request_id": &request_id,
            "tool_name": &input.tool_name,
            "tool_input": &input.tool_input,
            "context": &input.context,
            "agent_type": &input.agent_type,
            "task_id": &input.task_id,
            "context_type": &input.context_type,
            "context_id": &input.context_id,
        }),
    );

    Json(PermissionRequestResponse { request_id })
}

/// Remove the request from state and return the resolved decision on the success path.
/// Does NOT emit `permission:expired` — expiry events are only for timeout/error paths.
async fn remove_and_return(
    state: &HttpServerState,
    request_id: &str,
    decision: PermissionDecision,
) -> Result<Json<PermissionDecision>, StatusCode> {
    state.app_state.permission_state.remove(request_id).await;
    Ok(Json(decision))
}

/// Remove the request from state and emit `permission:expired` on all timeout/error paths.
/// Returns `Err(code)` so callers can propagate it directly.
pub(crate) async fn expire_permission_and_emit(
    state: &HttpServerState,
    request_id: &str,
    code: StatusCode,
) -> Result<Json<PermissionDecision>, StatusCode> {
    let removed = state.app_state.permission_state.remove(request_id).await;
    if removed {
        state
            .app_state
            .notification_service()
            .resolve_workflow_notification(&permission_notification_key(request_id))
            .await;
    }
    crate::http_server::emit_http_event(
        state,
        "permission:expired",
        serde_json::json!({ "request_id": request_id }),
    );
    Err(code)
}

pub async fn await_permission(
    State(state): State<HttpServerState>,
    Path(request_id): Path<String>,
) -> Result<Json<PermissionDecision>, StatusCode> {
    // Get the receiver for this request
    let mut rx = {
        let pending = state.app_state.permission_state.pending.lock().await;
        match pending.get(&request_id).map(|req| req.sender.subscribe()) {
            Some(rx) => rx,
            None => return Err(StatusCode::NOT_FOUND),
        }
    };

    // Wait for decision with the shared permission-request timeout.
    let timeout = PERMISSION_REQUEST_TTL;
    let start = tokio::time::Instant::now();

    // Use loop to poll for changes
    loop {
        // Check if value is Some - extract and drop borrow immediately
        let maybe_decision: Option<PermissionDecision> = {
            let current = rx.borrow();
            current.clone()
        };

        if let Some(decision) = maybe_decision {
            // Clean up — success path does not emit permission:expired
            return remove_and_return(&state, &request_id, decision).await;
        }

        // Check timeout
        if start.elapsed() >= timeout {
            return expire_permission_and_emit(&state, &request_id, StatusCode::REQUEST_TIMEOUT)
                .await;
        }

        // Wait for change with remaining timeout
        let remaining = timeout.saturating_sub(start.elapsed());
        match tokio::time::timeout(remaining, rx.changed()).await {
            Ok(Ok(())) => continue, // Value changed, loop again to check
            Ok(Err(_)) => {
                // Channel closed
                return expire_permission_and_emit(
                    &state,
                    &request_id,
                    StatusCode::INTERNAL_SERVER_ERROR,
                )
                .await;
            }
            Err(_) => {
                // Timeout
                return expire_permission_and_emit(
                    &state,
                    &request_id,
                    StatusCode::REQUEST_TIMEOUT,
                )
                .await;
            }
        }
    }
}

pub async fn resolve_permission(
    State(state): State<HttpServerState>,
    Json(input): Json<ResolvePermissionInput>,
) -> StatusCode {
    let resolved = state
        .app_state
        .permission_state
        .resolve(
            &input.request_id,
            PermissionDecision {
                decision: input.decision,
                message: input.message,
            },
        )
        .await;

    if resolved {
        state
            .app_state
            .notification_service()
            .resolve_workflow_notification(&permission_notification_key(&input.request_id))
            .await;
        crate::http_server::emit_http_event(
            &state,
            PERMISSION_RESOLVED_EVENT,
            serde_json::json!({ "request_id": &input.request_id }),
        );
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}

#[cfg(test)]
#[path = "permissions_tests.rs"]
mod tests;
