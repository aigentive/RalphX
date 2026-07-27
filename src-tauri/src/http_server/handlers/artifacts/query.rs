use super::*;

pub async fn get_session_plan(
    State(state): State<HttpServerState>,
    Path(session_id): Path<String>,
) -> Result<Json<Option<ArtifactResponse>>, StatusCode> {
    let session_id = IdeationSessionId::from_string(session_id);

    let session = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .map_err(|e| {
            error!(
                "Failed to get session {} for plan retrieval: {}",
                session_id.as_str(),
                e
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    let (artifact_id, blueprint_id, is_inherited) =
        if let Some(own_plan_id) = session.plan_artifact_id.clone() {
            (
                own_plan_id,
                session.plan_blueprint_artifact_id.clone(),
                false,
            )
        } else if let Some(inherited_id) = session.inherited_plan_artifact_id.clone() {
            (
                inherited_id,
                session.inherited_plan_blueprint_artifact_id.clone(),
                true,
            )
        } else {
            return Ok(Json(None));
        };

    let artifact = state
        .app_state
        .artifact_repo
        .get_by_id(&artifact_id)
        .await
        .map_err(|e| {
            error!(
                "Failed to get plan artifact {} for session {}: {}",
                artifact_id.as_str(),
                session_id.as_str(),
                e
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    let blueprint = if let Some(blueprint_id) = blueprint_id {
        Some(
            state
                .app_state
                .artifact_repo
                .get_by_id(&blueprint_id)
                .await
                .map_err(|e| {
                    error!(
                        "Failed to get blueprint artifact {} for session {}: {}",
                        blueprint_id.as_str(),
                        session_id.as_str(),
                        e
                    );
                    StatusCode::INTERNAL_SERVER_ERROR
                })?
                .ok_or(StatusCode::NOT_FOUND)?,
        )
    } else {
        None
    };

    if !is_inherited {
        let session_id_str = session_id.as_str().to_string();
        let overview_version = artifact.metadata.version as i32;
        let blueprint_version = blueprint
            .as_ref()
            .map(|artifact| artifact.metadata.version as i32);
        state
            .app_state
            .db
            .run(move |conn| {
                SessionRepo::acknowledge_plan_bundle_read_sync(
                    conn,
                    &session_id_str,
                    overview_version,
                    blueprint_version,
                )
            })
            .await
            .map_err(|e| {
                error!(
                    "Failed to acknowledge plan bundle read for session {}: {}",
                    session_id.as_str(),
                    e
                );
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
    }

    let project_working_dir = state
        .app_state
        .project_repo
        .get_by_id(&session.project_id)
        .await
        .ok()
        .flatten()
        .map(|p| p.working_directory.clone());

    let mut response = ArtifactResponse::from(artifact);
    response.artifact_role = Some("overview".to_string());
    if let Some(blueprint) = blueprint {
        let mut blueprint_response = ArtifactResponse::from(blueprint);
        blueprint_response.artifact_role = Some("blueprint".to_string());
        response.blueprint_artifact = Some(Box::new(blueprint_response));
    }
    response.plan_contract_version = Some(session.plan_contract_version);
    response.plan_target_id = session
        .plan_artifact_bundle()
        .map(|bundle| bundle.action_target_id());
    response.is_inherited = Some(is_inherited);
    response.project_working_directory = project_working_dir;
    if session.session_flow == IdeationSessionFlow::Planning && !is_inherited {
        let session_id_str = session_id.as_str().to_string();
        let artifact_id_str = response.id.clone();
        let artifact_version = response.version;
        let blueprint_artifact_id = response
            .blueprint_artifact
            .as_ref()
            .map(|artifact| artifact.id.clone());
        let blueprint_artifact_version = response
            .blueprint_artifact
            .as_ref()
            .map(|artifact| artifact.version);
        let approval = state
            .app_state
            .db
            .run(move |conn| {
                plan_approval_view_sync(
                    conn,
                    &session_id_str,
                    &artifact_id_str,
                    artifact_version,
                    blueprint_artifact_id.as_deref(),
                    blueprint_artifact_version,
                )
            })
            .await
            .map_err(|e| {
                error!(
                    "Failed to get plan approval state for session {}: {}",
                    session_id.as_str(),
                    e
                );
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        attach_plan_approval(&mut response, approval);
    }
    Ok(Json(Some(response)))
}

/// Get version history for a plan artifact
/// Returns list of version summaries from newest to oldest
pub async fn get_artifact_history(
    State(state): State<HttpServerState>,
    Path(artifact_id): Path<String>,
) -> Result<Json<Vec<ArtifactVersionSummaryResponse>>, StatusCode> {
    let artifact_id = ArtifactId::from_string(artifact_id);

    state
        .app_state
        .artifact_repo
        .get_by_id(&artifact_id)
        .await
        .map_err(|e| {
            error!(
                "Failed to get artifact {} for history: {}",
                artifact_id.as_str(),
                e
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    let history = state
        .app_state
        .artifact_repo
        .get_version_history(&artifact_id)
        .await
        .map_err(|e| {
            error!(
                "Failed to get history for artifact {}: {}",
                artifact_id.as_str(),
                e
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(
        history
            .into_iter()
            .map(ArtifactVersionSummaryResponse::from)
            .collect(),
    ))
}
