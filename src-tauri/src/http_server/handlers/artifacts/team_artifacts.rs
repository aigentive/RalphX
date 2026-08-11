use super::*;
use crate::domain::entities::{
    AgentRunId, AgentRunStatus, ArtifactRelation, ArtifactRelationId, ArtifactRelationType,
    ChatContextType, ChatConversationId,
};
use tracing::info;

const PLACEHOLDER_SESSION_IDS: &[&str] = &["SESSION_ID", "unknown", "<session_id>"];
const CALLER_AGENT_HEADER: &str = "x-ralphx-agent-type";

fn is_placeholder_session_id(session_id: &str) -> bool {
    let trimmed = session_id.trim();
    trimmed.is_empty()
        || PLACEHOLDER_SESSION_IDS
            .iter()
            .any(|value| trimmed.eq_ignore_ascii_case(value))
}

pub(super) fn artifact_author(
    headers: &axum::http::HeaderMap,
) -> Result<String, (StatusCode, String)> {
    let Some(raw) = headers.get(CALLER_AGENT_HEADER) else {
        return Ok("system".to_string());
    };
    let agent_type = raw.to_str().map(str::trim).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "Invalid caller agent attribution header".to_string(),
        )
    })?;
    if agent_type.is_empty() || agent_type.eq_ignore_ascii_case("unknown") {
        return Ok("system".to_string());
    }
    let Some(config) = crate::infrastructure::agents::claude::get_agent_config(agent_type) else {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Unknown canonical caller agent '{agent_type}'"),
        ));
    };
    Ok(config.name.clone())
}

