use super::*;
use crate::application::harness_runtime_registry::default_verification_max_rounds;
use crate::domain::entities::{
    build_child_session, matching_blocker_followup_session, ChildSessionDraftInput, TaskId,
};
use crate::http_server::helpers::get_task_context_impl;

async fn initialize_verification_state(
    state: &HttpServerState,
    parent_id: &IdeationSessionId,
    parent: &IdeationSession,
) -> Result<Option<i32>, JsonError> {
    if parent.plan_artifact_id.is_none() {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "Cannot start verification: parent session has no plan artifact",
        ));
    }

    if parent.verification_in_progress {
        crate::http_server::handlers::ideation::repair_blank_orphaned_verification_generation(
            &state.app_state,
            parent,
        )
        .await
        .map_err(|e| {
            error!(
                "Failed to repair stale verification state for {}: {}",
                parent_id.as_str(),
                e
            );
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to repair verification state: {}", e),
            )
        })?;
    }

    let (_, effective_in_progress) = crate::domain::services::load_effective_verification_status(
        state.app_state.ideation_session_repo.as_ref(),
        parent,
    )
    .await
    .map_err(|e| {
        error!(
            "Failed to load effective verification status for {}: {}",
            parent_id.as_str(),
            e
        );
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to check verification state: {}", e),
        )
    })?;
    if effective_in_progress {
        return Err(json_error(
            StatusCode::CONFLICT,
            "Verification already in progress",
        ));
    }

    let verification_max_rounds = default_verification_max_rounds();
    let parent_id_str = parent_id.as_str().to_string();
    let verify_result = state
        .app_state
        .db
        .run(move |conn| SessionRepo::trigger_auto_verify_sync(conn, &parent_id_str))
        .await
        .map_err(|e| {
            error!("Failed to trigger verification sync: {}", e);
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to initialize verification state: {}", e),
            )
        })?;

    match verify_result {
        Some(new_generation) => {
            emit_verification_started(
                state.app_state.events.as_ref(),
                parent_id.as_str(),
                new_generation,
                verification_max_rounds,
            );
            Ok(Some(new_generation))
        }
        None => {
            let parent_id_str = parent_id.as_str().to_string();
            let fresh_parent = state
                .app_state
                .db
                .run(move |conn| SessionRepo::get_by_id_sync(conn, &parent_id_str))
                .await
                .map_err(|e| {
                    error!("Failed to re-query parent session: {}", e);
                    json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to check verification state: {}", e),
                    )
                })?;
            let fresh_parent = fresh_parent
                .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Parent session not found"))?;
            let (_, fresh_in_progress) =
                crate::domain::services::load_effective_verification_status(
                    state.app_state.ideation_session_repo.as_ref(),
                    &fresh_parent,
                )
                .await
                .map_err(|e| {
                    error!("Failed to re-check effective verification status: {}", e);
                    json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to check verification state: {}", e),
                    )
                })?;
            if fresh_in_progress {
                Err(json_error(
                    StatusCode::CONFLICT,
                    "Verification already in progress",
                ))
            } else {
                Err(json_error(
                    StatusCode::BAD_REQUEST,
                    "Cannot verify imported-verified sessions",
                ))
            }
        }
    }
}

fn build_effective_prompts(
    req: &CreateChildSessionRequest,
    verification_generation: Option<i32>,
    max_rounds: u32,
    parent_session_id: &str,
) -> (Option<String>, Option<String>) {
    let effective_initial_prompt = req.initial_prompt.as_ref().map(|prompt| {
        if let Some(generation) = verification_generation {
            format!(
                "{}\n\nparent_session_id: {}, generation: {}, max_rounds: {}",
                prompt, parent_session_id, generation, max_rounds
            )
        } else {
            prompt.clone()
        }
    });
    let effective_description = req.description.as_ref().map(|description| {
        if let Some(generation) = verification_generation {
            format!(
                "{}\n\nparent_session_id: {}, generation: {}, max_rounds: {}",
                description, parent_session_id, generation, max_rounds
            )
        } else {
            description.clone()
        }
    });

    let effective_initial_prompt = effective_initial_prompt.or_else(|| {
        synthesize_verification_prompt(
            &req.purpose,
            verification_generation,
            max_rounds,
            &effective_description,
            parent_session_id,
        )
    });

    (effective_initial_prompt, effective_description)
}

