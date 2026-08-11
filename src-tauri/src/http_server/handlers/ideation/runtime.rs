use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use tracing::error;

use crate::application::chat_service::{ChatService, SendMessageOptions};
use crate::application::InteractiveProcessKey;
use crate::domain::entities::ideation::IdeationSessionStatus;
use crate::domain::entities::{ChatContextType, IdeationSessionId};
use crate::domain::services::running_agent_registry::RunningAgentKey;
use crate::domain::services::QueueKey;
use crate::http_server::types::{
    ChildSessionStatusParams, ChildSessionStatusResponse, GetSessionMessagesRequest,
    GetSessionMessagesResponse, HttpServerState, SendSessionMessageRequest,
    SendSessionMessageResponse, SessionMessageResponse,
};

use super::{json_error, JsonError};

/// Get messages for an ideation session (context recovery for agents)
/// Returns messages newest-first with optional truncation
pub async fn get_session_messages(
    State(state): State<HttpServerState>,
    Json(req): Json<GetSessionMessagesRequest>,
) -> Result<Json<GetSessionMessagesResponse>, JsonError> {
    let session_id = IdeationSessionId::from_string(req.session_id.clone());

    // Cap limit at 200
    let limit = req.limit.clamp(1, 200);
    let offset = req.offset;

    // Get total count first
    let total_available = state
        .app_state
        .chat_message_repo
        .count_by_session(&session_id)
        .await
        .map_err(|e| {
            error!(
                "Failed to count messages for session {}: {}",
                session_id.as_str(),
                e
            );
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to count messages: {}", e),
            )
        })? as usize;

    // Get messages with offset for pagination support
    let messages = state
        .app_state
        .chat_message_repo
        .get_recent_by_session_paginated(&session_id, limit as u32, offset as u32)
        .await
        .map_err(|e| {
            error!(
                "Failed to get messages for session {}: {}",
                session_id.as_str(),
                e
            );
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get messages: {}", e),
            )
        })?;

    // Convert to response format
    let response_messages: Vec<SessionMessageResponse> = messages
        .into_iter()
        .map(|msg| {
            // If include_tool_calls is false and message has tool_calls,
            // we may want to append a note. For now, just return content.
            // The tool_calls field is excluded from SessionMessageResponse by design.
            SessionMessageResponse {
                role: msg.role.to_string(),
                content: msg.content,
                created_at: msg.created_at.to_rfc3339(),
            }
        })
        .collect();

    let count = response_messages.len();
    let truncated = total_available > limit + offset;

    Ok(Json(GetSessionMessagesResponse {
        messages: response_messages,
        count,
        truncated,
        total_available,
    }))
}
// GET /api/ideation/sessions/:id/child-status?include_messages=true&message_limit=10
pub async fn get_child_session_status_handler(
    State(state): State<HttpServerState>,
    Path(session_id): Path<String>,
    Query(params): Query<ChildSessionStatusParams>,
) -> Result<Json<ChildSessionStatusResponse>, JsonError> {
    Ok(Json(
        build_child_session_status_response(&state, &session_id, &params).await?,
    ))
}

