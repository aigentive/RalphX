use super::*;

/// Send an `<auto-propose>` message to the orchestrator agent for external sessions
/// that reached verification convergence via `zero_blocking`.
///
/// Retries up to 3 times with exponential backoff (1s/2s/4s between retries).
/// On final failure: emits `ideation:auto_propose_failed` to the external_events table.
pub(super) async fn auto_propose_for_external(
    session_id: &str,
    session: &crate::domain::entities::ideation::IdeationSession,
    state: &HttpServerState,
) {
    use crate::domain::entities::ideation::SessionOrigin;
    if session.origin != SessionOrigin::External {
        return;
    }

    // Transition external activity phase to "proposing" before attempting delivery so the
    // later "ready" write cannot be clobbered by a delayed fire-and-forget update.
    {
        let sid = crate::domain::entities::IdeationSessionId::from_string(session_id.to_string());
        if let Err(e) = state
            .app_state
            .ideation_session_repo
            .update_external_activity_phase(&sid, Some("proposing"))
            .await
        {
            tracing::error!(
                "Failed to set activity phase 'proposing' for session {}: {}",
                sid.as_str(),
                e
            );
        }
    }

    let app = &state.app_state;
    let chat_service =
        app.build_chat_service_with_execution_state(Arc::clone(&state.execution_state));

    let project_id = session.project_id.as_str().to_string();
    auto_propose_with_retry(
        session_id,
        &project_id,
        &chat_service,
        Arc::clone(&app.external_events_repo),
        app.webhook_publisher.as_ref().map(Arc::clone),
        &[1_000, 2_000, 4_000],
    )
    .await;

    // Set "ready" phase after proposing completes
    {
        let repo_ready = std::sync::Arc::clone(&state.app_state.ideation_session_repo);
        let sid_ready =
            crate::domain::entities::IdeationSessionId::from_string(session_id.to_string());
        if let Err(e) = repo_ready
            .update_external_activity_phase(&sid_ready, Some("ready"))
            .await
        {
            tracing::error!(
                "Failed to set activity phase 'ready' for session {}: {}",
                session_id,
                e
            );
        }
    }
}

/// Core retry logic for auto-propose delivery.
///
/// Attempts delivery up to `retry_delays_ms.len() + 1` times. Between each retry,
/// sleeps for the corresponding duration from `retry_delays_ms`. On final failure,
/// writes an `ideation:auto_propose_failed` row to the `external_events` table.
///
/// # Test usage
/// Pass `retry_delays_ms = &[0, 0, 0]` to eliminate sleep delays in tests.
#[doc(hidden)]
pub async fn auto_propose_with_retry(
    session_id: &str,
    project_id: &str,
    chat_service: &dyn ChatService,
    external_events_repo: Arc<dyn ExternalEventsRepository>,
    webhook_publisher: Option<Arc<dyn WebhookPublisher>>,
    retry_delays_ms: &[u64],
) {
    let message = "<auto-propose>\nThe plan has been verified with zero blocking gaps (convergence: zero_blocking).\nThis is an external MCP session. Auto-propose triggered.\n</auto-propose>";
    let max_attempts = retry_delays_ms.len() + 1;
    let mut last_error: Option<String> = None;

    for attempt in 0..max_attempts {
        match chat_service
            .send_message(
                ChatContextType::Ideation,
                session_id,
                message,
                SendMessageOptions::default(),
            )
            .await
        {
            Ok(result) => {
                tracing::info!(
                    session_id = %session_id,
                    attempt = attempt + 1,
                    delivery_status = if result.was_queued { "queued" } else { "spawned" },
                    "auto_propose_for_external: message delivered"
                );
                // Layer 2: persist IdeationAutoProposeSent to external_events table (non-fatal)
                let sent_payload = serde_json::json!({
                    "session_id": session_id,
                    "project_id": project_id,
                });
                if let Err(e) = external_events_repo
                    .insert_event(
                        "ideation:auto_propose_sent",
                        project_id,
                        &sent_payload.to_string(),
                    )
                    .await
                {
                    tracing::warn!(
                        session_id = %session_id,
                        error = %e,
                        "auto_propose_with_retry: failed to persist auto_propose_sent event (non-fatal)"
                    );
                }
                // Layer 3: webhook push (non-fatal, fire-and-forget)
                if let Some(ref publisher) = webhook_publisher {
                    let _ = publisher
                        .publish(
                            ralphx_domain::entities::EventType::IdeationAutoProposeSent,
                            project_id,
                            sent_payload,
                        )
                        .await;
                }
                return;
            }
            Err(e) => {
                let err_str = e.to_string();
                tracing::warn!(
                    session_id = %session_id,
                    attempt = attempt + 1,
                    max_attempts = max_attempts,
                    error = %err_str,
                    "auto_propose_for_external: send attempt failed"
                );
                last_error = Some(err_str);
                if attempt < retry_delays_ms.len() {
                    tokio::time::sleep(std::time::Duration::from_millis(retry_delays_ms[attempt]))
                        .await;
                }
            }
        }
    }

    // All attempts exhausted — emit failure event to external_events table.
    let error_msg = last_error.unwrap_or_else(|| "unknown error".to_string());
    tracing::error!(
        session_id = %session_id,
        max_attempts = max_attempts,
        error = %error_msg,
        "auto_propose_for_external: all retry attempts exhausted, emitting failure event"
    );
    let payload = serde_json::json!({
        "session_id": session_id,
        "project_id": project_id,
        "error": error_msg,
    });
    if let Err(insert_err) = external_events_repo
        .insert_event(
            "ideation:auto_propose_failed",
            project_id,
            &payload.to_string(),
        )
        .await
    {
        tracing::warn!(
            session_id = %session_id,
            error = %insert_err,
            "auto_propose_for_external: failed to persist failure event (non-fatal)"
        );
    }
    // Layer 3: webhook push for failure (Layer 2 insert above) — non-fatal, fire-and-forget
    if let Some(ref publisher) = webhook_publisher {
        let _ = publisher
            .publish(
                ralphx_domain::entities::EventType::IdeationAutoProposeFailed,
                project_id,
                payload,
            )
            .await;
    }
}
