use super::*;
use crate::application::interactive_notification_producer::InteractiveNotificationProducer;
use crate::application::NotificationContextResolver;

pub async fn create_plan_artifact(
    State(state): State<HttpServerState>,
    Json(req): Json<CreatePlanArtifactRequest>,
) -> Result<Json<ArtifactResponse>, HttpError> {
    let session_id_str = req.session_id.clone();
    let title = req.title.clone();
    let content = req.content.clone();
    let (session_id, created, project_id, session_title, is_planning_flow, notification_session) =
        state
            .app_state
            .db
            .run_transaction(move |conn| {
                let sid = IdeationSessionId::from_string(session_id_str);

                let session = SessionRepo::get_by_id_sync(conn, sid.as_str())?
                    .ok_or_else(|| AppError::NotFound(format!("Session {} not found", sid)))?;

                let is_planning_flow = session.session_flow == IdeationSessionFlow::Planning;
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

                let session_title = session.title.clone();
                Ok((
                    sid,
                    created,
                    session.project_id.clone(),
                    session_title,
                    is_planning_flow,
                    session,
                ))
            })
            .await
            .map_err(|e| {
                error!("create_plan_artifact transaction failed: {}", e);
                map_app_err(e)
            })?;

    if is_planning_flow {
        let notification_context = NotificationContextResolver::from_app_state(&state.app_state);
        let excluded = match notification_context
            .session_is_automation_owned(&notification_session)
            .await
        {
            Ok(true) => true,
            Ok(false) => match notification_context
                .session_has_implementation_task(&notification_session)
                .await
            {
                Ok(has_task) => has_task,
                Err(error) => {
                    tracing::warn!(error = %error, session_id = %session_id, "Failed to check implementation ownership for plan notification");
                    true
                }
            },
            Err(error) => {
                tracing::warn!(error = %error, session_id = %session_id, "Failed to check automation ownership for plan notification");
                true
            }
        };
        if !excluded {
            match notification_context
                .resolve_ideation_session_target(&notification_session)
                .await
            {
                Ok(resolved) => {
                    state
                        .app_state
                        .notification_service()
                        .record(InteractiveNotificationProducer::plan_approval(
                            project_id.to_string(),
                            session_id.as_str(),
                            created.id.as_str(),
                            session_title.as_deref(),
                            resolved.target,
                        ))
                        .await;
                }
                Err(error) => {
                    tracing::warn!(error = %error, session_id = %session_id, "Failed to resolve plan notification target");
                }
            }
        }
    }

    let content_text = match &created.content {
        ArtifactContent::Inline { text } => text.clone(),
        ArtifactContent::File { path } => format!("[File: {}]", path),
    };
    crate::http_server::emit_http_event(
        &state,
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

    // Tauri/frontend payload is intentionally emitted before external enrichment.
    crate::http_server::emit_http_event(
        &state,
        "ideation:plan_created",
        ideation_plan_payload.clone(),
    );

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

    let mut response = ArtifactResponse::from(created);
    if is_planning_flow {
        attach_plan_approval(&mut response, PlanApprovalView::draft());
    }
    Ok(Json(response))
}
