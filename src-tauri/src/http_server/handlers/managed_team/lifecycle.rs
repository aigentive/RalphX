//! Ensure/status/roster endpoints for managed Teams.
//!
//! These endpoints project durable Team state; they never accept run or
//! member IDs as authority and mutate nothing beyond idempotent ensure.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use crate::domain::entities::{
    ChatConversationId, ProjectId, TeamMember, TeamSession, TeamSessionId,
};
use crate::http_server::types::{
    EnsureManagedTeamRequest, HttpServerState, ManagedTeamMemberSummary, ManagedTeamSessionSummary,
    ManagedTeamStatusResponse,
};

type JsonError = (StatusCode, Json<serde_json::Value>);

fn json_error(status: StatusCode, error: impl Into<String>) -> JsonError {
    (
        status,
        Json(serde_json::json!({
            "status": status.as_u16(),
            "error": error.into(),
        })),
    )
}

/// Serializes a snake_case serde enum into its wire string.
fn enum_wire_value<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn session_summary(session: &TeamSession) -> ManagedTeamSessionSummary {
    ManagedTeamSessionSummary {
        id: session.id.as_str().to_string(),
        project_id: session.project_id.as_str().to_string(),
        coordinator_conversation_id: session.coordinator_conversation_id.as_str(),
        status: enum_wire_value(&session.status),
        configured_concurrency: session.configured_concurrency,
        effective_concurrency: session.effective_concurrency,
        automatic_wake_limit: session.automatic_wake_limit,
        version: session.version,
        created_at: session.created_at.to_rfc3339(),
        updated_at: session.updated_at.to_rfc3339(),
    }
}

fn member_summary(member: &TeamMember) -> ManagedTeamMemberSummary {
    ManagedTeamMemberSummary {
        id: member.id.as_str().to_string(),
        team_id: member.team_id.as_str().to_string(),
        name: member.name.clone(),
        normalized_name: member.normalized_name.clone(),
        canonical_agent_name: member.canonical_agent_name.clone(),
        role_summary: member.role_summary.clone(),
        status: enum_wire_value(&member.status),
        generation: member.generation,
    }
}

/// POST /api/managed_team/ensure — idempotently ensures one open Team for the
/// coordinator conversation and returns it.
pub async fn ensure_managed_team(
    State(state): State<HttpServerState>,
    Json(request): Json<EnsureManagedTeamRequest>,
) -> Result<Json<ManagedTeamSessionSummary>, JsonError> {
    let managed_team = &state.app_state.managed_team;
    let enabled = managed_team
        .team_capability_enabled()
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    if !enabled {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            "Team capability is disabled",
        ));
    }
    let conversation_id = ChatConversationId::from_string(&request.conversation_id);
    let session = managed_team
        .ensure_team(ProjectId::from_string(request.project_id), &conversation_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(session_summary(&session)))
}

/// GET /api/managed_team/status/:conversation_id — open Team session plus
/// roster for a coordinator conversation, or `null`.
pub async fn get_managed_team_status(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
) -> Result<Json<Option<ManagedTeamStatusResponse>>, JsonError> {
    let conversation_id = ChatConversationId::from_string(&conversation_id);
    let status = state
        .app_state
        .managed_team
        .team_status(&conversation_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(status.map(|status| ManagedTeamStatusResponse {
        session: session_summary(&status.session),
        members: status.members.iter().map(member_summary).collect(),
    })))
}

/// GET /api/managed_team/roster/:team_id — roster members for a Team.
pub async fn get_managed_team_roster(
    State(state): State<HttpServerState>,
    Path(team_id): Path<String>,
) -> Result<Json<Vec<ManagedTeamMemberSummary>>, JsonError> {
    let members = state
        .app_state
        .managed_team
        .roster(&TeamSessionId::from_string(team_id))
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(members.iter().map(member_summary).collect()))
}
