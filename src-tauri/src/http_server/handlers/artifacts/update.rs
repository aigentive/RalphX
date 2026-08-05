use super::*;

pub async fn update_plan_artifact(
    State(state): State<HttpServerState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<UpdatePlanArtifactRequest>,
) -> Result<Json<ArtifactResponse>, HttpError> {
    let caller_session_id = resolve_caller_session_id(&headers, req.caller_session_id.as_deref());
    let authority = resolve_artifact_mutation_authority(&headers);
    let result = crate::application::plan_artifact_edit::update_plan_artifact_for_state(
        &state.app_state,
        req.artifact_id,
        req.content,
        caller_session_id.as_deref(),
        authority.as_ref(),
    )
    .await
    .map_err(|error| {
        tracing::error!(%error, "update_plan_artifact failed");
        map_app_err(error)
    })?;
    let mut response = ArtifactResponse::from(result.artifact);
    response.artifact_role = result.sessions.first().map(|session| {
        if session
            .plan_blueprint_artifact_id
            .as_ref()
            .map(|id| id.as_str())
            == Some(result.previous_artifact_id.as_str())
        {
            "blueprint".to_string()
        } else {
            "overview".to_string()
        }
    });
    response.previous_artifact_id = Some(result.previous_artifact_id);
    response.session_id = result.sessions.first().map(|s| s.id.to_string());
    if result
        .sessions
        .first()
        .is_some_and(|session| session.session_flow == IdeationSessionFlow::Planning)
    {
        attach_plan_approval(&mut response, PlanApprovalView::draft());
    }
    Ok(Json(response))
}
