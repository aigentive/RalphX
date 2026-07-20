use super::*;
use crate::domain::entities::{ArtifactRelation, ArtifactRelationId, ArtifactRelationType};
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
    let Some(config) = crate::infrastructure::agents::claude::get_agent_config(agent_type) else {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Unknown canonical caller agent '{agent_type}'"),
        ));
    };
    Ok(config.name.clone())
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
    let author = artifact_author(&headers)?;
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

    let artifact_id = artifact.id.to_string();

    state
        .app_state
        .artifact_repo
        .create(artifact)
        .await
        .map_err(|e| {
            error!("Failed to create team artifact: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;

    // Link to related artifact if provided
    if let Some(related_id) = &req.related_artifact_id {
        let relation = ArtifactRelation {
            id: ArtifactRelationId::new(),
            from_artifact_id: ArtifactId::from_string(artifact_id.clone()),
            to_artifact_id: ArtifactId::from_string(related_id.clone()),
            relation_type: ArtifactRelationType::RelatedTo,
        };
        let _ = state.app_state.artifact_repo.add_relation(relation).await;
    }

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
