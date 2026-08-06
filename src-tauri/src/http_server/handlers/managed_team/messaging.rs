//! Transport-bound Team message and member-roster handlers.

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use sha2::{Digest, Sha256};

use crate::application::managed_team::{
    ManagedTeamMessageRequest, ManagedTeamMessageSender, ManagedTeamMessageTarget,
};
use crate::domain::entities::{AgentRunId, ChatConversationId, TeamMessageKind};
use crate::http_server::handlers::managed_team::authority::{
    resolve_team_authority, TeamAuthorityKind,
};
use crate::http_server::types::{
    HttpServerState, ManagedTeamMessageResponse, ManagedTeamRosterEntry,
    SendManagedTeamMessageRequest,
};

type JsonError = (StatusCode, Json<serde_json::Value>);

pub(super) fn team_tool_idempotency_key(
    source_run_id: &AgentRunId,
    target: &ManagedTeamMessageTarget,
    kind: TeamMessageKind,
    content: &str,
) -> String {
    // The key is persisted in UNIQUE(team_id, idempotency_key), so it must be
    // stable across processes: fixed field order, unit-separated, SHA-256.
    let target_identity = match target {
        ManagedTeamMessageTarget::Coordinator => "coordinator".to_string(),
        ManagedTeamMessageTarget::MemberName(name) => format!("member:{name}"),
        ManagedTeamMessageTarget::Broadcast => "broadcast".to_string(),
    };
    let kind_identity = serde_json::to_value(kind)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{kind:?}"));
    let identity = format!("{target_identity}\u{1f}{kind_identity}\u{1f}{content}");
    let digest = Sha256::digest(identity.as_bytes());
    format!("team-tool:{}:{digest:x}", source_run_id.as_str())
}

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
    let resolution = resolve_team_authority(state, headers).await?;
    match resolution.kind {
        TeamAuthorityKind::Coordinator => Ok(MessageAuthority::Coordinator {
            team_id: resolution.session.id,
            conversation_id: resolution.conversation_id,
            run_id: resolution.run.id,
        }),
        TeamAuthorityKind::Member {
            member_id,
            generation,
        } => Ok(MessageAuthority::Member {
            team_id: resolution.session.id,
            member_id,
            generation,
            run_id: resolution.run.id,
        }),
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
        // `System` is backend-originated only; the sender above is always built
        // from coordinator or member run authority, never from a tool request.
        ManagedTeamMessageSender::System { .. } => {
            return Err(json_error(
                StatusCode::FORBIDDEN,
                "System Team messages cannot originate from a tool request",
            ));
        }
    };
    let idempotency_key =
        team_tool_idempotency_key(&source_run_id, &target, kind, &request.content);
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
