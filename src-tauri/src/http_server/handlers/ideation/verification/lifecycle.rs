use super::*;

async fn resolve_verification_parent_session(
    state: &HttpServerState,
    requested_session_id: String,
) -> Result<
    (
        String,
        crate::domain::entities::IdeationSessionId,
        crate::domain::entities::IdeationSession,
    ),
    JsonError,
> {
    let requested_session_id_obj =
        crate::domain::entities::IdeationSessionId::from_string(requested_session_id.clone());

    let requested_session = state
        .app_state
        .ideation_session_repo
        .get_by_id(&requested_session_id_obj)
        .await
        .map_err(|e| {
            error!("Failed to get session {}: {}", requested_session_id, e);
            json_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to get session")
        })?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Session not found"))?;

    if requested_session.session_purpose == crate::domain::entities::SessionPurpose::Verification {
        let parent_id = requested_session.parent_session_id.clone().ok_or_else(|| {
            json_error(
                StatusCode::BAD_REQUEST,
                "Cannot resolve verification parent for a verification child session without a parent session.",
            )
        })?;
        let parent_session = state
            .app_state
            .ideation_session_repo
            .get_by_id(&parent_id)
            .await
            .map_err(|e| {
                error!(
                    "Failed to load parent session {} for verification child {}: {}",
                    parent_id.as_str(),
                    requested_session_id,
                    e
                );
                json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to get parent session",
                )
            })?
            .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Parent session not found"))?;
        tracing::info!(
            requested_session_id = %requested_session_id,
            parent_session_id = %parent_id.as_str(),
            "Auto-remapping verification lifecycle operation from child session to parent session"
        );
        Ok((parent_id.as_str().to_string(), parent_id, parent_session))
    } else {
        Ok((
            requested_session_id,
            requested_session_id_obj,
            requested_session,
        ))
    }
}

/// Validate that a session is eligible for verification operations (stop, revert-and-skip).
///
/// Fetches the session by ID and enforces:
/// 1. Session exists (404 if not found)
/// 2. Session is not from an external origin (403 for external sessions)
/// 3. Session is active (422 if not active)
///
/// Returns the fetched session on success so callers avoid a second DB read.
pub(crate) async fn validate_verification_session(
    session_id: &str,
    session_id_obj: &crate::domain::entities::IdeationSessionId,
    app_state: &AppState,
) -> Result<crate::domain::entities::IdeationSession, JsonError> {
    let session = app_state
        .ideation_session_repo
        .get_by_id(session_id_obj)
        .await
        .map_err(|e| {
            error!("Failed to get session {}: {}", session_id, e);
            json_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to get session")
        })?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Session not found"))?;

    if session.origin == crate::domain::entities::ideation::SessionOrigin::External {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            "External sessions cannot perform this verification operation.",
        ));
    }

    if !session.is_active() {
        return Err(json_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Session is not active",
        ));
    }

    Ok(session)
}

/// Selects which child sessions to act on.
pub(crate) enum ChildFilter {
    /// All child sessions regardless of purpose.
    AllChildren,
    /// Only verification-purpose child sessions.
    VerificationOnly,
}