fn build_child_session_response(
    session: &IdeationSession,
    parent_id: &IdeationSessionId,
    req: &CreateChildSessionRequest,
    parent_context: Option<ParentContextResponse>,
    orchestration_triggered: bool,
    generation: Option<i32>,
    pending_initial_prompt: Option<String>,
) -> CreateChildSessionResponse {
    let team_config = session
        .team_config_json
        .as_ref()
        .and_then(|json_str| serde_json::from_str(json_str).ok());

    CreateChildSessionResponse {
        session_id: session.id.as_str().to_string(),
        parent_session_id: parent_id.as_str().to_string(),
        title: session
            .title
            .clone()
            .unwrap_or_else(|| "Child Session".to_string()),
        status: session.status.to_string(),
        created_at: session.created_at.to_rfc3339(),
        inherited_plan_id: session
            .inherited_plan_artifact_id
            .as_ref()
            .map(ToString::to_string),
        initial_prompt: req.initial_prompt.clone(),
        parent_context,
        orchestration_triggered,
        team_mode: session.team_mode.clone(),
        team_config,
        generation,
        pending_initial_prompt,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChildOrchestrationResult {
    Started,
    DeferredCapacity,
    Failed,
}

async fn find_existing_blocker_followup(
    state: &HttpServerState,
    parent_id: &IdeationSessionId,
    source_task_id: &str,
    blocker_fingerprint: &str,
) -> Result<Option<IdeationSession>, JsonError> {
    let children = state
        .app_state
        .ideation_session_repo
        .get_children(parent_id)
        .await
        .map_err(|e| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to inspect existing child sessions: {}", e),
            )
        })?;

    Ok(matching_blocker_followup_session(
        &children,
        source_task_id,
        blocker_fingerprint,
    ))
}

async fn resolve_blocker_fingerprint(
    state: &HttpServerState,
    req: &CreateChildSessionRequest,
) -> Result<Option<String>, JsonError> {
    if let Some(blocker_fingerprint) = &req.blocker_fingerprint {
        return Ok(Some(blocker_fingerprint.clone()));
    }

    if req.spawn_reason.as_deref() != Some("out_of_scope_failure") {
        return Ok(None);
    }

    let Some(source_task_id) = req.source_task_id.as_ref() else {
        return Ok(None);
    };

    let task_id = TaskId::from_string(source_task_id.clone());
    let task_context = get_task_context_impl(&state.app_state, &task_id)
        .await
        .map_err(|e| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!(
                    "Failed to resolve blocker fingerprint from task context: {}",
                    e
                ),
            )
        })?;

    Ok(task_context.out_of_scope_blocker_fingerprint)
}

async fn spawn_child_orchestration(
    state: &HttpServerState,
    created_session: &IdeationSession,
    child_session_str: &str,
    prompt: &str,
    parent_id: &IdeationSessionId,
    verification_generation: &mut Option<i32>,
    error_context: &'static str,
) -> ChildOrchestrationResult {
    let chat_service = build_ideation_chat_service(state, created_session);
    match chat_service
        .send_message(
            ChatContextType::Ideation,
            child_session_str,
            prompt,
            Default::default(),
        )
        .await
    {
        Ok(send_result) => {
            if send_result.queued_as_pending {
                tracing::info!(
                    session_id = child_session_str,
                    "Child session launch deferred because ideation capacity is full"
                );
                if let Some(current_generation) = verification_generation.take() {
                    rollback_verification_state(
                        state,
                        parent_id,
                        current_generation,
                        "capacity-deferred spawn",
                    )
                    .await;
                }
                ChildOrchestrationResult::DeferredCapacity
            } else {
                ChildOrchestrationResult::Started
            }
        }
        Err(e) => {
            error!("{error_context} {}: {}", child_session_str, e);
            if let Some(current_generation) = verification_generation.take() {
                rollback_verification_state(state, parent_id, current_generation, "spawn failure")
                    .await;
            }
            // Only archive verification children on spawn failure — general follow-up
            // children remain Active so users can retry orchestration later.
            if created_session.session_purpose == SessionPurpose::Verification {
                let child_id_obj = IdeationSessionId::from_string(child_session_str.to_string());
                if let Err(archive_err) = state
                    .app_state
                    .ideation_session_repo
                    .update_status(&child_id_obj, IdeationSessionStatus::Archived)
                    .await
                {
                    error!(
                        "Failed to archive verification child session {} after spawn failure: {}",
                        child_session_str, archive_err
                    );
                }
            }
            ChildOrchestrationResult::Failed
        }
    }
}