pub(super) async fn authorized_team_artifact_author(
    app_state: &crate::application::AppState,
    headers: &axum::http::HeaderMap,
    resolved_session_id: &str,
) -> Result<String, (StatusCode, String)> {
    let author = artifact_author(headers)?;
    if author == "system" {
        return Ok(author);
    }

    let config = crate::infrastructure::agents::claude::get_agent_config(&author)
        .expect("artifact_author already validated the canonical agent");
    if !config
        .allowed_mcp_tools
        .iter()
        .any(|tool| tool == "create_team_artifact" || tool.ends_with("__create_team_artifact"))
    {
        return Err((
            StatusCode::FORBIDDEN,
            format!("Caller agent '{author}' cannot create team artifacts"),
        ));
    }

    let authority = resolve_artifact_mutation_authority(headers).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "Canonical agent attribution requires runtime run and conversation authority"
                .to_string(),
        )
    })?;
    let run = app_state
        .agent_run_repo
        .get_by_id(&AgentRunId::from_string(&authority.agent_run_id))
        .await
        .map_err(|error| {
            error!(%error, "Failed to validate team artifact run authority");
            (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
        })?
        .ok_or_else(|| {
            (
                StatusCode::CONFLICT,
                "Team artifact run authority is stale or missing".to_string(),
            )
        })?;
    if run.status != AgentRunStatus::Running
        || run.conversation_id.as_str() != authority.conversation_id
    {
        return Err((
            StatusCode::CONFLICT,
            "Team artifact run authority is not current for the conversation".to_string(),
        ));
    }

    let conversation = app_state
        .chat_conversation_repo
        .get_by_id(&ChatConversationId::from_string(&authority.conversation_id))
        .await
        .map_err(|error| {
            error!(%error, "Failed to validate team artifact conversation authority");
            (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
        })?
        .ok_or_else(|| {
            (
                StatusCode::CONFLICT,
                "Team artifact conversation authority is stale or missing".to_string(),
            )
        })?;

    let parent_conversation = if conversation.context_type == ChatContextType::Delegation {
        if conversation.bound_agent_name.as_deref() != Some(author.as_str()) {
            return Err((
                StatusCode::FORBIDDEN,
                "Caller agent does not match the delegated conversation binding".to_string(),
            ));
        }
        let parent_conversation_id =
            conversation
                .parent_conversation_id
                .as_deref()
                .ok_or_else(|| {
                    (
                        StatusCode::CONFLICT,
                        "Delegated team artifact conversation has no parent lineage".to_string(),
                    )
                })?;
        app_state
            .chat_conversation_repo
            .get_by_id(&ChatConversationId::from_string(parent_conversation_id))
            .await
            .map_err(|error| {
                error!(%error, "Failed to validate team artifact parent conversation");
                (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
            })?
            .ok_or_else(|| {
                (
                    StatusCode::CONFLICT,
                    "Delegated team artifact parent conversation is missing".to_string(),
                )
            })?
    } else {
        conversation
    };

    if parent_conversation.context_type != ChatContextType::Ideation
        || parent_conversation.context_id != resolved_session_id
    {
        return Err((
            StatusCode::FORBIDDEN,
            "Team artifact session does not match the caller conversation lineage".to_string(),
        ));
    }

    Ok(author)
}

async fn validate_team_artifact_session_id(
    state: &HttpServerState,
    session_id: &str,
    action: &str,
) -> Result<String, (StatusCode, String)> {
    if is_placeholder_session_id(session_id) {
        return Err((
            StatusCode::BAD_REQUEST,
            "Invalid session_id for team artifact. Use the parent ideation session_id \
             or the real team/execution session id; do not send placeholder values like \
             'SESSION_ID' or 'unknown'."
                .to_string(),
        ));
    }

    let session_id_obj =
        crate::domain::entities::IdeationSessionId::from_string(session_id.to_string());
    if let Some(session) = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id_obj)
        .await
        .map_err(|e| {
            error!(
                "Failed to validate team artifact session {}: {}",
                session_id, e
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to validate session: {}", e),
            )
        })?
    {
        if session.session_purpose == crate::domain::entities::SessionPurpose::Verification {
            if let Some(parent_id) = session.parent_session_id.as_ref() {
                let parent_id = parent_id.as_str().to_string();
                info!(
                    verification_child_session_id = %session_id,
                    parent_session_id = %parent_id,
                    action,
                    "Auto-corrected verification child session id to parent ideation session for team artifact operation"
                );
                return Ok(parent_id);
            }

            return Err((
                StatusCode::BAD_REQUEST,
                format!(
                    "Cannot {action} team artifacts on a verification child session with no \
                     parent_session_id. Use the PARENT ideation session_id instead."
                ),
            ));
        }
    }

    Ok(session_id.to_string())
}

pub(super) async fn persist_team_artifact(
    app_state: &crate::application::AppState,
    artifact: Artifact,
    related_artifact_id: Option<String>,
) -> Result<ArtifactId, (StatusCode, String)> {
    app_state
        .db
        .run_transaction(move |conn| {
            if let Some(related_id) = related_artifact_id.as_deref() {
                if ArtifactRepo::get_by_id_sync(conn, related_id)?.is_none() {
                    return Err(AppError::Validation(format!(
                        "Related artifact '{related_id}' does not exist"
                    )));
                }
            }

            let artifact_id = artifact.id.clone();
            ArtifactRepo::create_sync(conn, artifact)?;
            if let Some(related_id) = related_artifact_id {
                ArtifactRepo::add_relation_sync(
                    conn,
                    ArtifactRelation {
                        id: ArtifactRelationId::new(),
                        from_artifact_id: artifact_id.clone(),
                        to_artifact_id: ArtifactId::from_string(related_id),
                        relation_type: ArtifactRelationType::RelatedTo,
                    },
                )?;
            }
            Ok(artifact_id)
        })
        .await
        .map_err(|error| match error {
            AppError::Validation(message) => (StatusCode::BAD_REQUEST, message),
            other => {
                error!(%other, "Failed to persist team artifact transaction");
                (StatusCode::INTERNAL_SERVER_ERROR, other.to_string())
            }
        })
}

pub async fn create_team_artifact(
    State(state): State<HttpServerState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<CreateTeamArtifactRequest>,
) -> Result<Json<CreateTeamArtifactResponse>, (StatusCode, String)> {
    let resolved_session_id =
        validate_team_artifact_session_id(&state, &req.session_id, "create").await?;

    // Map team artifact types to ArtifactType
    let artifact_type = match req.artifact_type.as_str() {
        "TeamResearch" => ArtifactType::TeamResearch,
        "TeamAnalysis" => ArtifactType::TeamAnalysis,
        "TeamSummary" => ArtifactType::TeamSummary,
        other => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!(
                    "Invalid artifact_type: '{}'. Valid: TeamResearch, TeamAnalysis, TeamSummary",
                    other
                ),
            ));
        }
    };

    // Create the artifact
    let author =
        authorized_team_artifact_author(&state.app_state, &headers, &resolved_session_id).await?;
    let mut artifact = Artifact::new_inline(&req.title, artifact_type, &req.content, "system");

    // Set bucket to team-findings
    artifact.bucket_id = Some(ArtifactBucketId::from_string("team-findings"));

    // Store team metadata with session_id
    artifact.metadata.team_metadata = Some(crate::domain::entities::TeamArtifactMetadata {
        team_name: "team".to_string(),
        author_teammate: author,
        session_id: Some(resolved_session_id.clone()),
        team_phase: None,
    });

    let artifact_id =
        persist_team_artifact(&state.app_state, artifact, req.related_artifact_id.clone())
            .await?
            .to_string();

    info!(
        artifact_id = %artifact_id,
        session_id = %resolved_session_id,
        requested_session_id = %req.session_id,
        artifact_type = %req.artifact_type,
        "Team artifact created"
    );

    use crate::application::chat_service::{events, TeamArtifactCreatedPayload};
    crate::http_server::emit_serialized_http_event(
        &state,
        events::TEAM_ARTIFACT_CREATED,
        &TeamArtifactCreatedPayload {
            artifact_id: artifact_id.clone(),
            session_id: resolved_session_id.clone(),
            artifact_type: req.artifact_type.clone(),
            title: req.title.clone(),
        },
    );

    Ok(Json(CreateTeamArtifactResponse { artifact_id }))
}

