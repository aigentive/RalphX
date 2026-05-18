use super::*;

#[derive(Debug, Serialize)]
pub struct IdeationStatusResponse {
    pub session_id: String,
    pub project_id: String,
    pub title: Option<String>,
    pub status: String,
    pub agent_running: bool,
    pub agent_status: String,
    pub proposal_count: u32,
    pub created_at: String,
    pub verification_status: String,
    pub verification_in_progress: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_proposal_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_accept_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_accept_started_at: Option<String>,
    pub next_action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    pub queued_message_count: u32,
    pub unread_message_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_activity_phase: Option<String>,
}

/// GET /api/external/ideation_status/:id
/// Get ideation session status.
pub async fn get_ideation_status_http(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Path(id): Path<String>,
) -> Result<Json<IdeationStatusResponse>, HttpError> {
    let session_id = IdeationSessionId::from_string(id);

    let session = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .map_err(|e| {
            error!(
                "Failed to get ideation session {}: {}",
                session_id.as_str(),
                e
            );
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("Failed to get ideation session".to_string()),
            }
        })?
        .ok_or(HttpError {
            status: StatusCode::NOT_FOUND,
            message: Some("Session not found".to_string()),
        })?;

    session
        .assert_project_scope(&scope)
        .map_err(|e| HttpError {
            status: e.status,
            message: e.message,
        })?;

    let proposal_count = state
        .app_state
        .task_proposal_repo
        .count_by_session(&session_id)
        .await
        .map_err(|e| {
            error!("Failed to count proposals: {}", e);
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("Failed to count proposals".to_string()),
            }
        })?;

    let agent_key = crate::domain::services::running_agent_registry::RunningAgentKey::new(
        "ideation",
        session_id.as_str(),
    );
    let agent_running = state
        .app_state
        .running_agent_registry
        .is_running(&agent_key)
        .await;

    let agent_status = determine_agent_status(
        state.app_state.running_agent_registry.as_ref(),
        &state.app_state.interactive_process_registry,
        session_id.as_str(),
    )
    .await;

    let delivery_status =
        if session.status == crate::domain::entities::ideation::IdeationSessionStatus::Accepted {
            let tasks = state
                .app_state
                .task_repo
                .get_by_ideation_session(&session_id)
                .await
                .unwrap_or_default();
            Some(derive_delivery_status(&tasks))
        } else {
            None
        };

    let unread_message_count = state
        .app_state
        .chat_message_repo
        .count_unread_assistant_messages(
            session_id.as_str(),
            session.external_last_read_message_id.as_deref(),
        )
        .await
        .unwrap_or(0);

    let queued_message_count = state
        .app_state
        .message_queue
        .count_for_context("ideation", session_id.as_str()) as u32;

    let (effective_verification_status, effective_verification_in_progress) =
        crate::domain::services::load_effective_verification_status(
            state.app_state.ideation_session_repo.as_ref(),
            &session,
        )
        .await
        .map_err(|e| {
            error!(
                "Failed to load effective verification status for session {}: {}",
                session_id.as_str(),
                e
            );
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("Failed to load verification status".to_string()),
            }
        })?;

    let (next_action, hint) = match agent_status.as_str() {
        "waiting_for_input" if unread_message_count > 0 => (
            "fetch_messages".to_string(),
            Some("Agent has responded. Fetch messages before sending.".to_string()),
        ),
        "waiting_for_input" => (
            "send_message".to_string(),
            Some("Agent is ready for input.".to_string()),
        ),
        "generating" => (
            "wait".to_string(),
            Some("Agent is working. Poll again in 5-10s.".to_string()),
        ),
        _ => (
            "send_message".to_string(),
            Some("No agent running. Send a message to start.".to_string()),
        ),
    };

    Ok(Json(IdeationStatusResponse {
        session_id: session.id.to_string(),
        project_id: session.project_id.to_string(),
        title: session.title.clone(),
        status: session.status.to_string(),
        agent_running,
        agent_status,
        proposal_count,
        created_at: session.created_at.to_rfc3339(),
        verification_status: effective_verification_status.to_string(),
        verification_in_progress: effective_verification_in_progress,
        delivery_status,
        expected_proposal_count: session.expected_proposal_count,
        auto_accept_status: session.auto_accept_status.clone(),
        auto_accept_started_at: session.auto_accept_started_at.clone(),
        next_action,
        hint,
        queued_message_count,
        unread_message_count,
        external_activity_phase: session.external_activity_phase.clone(),
    }))
}
