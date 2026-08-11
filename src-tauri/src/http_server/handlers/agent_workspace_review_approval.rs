use std::str::FromStr;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};

use super::agent_workspaces::{
    settle_workspace_review_publish_authorization, AgentWorkspaceReviewMonitorResponse,
};
use super::*;
use crate::application::agent_workspace_review_approval::approve_agent_workspace_review_anyway;
use crate::domain::entities::{
    AgentWorkspaceReviewApprovalSnapshot, AgentWorkspaceReviewTargetScope, ArtifactId,
    ChatConversationId,
};
use crate::error::AppError;

#[derive(Debug, serde::Deserialize)]
pub struct ApproveAgentWorkspaceReviewAnywayRequest {
    pub target_scope: String,
    pub diff_fingerprint: String,
    pub artifact_id: String,
    pub artifact_version: u32,
}

#[derive(Debug, serde::Serialize)]
pub struct ApproveAgentWorkspaceReviewAnywayResponse {
    pub success: bool,
    pub monitor: AgentWorkspaceReviewMonitorResponse,
}

pub async fn approve_agent_workspace_review_anyway_handler(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
    Json(request): Json<ApproveAgentWorkspaceReviewAnywayRequest>,
) -> Result<Json<ApproveAgentWorkspaceReviewAnywayResponse>, JsonError> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let target_scope = AgentWorkspaceReviewTargetScope::from_str(request.target_scope.trim())
        .map_err(|error| json_error(StatusCode::BAD_REQUEST, error, None))?;
    let diff_fingerprint = required_value(request.diff_fingerprint, "diff_fingerprint")?;
    let artifact_id = required_value(request.artifact_id, "artifact_id")?;
    if request.artifact_version == 0 {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "artifact_version must be greater than zero",
            None,
        ));
    }
    let snapshot = AgentWorkspaceReviewApprovalSnapshot {
        target_scope,
        diff_fingerprint,
        artifact_id: ArtifactId::from_string(artifact_id),
        artifact_version: request.artifact_version,
    };
    let workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(approval_error)?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Agent workspace not found", None))?;
    let monitor =
        approve_agent_workspace_review_anyway(state.app_state.as_ref(), &workspace, &snapshot)
            .await
            .map_err(approval_error)?;

    settle_workspace_review_publish_authorization(&state, &conversation_id, &workspace, &monitor)
        .await?;

    Ok(Json(ApproveAgentWorkspaceReviewAnywayResponse {
        success: true,
        monitor: AgentWorkspaceReviewMonitorResponse::from(monitor),
    }))
}

fn required_value(value: String, field: &str) -> Result<String, JsonError> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            format!("{field} must not be empty"),
            None,
        ));
    }
    Ok(value)
}

fn approval_error(error: AppError) -> JsonError {
    let status = match &error {
        AppError::Validation(_) | AppError::Conflict(_) => StatusCode::CONFLICT,
        AppError::NotFound(_) | AppError::ProjectNotFound(_) => StatusCode::NOT_FOUND,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    json_error(status, error.to_string(), None)
}
