use super::*;

pub async fn update_plan_artifact(
    State(state): State<HttpServerState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<UpdatePlanArtifactRequest>,
) -> Result<Json<ArtifactResponse>, HttpError> {
    let input_artifact_id = req.artifact_id.clone();
    let caller_session_id = resolve_caller_session_id(&headers, req.caller_session_id.as_deref());
    let mutation_authority = resolve_artifact_mutation_authority(&headers);
    let content = req.content;

    let id_for_freeze = input_artifact_id.clone();
    let latest_artifact_id = state
        .app_state
        .db
        .run(move |conn| ArtifactRepo::resolve_latest_sync(conn, &id_for_freeze))
        .await
        .map_err(map_app_err)?;

    let lookup_id = latest_artifact_id.clone();
    let owning_sessions = state
        .app_state
        .db
        .run(move |conn| {
            let mut sessions = SessionRepo::get_by_plan_artifact_id_sync(conn, &lookup_id)?;
            sessions.extend(SessionRepo::get_by_plan_blueprint_artifact_id_sync(
                conn, &lookup_id,
            )?);
            Ok(sessions)
        })
        .await
        .map_err(map_app_err)?;

    check_verification_freeze(
        &owning_sessions,
        caller_session_id.as_deref(),
        state.app_state.running_agent_registry.as_ref(),
        state.app_state.ideation_session_repo.as_ref(),
    )
    .await
    .map_err(map_app_err)?;
    let transaction_authority = mutation_authority.clone();

    let (
        created,
        old_artifact_id_str,
        sessions,
        linked_proposal_ids,
        verification_reset,
    ) = state
        .app_state
        .db
        .run_transaction(move |conn| {
            let old_id = ArtifactRepo::resolve_latest_sync(conn, &input_artifact_id)?;
            let old_artifact = ArtifactRepo::get_by_id_sync(conn, &old_id)?
                .ok_or_else(|| AppError::NotFound(format!("Artifact {} not found", old_id)))?;

            let mut owning_sessions = SessionRepo::get_by_plan_artifact_id_sync(conn, &old_id)?;
            owning_sessions.extend(SessionRepo::get_by_plan_blueprint_artifact_id_sync(
                conn,
                &old_id,
            )?);
            if let Some(session) = owning_sessions.first() {
                crate::http_server::helpers::assert_session_mutable(session)?;
            }

            if owning_sessions.is_empty() {
                let inherited =
                    SessionRepo::get_by_inherited_plan_artifact_id_sync(conn, &old_id)?;
                let inherited_blueprints =
                    SessionRepo::get_by_inherited_plan_blueprint_artifact_id_sync(conn, &old_id)?;
                if !inherited.is_empty() || !inherited_blueprints.is_empty() {
                    return Err(AppError::Validation(
                        "Cannot update inherited plan. Use create_plan_artifact to create a session-specific plan.".to_string(),
                    ));
                }
            }

            finalize_plan_update(conn, &old_artifact, content, transaction_authority.as_ref())
        })
        .await
        .map_err(|e| {
            error!("update_plan_artifact transaction failed: {}", e);
            map_app_err(e)
        })?;

    emit_plan_update_events(
        &state,
        &created,
        &old_artifact_id_str,
        &sessions,
        linked_proposal_ids,
        verification_reset,
    );
    reconcile_plan_notifications(&state, &created, &sessions, mutation_authority.as_ref()).await;
    let mut response = ArtifactResponse::from(created);
    response.artifact_role = sessions.first().map(|session| {
        if session
            .plan_blueprint_artifact_id
            .as_ref()
            .map(|id| id.as_str())
            == Some(old_artifact_id_str.as_str())
        {
            "blueprint".to_string()
        } else {
            "overview".to_string()
        }
    });
    response.previous_artifact_id = Some(old_artifact_id_str);
    response.session_id = sessions.first().map(|s| s.id.to_string());
    if sessions
        .first()
        .is_some_and(|session| session.session_flow == IdeationSessionFlow::Planning)
    {
        attach_plan_approval(&mut response, PlanApprovalView::draft());
    }

    Ok(Json(response))
}
