//! Handlers for the finalize acceptance gate.
//!
//! These endpoints let users accept or reject proposals that were
//! paused by the `require_accept_for_finalize` confirmation gate.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use tracing::error;

use crate::application::spawn_ready_task_scheduler_if_needed;
use crate::domain::entities::{AcceptanceStatus, IdeationSessionId};
use crate::http_server::helpers::apply_pending_proposals_core_for_session;
use crate::http_server::types::{
    AcceptFinalizeRequest, AcceptanceActionResponse, AcceptanceStatusResponse, HttpServerState,
    PendingConfirmationItem, PendingConfirmationsResponse, RejectFinalizeRequest,
};

use super::{json_error, JsonError};

/// Accept the pending finalize confirmation for a session.
///
/// Applies the pending proposal set and consumes Pending → Accepted in the
/// canonical task-creation transaction.
pub async fn accept_finalize(
    State(state): State<HttpServerState>,
    Json(req): Json<AcceptFinalizeRequest>,
) -> Result<Json<AcceptanceActionResponse>, JsonError> {
    // Verification admission and every other precondition run before the pending
    // confirmation is consumed. The final CAS shares the task transaction, so a
    // failed/queued verification or concurrent rejection leaves no Kanban rows.
    let result = apply_pending_proposals_core_for_session(&state.app_state, &req.session_id)
        .await
        .map_err(|e| {
            error!("Failed to apply proposals after acceptance: {}", e);
            let status = match &e {
                crate::error::AppError::Validation(_) => StatusCode::BAD_REQUEST,
                crate::error::AppError::NotFound(_) => StatusCode::NOT_FOUND,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            json_error(status, e.to_string())
        })?;

    spawn_ready_task_scheduler_if_needed(
        &state.app_state,
        std::sync::Arc::clone(&state.execution_state),
        state.app_state.app_handle.as_ref().cloned(),
        result.any_ready_tasks,
    );

    Ok(Json(AcceptanceActionResponse {
        status: "accepted".to_string(),
        session_id: req.session_id,
    }))
}

/// Reject the pending finalize confirmation.
///
/// Resets acceptance_status to null, allowing the agent to re-finalize.
pub async fn reject_finalize(
    State(state): State<HttpServerState>,
    Json(req): Json<RejectFinalizeRequest>,
) -> Result<Json<AcceptanceActionResponse>, JsonError> {
    let session_id = IdeationSessionId::from_string(req.session_id.clone());

    // CAS: only transition from Pending → None
    let was_pending = state
        .app_state
        .ideation_session_repo
        .update_acceptance_status(&session_id, Some(AcceptanceStatus::Pending), None)
        .await
        .map_err(|e| {
            error!("Failed to reset acceptance_status: {}", e);
            json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;

    if !was_pending {
        return Err(json_error(
            StatusCode::CONFLICT,
            "Session is not in pending_acceptance state",
        ));
    }

    Ok(Json(AcceptanceActionResponse {
        status: "rejected".to_string(),
        session_id: req.session_id,
    }))
}

/// Get the acceptance_status for a session.
pub async fn get_acceptance_status(
    State(state): State<HttpServerState>,
    Path(session_id): Path<String>,
) -> Result<Json<AcceptanceStatusResponse>, JsonError> {
    let session_id_typed = IdeationSessionId::from_string(session_id.clone());
    let session = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id_typed)
        .await
        .map_err(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| {
            json_error(
                StatusCode::NOT_FOUND,
                format!("Session {} not found", session_id),
            )
        })?;

    Ok(Json(AcceptanceStatusResponse {
        session_id,
        acceptance_status: session.acceptance_status.map(|s| s.to_string()),
    }))
}

/// Query params for get_pending_confirmations
#[derive(Debug, Deserialize)]
pub struct PendingConfirmationsQuery {
    pub project_id: String,
}

/// Get all sessions with pending acceptance confirmation for a project.
pub async fn get_pending_confirmations(
    State(state): State<HttpServerState>,
    Query(params): Query<PendingConfirmationsQuery>,
) -> Result<Json<PendingConfirmationsResponse>, JsonError> {
    let project_id = crate::domain::entities::ProjectId::from_string(params.project_id);

    let sessions = state
        .app_state
        .ideation_session_repo
        .get_sessions_with_pending_acceptance(&project_id)
        .await
        .map_err(|e| json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let items = sessions
        .into_iter()
        .map(|s| PendingConfirmationItem {
            session_id: s.id.as_str().to_string(),
            session_title: s.title,
        })
        .collect();

    Ok(Json(PendingConfirmationsResponse { sessions: items }))
}