pub(crate) async fn build_child_session_status_response(
    state: &HttpServerState,
    session_id: &str,
    params: &ChildSessionStatusParams,
) -> Result<ChildSessionStatusResponse, JsonError> {
    use crate::domain::entities::ideation::VerificationStatus;
    use crate::domain::entities::IdeationSessionId;
    use crate::domain::services::RunningAgentKey;
    use crate::http_server::types::{
        AgentStateInfo, ChatMessageSummary, ChildSessionStatusResponse, IdeationSessionSummary,
        VerificationInfo,
    };
    use crate::infrastructure::agents::claude::ideation_activity_threshold_secs;

    let session_id_obj = IdeationSessionId::from_string(session_id.to_string());

    // Step 1: Fetch session — 404 if not found
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

    // Step 2: Check RunningAgentRegistry under both "session" and "ideation" keys.
    // Ideation sessions can be registered under either key depending on how they were spawned.
    let session_key = RunningAgentKey::new("session", session_id);
    let ideation_key = RunningAgentKey::new("ideation", session_id);
    let registry = &state.app_state.running_agent_registry;

    let agent_info = if let Some(info) = registry.get(&session_key).await {
        Some(info)
    } else {
        registry.get(&ideation_key).await
    };

    // Step 3: Derive estimated_status from last_active_at using config threshold
    let threshold_secs = ideation_activity_threshold_secs();
    let agent_state = match &agent_info {
        None => AgentStateInfo {
            is_running: false,
            started_at: None,
            last_active_at: None,
            pid: None,
            estimated_status: "idle".to_string(),
        },
        Some(info) => {
            let estimated_status = if let Some(last_active) = info.last_active_at {
                let elapsed = chrono::Utc::now()
                    .signed_duration_since(last_active)
                    .num_seconds();
                if elapsed >= 0 && (elapsed as u64) < threshold_secs {
                    "likely_generating"
                } else {
                    "likely_waiting"
                }
            } else {
                // In registry but no heartbeat yet — assume still generating
                "likely_generating"
            };
            AgentStateInfo {
                is_running: true,
                started_at: Some(info.started_at.to_rfc3339()),
                last_active_at: info.last_active_at.map(|t| t.to_rfc3339()),
                pid: Some(info.pid),
                estimated_status: estimated_status.to_string(),
            }
        }
    };

    // Step 4: Optionally fetch recent messages, clamped to max 50
    let recent_messages = if params.include_messages.unwrap_or(false) {
        let limit = u32::min(params.message_limit.unwrap_or(5), 50);
        let messages = state
            .app_state
            .chat_message_repo
            .get_recent_by_session(&session_id_obj, limit)
            .await
            .map_err(|e| {
                error!("Failed to get messages for session {}: {}", session_id, e);
                json_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to get messages")
            })?;
        Some(
            messages
                .into_iter()
                .map(|m| {
                    // Truncate content to 500 chars (char-boundary safe)
                    let content = m.content.chars().take(500).collect::<String>();
                    ChatMessageSummary {
                        role: m.role.to_string(),
                        content,
                        created_at: m.created_at.to_rfc3339(),
                    }
                })
                .collect::<Vec<_>>(),
        )
    } else {
        None
    };

    let (effective_verification_status, _) =
        crate::domain::services::load_effective_verification_status(
            state.app_state.ideation_session_repo.as_ref(),
            &session,
        )
        .await
        .map_err(|e| {
            error!(
                "Failed to load effective verification status for session {}: {}",
                session_id, e
            );
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to get verification status",
            )
        })?;

    // Step 5: Build VerificationInfo from the native verification snapshot when verification
    // has started.
    let verification = if effective_verification_status != VerificationStatus::Unverified {
        let native_snapshot = state
            .app_state
            .ideation_session_repo
            .get_verification_run_snapshot(&session_id_obj, session.verification_generation)
            .await
            .map_err(|e| {
                error!(
                    "Failed to get native verification snapshot for session {}: {}",
                    session_id, e
                );
                json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to get verification snapshot",
                )
            })?;

        let (status, current_round, gap_score) = if let Some(snapshot) = native_snapshot {
            let round = (snapshot.current_round > 0).then_some(snapshot.current_round);
            let score = snapshot.rounds.last().map(|r| r.gap_score);
            (snapshot.status, round, score)
        } else {
            (effective_verification_status, None, None)
        };
        Some(VerificationInfo {
            status: status.to_string(),
            generation: session.verification_generation,
            current_round,
            gap_score,
        })
    } else {
        None
    };

    // Step 6: Build IdeationSessionSummary from session entity fields
    let session_summary = IdeationSessionSummary {
        id: session.id.as_str().to_string(),
        title: session.title.clone().unwrap_or_default(),
        status: session.status.to_string(),
        session_purpose: Some(session.session_purpose.to_string()),
        parent_session_id: session
            .parent_session_id
            .as_ref()
            .map(|id| id.as_str().to_string()),
        created_at: session.created_at.to_rfc3339(),
        updated_at: session.updated_at.to_rfc3339(),
        last_effective_model: session.last_effective_model.clone(),
    };

    Ok(ChildSessionStatusResponse {
        session: session_summary,
        agent_state,
        verification,
        recent_messages,
        pending_initial_prompt: session.pending_initial_prompt.clone(),
    })
}

