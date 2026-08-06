use axum::{
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::domain::entities::{
    AgentRun, AgentRunId, ChatConversationId, TeamMemberId, TeamRunBindingStatus, TeamSession,
};
use crate::http_server::handlers::trusted_run_authority::{
    resolve_live_caller_run, TrustedRunRejection,
};
use crate::http_server::types::HttpServerState;

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

pub(super) struct TeamAuthorityResolution {
    pub run: AgentRun,
    pub session: TeamSession,
    pub conversation_id: ChatConversationId,
    pub kind: TeamAuthorityKind,
}

pub(super) enum TeamAuthorityKind {
    Coordinator,
    Member {
        member_id: TeamMemberId,
        generation: i64,
    },
}

/// Resolves Team authority from the trusted run → binding → session chain.
pub(super) async fn resolve_team_authority(
    state: &HttpServerState,
    headers: &HeaderMap,
) -> Result<TeamAuthorityResolution, JsonError> {
    let enabled = state
        .app_state
        .managed_team
        .team_capability_enabled()
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    if !enabled {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            "Team capability is disabled",
        ));
    }
    let conversation_id = headers
        .get("x-ralphx-conversation-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ChatConversationId::from_string)
        .ok_or_else(|| {
            json_error(
                StatusCode::BAD_REQUEST,
                "Team tools require trusted caller conversation context",
            )
        })?;
    let run_id = headers
        .get("x-ralphx-agent-run-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(AgentRunId::from_string)
        .ok_or_else(|| {
            json_error(
                StatusCode::BAD_REQUEST,
                "Team tools require trusted caller run context",
            )
        })?;
    let active =
        resolve_live_caller_run(&state.app_state.agent_run_repo, &conversation_id, &run_id)
            .await
            .map_err(|rejection| match rejection {
                TrustedRunRejection::RunNotFound => {
                    tracing::warn!(
                        conversation_id = %conversation_id,
                        run_id = %run_id,
                        reason = "run_not_found",
                        "resolve_team_authority rejected"
                    );
                    json_error(StatusCode::CONFLICT, "Trusted Team run is not active")
                }
                TrustedRunRejection::ConversationMismatch => {
                    tracing::warn!(
                        conversation_id = %conversation_id,
                        run_id = %run_id,
                        reason = "conversation_mismatch",
                        "resolve_team_authority rejected"
                    );
                    json_error(
                        StatusCode::CONFLICT,
                        "Trusted Team run does not belong to the caller conversation",
                    )
                }
                TrustedRunRejection::RunTerminal { status } => {
                    tracing::warn!(
                        conversation_id = %conversation_id,
                        run_id = %run_id,
                        run_status = %status,
                        reason = "run_terminal",
                        "resolve_team_authority rejected"
                    );
                    json_error(
                        StatusCode::CONFLICT,
                        format!("Trusted Team run has already finished (status: {status})"),
                    )
                }
                TrustedRunRejection::RepositoryError(error) => {
                    tracing::warn!(
                        conversation_id = %conversation_id,
                        run_id = %run_id,
                        reason = "repository_error",
                        %error,
                        "resolve_team_authority rejected"
                    );
                    json_error(StatusCode::INTERNAL_SERVER_ERROR, error)
                }
            })?;
    let binding = state
        .app_state
        .managed_team
        .run_binding_repo()
        .get_by_agent_run_id(&run_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Trusted run has no Team binding"))?;
    if binding.conversation_id != conversation_id
        || !matches!(
            binding.status,
            TeamRunBindingStatus::Planned
                | TeamRunBindingStatus::Launching
                | TeamRunBindingStatus::Running
        )
    {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            "Trusted run is not current Team authority",
        ));
    }
    let session = state
        .app_state
        .managed_team
        .team_repo()
        .get_session(&binding.team_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Team session was not found"))?;
    if session.status.is_closed() {
        return Err(json_error(StatusCode::CONFLICT, "Team session is closed"));
    }
    let kind = match (binding.team_member_id, binding.team_member_generation) {
        (None, None) if session.coordinator_conversation_id == conversation_id => {
            TeamAuthorityKind::Coordinator
        }
        (Some(member_id), Some(generation)) => {
            let member = state
                .app_state
                .managed_team
                .team_repo()
                .get_member(&member_id)
                .await
                .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
                .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Team member was not found"))?;
            if member.team_id != session.id
                || !member.is_current_generation(generation)
                || !member.current_run_is_authoritative(generation, &run_id)
            {
                return Err(json_error(
                    StatusCode::CONFLICT,
                    "Trusted member run has stale Team generation authority",
                ));
            }
            TeamAuthorityKind::Member {
                member_id,
                generation,
            }
        }
        _ => {
            return Err(json_error(
                StatusCode::FORBIDDEN,
                "Trusted Team run has invalid member authority",
            ));
        }
    };
    Ok(TeamAuthorityResolution {
        run: active,
        session,
        conversation_id,
        kind,
    })
}