pub async fn get_team_artifacts(
    State(state): State<HttpServerState>,
    Path(session_id): Path<String>,
) -> Result<Json<GetTeamArtifactsResponse>, (StatusCode, String)> {
    let resolved_session_id =
        validate_team_artifact_session_id(&state, &session_id, "read").await?;

    // Get all artifacts from the team-findings bucket
    let bucket_id = ArtifactBucketId::from_string("team-findings");
    let artifacts = state
        .app_state
        .artifact_repo
        .get_by_bucket(&bucket_id)
        .await
        .map_err(|e| {
            error!("Failed to get team artifacts: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;

    // Filter by session_id in team metadata
    let filtered: Vec<TeamArtifactSummary> = artifacts
        .into_iter()
        .filter(|a| {
            a.metadata
                .team_metadata
                .as_ref()
                .and_then(|tm| tm.session_id.as_deref())
                == Some(resolved_session_id.as_str())
        })
        .map(|a| {
            let content_preview = match &a.content {
                ArtifactContent::Inline { text } => {
                    if text.chars().count() <= 200 {
                        text.clone()
                    } else {
                        let truncated: String = text.chars().take(200).collect();
                        format!("{truncated}...")
                    }
                }
                ArtifactContent::File { path } => format!("[File: {}]", path),
            };
            let author_teammate = a
                .metadata
                .team_metadata
                .as_ref()
                .map(|tm| tm.author_teammate.clone());
            TeamArtifactSummary {
                id: a.id.to_string(),
                name: a.name.clone(),
                artifact_type: format!("{:?}", a.artifact_type),
                version: a.metadata.version,
                content_preview,
                created_at: a.metadata.created_at.to_rfc3339(),
                author_teammate,
            }
        })
        .collect();

    let count = filtered.len();
    Ok(Json(GetTeamArtifactsResponse {
        artifacts: filtered,
        count,
    }))
}
