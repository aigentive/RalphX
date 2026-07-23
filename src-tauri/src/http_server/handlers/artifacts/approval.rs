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

    if let Ok(Some(session)) = state
        .app_state
        .ideation_session_repo
        .get_by_id(&approved.session_id)
        .await
    {
        let conversation_id = state
            .app_state
            .agent_conversation_workspace_repo
            .get_by_linked_ideation_session_id(&approved.session_id)
            .await
            .ok()
            .flatten()
            .map(|workspace| workspace.conversation_id.as_str());
        if let Err(error) = crate::application::plan_verdict_history::record_plan_verdict(
            std::sync::Arc::clone(&state.app_state.task_outcome_repo),
            crate::application::plan_verdict_history::PlanVerdictCapture {
                project_id: session.project_id,
                conversation_id,
                session_id: approved.session_id.as_str().to_string(),
                artifact_id: response_id.clone(),
                artifact_version: response_version,
                actor: crate::domain::repositories::PlanApprovalActor::User,
                verdict: crate::application::plan_verdict_history::PlanVerdict::Accepted,
                origin: "approve_plan_artifact",
                summary: None,
            },
        )
        .await
        {
            tracing::warn!(
                session_id = approved.session_id.as_str(),
                artifact_id = response_id,
                artifact_version = response_version,
                error = %error,
                "Plan approval committed but verdict history capture failed"
            );
        }
    }

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
    state
        .app_state
        .notification_service()
        .resolve_workflow_notification(
            &crate::application::interactive_notification_producer::plan_notification_key(
                approved.session_id.as_str(),
                &response_id,
            ),
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
