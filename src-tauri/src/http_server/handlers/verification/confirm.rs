use super::helpers::spawn_verification_agent;
use super::*;

pub async fn confirm_verification(
    State(state): State<HttpServerState>,
    Json(req): Json<ConfirmVerificationRequest>,
) -> Result<Json<VerificationActionResponse>, HttpError> {
    let session_id_str = req.session_id.clone();
    let disabled_specialists = req.disabled_specialists.clone();

    // Set DB status to 'accepted' — marks that user has confirmed and verification is starting.
    let session_id_for_status = IdeationSessionId::from_string(session_id_str.clone());
    state
        .app_state
        .ideation_session_repo
        .set_verification_confirmation_status(
            &session_id_for_status,
            Some(VerificationConfirmationStatus::Accepted),
        )
        .await
        .map_err(map_app_err_local)?;

    if let Some(session) = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id_for_status)
        .await
        .map_err(map_app_err_local)?
    {
        crate::http_server::handlers::ideation::repair_blank_orphaned_verification_generation(
            &state.app_state,
            &session,
        )
        .await
        .map_err(map_app_err_local)?;

        let (_, effective_in_progress) =
            crate::domain::services::load_effective_verification_status(
                state.app_state.ideation_session_repo.as_ref(),
                &session,
            )
            .await
            .map_err(map_app_err_local)?;
        if effective_in_progress {
            return Ok(Json(VerificationActionResponse {
                status: "ok".to_string(),
            }));
        }
    }

    // Run transaction: verify session exists + trigger auto-verify
    let sid_clone = session_id_str.clone();
    let (session_id, maybe_generation) = state
        .app_state
        .db
        .run_transaction(move |conn| {
            let sid = IdeationSessionId::from_string(sid_clone);
            // Ensure session exists
            let _session = SessionRepo::get_by_id_sync(conn, sid.as_str())?
                .ok_or_else(|| AppError::NotFound(format!("Session {} not found", sid)))?;

            let generation = SessionRepo::trigger_auto_verify_sync(conn, sid.as_str())?;

            Ok((sid, generation))
        })
        .await
        .map_err(|e| {
            error!("confirm_verification transaction failed: {}", e);
            map_app_err_local(e)
        })?;

    // When trigger returns None, verification may already be running — check before erroring.
    let generation = match maybe_generation {
        Some(gen) => gen,
        None => {
            let vs = state
                .app_state
                .ideation_session_repo
                .get_verification_status(&session_id)
                .await
                .map_err(map_app_err_local)?;
            if matches!(vs, Some((_, true))) {
                // Verification already running — idempotent success.
                return Ok(Json(VerificationActionResponse {
                    status: "ok".to_string(),
                }));
            }
            error!("confirm_verification: trigger_auto_verify_sync returned None and verification is not running");
            return Err(HttpError::from(StatusCode::INTERNAL_SERVER_ERROR));
        }
    };

    spawn_verification_agent(&state, &session_id, generation, &disabled_specialists).await;

    Ok(Json(VerificationActionResponse {
        status: "ok".to_string(),
    }))
}