/// Stop running child agents and optionally archive the child sessions.
///
/// For each matching child, stops any running agent (emitting `agent:stopped` +
/// `agent:run_completed` events for UI consistency). When `archive_after_stop` is
/// true, also archives the child row via `update_status(Archived)` so it no longer
/// consumes ideation capacity or appears as an orphan.
///
/// Best-effort: errors during stop or archive are logged but do not abort the loop.
pub(crate) async fn stop_and_archive_children(
    session_id: &str,
    app_state: &AppState,
    filter: ChildFilter,
    archive_after_stop: bool,
) -> Result<(), AppError> {
    let session_id_typed = IdeationSessionId::from_string(session_id.to_string());
    let children = match filter {
        ChildFilter::VerificationOnly => {
            app_state
                .ideation_session_repo
                .get_verification_children(&session_id_typed)
                .await?
        }
        ChildFilter::AllChildren => {
            app_state
                .ideation_session_repo
                .get_children(&session_id_typed)
                .await?
        }
    };

    for child in &children {
        let key = RunningAgentKey::new("ideation", child.id.as_str());
        if app_state.running_agent_registry.is_running(&key).await {
            if let Ok(Some(info)) = app_state.running_agent_registry.stop(&key).await {
                // Remove from interactive process registry (closes stdin pipe)
                let ipr_key = InteractiveProcessKey::new("ideation", child.id.as_str());
                app_state
                    .interactive_process_registry
                    .remove(&ipr_key)
                    .await;

                // Mark agent run as failed
                let run_id = crate::domain::entities::AgentRunId::from_string(&info.agent_run_id);
                app_state
                    .agent_run_repo
                    .fail(&run_id, "Verification cancelled")
                    .await
                    .ok();

                crate::http_server::emit_app_event(
                    app_state,
                    "agent:stopped",
                    serde_json::json!({
                        "conversation_id": info.conversation_id.clone(),
                        "agent_run_id": info.agent_run_id,
                        "context_type": "ideation",
                        "context_id": child.id.as_str(),
                    }),
                );
                let completed_payload = AgentRunCompletedPayload::with_provider_session(
                    info.conversation_id.clone(),
                    "ideation".to_string(),
                    child.id.as_str().to_string(),
                    None,
                    None,
                    None,
                );
                if let Err(error) = ralphx_events::emit_serialized(
                    app_state.events.as_ref(),
                    "agent:run_completed",
                    &completed_payload,
                ) {
                    tracing::warn!(
                        event = "agent:run_completed",
                        %error,
                        "Failed to serialize agent run completion event payload"
                    );
                }
            }
        }

        if archive_after_stop {
            if let Err(e) = app_state
                .ideation_session_repo
                .update_status(&child.id, IdeationSessionStatus::Archived)
                .await
            {
                error!(
                    "Failed to archive child session {} after stop: {}",
                    child.id.as_str(),
                    e
                );
            }
        }
    }
    Ok(())
}

/// Stop any running verification child agents for a session.
///
/// Called when verification is skipped or reverted to immediately release the write lock
/// so the parent session can resume plan editing. Best-effort: errors are swallowed so the
/// caller's skip/revert succeeds even if the agent is already dead.
pub(crate) async fn stop_verification_children(
    session_id: &str,
    app_state: &AppState,
) -> Result<(), AppError> {
    stop_and_archive_children(session_id, app_state, ChildFilter::VerificationOnly, true).await
}

/// Stop an in-progress verification loop for a session.
///
/// Kills any running verification child agents, sets verification status to `skipped`
/// with `convergence_reason: "user_stopped"`, clears the `verification_in_progress` flag,
/// and increments the verification generation to prevent zombie agents from writing stale state.
///
/// Idempotent: if no verification is in progress, returns 200 with a message.
///
/// Route: `POST /api/ideation/sessions/:id/stop-verification`
pub async fn stop_verification(
    State(state): State<HttpServerState>,
    Path(session_id): Path<String>,
) -> Result<Json<SuccessResponse>, JsonError> {
    use crate::domain::entities::ideation::VerificationStatus;

    let session_id_obj =
        crate::domain::entities::IdeationSessionId::from_string(session_id.clone());

    // Read session
    let session = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id_obj)
        .await
        .map_err(|e| {
            error!("Failed to get session {}: {}", session_id, e);
            json_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to get session")
        })?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Session not found"))?;

    // Guard: reject calls targeting verification child sessions — orchestrators must use parent session_id
    if session.session_purpose == crate::domain::entities::SessionPurpose::Verification {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "Cannot stop verification on a verification child session. Use the parent session_id.",
        ));
    }

    // Session must be active
    if !session.is_active() {
        return Err(json_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Session is not active",
        ));
    }

    // Guard: external sessions cannot stop plan verification
    if session.origin == crate::domain::entities::ideation::SessionOrigin::External {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            "External sessions cannot stop plan verification.",
        ));
    }

    let (effective_status, effective_in_progress) =
        crate::domain::services::load_effective_verification_status(
            state.app_state.ideation_session_repo.as_ref(),
            &session,
        )
        .await
        .map_err(|e| {
            error!(
                "Failed to load effective verification status for {} before stop: {}",
                session_id, e
            );
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to load verification status",
            )
        })?;

    // Idempotent: if no verification is running, return 200 without doing anything
    if !effective_in_progress {
        return Ok(Json(SuccessResponse {
            success: true,
            message: "Verification is not in progress".to_string(),
        }));
    }

    // Kill any running verification child agents (best-effort)
    stop_verification_children(&session_id, &state.app_state)
        .await
        .ok();

    // Update native verification snapshot state for the current generation.
    let mut snapshot = crate::domain::services::load_current_verification_snapshot_or_default(
        state.app_state.ideation_session_repo.as_ref(),
        &session,
        effective_status,
        effective_in_progress,
    )
    .await
    .map_err(|e| {
        error!(
            "Failed to load verification snapshot for {} before stop: {}",
            session_id, e
        );
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to load verification snapshot",
        )
    })?;
    snapshot.status = VerificationStatus::Skipped;
    snapshot.in_progress = false;
    snapshot.convergence_reason = Some("user_stopped".to_string());

    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(&session_id_obj, &snapshot)
        .await
        .map_err(|e| {
            error!(
                "Failed to persist verification snapshot for {} after stop: {}",
                session_id, e
            );
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to persist verification snapshot",
            )
        })?;

    tracing::info!(
        session_id = %session_id,
        "Verification stopped by user"
    );

    // Increment generation to prevent zombie verifier from writing stale terminal status
    state
        .app_state
        .ideation_session_repo
        .increment_verification_generation(&session_id_obj)
        .await
        .ok();

    // Emit plan_verification:status_changed event so frontend VerificationBadge updates
    emit_verification_status_changed(
        state.app_state.events.as_ref(),
        &session_id,
        VerificationStatus::Skipped,
        false,
        Some(&snapshot),
        Some("user_stopped"),
        Some(session.verification_generation),
    );

    Ok(Json(SuccessResponse {
        success: true,
        message: "Verification stopped".to_string(),
    }))
}

