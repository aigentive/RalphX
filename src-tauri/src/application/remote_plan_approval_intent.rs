//! Spawn-free intent twin: this surface only validates and persists host-owned plan approval
//! intent. It deliberately has no AppHandle, ExecutionState, or ChatService access.

use crate::application::AppState;
use crate::domain::entities::{
    IdeationSessionFlow, IdeationSessionId, IdeationSessionStatus, RemotePlanApprovalRequest,
    RemotePlanApprovalRequestStatus,
};
use serde::{Deserialize, Serialize};

pub const REMOTE_PLAN_APPROVAL_LOOKUP_FAILED: &str = "REMOTE_PLAN_APPROVAL_LOOKUP_FAILED";
pub const REMOTE_PLAN_APPROVAL_SESSION_NOT_FOUND: &str = "REMOTE_PLAN_APPROVAL_SESSION_NOT_FOUND";
pub const REMOTE_PLAN_APPROVAL_AUTHORITY_CHANGED: &str = "REMOTE_PLAN_APPROVAL_AUTHORITY_CHANGED";
pub const REMOTE_PLAN_APPROVAL_ARTIFACT_REQUIRED: &str = "REMOTE_PLAN_APPROVAL_ARTIFACT_REQUIRED";
pub const REMOTE_PLAN_APPROVAL_ENQUEUE_FAILED: &str = "REMOTE_PLAN_APPROVAL_ENQUEUE_FAILED";
pub const REMOTE_PLAN_APPROVAL_REQUEST_NOT_FOUND: &str = "REMOTE_PLAN_APPROVAL_REQUEST_NOT_FOUND";
pub const REMOTE_PLAN_APPROVAL_HOST_FAILED: &str = "REMOTE_PLAN_APPROVAL_HOST_FAILED";
pub const REMOTE_PLAN_APPROVAL_PLAN_CHANGED: &str = "REMOTE_PLAN_APPROVAL_PLAN_CHANGED";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRemotePlanApprovalInput {
    pub session_id: String,
    pub artifact_id: String,
    pub blueprint_artifact_id: Option<String>,
    pub blueprint_artifact_version: Option<u32>,
}
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RemotePlanApprovalIntentResponse {
    pub request_id: String,
    pub status: RemotePlanApprovalRequestStatus,
    pub deduplicated: bool,
    pub created_at: String,
}
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RemotePlanApprovalRequestView {
    pub request_id: String,
    pub status: RemotePlanApprovalRequestStatus,
    pub error_code: Option<String>,
    pub result: Option<serde_json::Value>,
    pub created_at: String,
    pub updated_at: String,
}

pub async fn request_remote_plan_approval_for_state(
    state: &AppState,
    input: RequestRemotePlanApprovalInput,
) -> Result<RemotePlanApprovalIntentResponse, String> {
    let session_id = IdeationSessionId::from_string(input.session_id);
    let session = state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .map_err(|_| REMOTE_PLAN_APPROVAL_LOOKUP_FAILED.to_string())?
        .ok_or_else(|| REMOTE_PLAN_APPROVAL_SESSION_NOT_FOUND.to_string())?;
    if session.status != IdeationSessionStatus::Active
        || session.session_flow != IdeationSessionFlow::Planning
    {
        return Err(REMOTE_PLAN_APPROVAL_AUTHORITY_CHANGED.to_string());
    }
    if input.artifact_id.trim().is_empty() {
        return Err(REMOTE_PLAN_APPROVAL_ARTIFACT_REQUIRED.to_string());
    }
    if let Some(existing) = state
        .remote_plan_approval_request_repo
        .find_unsettled_for_session(&session_id)
        .await
        .map_err(|_| REMOTE_PLAN_APPROVAL_LOOKUP_FAILED.to_string())?
    {
        if existing.artifact_id == input.artifact_id
            && existing.blueprint_artifact_id == input.blueprint_artifact_id
            && existing.blueprint_artifact_version == input.blueprint_artifact_version
        {
            return Ok(response(existing, true));
        }
        return Err(REMOTE_PLAN_APPROVAL_AUTHORITY_CHANGED.to_string());
    }
    let now = chrono::Utc::now();
    let row = RemotePlanApprovalRequest {
        id: uuid::Uuid::new_v4().to_string(),
        session_id,
        artifact_id: input.artifact_id,
        blueprint_artifact_id: input.blueprint_artifact_id,
        blueprint_artifact_version: input.blueprint_artifact_version,
        status: RemotePlanApprovalRequestStatus::Pending,
        error_code: None,
        result: None,
        claimed_at: None,
        created_at: now,
        updated_at: now,
    };
    state
        .remote_plan_approval_request_repo
        .create_remote_plan_approval_request(row)
        .await
        .map(|v| response(v, false))
        .map_err(|_| REMOTE_PLAN_APPROVAL_ENQUEUE_FAILED.to_string())
}
fn response(
    row: RemotePlanApprovalRequest,
    deduplicated: bool,
) -> RemotePlanApprovalIntentResponse {
    RemotePlanApprovalIntentResponse {
        request_id: row.id,
        status: row.status,
        deduplicated,
        created_at: row.created_at.to_rfc3339(),
    }
}

pub async fn get_remote_plan_approval_request_for_state(
    state: &AppState,
    request_id: String,
) -> Result<RemotePlanApprovalRequestView, String> {
    let row = state
        .remote_plan_approval_request_repo
        .get(&request_id)
        .await
        .map_err(|_| REMOTE_PLAN_APPROVAL_LOOKUP_FAILED.to_string())?
        .ok_or_else(|| REMOTE_PLAN_APPROVAL_REQUEST_NOT_FOUND.to_string())?;
    Ok(RemotePlanApprovalRequestView {
        request_id: row.id,
        status: row.status,
        error_code: row.error_code,
        result: row.result,
        created_at: row.created_at.to_rfc3339(),
        updated_at: row.updated_at.to_rfc3339(),
    })
}
