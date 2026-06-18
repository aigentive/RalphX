use super::*;
use crate::application::harness_runtime_registry::default_verification_auto_verify_enabled;

pub async fn create_plan_artifact(
    State(state): State<HttpServerState>,
    Json(req): Json<CreatePlanArtifactRequest>,
) -> Result<Json<ArtifactResponse>, HttpError> {
    let session_id_str = req.session_id.clone();
    let title = req.title.clone();
    let content = req.content.clone();
    let auto_verify_enabled = default_verification_auto_verify_enabled();

    // Check in-memory auto-accept state BEFORE the transaction (async lock)
    let is_auto_accept = {
        let auto_accept = state.app_state.auto_accept_sessions.lock().await;
        auto_accept.contains(&session_id_str)
    };

    let (
        session_id,
        created,
        auto_verify_generation,
        project_id,
        session_title,
        is_planning_flow,
        should_auto_verify,
        should_offer_verification_confirmation,
    ) =
        state
            .app_state
            .db
            .run_transaction(move |conn| {
                let sid = IdeationSessionId::from_string(session_id_str);

                let session = SessionRepo::get_by_id_sync(conn, sid.as_str())?
                    .ok_or_else(|| AppError::NotFound(format!("Session {} not found", sid)))?;

                let is_external = session.origin == SessionOrigin::External;
                let is_planning_flow = session.session_flow == IdeationSessionFlow::Planning;
                let should_auto_verify =
                    !is_planning_flow && (auto_verify_enabled || is_external || is_auto_accept);
                let should_offer_verification_confirmation =
                    !is_planning_flow && session.origin != SessionOrigin::External;

                crate::http_server::helpers::assert_session_mutable(&session)?;

                let bucket_id = ArtifactBucketId::from_string("prd-library");
                let artifact = Artifact {
                    id: ArtifactId::new(),
                    artifact_type: ArtifactType::Specification,
                    name: title,
                    content: ArtifactContent::inline(&content),
                    metadata: ArtifactMetadata::new("orchestrator").with_version(1),
                    derived_from: vec![],
                    bucket_id: Some(bucket_id),
                    archived_at: None,
                };

                let created = if let Some(existing_plan_id) = &session.plan_artifact_id {
                    let prev_id = existing_plan_id.as_str().to_string();
                    ArtifactRepo::create_with_previous_version_sync(conn, artifact, &prev_id)?
                } else {
                    ArtifactRepo::create_sync(conn, artifact)?
                };

                SessionRepo::update_plan_artifact_id_sync(
                    conn,
                    sid.as_str(),
                    Some(created.id.as_str()),
                )?;
                SessionRepo::update_plan_version_last_read_sync(conn, sid.as_str(), 1)?;

                let auto_verify_generation = if should_auto_verify {
                    let gen = SessionRepo::trigger_auto_verify_sync(conn, sid.as_str())?;
                    if gen.is_some() {
                        conn.execute(
                            "UPDATE ideation_sessions SET verification_confirmation_status = NULL WHERE id = ?1",
                            rusqlite::params![sid.as_str()],
                        )?;
                    }
                    gen
                } else {
                    None
                };

                let session_title = session.title.clone();
                Ok((sid, created, auto_verify_generation, session.project_id.clone(), session_title, is_planning_flow, should_auto_verify, should_offer_verification_confirmation))
            })
            .await
            .map_err(|e| {
                error!("create_plan_artifact transaction failed: {}", e);
                map_app_err(e)
            })?;

    if let Some(app_handle) = &state.app_state.app_handle {
        let content_text = match &created.content {
            ArtifactContent::Inline { text } => text.clone(),
            ArtifactContent::File { path } => format!("[File: {}]", path),
        };
        let _ = app_handle.emit(
            "plan_artifact:created",
            serde_json::json!({
                "sessionId": session_id.as_str(),
                "artifact": {
                    "id": created.id.as_str(),
                    "name": created.name,
                    "content": content_text,
                    "version": created.metadata.version,
                }
            }),
        );
    }

    // Project lookup for webhook enrichment (non-fatal if not found)
    let project_name = state
        .app_state
        .project_repo
        .get_by_id(&project_id)
        .await
        .ok()
        .flatten()
        .map(|p| p.name);

    let presentation_ctx = crate::domain::services::WebhookPresentationContext {
        project_name,
        session_title: session_title.clone(),
        presentation_kind: Some(crate::domain::services::PresentationKind::PlanCreated),
        task_title: None,
    };

    let mut ideation_plan_payload = serde_json::json!({
        "session_id": session_id.as_str(),
        "project_id": project_id.as_str(),
        "artifact_id": created.id.as_str(),
        "plan_title": created.name,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });

    // Tauri frontend-only emit — unenriched (frontend payload unchanged)
    if let Some(app_handle) = &state.app_state.app_handle {
        let _ = app_handle.emit("ideation:plan_created", &ideation_plan_payload);
    }

    // Enrich payload for external channel
    presentation_ctx.inject_into(&mut ideation_plan_payload);

    // External emit via mandatory helper
    if let Some(ref publisher) = state.app_state.webhook_publisher {
        if let Err(msg) = crate::domain::services::emit_external_webhook_event(
            "ideation:plan_created",
            project_id.as_str(),
            ideation_plan_payload,
            &state.app_state.external_events_repo,
            publisher,
        )
        .await
        {
            tracing::warn!(error = %msg, "Failed to emit ideation:plan_created external event (non-fatal)");
        }
    } else if let Err(e) = state
        .app_state
        .external_events_repo
        .insert_event(
            "ideation:plan_created",
            project_id.as_str(),
            &ideation_plan_payload.to_string(),
        )
        .await
    {
        tracing::warn!(error = %e, "Failed to persist IdeationPlanCreated event (non-fatal)");
    }

    if let Some(generation) = auto_verify_generation {
        let spawned = crate::http_server::handlers::verification::spawn_verification_agent(
            &state,
            &session_id,
            generation,
            &[],
        )
        .await;
        if !spawned {
            if let Err(e) = state
                .app_state
                .ideation_session_repo
                .set_verification_confirmation_status(
                    &session_id,
                    Some(crate::domain::entities::VerificationConfirmationStatus::Pending),
                )
                .await
            {
                tracing::warn!(
                    error = %e,
                    "Failed to re-set verification_confirmation_status to pending after spawn failure for session {} (non-fatal)",
                    session_id.as_str()
                );
            }
        }
    } else if should_auto_verify {
        // should_auto_verify=true but trigger returned None: session is already-verifying
        // or has ImportedVerified status. Check which case for observability, then suppress
        // dialog in either case — no pending_confirmation event for duplicate calls or
        // pre-verified sessions.
        match state
            .app_state
            .ideation_session_repo
            .get_verification_status(&session_id)
            .await
        {
            Ok(Some((status, verification_in_progress))) => {
                if !verification_in_progress
                    && !matches!(status, VerificationStatus::ImportedVerified)
                {
                    tracing::warn!(
                        session_id = session_id.as_str(),
                        ?status,
                        "trigger_auto_verify_sync returned None unexpectedly (not in_progress, not ImportedVerified); suppressing dialog"
                    );
                }
            }
            Ok(None) => {
                tracing::warn!(
                    session_id = session_id.as_str(),
                    "Session not found when checking verification status after trigger=None"
                );
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    session_id = session_id.as_str(),
                    "Failed to fetch verification status after trigger=None (non-fatal)"
                );
            }
        }
    } else if should_offer_verification_confirmation {
        // UI session without auto-verify: set DB status to 'pending' (D7: also resets 'rejected')
        // and emit confirmation event so the UI shows the dialog immediately.
        if let Err(e) = state
            .app_state
            .ideation_session_repo
            .set_verification_confirmation_status(
                &session_id,
                Some(crate::domain::entities::VerificationConfirmationStatus::Pending),
            )
            .await
        {
            tracing::warn!(
                error = %e,
                "Failed to set verification_confirmation_status to pending for session {} (non-fatal)",
                session_id.as_str()
            );
        }
        if let Some(app_handle) = &state.app_state.app_handle {
            crate::domain::services::emit_verification_pending_confirmation(
                app_handle,
                session_id.as_str(),
                &session_title.unwrap_or_default(),
                created.id.as_str(),
            );
        }
    }

    let mut response = ArtifactResponse::from(created);
    if is_planning_flow {
        attach_plan_approval(&mut response, PlanApprovalView::draft());
    }
    Ok(Json(response))
}
