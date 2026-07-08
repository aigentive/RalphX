use super::*;

/// Emit the verification-started event, build the round-loop description, spawn the verification
/// child session, and handle any spawn failures.
///
/// Returns `true` when the agent was spawned successfully, `false` when the spawn failed (in
/// which case [`handle_verification_spawn_failure`] has already been called).
pub async fn spawn_verification_agent(
    state: &HttpServerState,
    session_id: &IdeationSessionId,
    generation: i32,
    disabled_specialists: &[String],
) -> bool {
    crate::application::verification_child_session::spawn_verification_agent(
        &state.app_state,
        session_id,
        generation,
        disabled_specialists,
        |created_session| {
            crate::http_server::handlers::session_linking::build_ideation_chat_service(
                state,
                created_session,
            )
        },
    )
    .await
    .spawned
}

/// Handle a failed verification agent spawn: reset auto-verify state and emit status-changed event.
///
/// Called from both `create_plan_artifact` and `confirm_verification` when
/// `create_verification_child_session` returns `Ok(false)` or `Err(e)`.
pub async fn handle_verification_spawn_failure(
    state: &HttpServerState,
    session_id: &IdeationSessionId,
    generation: i32,
    error: Option<&str>,
) {
    crate::application::verification_child_session::handle_verification_spawn_failure(
        &state.app_state,
        session_id,
        generation,
        error,
    )
    .await;
}
