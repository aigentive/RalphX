use axum::{extract::State, http::HeaderMap, http::StatusCode, Json};

use crate::application::agent_workspace_pr_description::escape_xml_text;
use crate::domain::entities::{ChatConversationId, MessageRole};
use crate::http_server::handlers::agent_tasks::trusted_delegate_identity;
use crate::http_server::types::{
    ChatMessageSummary, GetDelegateParentContextRequest, GetDelegateParentContextResponse,
    HttpServerState,
};

use super::{json_error, JsonError};

const DEFAULT_PARENT_CONTEXT_MESSAGE_LIMIT: u32 = 20;
const MAX_PARENT_CONTEXT_MESSAGE_LIMIT: u32 = 50;
const MAX_PARENT_CONTEXT_MESSAGE_CHARS: usize = 500;

pub async fn get_delegate_parent_context(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(req): Json<GetDelegateParentContextRequest>,
) -> Result<Json<GetDelegateParentContextResponse>, JsonError> {
    let (delegated_session_id, delegated_run_id, delegated_conversation_id) =
        trusted_delegate_identity(&state, &headers, "get_parent_context")
            .await
            .map_err(|error| json_error(StatusCode::BAD_REQUEST, error))?;

    let session = state
        .app_state
        .delegated_session_repo
        .get_by_id(&delegated_session_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load delegated context authorization: {error}"),
            )
        })?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Delegated session not found"))?;
    if session.status != "running" {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "get_parent_context delegated session is not currently running",
        ));
    }
    if !session.delegate_context_authorized {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            "This delegated session was started with context inheritance disabled",
        ));
    }

    let delegated_conversation = state
        .app_state
        .chat_conversation_repo
        .get_by_id(&delegated_conversation_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load delegated conversation: {error}"),
            )
        })?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Delegated conversation not found"))?;
    let source_conversation_id = session
        .caller_conversation_id
        .as_deref()
        .or(delegated_conversation.parent_conversation_id.as_deref())
        .ok_or_else(|| {
            json_error(
                StatusCode::NOT_FOUND,
                "No authorized parent conversation is linked to this delegated session",
            )
        })?
        .parse::<ChatConversationId>()
        .map_err(|_| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Stored delegated parent conversation id is invalid",
            )
        })?;
    let source_conversation = state
        .app_state
        .chat_conversation_repo
        .get_by_id(&source_conversation_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load delegated parent conversation: {error}"),
            )
        })?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Parent conversation not found"))?;

    let eligible_messages = state
        .app_state
        .chat_message_repo
        .get_by_conversation(&source_conversation_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load delegated parent messages: {error}"),
            )
        })?
        .into_iter()
        .filter(|message| message.role != MessageRole::System)
        .collect::<Vec<_>>();
    let (current_session_id, current_run_id, current_conversation_id) =
        trusted_delegate_identity(&state, &headers, "get_parent_context")
            .await
            .map_err(|error| json_error(StatusCode::BAD_REQUEST, error))?;
    if current_session_id != delegated_session_id
        || current_run_id != delegated_run_id
        || current_conversation_id != delegated_conversation_id
    {
        return Err(json_error(
            StatusCode::CONFLICT,
            "get_parent_context delegated authority changed during the read",
        ));
    }
    let total_available = eligible_messages.len();
    let limit = req
        .limit
        .unwrap_or(DEFAULT_PARENT_CONTEXT_MESSAGE_LIMIT)
        .clamp(1, MAX_PARENT_CONTEXT_MESSAGE_LIMIT) as usize;
    let tail_start = total_available.saturating_sub(limit);
    let mut content_was_truncated = false;
    let messages = eligible_messages
        .into_iter()
        .skip(tail_start)
        .map(|message| {
            if message.content.chars().count() > MAX_PARENT_CONTEXT_MESSAGE_CHARS {
                content_was_truncated = true;
            }
            ChatMessageSummary {
                role: message.role.to_string(),
                content: escape_xml_text(
                    &message
                        .content
                        .chars()
                        .take(MAX_PARENT_CONTEXT_MESSAGE_CHARS)
                        .collect::<String>(),
                ),
                created_at: message.created_at.to_rfc3339(),
            }
        })
        .collect::<Vec<_>>();

    Ok(Json(GetDelegateParentContextResponse {
        source_conversation_id: source_conversation.id.as_str(),
        source_context_type: source_conversation.context_type.to_string(),
        messages,
        truncated: total_available > limit || content_was_truncated,
        total_available: u32::try_from(total_available).unwrap_or(u32::MAX),
    }))
}
