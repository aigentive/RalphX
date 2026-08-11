use super::*;
use crate::application::agent_workspace_continuation::{
    classify_agent_workspace_continuation_with_plan_branch, AgentWorkspaceContinuationBlock,
};
use crate::application::harness_runtime_registry::default_external_mcp_message_queue_cap;
use crate::domain::services::QueueKey;
use std::collections::HashSet;

#[derive(Debug, Deserialize)]
pub struct IdeationMessageRequest {
    pub session_id: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct IdeationMessageResponse {
    /// Delivery outcome: "sent" | "queued" | "spawned"
    pub status: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queued_as_pending: Option<bool>,
    pub next_action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// POST /api/external/ideation_message
/// Send a message to an active ideation session.
///
/// Tri-state delivery:
/// 1. "sent"    — interactive process is open; message written directly to stdin
/// 2. "queued"  — agent is running but has no open stdin; message queued for resume
/// 3. "spawned" — no agent running; new agent process is spawned with the message
pub async fn ideation_message_http(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<IdeationMessageRequest>,
) -> Result<Json<IdeationMessageResponse>, (StatusCode, Json<serde_json::Value>)> {
    let session_id = IdeationSessionId::from_string(req.session_id.clone());

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
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to get ideation session"})),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Session not found"})),
            )
        })?;

    session.assert_project_scope(&scope).map_err(|e| {
        (
            e.status,
            Json(serde_json::json!({"error": e.message.unwrap_or_default()})),
        )
    })?;

    if session.status != crate::domain::entities::ideation::IdeationSessionStatus::Active {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Session is not active"})),
        ));
    }
    ensure_linked_agent_workspace_continuable(&state, &session_id).await?;

    let session_id_str = session_id.as_str().to_string();
    let current_phase = session.external_activity_phase.clone();

    let maybe_transition_to_planning =
        |repo: Arc<dyn crate::domain::repositories::IdeationSessionRepository>,
         sid: IdeationSessionId,
         phase: Option<String>| {
            if phase.as_deref() == Some("created") {
                tokio::spawn(async move {
                    if let Err(e) = repo
                        .update_external_activity_phase(&sid, Some("planning"))
                        .await
                    {
                        error!(
                            "Failed to set activity phase 'planning' for session {}: {}",
                            sid.as_str(),
                            e
                        );
                    }
                });
            }
        };

    let last_read = session.external_last_read_message_id.as_deref();
    match state
        .app_state
        .chat_message_repo
        .count_unread_messages(&session_id_str, last_read)
        .await
    {
        Ok(unread_count) if unread_count > 0 => {
            return Err((
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "unread_messages",
                    "unread_count": unread_count,
                    "hint": format!(
                        "You have {} unread message(s) (user or orchestrator) that you have not yet read. \
                Call v1_get_ideation_messages with offset=0 (default) to advance your read cursor to the latest message, \
                then retry sending.",
                        unread_count
                    ),
                    "next_action": "fetch_messages"
                })),
            ));
        }
        Ok(_) => {}
        Err(e) => {
            error!(
                "Failed to count unread messages for session {}: {}",
                session_id_str, e
            );
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to check message read status"})),
            ));
        }
    }

    let ipr_key = crate::application::InteractiveProcessKey {
        context_type: "ideation".to_string(),
        context_id: session_id_str.clone(),
    };
    if state
        .app_state
        .interactive_process_registry
        .has_process(&ipr_key)
        .await
    {
        let stream_json_message = crate::http_server::handlers::format_interactive_stdin_message(
            ChatContextType::Ideation,
            &session_id_str,
            &req.message,
        );
        match state
            .app_state
            .interactive_process_registry
            .write_message(&ipr_key, &stream_json_message)
            .await
        {
            Ok(_) => {
                maybe_transition_to_planning(
                    Arc::clone(&state.app_state.ideation_session_repo),
                    IdeationSessionId::from_string(session_id_str.clone()),
                    current_phase,
                );
                return Ok(Json(IdeationMessageResponse {
                    status: "sent".to_string(),
                    session_id: session_id_str,
                    queued_as_pending: None,
                    next_action: "poll_status".to_string(),
                    hint: Some(
                        "Wait for agent to respond. Poll v1_get_ideation_status (5-10s interval)"
                            .to_string(),
                    ),
                }));
            }
            Err(e) => {
                error!(
                    "Failed to write to interactive process for session {}: {}",
                    session_id_str, e
                );
            }
        }
    }

    let agent_key = crate::domain::services::running_agent_registry::RunningAgentKey::new(
        "ideation",
        &session_id_str,
    );
    if state
        .app_state
        .running_agent_registry
        .is_running(&agent_key)
        .await
    {
        let cap = default_external_mcp_message_queue_cap();
        let queue_key = QueueKey::new(ChatContextType::Ideation, &session_id_str);
        let mut queued_ids: HashSet<String> = state
            .app_state
            .message_queue
            .get_queued_with_key(&queue_key)
            .into_iter()
            .map(|message| message.id)
            .collect();
        for message in state
            .app_state
            .queued_message_repo
            .list(&queue_key)
            .await
            .map_err(|error| {
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": error.to_string() })),
                )
            })?
        {
            queued_ids.insert(message.id);
        }
        let queued_count = queued_ids.len();
        if queued_count >= cap {
            return Err((
                axum::http::StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({
                    "error": "queue_full",
                    "queued_count": queued_count,
                    "hint": format!(
                        "Message queue is full ({queued_count} pending). Wait for the agent to process messages. Poll v1_get_ideation_status."
                    ),
                    "next_action": "poll_status"
                })),
            ));
        }

        let queued = state.app_state.message_queue.queue(
            ChatContextType::Ideation,
            &session_id_str,
            req.message.clone(),
        );
        state
            .app_state
            .queued_message_repo
            .enqueue_back(&queue_key, &queued)
            .await
            .map_err(|error| {
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": error.to_string() })),
                )
            })?;
        maybe_transition_to_planning(
            Arc::clone(&state.app_state.ideation_session_repo),
            IdeationSessionId::from_string(session_id_str.clone()),
            current_phase,
        );
        return Ok(Json(IdeationMessageResponse {
            status: "queued".to_string(),
            session_id: session_id_str,
            queued_as_pending: None,
            next_action: "poll_status".to_string(),
            hint: Some(
                "Wait for agent to respond. Poll v1_get_ideation_status (5-10s interval)"
                    .to_string(),
            ),
        }));
    }

    let chat_service = build_chat_service(&state.app_state, &state.execution_state);
    let send_result = chat_service
        .send_message(
            ChatContextType::Ideation,
            &session_id_str,
            &req.message,
            SendMessageOptions {
                is_external_mcp: true,
                ..Default::default()
            },
        )
        .await;

    match send_result {
        Ok(result) if result.was_queued => {
            maybe_transition_to_planning(
                Arc::clone(&state.app_state.ideation_session_repo),
                IdeationSessionId::from_string(session_id_str.clone()),
                current_phase,
            );
            let queued_as_pending = result.queued_as_pending;
            return Ok(Json(IdeationMessageResponse {
                status: "queued".to_string(),
                session_id: session_id_str,
                queued_as_pending: queued_as_pending.then_some(true),
                next_action: if queued_as_pending {
                    "wait_for_resume".to_string()
                } else {
                    "poll_status".to_string()
                },
                hint: Some(if queued_as_pending {
                    "The message is saved as the pending ideation prompt, but execution is paused. Resume execution to launch the run.".to_string()
                } else {
                    "Wait for agent to respond. Poll v1_get_ideation_status (5-10s interval)"
                        .to_string()
                }),
            }));
        }
        Ok(_) => {}
        Err(e) => {
            error!(
                "Failed to send message to ideation session {}: {}",
                session_id_str, e
            );
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to send message"})),
            ));
        }
    }

    maybe_transition_to_planning(
        Arc::clone(&state.app_state.ideation_session_repo),
        IdeationSessionId::from_string(session_id_str.clone()),
        current_phase,
    );
    Ok(Json(IdeationMessageResponse {
        status: "spawned".to_string(),
        session_id: session_id_str,
        queued_as_pending: None,
        next_action: "poll_status".to_string(),
        hint: Some(
            "Wait for agent to respond. Poll v1_get_ideation_status (5-10s interval)".to_string(),
        ),
    }))
}

