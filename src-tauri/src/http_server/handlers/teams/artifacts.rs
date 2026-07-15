use super::*;
use crate::domain::entities::{VerificationFindingGap, VerificationFindingMetadata};

const PLACEHOLDER_SESSION_IDS: &[&str] = &["SESSION_ID", "unknown", "<session_id>"];
const MAX_VERIFICATION_FINDING_GAPS: usize = 250;

fn is_placeholder_session_id(session_id: &str) -> bool {
    let trimmed = session_id.trim();
    trimmed.is_empty()
        || PLACEHOLDER_SESSION_IDS
            .iter()
            .any(|value| trimmed.eq_ignore_ascii_case(value))
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

fn normalize_verification_critic(critic: &str) -> String {
    critic.trim().to_lowercase()
}

fn verification_title_prefix(critic: &str) -> String {
    match normalize_verification_critic(critic).as_str() {
        "completeness" => "Completeness".to_string(),
        "feasibility" => "Feasibility".to_string(),
        "intent" => "IntentAlignment".to_string(),
        "code-quality" => "CodeQuality".to_string(),
        "ux" => "UX".to_string(),
        "prompt-quality" => "PromptQuality".to_string(),
        "pipeline-safety" => "PipelineSafety".to_string(),
        "state-machine" => "StateMachine".to_string(),
        other => {
            let mut chars = other.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                None => "Verification".to_string(),
            }
        }
    }
}

fn build_verification_finding_title(
    critic: &str,
    round: u32,
    title_suffix: Option<&str>,
) -> String {
    let prefix = verification_title_prefix(critic);
    let suffix = title_suffix
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!(" - {value}"))
        .unwrap_or_default();
    format!("{prefix}: Round {round}{suffix}")
}

fn render_verification_finding_content(finding: &VerificationFindingMetadata) -> String {
    let mut lines = vec![
        "## Verification Finding".to_string(),
        format!("- Critic: {}", finding.critic),
        format!("- Round: {}", finding.round),
        format!("- Status: {}", finding.status),
    ];

    if let Some(coverage) = finding.coverage.as_deref() {
        lines.push(format!("- Coverage: {coverage}"));
    }

    lines.push(String::new());
    lines.push("## Summary".to_string());
    lines.push(finding.summary.clone());

    if !finding.gaps.is_empty() {
        lines.push(String::new());
        lines.push("## Gaps".to_string());
        for gap in &finding.gaps {
            let mut line = format!("- [{}] {}: {}", gap.severity, gap.category, gap.description);
            if let Some(lens) = gap.lens.as_deref().filter(|value| !value.is_empty()) {
                line.push_str(&format!(" (lens: {lens})"));
            }
            lines.push(line);
            if let Some(why) = gap
                .why_it_matters
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                lines.push(format!("  Why it matters: {why}"));
            }
        }
    }

    lines.join("\n")
}

fn map_verification_gap_payload(gap: &VerificationFindingGap) -> VerificationFindingGapPayload {
    VerificationFindingGapPayload {
        severity: gap.severity.clone(),
        category: gap.category.clone(),
        description: gap.description.clone(),
        why_it_matters: gap.why_it_matters.clone(),
        source: gap.source.clone(),
        lens: gap.lens.clone(),
    }
}

fn parse_created_after_filter(
    created_after: Option<&str>,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, (StatusCode, String)> {
    let Some(created_after) = created_after
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };

    let parsed = chrono::DateTime::parse_from_rfc3339(created_after).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            format!("Invalid created_after timestamp '{created_after}': {error}"),
        )
    })?;
    Ok(Some(parsed.with_timezone(&chrono::Utc)))
}

