use super::*;

pub async fn approve_plan_artifact(
    State(state): State<HttpServerState>,
    Json(req): Json<ApprovePlanArtifactRequest>,
) -> Result<Json<ArtifactResponse>, HttpError> {
    approve_plan_artifact_for_state(&state.app_state, req)
        .await
        .map(Json)
        .map_err(|e| {
            error!("approve_plan_artifact failed: {}", e);
            map_app_err(e)
        })
}

pub(crate) async fn approve_plan_artifact_for_state(
    app_state: &crate::application::AppState,
    req: ApprovePlanArtifactRequest,
) -> crate::error::AppResult<ArtifactResponse> {
    let approved = crate::application::plan_artifact_approval::approve_plan_artifact_for_state(
        app_state,
        req.session_id,
        req.artifact_id,
        req.blueprint_artifact_id,
        req.blueprint_artifact_version,
    )
    .await?;

    let blueprint_id = approved
        .blueprint_artifact
        .as_ref()
        .map(|artifact| artifact.id.to_string());
    let blueprint_version = approved
        .blueprint_artifact
        .as_ref()
        .map(|artifact| artifact.metadata.version);
    let mut response = ArtifactResponse::from(approved.artifact);
    if let Some(blueprint) = approved.blueprint_artifact {
        let mut blueprint_response = ArtifactResponse::from(blueprint);
        blueprint_response.artifact_role = Some("blueprint".to_string());
        response.blueprint_artifact = Some(Box::new(blueprint_response));
        response.plan_contract_version = Some(2);
    }
    response.session_id = Some(approved.session_id.as_str().to_string());
    let response_id = response.id.clone();
    let response_version = response.version;
    attach_plan_approval(
        &mut response,
        PlanApprovalView::approved(
            response_id.clone(),
            response_version,
            blueprint_id.clone(),
            blueprint_version,
            approved.approved_at.clone(),
        ),
    );

    Ok(response)
}