async fn ensure_linked_agent_workspace_continuable(
    state: &HttpServerState,
    session_id: &IdeationSessionId,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_linked_ideation_session_id(session_id)
        .await
        .map_err(|error| {
            error!(
                session_id = %session_id.as_str(),
                error = %error,
                "Failed to load linked agent workspace for ideation message preflight"
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "Failed to check linked agent workspace"
                })),
            )
        })?;
    let Some(workspace) = workspace else {
        return Ok(());
    };

    let project = state
        .app_state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .map_err(|error| {
            error!(
                session_id = %session_id.as_str(),
                conversation_id = %workspace.conversation_id.as_str(),
                error = %error,
                "Failed to load project for linked agent workspace preflight"
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "Failed to check linked agent workspace project"
                })),
            )
        })?
        .ok_or_else(|| {
            let reason = AgentWorkspaceContinuationBlock::UnknownRequiresManualCheck(
                "project not found".to_string(),
            );
            non_resumable_ideation_response(session_id, &workspace, &reason)
        })?;

    if let Some(reason) = classify_agent_workspace_continuation_with_plan_branch(
        &project,
        &workspace,
        Some(state.app_state.plan_branch_repo.as_ref()),
    )
    .await
    .blocked_reason()
    {
        return Err(non_resumable_ideation_response(
            session_id, &workspace, reason,
        ));
    }

    Ok(())
}

fn non_resumable_ideation_response(
    session_id: &IdeationSessionId,
    workspace: &crate::domain::entities::AgentConversationWorkspace,
    reason: &AgentWorkspaceContinuationBlock,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({
            "error": "agent_workspace_non_resumable",
            "reason": reason.code(),
            "session_id": session_id.as_str(),
            "conversation_id": workspace.conversation_id.as_str(),
            "next_action": "start_fresh_agent_conversation",
            "hint": reason.user_message(),
        })),
    )
}
