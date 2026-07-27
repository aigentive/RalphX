use super::*;

pub async fn create_plan_artifact(
    State(state): State<HttpServerState>,
    Json(req): Json<CreatePlanArtifactRequest>,
) -> Result<Json<ArtifactResponse>, HttpError> {
    create_plan_artifact_with_headers(State(state), axum::http::HeaderMap::new(), Json(req)).await
}

pub async fn create_plan_artifact_with_headers(
    State(state): State<HttpServerState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<CreatePlanArtifactRequest>,
) -> Result<Json<ArtifactResponse>, HttpError> {
    let mutation_authority = resolve_artifact_mutation_authority(&headers);
    let transaction_authority = mutation_authority.clone();
    let session_id_str = req.session_id.clone();
    let title = req.title.clone();
    let content = req.content.clone();
    let blueprint_content = req.blueprint_content.ok_or_else(|| {
        HttpError::validation(
            "blueprint_content is required for the plan bundle; generate the implementation blueprint in the planning conversation".to_string(),
        )
    })?;
    if blueprint_content.trim().is_empty() {
        return Err(HttpError::validation(
            "blueprint_content must not be empty".to_string(),
        ));
    }
    let blueprint_title = req
        .blueprint_title
        .unwrap_or_else(|| format!("{title} — Implementation Blueprint"));
    let (
        session_id,
        created,
        blueprint_created,
        project_id,
        session_title,
        notification_session,
        plan_target_id,
    ) = state
        .app_state
        .db
        .run_transaction(move |conn| {
            let sid = IdeationSessionId::from_string(session_id_str);

            let session = SessionRepo::get_by_id_sync(conn, sid.as_str())?
                .ok_or_else(|| AppError::NotFound(format!("Session {} not found", sid)))?;

            crate::http_server::helpers::assert_session_mutable(&session)?;
            let prior_blueprint_id = session
                .plan_blueprint_artifact_id
                .as_ref()
                .map(|artifact_id| artifact_id.to_string());
            let prior_target_id = session
                .plan_artifact_bundle()
                .map(|bundle| bundle.action_target_id());
            let overview_version =
                next_artifact_version_sync(conn, session.plan_artifact_id.as_ref())?;
            let blueprint_version =
                next_artifact_version_sync(conn, session.plan_blueprint_artifact_id.as_ref())?;

            let bucket_id = ArtifactBucketId::from_string("prd-library");
            let artifact = Artifact {
                id: ArtifactId::new(),
                artifact_type: ArtifactType::Specification,
                name: title,
                content: ArtifactContent::inline(&content),
                metadata: ArtifactMetadata::new("orchestrator").with_version(overview_version),
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

            let blueprint = Artifact {
                id: ArtifactId::new(),
                artifact_type: ArtifactType::Specification,
                name: blueprint_title,
                content: ArtifactContent::inline(&blueprint_content),
                metadata: ArtifactMetadata::new("orchestrator").with_version(blueprint_version),
                derived_from: vec![],
                bucket_id: Some(ArtifactBucketId::from_string("prd-library")),
                archived_at: None,
            };
            let blueprint_created = if let Some(existing_blueprint_id) = prior_blueprint_id {
                ArtifactRepo::create_with_previous_version_sync(
                    conn,
                    blueprint,
                    &existing_blueprint_id,
                )?
            } else {
                ArtifactRepo::create_sync(conn, blueprint)?
            };

            delete_current_bundle_relation_sync(conn, &session)?;
            ArtifactRepo::add_relation_sync(
                conn,
                ArtifactRelation {
                    id: ArtifactRelationId::new(),
                    from_artifact_id: created.id.clone(),
                    to_artifact_id: blueprint_created.id.clone(),
                    relation_type: ArtifactRelationType::RelatedTo,
                },
            )?;
            SessionRepo::update_plan_bundle_sync(
                conn,
                sid.as_str(),
                created.id.as_str(),
                blueprint_created.id.as_str(),
                created.metadata.version as i32,
                blueprint_created.metadata.version as i32,
            )?;
            let updated_session = SessionRepo::get_by_id_sync(conn, sid.as_str())?
                .ok_or_else(|| AppError::NotFound(format!("Session {} not found", sid)))?;
            retarget_verification_authority_sync(
                conn,
                transaction_authority.as_ref(),
                sid.as_str(),
                prior_target_id.as_deref(),
                &updated_session,
            )?;
            let plan_target_id = updated_session
                .plan_artifact_bundle()
                .ok_or_else(|| AppError::Validation("Plan bundle became incomplete".to_string()))?
                .action_target_id();

            let session_title = session.title.clone();
            Ok((
                sid,
                created,
                blueprint_created,
                session.project_id.clone(),
                session_title,
                session,
                plan_target_id,
            ))
        })
        .await
        .map_err(|e| {
            error!("create_plan_artifact transaction failed: {}", e);
            map_app_err(e)
        })?;
    let is_planning_flow = notification_session.session_flow == IdeationSessionFlow::Planning;

    reconcile_plan_notifications(
        &state,
        &created,
        std::slice::from_ref(&notification_session),
        mutation_authority.as_ref(),
    )
    .await;

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
            },
            "blueprintArtifact": {
                "id": blueprint_created.id.as_str(),
                "name": blueprint_created.name,
                "version": blueprint_created.metadata.version,
            },
            "planTargetId": plan_target_id,
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
        "blueprint_artifact_id": blueprint_created.id.as_str(),
        "plan_target_id": plan_target_id,
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
    let mut blueprint_response = ArtifactResponse::from(blueprint_created);
    blueprint_response.artifact_role = Some("blueprint".to_string());
    response.artifact_role = Some("overview".to_string());
    response.blueprint_artifact = Some(Box::new(blueprint_response));
    response.plan_contract_version = Some(2);
    response.plan_target_id = Some(plan_target_id);
    if is_planning_flow {
        attach_plan_approval(&mut response, PlanApprovalView::draft());
    }
    Ok(Json(response))
}
