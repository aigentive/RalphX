//! Spawn-free intent twin: this surface only validates and persists a host-owned finalize decision
//! intent. It deliberately has no AppHandle, ExecutionState, or ChatService access.

use crate::application::AppState;
use crate::domain::entities::{
    AcceptanceStatus, IdeationSessionId, IdeationSessionStatus, RemoteFinalizeDecision,
    RemoteFinalizeDecisionRequest, RemoteFinalizeDecisionRequestStatus,
};
use serde::{Deserialize, Serialize};

pub const REMOTE_FINALIZE_LOOKUP_FAILED: &str = "REMOTE_FINALIZE_LOOKUP_FAILED";
pub const REMOTE_FINALIZE_SESSION_NOT_FOUND: &str = "REMOTE_FINALIZE_SESSION_NOT_FOUND";
pub const REMOTE_FINALIZE_AUTHORITY_CHANGED: &str = "REMOTE_FINALIZE_AUTHORITY_CHANGED";
pub const REMOTE_FINALIZE_NOT_PENDING: &str = "REMOTE_FINALIZE_NOT_PENDING";
pub const REMOTE_FINALIZE_ENQUEUE_FAILED: &str = "REMOTE_FINALIZE_ENQUEUE_FAILED";
pub const REMOTE_FINALIZE_REQUEST_NOT_FOUND: &str = "REMOTE_FINALIZE_REQUEST_NOT_FOUND";
pub const REMOTE_FINALIZE_HOST_FAILED: &str = "REMOTE_FINALIZE_HOST_FAILED";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRemoteFinalizeDecisionInput {
    pub session_id: String,
    pub decision: RemoteFinalizeDecision,
}
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFinalizeDecisionIntentResponse {
    pub request_id: String,
    pub status: RemoteFinalizeDecisionRequestStatus,
    pub deduplicated: bool,
    pub created_at: String,
}
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFinalizeDecisionRequestView {
    pub request_id: String,
    pub status: RemoteFinalizeDecisionRequestStatus,
    pub error_code: Option<String>,
    pub result: Option<serde_json::Value>,
    pub created_at: String,
    pub updated_at: String,
}

pub async fn request_remote_finalize_decision_for_state(
    state: &AppState,
    input: RequestRemoteFinalizeDecisionInput,
) -> Result<RemoteFinalizeDecisionIntentResponse, String> {
    let session_id = IdeationSessionId::from_string(input.session_id);
    let session = state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .map_err(|_| REMOTE_FINALIZE_LOOKUP_FAILED.to_string())?
        .ok_or_else(|| REMOTE_FINALIZE_SESSION_NOT_FOUND.to_string())?;
    if session.status != IdeationSessionStatus::Active {
        return Err(REMOTE_FINALIZE_AUTHORITY_CHANGED.to_string());
    }
    if session.acceptance_status != Some(AcceptanceStatus::Pending) {
        return Err(REMOTE_FINALIZE_NOT_PENDING.to_string());
    }
    if let Some(existing) = state
        .remote_finalize_decision_request_repo
        .find_unsettled_for_session(&session_id)
        .await
        .map_err(|_| REMOTE_FINALIZE_LOOKUP_FAILED.to_string())?
    {
        if existing.decision == input.decision {
            return Ok(response(existing, true));
        }
        return Err(REMOTE_FINALIZE_AUTHORITY_CHANGED.to_string());
    }
    let now = chrono::Utc::now();
    let row = RemoteFinalizeDecisionRequest {
        id: uuid::Uuid::new_v4().to_string(),
        session_id,
        decision: input.decision,
        status: RemoteFinalizeDecisionRequestStatus::Pending,
        error_code: None,
        result: None,
        claimed_at: None,
        created_at: now,
        updated_at: now,
    };
    state
        .remote_finalize_decision_request_repo
        .create_remote_finalize_decision_request(row)
        .await
        .map(|v| response(v, false))
        .map_err(|_| REMOTE_FINALIZE_ENQUEUE_FAILED.to_string())
}
fn response(
    row: RemoteFinalizeDecisionRequest,
    deduplicated: bool,
) -> RemoteFinalizeDecisionIntentResponse {
    RemoteFinalizeDecisionIntentResponse {
        request_id: row.id,
        status: row.status,
        deduplicated,
        created_at: row.created_at.to_rfc3339(),
    }
}

pub async fn get_remote_finalize_decision_request_for_state(
    state: &AppState,
    request_id: String,
) -> Result<RemoteFinalizeDecisionRequestView, String> {
    let row = state
        .remote_finalize_decision_request_repo
        .get(&request_id)
        .await
        .map_err(|_| REMOTE_FINALIZE_LOOKUP_FAILED.to_string())?
        .ok_or_else(|| REMOTE_FINALIZE_REQUEST_NOT_FOUND.to_string())?;
    Ok(RemoteFinalizeDecisionRequestView {
        request_id: row.id,
        status: row.status,
        error_code: row.error_code,
        result: row.result,
        created_at: row.created_at.to_rfc3339(),
        updated_at: row.updated_at.to_rfc3339(),
    })
}
