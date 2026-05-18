use super::*;

/// A single message returned to external consumers.
#[derive(Debug, Serialize)]
pub struct IdeationMessageSummary {
    pub id: String,
    /// "user" or "assistant" (Orchestrator is mapped to "assistant")
    pub role: String,
    pub content: String,
    pub created_at: String,
}

/// Response for GET /api/external/ideation_messages/:session_id
#[derive(Debug, Serialize)]
pub struct GetIdeationMessagesResponse {
    pub messages: Vec<IdeationMessageSummary>,
    pub has_more: bool,
    /// "idle" | "generating" | "waiting_for_input"
    pub agent_status: String,
    pub next_action: String,
}

/// Query params for pagination.
#[derive(Debug, Deserialize)]
pub struct GetIdeationMessagesQuery {
    #[serde(default = "default_messages_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

fn default_messages_limit() -> u32 {
    50
}

/// GET /api/external/ideation_messages/:session_id
///
/// Returns orchestrator and user messages for an ideation session.
/// Filter: User + Orchestrator roles only (Orchestrator → "assistant").
pub async fn get_ideation_messages_http(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Path(session_id): Path<String>,
    Query(params): Query<GetIdeationMessagesQuery>,
) -> Result<Json<GetIdeationMessagesResponse>, StatusCode> {
    use crate::domain::entities::ideation::MessageRole;

    let session_id = IdeationSessionId::from_string(session_id);

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
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    session.assert_project_scope(&scope).map_err(|e| e.status)?;

    let fetch_limit = params.limit.saturating_add(1);
    let raw_messages = state
        .app_state
        .chat_message_repo
        .get_recent_by_session_paginated(&session_id, fetch_limit, params.offset)
        .await
        .map_err(|e| {
            error!(
                "Failed to get messages for session {}: {}",
                session_id.as_str(),
                e
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let has_more = raw_messages.len() > params.limit as usize;
    let messages_slice = if has_more {
        &raw_messages[..params.limit as usize]
    } else {
        &raw_messages[..]
    };

    let messages: Vec<IdeationMessageSummary> = messages_slice
        .iter()
        .filter(|msg| matches!(msg.role, MessageRole::User | MessageRole::Orchestrator))
        .map(|msg| {
            let role = match msg.role {
                MessageRole::Orchestrator => "assistant".to_string(),
                _ => "user".to_string(),
            };
            IdeationMessageSummary {
                id: msg.id.to_string(),
                role,
                content: msg.content.clone(),
                created_at: msg.created_at.to_rfc3339(),
            }
        })
        .collect();

    if let Some(latest_msg) = messages.last() {
        let latest_id = latest_msg.id.clone();
        if let Err(e) = state
            .app_state
            .ideation_session_repo
            .update_external_last_read_message_id(&session_id, &latest_id)
            .await
        {
            error!(
                "Failed to update external_last_read_message_id for session {}: {}",
                session_id.as_str(),
                e
            );
        }
    }

    let agent_status = determine_agent_status(
        state.app_state.running_agent_registry.as_ref(),
        &state.app_state.interactive_process_registry,
        session_id.as_str(),
    )
    .await;

    let next_action = match agent_status.as_str() {
        "waiting_for_input" => "send_message".to_string(),
        "generating" => "wait".to_string(),
        _ => "send_message".to_string(),
    };

    Ok(Json(GetIdeationMessagesResponse {
        messages,
        has_more,
        agent_status,
        next_action,
    }))
}
