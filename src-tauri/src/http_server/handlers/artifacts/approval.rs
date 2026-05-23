use super::*;

pub async fn approve_plan_artifact(
    State(state): State<HttpServerState>,
    Json(req): Json<ApprovePlanArtifactRequest>,
) -> Result<Json<ArtifactResponse>, HttpError> {
    let session_id_str = req.session_id.clone();
    let requested_artifact_id = req.artifact_id.clone();

    let (session_id, artifact, approved_at) = state
        .app_state
        .db
        .run_transaction(move |conn| {
            let sid = IdeationSessionId::from_string(session_id_str);
            let session = SessionRepo::get_by_id_sync(conn, sid.as_str())?
                .ok_or_else(|| AppError::NotFound(format!("Session {} not found", sid)))?;

            crate::http_server::helpers::assert_session_mutable(&session)?;

            if session.session_flow != IdeationSessionFlow::Planning {
                return Err(AppError::Validation(
                    "Plan approval is only available for planning sessions".to_string(),
                ));
            }

            let plan_artifact_id = session.plan_artifact_id.as_ref().ok_or_else(|| {
                AppError::Validation(
                    "Cannot approve plan: planning session does not have an owned plan artifact"
                        .to_string(),
                )
            })?;

            if let Some(requested) = requested_artifact_id.as_deref() {
                if requested != plan_artifact_id.as_str() {
                    return Err(AppError::Conflict(
                        "Plan changed before approval. Refresh the current plan and approve again."
                            .to_string(),
                    ));
                }
            }

            let artifact = ArtifactRepo::get_by_id_sync(conn, plan_artifact_id.as_str())?
                .ok_or_else(|| {
                    AppError::NotFound(format!(
                        "Plan artifact {} not found",
                        plan_artifact_id.as_str()
                    ))
                })?;
            let approved_at = chrono::Utc::now().to_rfc3339();
            upsert_plan_approval_sync(conn, sid.as_str(), &artifact, &approved_at)?;

            Ok((sid, artifact, approved_at))
        })
        .await
        .map_err(|e| {
            error!("approve_plan_artifact transaction failed: {}", e);
            map_app_err(e)
        })?;

    let mut response = ArtifactResponse::from(artifact);
    response.session_id = Some(session_id.as_str().to_string());
    let response_id = response.id.clone();
    let response_version = response.version;
    attach_plan_approval(
        &mut response,
        PlanApprovalView::approved(response_id.clone(), response_version, approved_at.clone()),
    );

    if let Some(app_handle) = &state.app_state.app_handle {
        let _ = app_handle.emit(
            "plan_artifact:approved",
            serde_json::json!({
                "sessionId": session_id.as_str(),
                "artifactId": response_id,
                "version": response_version,
                "approvedAt": approved_at,
            }),
        );
    }

    Ok(Json(response))
}
