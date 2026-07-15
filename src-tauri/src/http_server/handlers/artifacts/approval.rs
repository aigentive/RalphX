use super::*;

pub async fn approve_plan_artifact(
    State(state): State<HttpServerState>,
    Json(req): Json<ApprovePlanArtifactRequest>,
) -> Result<Json<ArtifactResponse>, HttpError> {
    let session_id_str = req.session_id.clone();
    let requested_artifact_id = req.artifact_id.clone();

    let approved = state
        .app_state
        .db
        .run_transaction(move |conn| {
            crate::application::plan_artifact_approval::approve_current_plan_artifact_sync(
                conn,
                IdeationSessionId::from_string(session_id_str),
                requested_artifact_id.as_deref(),
                crate::domain::repositories::PlanApprovalActor::User,
            )
        })
        .await
        .map_err(|e| {
            error!("approve_plan_artifact transaction failed: {}", e);
            map_app_err(e)
        })?;

    let mut response = ArtifactResponse::from(approved.artifact);
    response.session_id = Some(approved.session_id.as_str().to_string());
    let response_id = response.id.clone();
    let response_version = response.version;
    attach_plan_approval(
        &mut response,
        PlanApprovalView::approved(
            response_id.clone(),
            response_version,
            approved.approved_at.clone(),
        ),
    );

    crate::http_server::emit_http_event(
        &state,
        "plan_artifact:approved",
        serde_json::json!({
            "sessionId": approved.session_id.as_str(),
            "artifactId": response_id.clone(),
            "version": response_version,
            "approvedAt": approved.approved_at,
        }),
    );

    crate::application::plan_complexity_assessment::spawn_plan_complexity_assessor_after_approval(
        std::sync::Arc::clone(&state.app_state),
        approved.session_id.as_str().to_string(),
        response_id,
        response_version,
    );

    Ok(Json(response))
}
