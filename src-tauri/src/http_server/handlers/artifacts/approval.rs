use super::*;

pub async fn approve_plan_artifact(
    State(state): State<HttpServerState>,
    Json(req): Json<ApprovePlanArtifactRequest>,
) -> Result<Json<ArtifactResponse>, HttpError> {
    let session_id_str = req.session_id.clone();
    let requested_artifact_id = req.artifact_id.clone();
    let requested_blueprint_artifact_id = req.blueprint_artifact_id.clone();
    let requested_blueprint_artifact_version = req.blueprint_artifact_version;

    let approved = state
        .app_state
        .db
        .run_transaction(move |conn| {
            crate::application::plan_artifact_approval::approve_current_plan_artifact_for_displayed_bundle_sync(
                conn,
                IdeationSessionId::from_string(session_id_str),
                requested_artifact_id.as_deref(),
                requested_blueprint_artifact_id.as_deref(),
                requested_blueprint_artifact_version,
                crate::domain::repositories::PlanApprovalActor::User,
            )
        })
        .await
        .map_err(|e| {
            error!("approve_plan_artifact transaction failed: {}", e);
            map_app_err(e)
        })?;

    let blueprint_id = approved
        .blueprint_artifact
        .as_ref()
        .map(|artifact| artifact.id.to_string());
    let blueprint_version = approved
        .blueprint_artifact
        .as_ref()
        .map(|artifact| artifact.metadata.version);
    let plan_bundle = PlanArtifactBundle {
        overview_id: approved.artifact.id.clone(),
        blueprint_id: approved
            .blueprint_artifact
            .as_ref()
            .map(|artifact| artifact.id.clone()),
        contract_version: if approved.blueprint_artifact.is_some() {
            2
        } else {
            1
        },
        is_inherited: false,
    };
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

    crate::http_server::emit_http_event(
        &state,
        "plan_artifact:approved",
        serde_json::json!({
            "sessionId": approved.session_id.as_str(),
            "artifactId": response_id.clone(),
            "version": response_version,
            "blueprintArtifactId": blueprint_id,
            "blueprintVersion": blueprint_version,
            "approvedAt": approved.approved_at,
        }),
    );
    let approval_notification_deps = crate::application::plan_approval_notification_service::PlanApprovalNotificationDeps::from_app_state(&state.app_state);
    crate::application::plan_approval_notification_service::resolve_plan_approval_notifications_with_deps(
        &approval_notification_deps,
        &approved.session_id,
        &plan_bundle,
    )
    .await;

    let tasks_enabled = state
        .app_state
        .ideation_settings_repo
        .get_settings()
        .await
        .map(|settings| settings.tasks_enabled)
        .unwrap_or(false);
    if tasks_enabled {
        crate::application::plan_complexity_assessment::spawn_plan_complexity_assessor_after_approval(
            std::sync::Arc::clone(&state.app_state),
            approved.session_id.as_str().to_string(),
            response_id,
            response_version,
        );
    }

    Ok(Json(response))
}
