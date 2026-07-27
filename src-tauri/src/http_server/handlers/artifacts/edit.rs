use super::*;

pub async fn edit_plan_artifact(
    State(state): State<HttpServerState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<EditPlanArtifactRequest>,
) -> Result<Json<ArtifactResponse>, HttpError> {
    let input_artifact_id = req.artifact_id.clone();
    let caller_session_id = resolve_caller_session_id(&headers, req.caller_session_id.as_deref());
    let mutation_authority = resolve_artifact_mutation_authority(&headers);
    let edits = req.edits;

    if edits.is_empty() {
        return Err(HttpError::validation(
            "edits array must not be empty".to_string(),
        ));
    }
    for (i, edit) in edits.iter().enumerate() {
        if edit.old_text.is_empty() {
            return Err(HttpError::validation(format!(
                "Edit #{i}: old_text must not be empty"
            )));
        }
        if edit.old_text.len() > 100_000 || edit.new_text.len() > 100_000 {
            return Err(HttpError::validation(format!(
                "Edit #{i}: old_text/new_text exceeds 100KB limit"
            )));
        }
    }

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
                .ok_or_else(|| AppError::NotFound(format!("Artifact {old_id} not found")))?;

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
                        "Cannot edit inherited plan. Use create_plan_artifact to create a session-specific plan.".to_string(),
                    ));
                }
            }

            let current_content = match &old_artifact.content {
                ArtifactContent::Inline { text } => text.clone(),
                ArtifactContent::File { .. } => {
                    return Err(AppError::Validation(
                        "Cannot edit file-backed artifacts. Use update_plan_artifact with full content.".to_string(),
                    ));
                }
            };

            let new_content = apply_edits(&current_content, &edits).map_err(|e| {
                let http_err: HttpError = e.into();
                AppError::Validation(
                    http_err
                        .message
                        .unwrap_or_else(|| "Edit failed".to_string()),
                )
            })?;

            if new_content.len() > 500_000 {
                return Err(AppError::Validation(format!(
                    "Resulting plan content exceeds 500KB limit ({} bytes). Use fewer/smaller edits.",
                    new_content.len()
                )));
            }

            finalize_plan_update(
                conn,
                &old_artifact,
                new_content,
                transaction_authority.as_ref(),
            )
        })
        .await
        .map_err(|e| {
            error!("edit_plan_artifact transaction failed: {}", e);
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