/// POST /api/ideation/sessions/:id/verification/infra-failure
///
/// End an in-progress verification run as a runtime/infrastructure failure without
/// converting the parent session into a content verdict. This resets the canonical
/// parent session to `unverified`, clears authoritative current gaps, preserves round
/// history/debug metadata where available, increments generation, and asynchronously
/// stops any verification children so the caller is not forced to self-orchestrate
/// verifier shutdown from the prompt.
pub async fn mark_verification_infra_failure(
    State(state): State<HttpServerState>,
    Path(session_id): Path<String>,
    Json(req): Json<VerificationInfraFailureRequest>,
) -> Result<Json<crate::application::plan_verification_service::PlanVerificationStatus>, JsonError>
{
    use crate::domain::entities::ideation::VerificationStatus;

    let (session_id, session_id_obj, session) =
        resolve_verification_parent_session(&state, session_id).await?;

    if !session.is_active() {
        return Err(json_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Session is not active",
        ));
    }

    if let Some(req_gen) = req.generation {
        if req_gen != session.verification_generation {
            return Err(json_error(
                StatusCode::CONFLICT,
                format!(
                    "Generation mismatch: request generation {} != current generation {}. \
                     Verification was reset — zombie agent detected. \
                     Call get_plan_verification on the parent session, read verification_generation, \
                     and retry only if in_progress is still true.",
                    req_gen, session.verification_generation
                ),
            ));
        }
    }

    let (effective_status, effective_in_progress) =
        crate::domain::services::load_effective_verification_status(
            state.app_state.ideation_session_repo.as_ref(),
            &session,
        )
        .await
        .map_err(|e| {
            error!(
                "Failed to load effective verification status for {} before infra failure: {}",
                session_id, e
            );
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to load verification status",
            )
        })?;

    if !effective_in_progress && effective_status != VerificationStatus::Reviewing {
        return Err(json_error(
            StatusCode::CONFLICT,
            "Verification is not in progress on the parent session.",
        ));
    }

    let mut snapshot = crate::domain::services::load_current_verification_snapshot_or_default(
        state.app_state.ideation_session_repo.as_ref(),
        &session,
        effective_status,
        effective_in_progress,
    )
    .await
    .map_err(|e| {
        error!(
            "Failed to load verification snapshot for {} before infra failure: {}",
            session_id, e
        );
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to load verification snapshot",
        )
    })?;
    if let Some(round) = req.round {
        snapshot.current_round = round;
    }
    if let Some(max_rounds) = req.max_rounds {
        snapshot.max_rounds = max_rounds;
    }
    snapshot.status = VerificationStatus::Unverified;
    snapshot.in_progress = false;
    snapshot.current_gaps.clear();
    snapshot.convergence_reason = Some(
        req.convergence_reason
            .unwrap_or_else(|| "agent_error".to_string()),
    );
    let convergence_reason = snapshot.convergence_reason.clone();
    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(&session_id_obj, &snapshot)
        .await
        .map_err(|e| {
            error!(
                "Failed to persist verification snapshot for infra failure on {}: {}",
                session_id, e
            );
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to persist verification snapshot",
            )
        })?;

    state
        .app_state
        .ideation_session_repo
        .increment_verification_generation(&session_id_obj)
        .await
        .map_err(|e| {
            error!(
                "Failed to increment verification generation for {}: {}",
                session_id, e
            );
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to finalize verification runtime failure",
            )
        })?;
    let next_generation = session.verification_generation + 1;

    emit_verification_status_changed(
        state.app_state.events.as_ref(),
        &session_id,
        VerificationStatus::Unverified,
        false,
        Some(&snapshot),
        convergence_reason.as_deref(),
        Some(next_generation),
    );

    let app_state = state.app_state.clone();
    let session_id_for_stop = session_id.clone();
    tauri::async_runtime::spawn(async move {
        stop_verification_children(&session_id_for_stop, &app_state)
            .await
            .ok();
    });

    get_plan_verification(State(state), ProjectScope(None), Path(session_id)).await
}
/// POST /api/ideation/sessions/:id/revert-and-skip
///
/// Atomically revert plan content to a previous version and skip verification.
/// Both the artifact INSERT and session UPDATE happen in a single `db.run(|conn| { ... })`
/// transaction — no partial failure where artifact is created but session update fails.
pub async fn revert_and_skip(
    State(state): State<HttpServerState>,
    Path(session_id): Path<String>,
    Json(req): Json<RevertAndSkipRequest>,
) -> Result<Json<SuccessResponse>, JsonError> {
    use crate::domain::entities::ideation::VerificationStatus;
    use crate::domain::entities::{ArtifactContent, ArtifactId};

    let session_id_obj =
        crate::domain::entities::IdeationSessionId::from_string(session_id.clone());

    // Validate: fetch session, check not-external, check is-active (common guards)
    let session =
        validate_verification_session(&session_id, &session_id_obj, &state.app_state)
            .await
            .map_err(|e| {
                // Rephrase the external-session error to be revert-and-skip specific
                if e.0 == StatusCode::FORBIDDEN {
                    json_error(
                        StatusCode::FORBIDDEN,
                        "External sessions cannot skip plan verification. Run verification to completion.",
                    )
                } else {
                    e
                }
            })?;

    // Read the plan artifact version to restore
    let restore_artifact_id = ArtifactId::from_string(req.plan_version_to_restore.clone());
    let artifact = state
        .app_state
        .artifact_repo
        .get_by_id(&restore_artifact_id)
        .await
        .map_err(|e| {
            error!(
                "Failed to get artifact {}: {}",
                req.plan_version_to_restore, e
            );
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to get plan artifact",
            )
        })?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Plan artifact not found"))?;

    // Extract inline text content (plan artifacts must be inline)
    let content_text = match &artifact.content {
        ArtifactContent::Inline { text } => text.clone(),
        ArtifactContent::File { .. } => {
            return Err(json_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Plan artifact must be inline text content",
            ));
        }
    };

    // Pre-generate artifact ID for logging before the atomic operation
    let new_artifact_id = ArtifactId::new();
    let new_artifact_id_str = new_artifact_id.as_str().to_string();
    let new_version = artifact.metadata.version + 1;

    // Single atomic operation: INSERT artifact + UPDATE session in one db.run() transaction.
    // Prevents the race where artifact is created but session update fails.
    state
        .app_state
        .ideation_session_repo
        .revert_plan_and_skip_with_artifact(
            &session_id_obj,
            new_artifact_id_str.clone(),
            artifact.artifact_type.to_string(),
            artifact.name.clone(),
            content_text,
            new_version,
            restore_artifact_id.as_str().to_string(),
            "user_reverted".to_string(),
        )
        .await
        .map_err(|e| {
            error!("Failed revert-and-skip for session {}: {}", session_id, e);
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to atomically revert plan and skip verification",
            )
        })?;

    tracing::info!(
        session_id = %session_id,
        plan_version = %req.plan_version_to_restore,
        new_artifact_id = %new_artifact_id_str,
        "Revert-and-skip completed atomically"
    );

    // Kill any running verification child agents before emitting events.
    // Generation increment is handled inside the atomic SQL transaction above.
    stop_verification_children(&session_id, &state.app_state)
        .await
        .ok();

    // Emit event with canonical payload (B3: was missing round/gaps/rounds fields)
    emit_verification_status_changed(
        state.app_state.events.as_ref(),
        &session_id,
        VerificationStatus::Skipped,
        false,
        None,
        Some("user_reverted"),
        Some(session.verification_generation),
    );

    Ok(Json(SuccessResponse {
        success: true,
        message: "Plan reverted and verification skipped".to_string(),
    }))
}