pub async fn create_team_artifact(
    State(state): State<HttpServerState>,
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
    let mut artifact = Artifact::new_inline(&req.title, artifact_type, &req.content, "team-lead");

    // Set bucket to team-findings
    artifact.bucket_id = Some(ArtifactBucketId::from_string("team-findings"));

    // Store team metadata with session_id
    artifact.metadata.team_metadata = Some(crate::domain::entities::TeamArtifactMetadata {
        team_name: "team".to_string(),
        author_teammate: "team-lead".to_string(),
        session_id: Some(resolved_session_id.clone()),
        team_phase: None,
        verification_finding: None,
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

pub async fn publish_verification_finding(
    State(state): State<HttpServerState>,
    Json(req): Json<PublishVerificationFindingRequest>,
) -> Result<Json<PublishVerificationFindingResponse>, (StatusCode, String)> {
    let resolved_session_id =
        validate_team_artifact_session_id(&state, &req.session_id, "create").await?;

    let critic = normalize_verification_critic(&req.critic);
    if critic.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "critic is required for publish_verification_finding".to_string(),
        ));
    }

    let status = req.status.trim().to_lowercase();
    if !matches!(status.as_str(), "complete" | "partial" | "error") {
        return Err((
            StatusCode::BAD_REQUEST,
            "status must be one of: complete, partial, error".to_string(),
        ));
    }

    let summary = req.summary.trim().to_string();
    if summary.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "summary is required for publish_verification_finding".to_string(),
        ));
    }

    let gap_count = req.gaps.len();
    if gap_count > MAX_VERIFICATION_FINDING_GAPS {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "verification finding gaps exceed the limit of {MAX_VERIFICATION_FINDING_GAPS}"
            ),
        ));
    }

    let mut gaps = Vec::with_capacity(gap_count);
    for gap in &req.gaps {
        let severity = gap.severity.trim().to_lowercase();
        if !matches!(severity.as_str(), "critical" | "high" | "medium" | "low") {
            return Err((
                StatusCode::BAD_REQUEST,
                format!(
                    "Invalid verification gap severity '{}'. Valid severities: critical, high, medium, low",
                    gap.severity
                ),
            ));
        }
        let category = gap.category.trim().to_string();
        let description = gap.description.trim().to_string();
        if category.is_empty() || description.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                "Each verification gap requires non-empty category and description".to_string(),
            ));
        }
        gaps.push(VerificationFindingGap {
            severity,
            category,
            description,
            why_it_matters: gap
                .why_it_matters
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
            source: gap
                .source
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
            lens: gap
                .lens
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
        });
    }

    let finding = VerificationFindingMetadata {
        critic: critic.clone(),
        round: req.round,
        status,
        coverage: req
            .coverage
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        summary,
        gaps,
    };

    let title = build_verification_finding_title(&critic, req.round, req.title_suffix.as_deref());
    let content = render_verification_finding_content(&finding);
    let mut artifact = Artifact::new_inline(
        &title,
        ArtifactType::VerificationFinding,
        content,
        "team-lead",
    );
    artifact.bucket_id = Some(ArtifactBucketId::from_string("team-findings"));
    artifact.metadata.team_metadata = Some(crate::domain::entities::TeamArtifactMetadata {
        team_name: "verification".to_string(),
        author_teammate: critic.clone(),
        session_id: Some(resolved_session_id.clone()),
        team_phase: Some(format!("round-{}", req.round)),
        verification_finding: Some(finding.clone()),
    });

    let artifact_id = artifact.id.to_string();
    state
        .app_state
        .artifact_repo
        .create(artifact)
        .await
        .map_err(|e| {
            error!("Failed to create verification finding artifact: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;

    info!(
        artifact_id = %artifact_id,
        session_id = %resolved_session_id,
        requested_session_id = %req.session_id,
        critic = %critic,
        round = req.round,
        "Verification finding artifact created"
    );

    use crate::application::chat_service::{events, TeamArtifactCreatedPayload};
    crate::http_server::emit_serialized_http_event(
        &state,
        events::TEAM_ARTIFACT_CREATED,
        &TeamArtifactCreatedPayload {
            artifact_id: artifact_id.clone(),
            session_id: resolved_session_id.clone(),
            artifact_type: "VerificationFinding".to_string(),
            title,
        },
    );

    Ok(Json(PublishVerificationFindingResponse { artifact_id }))
}

// ============================================================================
// GET /api/team/artifacts/:session_id — Get team artifacts for a session
// ============================================================================

/// Retrieve all team artifacts for a given session.
///
/// Filters artifacts in the 'team-findings' bucket by session_id in custom metadata.
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

pub async fn get_verification_findings(
    State(state): State<HttpServerState>,
    Path(session_id): Path<String>,
    Query(query): Query<VerificationFindingQuery>,
) -> Result<Json<GetVerificationFindingsResponse>, (StatusCode, String)> {
    let resolved_session_id =
        validate_team_artifact_session_id(&state, &session_id, "read").await?;
    let created_after = parse_created_after_filter(query.created_after.as_deref())?;
    let critic_filter = query
        .critic
        .as_deref()
        .map(normalize_verification_critic)
        .filter(|value| !value.is_empty());

    let bucket_id = ArtifactBucketId::from_string("team-findings");
    let artifacts = state
        .app_state
        .artifact_repo
        .get_by_bucket(&bucket_id)
        .await
        .map_err(|e| {
            error!("Failed to get verification finding artifacts: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;

    let mut findings: Vec<VerificationFindingSummary> = artifacts
        .into_iter()
        .filter(|artifact| artifact.artifact_type == ArtifactType::VerificationFinding)
        .filter_map(|artifact| {
            let team_metadata = artifact.metadata.team_metadata.as_ref()?;
            if team_metadata.session_id.as_deref() != Some(resolved_session_id.as_str()) {
                return None;
            }
            let verification = team_metadata.verification_finding.as_ref()?;
            if let Some(round) = query.round {
                if verification.round != round {
                    return None;
                }
            }
            if let Some(ref critic) = critic_filter {
                if normalize_verification_critic(&verification.critic) != *critic {
                    return None;
                }
            }
            if let Some(created_after) = created_after {
                if artifact.metadata.created_at < created_after {
                    return None;
                }
            }
            Some(VerificationFindingSummary {
                artifact_id: artifact.id.to_string(),
                title: artifact.name.clone(),
                created_at: artifact.metadata.created_at.to_rfc3339(),
                author_teammate: Some(team_metadata.author_teammate.clone()),
                critic: verification.critic.clone(),
                round: verification.round,
                status: verification.status.clone(),
                coverage: verification.coverage.clone(),
                summary: verification.summary.clone(),
                gaps: verification
                    .gaps
                    .iter()
                    .map(map_verification_gap_payload)
                    .collect(),
            })
        })
        .collect();

    findings.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    let count = findings.len();

    Ok(Json(GetVerificationFindingsResponse { findings, count }))
}