pub(crate) async fn create_child_session_impl(
    state: &HttpServerState,
    mut req: CreateChildSessionRequest,
) -> Result<CreateChildSessionResponse, JsonError> {
    let parent_id = IdeationSessionId::from_string(req.parent_session_id.clone());
    let verification_max_rounds = default_verification_max_rounds();
    let mut verification_generation = None;
    req.blocker_fingerprint = resolve_blocker_fingerprint(state, &req).await?;

    let parent = state
        .app_state
        .ideation_session_repo
        .get_by_id(&parent_id)
        .await
        .map_err(|e| {
            error!(
                "Failed to fetch parent session {}: {}",
                parent_id.as_str(),
                e
            );
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to fetch parent session: {}", e),
            )
        })?
        .ok_or_else(|| {
            error!("Parent session {} not found", parent_id.as_str());
            json_error(StatusCode::NOT_FOUND, "Parent session not found")
        })?;

    if let (Some(source_task_id), Some(blocker_fingerprint)) = (
        req.source_task_id.as_deref(),
        req.blocker_fingerprint.as_deref(),
    ) {
        if let Some(existing_session) =
            find_existing_blocker_followup(state, &parent_id, source_task_id, blocker_fingerprint)
                .await?
        {
            let parent_context = if req.inherit_context {
                Some(load_parent_context(state, &parent).await)
            } else {
                None
            };
            return Ok(build_child_session_response(
                &existing_session,
                &parent_id,
                &req,
                parent_context,
                false,
                None,
                existing_session.pending_initial_prompt.clone(),
            ));
        }
    }

    let ancestor_chain = state
        .app_state
        .ideation_session_repo
        .get_ancestor_chain(&parent_id)
        .await
        .map_err(|e| {
            error!(
                "Failed to get ancestor chain for {}: {}",
                parent_id.as_str(),
                e
            );
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to check for cycles: {}", e),
            )
        })?;

    if ancestor_chain.iter().any(|session| session.id == parent_id) {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "Circular reference detected: session cannot be its own parent",
        ));
    }

    if req.purpose.as_deref() == Some("verification") {
        verification_generation = initialize_verification_state(state, &parent_id, &parent).await?;
    }

    let (resolved_team_mode, resolved_team_config_json) = if let Some(mode) = &req.team_mode {
        let config_json = req
            .team_config
            .as_ref()
            .and_then(|config| serde_json::to_value(config).ok());
        (
            Some(mode.clone()),
            config_json.map(|value| value.to_string()),
        )
    } else if req.inherit_context {
        (parent.team_mode.clone(), parent.team_config_json.clone())
    } else {
        (None, None)
    };
    let team_mode_requested = resolved_team_mode
        .as_deref()
        .is_some_and(|mode| mode != "solo");
    let team_mode_supported =
        crate::application::ideation_harness_availability::ideation_team_mode_supported_for_project(
            &state.app_state.agent_lane_settings_repo,
            Some(parent.project_id.as_str()),
        )
        .await;
    let (resolved_team_mode, resolved_team_config_json) = if team_mode_requested
        && !team_mode_supported
    {
        tracing::info!(
            parent_session_id = %parent_id.as_str(),
            project_id = %parent.project_id,
            "Downgrading child ideation session team mode to solo because the primary harness does not support team mode"
        );
        (Some("solo".to_string()), None)
    } else {
        (resolved_team_mode, resolved_team_config_json)
    };

    let (team_mode, team_config_json) = validate_resolved_team_config(
        resolved_team_mode.as_ref(),
        resolved_team_config_json.as_ref(),
    );
    let purpose = req
        .purpose
        .as_deref()
        .and_then(|purpose| purpose.parse::<SessionPurpose>().ok())
        .unwrap_or_default();
    let child_session = build_child_session(
        parent_id.clone(),
        &parent,
        ChildSessionDraftInput {
            title: req.title.clone(),
            inherit_context: req.inherit_context,
            team_mode,
            team_config_json,
            source_task_id: req.source_task_id.clone(),
            source_context_type: req.source_context_type.clone(),
            source_context_id: req.source_context_id.clone(),
            spawn_reason: req.spawn_reason.clone(),
            blocker_fingerprint: req.blocker_fingerprint.clone(),
            purpose,
            is_external_trigger: req.is_external_trigger,
        },
    );

    let child_id = child_session.id.clone();
    let child_session_str = child_id.as_str().to_string();
    let parent_session_str = parent_id.as_str().to_string();

    let created_session = match state
        .app_state
        .ideation_session_repo
        .create(child_session)
        .await
    {
        Ok(session) => session,
        Err(e) => {
            error!("Failed to create child session: {}", e);
            if let Some(current_generation) = verification_generation {
                rollback_verification_state(
                    state,
                    &parent_id,
                    current_generation,
                    "child DB insert failure",
                )
                .await;
            }
            return Err(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to create session: {}", e),
            ));
        }
    };

    let link = SessionLink::new(
        parent_id.clone(),
        child_id.clone(),
        SessionRelationship::FollowOn,
    );
    if let Err(e) = state.app_state.session_link_repo.create(link).await {
        error!("Failed to create session link: {}", e);
        if let Some(current_generation) = verification_generation {
            rollback_verification_state(
                state,
                &parent_id,
                current_generation,
                "link creation failure",
            )
            .await;
        }
        return Err(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create session link: {}", e),
        ));
    }

    let parent_context = if req.inherit_context {
        Some(load_parent_context(state, &parent).await)
    } else {
        None
    };

    let title = created_session
        .title
        .clone()
        .unwrap_or_else(|| "Child Session".to_string());
    let (effective_initial_prompt, effective_description) = build_effective_prompts(
        &req,
        verification_generation,
        verification_max_rounds,
        &parent_session_str,
    );

    let orchestration_result = if let Some(ref prompt) = effective_initial_prompt {
        spawn_child_orchestration(
            state,
            &created_session,
            &child_session_str,
            prompt,
            &parent_id,
            &mut verification_generation,
            "Failed to auto-spawn agent on child session",
        )
        .await
    } else if let Some(ref description) = effective_description {
        if description.trim().is_empty() {
            ChildOrchestrationResult::Failed
        } else {
            spawn_child_orchestration(
                state,
                &created_session,
                &child_session_str,
                description,
                &parent_id,
                &mut verification_generation,
                "Failed to auto-spawn agent on child session (from description)",
            )
            .await
        }
    } else {
        ChildOrchestrationResult::Failed
    };
    let orchestration_triggered = orchestration_result == ChildOrchestrationResult::Started;

    // When launch is deferred because capacity is full, persist the effective prompt so the
    // drain service can auto-launch the session once a slot frees up.
    let persisted_pending_prompt =
        if orchestration_result == ChildOrchestrationResult::DeferredCapacity {
            let effective_prompt_for_defer = effective_initial_prompt.clone().or_else(|| {
                effective_description
                    .clone()
                    .filter(|d| !d.trim().is_empty())
            });
            if let Some(ref deferred_prompt) = effective_prompt_for_defer {
                if let Err(e) = state
                    .app_state
                    .ideation_session_repo
                    .set_pending_initial_prompt(&child_session_str, Some(deferred_prompt.clone()))
                    .await
                {
                    error!(
                        "Failed to persist pending_initial_prompt for session {}: {}",
                        child_session_str, e
                    );
                }
            }
            effective_prompt_for_defer
        } else {
            None
        };

    let mut event_payload = serde_json::json!({
        "sessionId": child_session_str,
        "parentSessionId": parent_session_str,
        "title": title,
        "purpose": created_session.session_purpose.to_string(),
        "orchestrationTriggered": orchestration_triggered
    });
    if let Some(ref prompt) = req.initial_prompt {
        event_payload["initialPrompt"] = serde_json::json!(prompt);
    }
    if let Some(ref pending_prompt) = persisted_pending_prompt {
        event_payload["pendingInitialPrompt"] = serde_json::json!(pending_prompt);
    }
    crate::http_server::emit_http_event(state, "ideation:child_session_created", event_payload);

    Ok(build_child_session_response(
        &created_session,
        &parent_id,
        &req,
        parent_context,
        orchestration_triggered,
        verification_generation,
        persisted_pending_prompt,
    ))
}

pub async fn create_child_session(
    State(state): State<HttpServerState>,
    Json(req): Json<CreateChildSessionRequest>,
) -> Result<Json<CreateChildSessionResponse>, JsonError> {
    let response = create_child_session_impl(&state, req).await?;
    Ok(Json(response))
}
