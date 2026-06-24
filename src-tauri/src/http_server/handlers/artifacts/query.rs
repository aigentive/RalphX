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

    let (artifact_id, is_inherited) = if let Some(own_plan_id) = session.plan_artifact_id {
        (own_plan_id, false)
    } else if let Some(inherited_id) = session.inherited_plan_artifact_id {
        (inherited_id, true)
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

    if !is_inherited {
        let session_id_str = session_id.as_str().to_string();
        let version = artifact.metadata.version as i32;
        let _ = state
            .app_state
            .db
            .run(move |conn| {
                SessionRepo::update_plan_version_last_read_sync(conn, &session_id_str, version)
            })
            .await;
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
    response.is_inherited = Some(is_inherited);
    response.project_working_directory = project_working_dir;
    if session.session_flow == IdeationSessionFlow::Planning && !is_inherited {
        let session_id_str = session_id.as_str().to_string();
        let artifact_id_str = response.id.clone();
        let artifact_version = response.version;
        let approval = state
            .app_state
            .db
            .run(move |conn| {
                plan_approval_view_sync(conn, &session_id_str, &artifact_id_str, artifact_version)
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