/// POST /api/ideation/sessions/:id/message
///
/// Tri-state delivery:
/// 1. "sent"    — interactive process open; message written directly to stdin
/// 2. "queued"  — agent running but no open stdin; message queued for resume
/// 3. "spawned" — no agent running; new agent process spawned with the message
pub async fn send_ideation_session_message_handler(
    State(state): State<HttpServerState>,
    Path(session_id): Path<String>,
    Json(req): Json<SendSessionMessageRequest>,
) -> Result<Json<SendSessionMessageResponse>, JsonError> {
    // Step 1: Validate session exists
    let sid = IdeationSessionId::from_string(session_id.clone());
    let session = state
        .app_state
        .ideation_session_repo
        .get_by_id(&sid)
        .await
        .map_err(|e| {
            error!("Failed to get ideation session {}: {}", session_id, e);
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to get ideation session",
            )
        })?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Session not found"))?;

    // Validate session status (enum comparison, not string per CLAUDE.md rule #5)
    if session.status != IdeationSessionStatus::Active {
        return Err(json_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Session is not active",
        ));
    }

    // Validate message length
    if req.message.is_empty() {
        return Err(json_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Message cannot be empty",
        ));
    }
    if req.message.len() > 10_000 {
        return Err(json_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Message too long (max 10000 chars)",
        ));
    }

    // Step 2: Try interactive process registry under BOTH context type keys.
    // Ideation sessions can be registered under "session" (HTTP-spawned) or "ideation" (Tauri IPC-spawned).
    for context_type in &["session", "ideation"] {
        let ipr_key = InteractiveProcessKey {
            context_type: context_type.to_string(),
            context_id: session_id.clone(),
        };
        if state
            .app_state
            .interactive_process_registry
            .has_process(&ipr_key)
            .await
        {
            let stream_json_message =
                crate::http_server::handlers::format_interactive_stdin_message(
                    ChatContextType::Ideation,
                    &session_id,
                    &req.message,
                );
            match state
                .app_state
                .interactive_process_registry
                .write_message(&ipr_key, &stream_json_message)
                .await
            {
                Ok(_) => {
                    return Ok(Json(SendSessionMessageResponse {
                        delivery_status: "sent".to_string(),
                        conversation_id: None,
                    }));
                }
                Err(e) => {
                    // Process may have closed between has_process and write_message; fall through
                    error!(
                        "Failed to write to interactive process for session {} ({}): {}",
                        session_id, context_type, e
                    );
                }
            }
        }
    }

    // Step 3: Queue if agent running (check both keys)
    for context_type in &["session", "ideation"] {
        let agent_key = RunningAgentKey::new(*context_type, &session_id);
        if state
            .app_state
            .running_agent_registry
            .is_running(&agent_key)
            .await
        {
            let queued = state.app_state.message_queue.queue(
                ChatContextType::Ideation,
                &session_id,
                req.message.clone(),
            );
            let key = QueueKey::new(ChatContextType::Ideation, &session_id);
            state
                .app_state
                .queued_message_repo
                .enqueue_back(&key, &queued)
                .await
                .map_err(|error| {
                    json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                })?;
            return Ok(Json(SendSessionMessageResponse {
                delivery_status: "queued".to_string(),
                conversation_id: None,
            }));
        }
    }

    // Step 4: Agent not running — construct the shared chat service and spawn.
    let app = &state.app_state;
    let chat_service =
        app.build_chat_service_with_execution_state(Arc::clone(&state.execution_state));

    match chat_service
        .send_message(
            ChatContextType::Ideation,
            &session_id,
            &req.message,
            SendMessageOptions::default(),
        )
        .await
    {
        Ok(result) if result.was_queued => Ok(Json(SendSessionMessageResponse {
            delivery_status: "queued".to_string(),
            conversation_id: None,
        })),
        Ok(result) => Ok(Json(SendSessionMessageResponse {
            delivery_status: "spawned".to_string(),
            conversation_id: Some(result.conversation_id),
        })),
        Err(e) => {
            error!(
                "Failed to send message to ideation session {}: {}",
                session_id, e
            );
            Err(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to send message",
            ))
        }
    }
}
