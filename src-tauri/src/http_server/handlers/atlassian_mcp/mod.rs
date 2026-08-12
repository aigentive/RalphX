//! Backend proxy for the Atlassian MCP tool surface.
//!
//! Enforcement here is independent of spawn-time tool filtering. The tier is
//! re-derived per request from the run's **persisted** routing role and project
//! id, so lowering a role or disabling the integration takes effect immediately
//! for in-flight sessions.
//!
//! `AgentRun::launch_role` is display attribution covering three agents and is
//! never consulted for authorization.

pub mod confluence;
pub mod jira;
pub mod raw;

use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;

use crate::application::{effective_atlassian_mcp_access, AtlassianApiError};
use crate::domain::agents::AtlassianMcpAccess;
use crate::domain::entities::{AgentRunId, ChatConversationId, ProjectId};
use crate::http_server::handlers::trusted_run_authority::{
    resolve_live_caller_run, TrustedRunRejection,
};
use crate::http_server::HttpServerState;

/// Why an Atlassian MCP tool call was refused or failed.
#[derive(Debug)]
pub enum AtlassianMcpHttpError {
    /// Transport identity headers were missing or unusable.
    MissingCallerIdentity,
    /// The transport-supplied run is not a live, correctly bound caller.
    UntrustedCaller,
    /// The run has no persisted routing role or project, or the resolved tier is
    /// below what this endpoint requires.
    AccessDenied,
    /// The Atlassian integration is disabled or not validated.
    IntegrationUnavailable,
    /// Request arguments were rejected before any Atlassian call.
    InvalidRequest(String),
    /// Atlassian answered with a failure, or the call could not be completed.
    Api(AtlassianApiError),
}

impl From<AtlassianApiError> for AtlassianMcpHttpError {
    fn from(error: AtlassianApiError) -> Self {
        Self::Api(error)
    }
}

impl IntoResponse for AtlassianMcpHttpError {
    fn into_response(self) -> axum::response::Response {
        let (status, code, message) = match self {
            Self::MissingCallerIdentity => (
                StatusCode::UNAUTHORIZED,
                "atlassian_mcp_missing_identity",
                "Atlassian MCP tools require trusted runtime identity.".to_string(),
            ),
            Self::UntrustedCaller => (
                StatusCode::UNAUTHORIZED,
                "atlassian_mcp_untrusted_caller",
                "Atlassian MCP tools require a live caller run bound to this conversation."
                    .to_string(),
            ),
            Self::AccessDenied => (
                StatusCode::FORBIDDEN,
                "atlassian_mcp_access_denied",
                "This agent role is not granted this Atlassian operation.".to_string(),
            ),
            Self::IntegrationUnavailable => (
                StatusCode::FAILED_DEPENDENCY,
                "atlassian_integration_unavailable",
                "The Atlassian integration is not enabled and validated.".to_string(),
            ),
            Self::InvalidRequest(message) => (
                StatusCode::BAD_REQUEST,
                "atlassian_mcp_invalid_request",
                message,
            ),
            // Classified by numeric status, never by message text.
            Self::Api(error) => {
                let status = match error.status {
                    Some(429) => StatusCode::TOO_MANY_REQUESTS,
                    Some(404) => StatusCode::NOT_FOUND,
                    Some(401) | Some(403) => StatusCode::FORBIDDEN,
                    Some(code) if (400..500).contains(&code) => StatusCode::BAD_REQUEST,
                    _ => StatusCode::BAD_GATEWAY,
                };
                (status, "atlassian_api_error", error.message)
            }
        };

        (
            status,
            axum::Json(serde_json::json!({
                "error": code,
                "details": message,
            })),
        )
            .into_response()
    }
}

/// Authorize an Atlassian MCP request.
///
/// Order is load-bearing: transport identity, then live-run authority, then the
/// persisted role/project, then the tier gate. Credentials stay owned by
/// [`crate::application::AtlassianIntegrationService`]; handlers never see them.
///
/// # Errors
///
/// Returns [`AtlassianMcpHttpError`] when identity is missing, the caller run is
/// not authoritative, the tier is insufficient, or the integration is unusable.
pub async fn authorize(
    state: &HttpServerState,
    headers: &HeaderMap,
    required: AtlassianMcpAccess,
) -> Result<AtlassianMcpAccess, AtlassianMcpHttpError> {
    let conversation_id = trusted_header(headers, "x-ralphx-conversation-id")?
        .parse::<ChatConversationId>()
        .map_err(|_| AtlassianMcpHttpError::MissingCallerIdentity)?;
    let run_id = AgentRunId::from_string(trusted_header(headers, "x-ralphx-agent-run-id")?);

    let run = resolve_live_caller_run(&state.app_state.agent_run_repo, &conversation_id, &run_id)
        .await
        .map_err(|rejection| {
            tracing::warn!(
                conversation_id = %conversation_id,
                run_id = %run_id,
                reason = ?rejection_reason(&rejection),
                "atlassian_mcp caller run rejected"
            );
            AtlassianMcpHttpError::UntrustedCaller
        })?;

    // Runs created before role persistence carry NULL and are denied.
    let Some(routing_role) = run.routing_role else {
        return Err(AtlassianMcpHttpError::AccessDenied);
    };
    let project_root = match run.project_id.as_deref() {
        Some(project_id) => state
            .app_state
            .project_repo
            .get_by_id(&ProjectId::from_string(project_id.to_string()))
            .await
            .map_err(|_| AtlassianMcpHttpError::AccessDenied)?
            .map(|project| std::path::PathBuf::from(project.working_directory)),
        None => None,
    };

    let integration = &state.app_state.atlassian_integration_service;
    let role_defaults = state.app_state.manual_role_default_service();
    let access = effective_atlassian_mcp_access(
        integration,
        &role_defaults,
        Some(routing_role),
        run.project_id.as_deref(),
        project_root.as_deref(),
    )
    .await;

    if access == AtlassianMcpAccess::None {
        // Distinguish "integration off" from "role denied" so the agent can tell
        // a configuration problem from an authorization one.
        return Err(if integration.is_usable().await {
            AtlassianMcpHttpError::AccessDenied
        } else {
            AtlassianMcpHttpError::IntegrationUnavailable
        });
    }
    if !meets(access, required) {
        return Err(AtlassianMcpHttpError::AccessDenied);
    }

    Ok(access)
}

fn meets(access: AtlassianMcpAccess, required: AtlassianMcpAccess) -> bool {
    match required {
        AtlassianMcpAccess::None => true,
        AtlassianMcpAccess::Read => access.allows_read(),
        AtlassianMcpAccess::ReadWrite => access.allows_write(),
    }
}

fn trusted_header(headers: &HeaderMap, name: &str) -> Result<String, AtlassianMcpHttpError> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or(AtlassianMcpHttpError::MissingCallerIdentity)
}

fn rejection_reason(rejection: &TrustedRunRejection) -> &'static str {
    match rejection {
        TrustedRunRejection::RunNotFound => "run_not_found",
        TrustedRunRejection::ConversationMismatch => "conversation_mismatch",
        TrustedRunRejection::RunTerminal { .. } => "run_terminal",
        TrustedRunRejection::RepositoryError(_) => "repository_error",
    }
}

/// Reject blank request fields before any Atlassian call.
///
/// # Errors
///
/// Returns [`AtlassianMcpHttpError::InvalidRequest`] when the value is blank.
pub(crate) fn required_field(value: &str, message: &str) -> Result<String, AtlassianMcpHttpError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AtlassianMcpHttpError::InvalidRequest(message.to_string()));
    }
    Ok(trimmed.to_string())
}
