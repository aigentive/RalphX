//! Transport-bound Team message and member-roster handlers.

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::application::managed_team::{
    ManagedTeamMessageRequest, ManagedTeamMessageSender, ManagedTeamMessageTarget,
};
use crate::domain::entities::{
    AgentRunId, ChatConversationId, TeamMessageKind, TeamRunBindingStatus,
};
use crate::http_server::types::{
    HttpServerState, ManagedTeamMessageResponse, ManagedTeamRosterEntry,
    SendManagedTeamMessageRequest,
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

enum MessageAuthority {
    Coordinator {
        team_id: crate::domain::entities::TeamSessionId,
        conversation_id: ChatConversationId,
        run_id: AgentRunId,
    },
    Member {
        team_id: crate::domain::entities::TeamSessionId,
        member_id: crate::domain::entities::TeamMemberId,
        generation: i64,
        run_id: AgentRunId,
    },
}

async fn resolve_message_authority(
    state: &HttpServerState,
    headers: &HeaderMap,
) -> Result<MessageAuthority, JsonError> {
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
    let active = state
        .app_state
        .agent_run_repo
        .get_active_for_conversation(&conversation_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .ok_or_else(|| json_error(StatusCode::CONFLICT, "Trusted Team run is not active"))?;
    if active.id != run_id {
        return Err(json_error(
            StatusCode::CONFLICT,
            "Trusted Team run is not active",
        ));
    }
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
    match (binding.team_member_id, binding.team_member_generation) {
        (None, None) if session.coordinator_conversation_id == conversation_id => {
            Ok(MessageAuthority::Coordinator {
                team_id: session.id,
                conversation_id,
                run_id,
            })
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
            Ok(MessageAuthority::Member {
                team_id: session.id,
                member_id,
                generation,
                run_id,
            })
        }
        _ => Err(json_error(
            StatusCode::FORBIDDEN,
            "Trusted Team run has invalid member authority",
        )),
    }
}

pub async fn send_managed_team_message(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<SendManagedTeamMessageRequest>,
) -> Result<Json<ManagedTeamMessageResponse>, JsonError> {
    let authority = resolve_message_authority(&state, &headers).await?;
    let kind = request
        .kind
        .as_deref()
        .map(|kind| serde_json::from_value(serde_json::Value::String(kind.to_string())))
        .transpose()
        .map_err(|_| json_error(StatusCode::BAD_REQUEST, "Invalid Team message kind"))?
        .unwrap_or(TeamMessageKind::Instruction);
    let (team_id, sender, target) = match authority {
        MessageAuthority::Coordinator {
            team_id,
            conversation_id,
            run_id,
        } => {
            let target = request.target.as_str();
            let target = match target {
                "member" => {
                    ManagedTeamMessageTarget::MemberName(request.member_name.ok_or_else(|| {
                        json_error(
                            StatusCode::BAD_REQUEST,
                            "Member target requires member_name",
                        )
                    })?)
                }
                "broadcast" => ManagedTeamMessageTarget::Broadcast,
                _ => {
                    return Err(json_error(
                        StatusCode::BAD_REQUEST,
                        "Coordinator messages must target a member or broadcast",
                    ));
                }
            };
            (
                team_id,
                ManagedTeamMessageSender::Coordinator {
                    conversation_id,
                    source_run_id: Some(run_id),
                },
                target,
            )
        }
        MessageAuthority::Member {
            team_id,
            member_id,
            generation,
            run_id,
        } => {
            let target = match request.target.as_str() {
                "coordinator" => ManagedTeamMessageTarget::Coordinator,
                "broadcast" => ManagedTeamMessageTarget::Broadcast,
                _ => {
                    return Err(json_error(
                        StatusCode::FORBIDDEN,
                        "Team members may message only the coordinator or broadcast",
                    ));
                }
            };
            (
                team_id,
                ManagedTeamMessageSender::Member {
                    member_id,
                    generation,
                    source_run_id: run_id,
                },
                target,
            )
        }
    };
    let source_run_id = match &sender {
        ManagedTeamMessageSender::Coordinator {
            source_run_id: Some(source_run_id),
            ..
        } => source_run_id.clone(),
        ManagedTeamMessageSender::Member { source_run_id, .. } => source_run_id.clone(),
        ManagedTeamMessageSender::Coordinator {
            source_run_id: None,
            ..
        } => {
            return Err(json_error(
                StatusCode::CONFLICT,
                "Trusted coordinator Team run has no source identity",
            ));
        }
    };
    let idempotency_key = format!("team-tool:{}", source_run_id.as_str());
    let (message, deliveries) = state
        .app_state
        .managed_team
        .send_team_message(ManagedTeamMessageRequest {
            team_id,
            sender,
            target,
            kind,
            content: request.content,
            idempotency_key,
        })
        .await
        .map_err(|error| json_error(StatusCode::CONFLICT, error.to_string()))?;
    Ok(Json(ManagedTeamMessageResponse {
        sequence: message.sequence,
        recipient_count: deliveries.len() as u32,
    }))
}

pub async fn get_managed_team_member_roster(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
) -> Result<Json<Vec<ManagedTeamRosterEntry>>, JsonError> {
    let authority = resolve_message_authority(&state, &headers).await?;
    let team_id = match authority {
        MessageAuthority::Coordinator { team_id, .. }
        | MessageAuthority::Member { team_id, .. } => team_id,
    };
    let members = state
        .app_state
        .managed_team
        .roster(&team_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(
        members
            .into_iter()
            .take(32)
            .map(|member| ManagedTeamRosterEntry {
                name: member.name,
                normalized_name: member.normalized_name,
                role_summary: member.role_summary,
                status: serde_json::to_value(member.status)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_string))
                    .unwrap_or_default(),
            })
            .collect(),
    ))
}
